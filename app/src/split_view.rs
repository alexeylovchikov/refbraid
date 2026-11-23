//! "Split into Atomic Commits": an AI agent reads the checked changes, works out which task
//! each change belongs to and proposes a series of commits; the plan window lists them (like
//! the Push window) and creates them one after another. Changes inside one file can go to
//! different commits.

use crate::ai;
use crate::i18n::tr;
use crate::app::{App, Job, file_name, status_color};
use crate::form::{self, Part};
use crate::git::{self, DiffSide, StatusEntry};
use crate::partial::{self, Block};
use crate::theme::{self, Icon};
use eframe::egui::{self, FontId, Key, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

/// One change the agent can place in a commit: a block of a file's diff, or a whole file
/// (new, deleted, renamed or binary files).
#[derive(Clone, Debug)]
pub struct Change {
    /// "path#n" for a block, "path" for a whole file.
    pub id: String,
    pub path: String,
    /// The block's hash; None for a whole file.
    pub hash: Option<u64>,
    /// First line of the change in the new file (1-based), 0 when not known.
    pub line: usize,
    pub kind: &'static str,
    /// The changed lines: '-' removed, '+' added.
    pub lines: Vec<(char, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedCommit {
    pub title: String,
    pub description: String,
    /// Indexes into the changes.
    pub changes: Vec<usize>,
}

enum Phase {
    Analysing(ai::Running),
    Failed(String),
    Plan,
}

pub struct SplitForm {
    files: Vec<StatusEntry>,
    changes: Vec<Change>,
    phase: Phase,
    agent: String,
    started: Instant,
    commits: Vec<PlannedCommit>,
    selected: usize,
    selected_file: Option<String>,
    /// Put each commit's description in its message body.
    add_body: bool,
}

const W: f32 = 940.0;
const H: f32 = 470.0;
const LEFT_W: f32 = 380.0;

/// Why a file can only be committed whole, if it can't be split.
fn whole_reason(e: &StatusEntry, has_head: bool) -> Option<&'static str> {
    if e.is_untracked() || e.x == 'A' || !has_head {
        Some("new file")
    } else if e.x == 'D' || e.y == 'D' {
        Some("deleted")
    } else if e.old_path.is_some() {
        Some("renamed")
    } else {
        None
    }
}

fn changed_lines(rows: &[git::DiffRow]) -> Vec<(char, String)> {
    let mut minus = vec![];
    let mut plus = vec![];
    let mut out = vec![];
    for r in rows {
        if r.kind == DiffSide::Context {
            out.append(&mut minus);
            out.append(&mut plus);
            continue;
        }
        if let Some((_, t)) = &r.left {
            minus.push(('-', t.clone()));
        }
        if let Some((_, t)) = &r.right {
            plus.push(('+', t.clone()));
        }
    }
    out.append(&mut minus);
    out.append(&mut plus);
    out
}

/// Number the changes of the checked files.
pub fn collect(dir: &Path, files: &[StatusEntry], has_head: bool) -> Vec<Change> {
    let mut out = vec![];
    for e in files {
        let d = git::worktree_diff_exact(dir, e, has_head);
        let mut reason = whole_reason(e, has_head);
        if reason.is_none() && (d.binary || d.images.is_some()) {
            reason = Some("binary");
        }
        let bl = if reason.is_none() { partial::blocks(&d.rows) } else { vec![] };
        if reason.is_none() && bl.is_empty() {
            reason = Some("modified");
        }
        if let Some(kind) = reason {
            let kind = match (kind, &e.old_path) {
                ("renamed", Some(_)) => "renamed",
                (k, _) => k,
            };
            out.push(Change { id: e.path.clone(), path: e.path.clone(), hash: None, line: 0, kind, lines: changed_lines(&d.rows) });
            continue;
        }
        for (n, b) in bl.iter().enumerate() {
            out.push(Change {
                id: format!("{}#{}", e.path, n + 1),
                path: e.path.clone(),
                hash: Some(b.hash),
                line: b.new_start + 1,
                kind: "modified",
                lines: changed_lines(&d.rows[b.first_row..=b.last_row]),
            });
        }
    }
    out
}

/// The request sent to the agent.
pub fn build_prompt(dir: &Path, branch: &str, files: &[StatusEntry], changes: &[Change], user_prompt: &str) -> String {
    let user_prompt = if user_prompt.trim().is_empty() { ai::DEFAULT_PROMPT } else { user_prompt };
    let user_prompt = user_prompt.replace("$GIT_BRANCH_NAME", branch);
    let recent = git::run(dir, &["log", "-12", "--format=%s"]).stdout;
    let mut list = String::new();
    const LIMIT: usize = 80_000;
    const PER_CHANGE: usize = 150;
    for c in changes {
        let from = files.iter().find(|e| e.path == c.path).and_then(|e| e.old_path.clone());
        let what = match (c.kind, from) {
            ("renamed", Some(f)) => format!("renamed from {f}"),
            (k, _) if c.line > 0 => format!("{k}, line {}", c.line),
            (k, _) => k.to_string(),
        };
        list += &format!("\n### {}  ({what})\n", c.id);
        if list.len() > LIMIT {
            continue;
        }
        for (k, t) in c.lines.iter().take(PER_CHANGE) {
            list += &format!("{k}{t}\n");
        }
        if c.lines.len() > PER_CHANGE {
            list += &format!("… ({} more lines)\n", c.lines.len() - PER_CHANGE);
        }
    }
    if list.len() > LIMIT {
        list += "\n… (the rest of the changes are listed by id only)\n";
    }
    format!(
        "You split the uncommitted changes of a git repository into atomic commits.\n\n\
Work out what each change is for and how the changes depend on each other, then group them into \
logically complete commits, one task per commit. Changes inside one file may go to different commits \
when they belong to different tasks. Order the commits so that each one builds on the ones before it \
(what others depend on comes first).\n\n\
Write each commit subject following these rules:\n{user_prompt}\n\n\
Reply with ONLY the plan in exactly this format, one block per commit, and no other text:\n\n\
COMMIT: <commit message subject>\n\
DESCRIPTION: <one short sentence: what the commit does and why>\n\
CHANGES: <ids of its changes, comma-separated>\n\n\
Every change id must appear in exactly one commit. Do not run tools or change any files.\n\n\
Current branch: {branch}\n\n\
Recent commit subjects in this repository (for style):\n{recent}\n\
Changes, each with its id:\n{list}"
    )
}

/// A made-up answer for the screenshot of the plan window (the demo repository's changes).
pub const DEMO_PLAN: &str = "COMMIT: [fix] Step 2 of the pipeline doubles the value
DESCRIPTION: The second step multiplied instead of adding, as the pipeline expects.
CHANGES: src/app.py#1
COMMIT: [feat] Greet the world and log the start of the app
DESCRIPTION: main() prints a friendlier greeting and writes a start message to the log.
CHANGES: src/app.py#2
COMMIT: [chore] Mark the database connection helper
DESCRIPTION: A note next to the engine creation for the coming connection work.
CHANGES: src/db.py#1
COMMIT: [docs] Add planning notes and a README heading
DESCRIPTION: Ideas, plan and to-do notes for the next release.
CHANGES: notes/ideas.md, notes/plan.md, notes/todo.md, notes/todo.txt#1, README.md#1
";

fn tidy(s: &str) -> String {
    s.trim().to_string()
}

/// Read the agent's answer. Unknown ids are ignored; a change named twice stays in the first
/// commit; a bare file name takes all of that file's changes not placed yet.
pub fn parse_plan(text: &str, changes: &[Change]) -> Vec<PlannedCommit> {
    let mut out: Vec<PlannedCommit> = vec![];
    let mut used: HashSet<usize> = HashSet::new();
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', '#', ' ']).replace("**", "");
        let line = line.trim();
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let key = key.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ');
        let value = value.trim();
        match key {
            "commit" | "title" | "subject" => out.push(PlannedCommit { title: value.trim_matches(['"', '`']).to_string(), description: String::new(), changes: vec![] }),
            "description" => {
                if let Some(c) = out.last_mut() {
                    c.description = value.to_string();
                }
            }
            "changes" => {
                let Some(c) = out.last_mut() else { continue };
                for id in value.split([',', ';']) {
                    let id: String = id.trim().trim_matches(['"', '`', '\'', '[', ']']).split_whitespace().collect::<Vec<_>>().join("");
                    if id.is_empty() {
                        continue;
                    }
                    let hits: Vec<usize> = match changes.iter().position(|x| x.id == id) {
                        Some(i) => vec![i],
                        None => changes.iter().enumerate().filter(|(_, x)| x.path == id).map(|(i, _)| i).collect(),
                    };
                    for i in hits {
                        if used.insert(i) {
                            c.changes.push(i);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out.retain(|c| !c.changes.is_empty() && !c.title.is_empty());
    out
}

fn file_mode(dir: &Path, path: &str) -> String {
    let mode = git::run(dir, &["ls-tree", "HEAD", "--", path]).stdout.split_whitespace().next().map(String::from);
    mode.unwrap_or_else(|| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exec = std::fs::metadata(dir.join(path)).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false);
            if exec { "100755".into() } else { "100644".into() }
        }
        #[cfg(not(unix))]
        {
            "100644".to_string()
        }
    })
}

/// The git commands that create the planned commits one after another. Each commit gets its
/// own changes on top of the ones before it, built in a separate index so the person's staged
/// files are not touched; changes left out of the plan stay uncommitted.
pub fn commit_commands(
    dir: &Path,
    files: &[StatusEntry],
    changes: &[Change],
    plan: &[PlannedCommit],
    has_head: bool,
    add_body: bool,
) -> Result<Vec<Vec<String>>, String> {
    let git_path = |name: &str| {
        let p = std::path::PathBuf::from(git::run(dir, &["rev-parse", "--git-path", name]).stdout.trim());
        if p.is_absolute() { p } else { dir.join(p) }
    };
    let index = git_path("refbraid-index");
    let scratch = git_path("refbraid-split");
    let env = format!("env:GIT_INDEX_FILE={}", index.display());
    let s = |x: &str| x.to_string();
    // The files as they are now: their blocks, old and new text.
    let mut now: HashMap<String, (Vec<Block>, Vec<u8>, Vec<u8>)> = HashMap::new();
    for c in plan.iter().flat_map(|p| p.changes.iter()).map(|&i| &changes[i]) {
        let Some(h) = c.hash else { continue };
        if !now.contains_key(&c.path) {
            let e = files.iter().find(|e| e.path == c.path).ok_or_else(|| format!("{} is no longer changed.", c.path))?;
            let d = git::worktree_diff_exact(dir, e, has_head);
            let old = git::blob(dir, "HEAD", &e.path).unwrap_or_default();
            let new = std::fs::read(dir.join(&e.path)).unwrap_or_default();
            now.insert(c.path.clone(), (partial::blocks(&d.rows), old, new));
        }
        if !now[&c.path].0.iter().any(|b| b.hash == h) {
            return Err(format!("{} was edited after the analysis. Run the analysis again.", c.path));
        }
    }
    let mut cmds: Vec<Vec<String>> = vec![];
    cmds.push(if has_head { vec![env.clone(), s("read-tree"), s("HEAD")] } else { vec![env.clone(), s("read-tree"), s("--empty")] });
    let mut taken: HashMap<String, HashSet<u64>> = HashMap::new();
    let mut all_paths: Vec<String> = vec![];
    for p in plan {
        let mut touched: Vec<&str> = vec![];
        for &i in &p.changes {
            let c = &changes[i];
            if let Some(h) = c.hash {
                taken.entry(c.path.clone()).or_default().insert(h);
            }
            if !touched.contains(&c.path.as_str()) {
                touched.push(&c.path);
            }
        }
        let mut whole = vec![env.clone(), s("add"), s("-A"), s("--")];
        let mut parts = vec![];
        for path in touched {
            let e = files.iter().find(|e| e.path == path).ok_or_else(|| format!("{path} is no longer changed."))?;
            let mut paths = vec![e.path.clone()];
            paths.extend(e.old_path.clone());
            for x in &paths {
                if !all_paths.contains(x) {
                    all_paths.push(x.clone());
                }
            }
            match now.get(path) {
                Some((bl, old, new)) if !bl.iter().all(|b| taken[path].contains(&b.hash)) => {
                    let t = &taken[path];
                    let content = partial::mix(old, new, bl, |i| t.contains(&bl[i].hash));
                    std::fs::write(&scratch, &content).map_err(|e| e.to_string())?;
                    let blob = git::run(dir, &["hash-object", "-w", &format!("--path={path}"), &scratch.display().to_string()]);
                    if !blob.ok {
                        return Err(blob.stderr);
                    }
                    parts.push(vec![env.clone(), s("update-index"), s("--add"), s("--cacheinfo"), format!("{},{},{path}", file_mode(dir, path), blob.stdout.trim())]);
                }
                _ => whole.extend(paths),
            }
        }
        if whole.len() > 4 {
            cmds.push(whole);
        }
        cmds.extend(parts);
        let msg = if add_body && !p.description.trim().is_empty() { format!("{}\n\n{}", p.title.trim(), p.description.trim()) } else { p.title.trim().to_string() };
        cmds.push(vec![env.clone(), s("commit"), s("-m"), msg]);
    }
    let _ = std::fs::remove_file(&scratch);
    let mut reset = vec![s("reset"), s("-q"), s("--")];
    reset.extend(all_paths);
    if has_head || plan.len() > 0 {
        cmds.push(reset);
    }
    Ok(cmds)
}

impl App {
    /// The checked files of the Changes view.
    fn split_files(&self) -> Vec<StatusEntry> {
        let Some(repo) = self.repo.as_ref() else { return vec![] };
        repo.status.iter().filter(|e| !e.is_conflict() && !repo.unchecked.contains(&e.path)).cloned().collect()
    }

    pub fn split_running(&self) -> bool {
        matches!(self.split.as_ref().map(|s| &s.phase), Some(Phase::Analysing(_)))
    }

    /// The menu under the stars button of the Changes toolbar.
    pub fn ai_tasks_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(300.0);
        ui.label(RichText::new(tr("AI Tasks")).color(theme::TEXT_DIM()).size(11.5));
        let n = self.split_files().len();
        let tip = if n == 0 {
            "Check the files to analyse first.".to_string()
        } else {
            format!("Analyse the {n} checked file{} and split the changes into commits, one task each", if n == 1 { "" } else { "s" })
        };
        let r = ui.add_enabled(n > 0, egui::Button::new(tr("Split Changes into Atomic Commits…")).shortcut_text(RichText::new(format!("{n} file{}", if n == 1 { "" } else { "s" })).color(theme::TEXT_DIM())));
        let r = if n > 0 { r.on_hover_text(tip) } else { r.on_disabled_hover_text(tip) };
        if r.clicked() {
            ui.close();
            self.start_split();
        }
    }

    pub fn start_split(&mut self) {
        let Some(h) = ai::harness(&self.settings.agent) else {
            self.open_settings(Some(crate::settings_view::Page::AiAgents));
            return;
        };
        let Some((_, program)) = self.agents.iter().find(|(id, _)| *id == h.id).cloned() else {
            self.open_settings(Some(crate::settings_view::Page::AiAgents));
            return;
        };
        let files = self.split_files();
        let Some(repo) = self.repo.as_ref() else { return };
        if files.is_empty() {
            return;
        }
        let has_head = git::run(&repo.path, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok;
        let changes = collect(&repo.path, &files, has_head);
        let branch = repo.state.head_branch.clone().unwrap_or_else(|| "HEAD".into());
        let prompt = build_prompt(&repo.path, &branch, &files, &changes, &self.settings.ai_prompt);
        let job = ai::start_with(h, &program, &repo.path, prompt, tidy);
        self.split = Some(SplitForm {
            files,
            changes,
            phase: Phase::Analysing(job),
            agent: h.name.to_string(),
            started: Instant::now(),
            commits: vec![],
            selected: 0,
            selected_file: None,
            add_body: false,
        });
    }

    /// Screenshot helper: the plan window with a made-up answer for the checked files.
    pub fn demo_split(&mut self, text: &str) {
        let files = self.split_files();
        let Some(repo) = self.repo.as_ref() else { return };
        let changes = collect(&repo.path, &files, true);
        let commits = parse_plan(text, &changes);
        self.split = Some(SplitForm { files, changes, phase: Phase::Plan, agent: "Claude Code".into(), started: Instant::now(), commits, selected: 1, selected_file: None, add_body: false });
    }

    pub fn split_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.split.take() else { return };
        let Some(dir) = self.repo.as_ref().map(|r| r.path.clone()) else { return };
        // The agent's answer.
        if let Phase::Analysing(job) = &f.phase {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            if let Ok(res) = job.rx.try_recv() {
                f.phase = match res {
                    Ok(text) => {
                        f.commits = parse_plan(&text, &f.changes);
                        if f.commits.is_empty() {
                            Phase::Failed(format!("{} did not return a plan in the expected form:\n\n{}", f.agent, text.chars().take(600).collect::<String>()))
                        } else {
                            Phase::Plan
                        }
                    }
                    Err(e) => Phase::Failed(e),
                };
            }
        }
        let mut keep = true;
        let mut create = false;
        let mut retry = false;
        let plan = matches!(f.phase, Phase::Plan);
        let ctrl_enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Enter));
        if ctrl_enter && plan {
            create = true;
        }
        let title = "Split Changes into Atomic Commits";
        let closed = form::show_with(ctx, "split_window", title, W, !plan, |ui, part| {
            match (&f.phase, part) {
                (Phase::Analysing(_), Part::Body) => {
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(18.0));
                        ui.add_space(6.0);
                        let n = f.files.len();
                        ui.label(RichText::new(format!("{} is analysing {} change{} in {n} file{}…", f.agent, f.changes.len(), if f.changes.len() == 1 { "" } else { "s" }, if n == 1 { "" } else { "s" })).size(14.0).color(theme::TEXT()));
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Working out what each change is for and how they depend on each other. {} s", f.started.elapsed().as_secs())).color(theme::TEXT_DIM()));
                    ui.add_space(14.0);
                }
                (Phase::Analysing(_), Part::Footer) => {
                    if form::button(ui, tr("Stop")) {
                        keep = false;
                    }
                }
                (Phase::Failed(e), Part::Body) => {
                    ui.label(RichText::new(tr("The analysis did not finish")).size(14.0).color(theme::RED()));
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(260.0 + crate::form::grow(ui)).show(ui, |ui| {
                        ui.label(RichText::new(e.as_str()).color(theme::TEXT_MID()).monospace());
                    });
                }
                (Phase::Failed(_), Part::Footer) => {
                    if form::button(ui, tr("Close")) {
                        keep = false;
                    }
                    if form::primary(ui, tr("Try Again"), true, false) {
                        retry = true;
                    }
                }
                (Phase::Plan, Part::Body) => Self::split_plan(ui, &mut f),
                (Phase::Plan, Part::Footer) => {
                    if form::button(ui, tr("Cancel")) {
                        keep = false;
                    }
                    let n = f.commits.len();
                    if form::primary(ui, &format!("Create {n} Commit{}", if n == 1 { "" } else { "s" }), n > 0 && f.commits.iter().all(|c| !c.title.trim().is_empty()), false) {
                        create = true;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add_space(2.0);
                        let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                        ui.painter().circle_stroke(r.center(), 10.5, Stroke::new(1.0, theme::TEXT_DIM()));
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "?", FontId::proportional(13.0), theme::TEXT_MID());
                        resp.on_hover_text(tr("The commits are created from top to bottom, each on top of the previous one.\nClick a commit to edit its message and see its changes.\nChanges not in any commit stay uncommitted. Ctrl+Enter creates the commits."));
                        ui.add_space(14.0);
                        ui.checkbox(&mut f.add_body, tr("Add descriptions to commit messages"));
                    });
                }
            }
        });
        if closed {
            keep = false;
        }
        if retry {
            self.split = None;
            self.start_split();
            return;
        }
        if create {
            let has_head = git::run(&dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok;
            match commit_commands(&dir, &f.files, &f.changes, &f.commits, has_head, f.add_body) {
                Ok(cmds) => {
                    let n = f.commits.len();
                    self.run_job(Job::many(&format!("Create {n} commit{}", if n == 1 { "" } else { "s" }), cmds));
                    keep = false;
                }
                Err(e) => f.phase = Phase::Failed(e),
            }
        }
        if !keep {
            if let Phase::Analysing(job) = &f.phase {
                ai::cancel(job);
            }
            return;
        }
        self.split = Some(f);
    }

    fn split_plan(ui: &mut Ui, f: &mut SplitForm) {
        // Fills the window, which can be made bigger; the divider between the panes can be dragged.
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(W * 0.7), H + form::grow(ui)), Sense::hover());
        let lw = form::split_width(ui.ctx(), rect, "split_window", LEFT_W, 220.0);
        let left = Rect::from_min_max(rect.min, pos2(rect.left() + lw, rect.bottom()));
        let right = Rect::from_min_max(pos2(left.right() + 1.0, rect.top()), rect.max);
        let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(left));
        lu.set_clip_rect(left);
        Self::split_commits(&mut lu, f, left);
        let mut ru = ui.new_child(egui::UiBuilder::new().max_rect(right));
        ru.set_clip_rect(right);
        Self::split_details(&mut ru, f, right);
        form::splitter(ui, rect, "split_window", LEFT_W, 220.0);
    }

    fn split_commits(ui: &mut Ui, f: &mut SplitForm, rect: Rect) {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let used: HashSet<usize> = f.commits.iter().flat_map(|c| c.changes.iter().copied()).collect();
        let left_out = f.changes.len() - used.len();
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), 30.0));
        let n = f.commits.len();
        ui.painter().text(
            pos2(head.left() + 14.0, head.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{n} commit{} from {} change{}", if n == 1 { "" } else { "s" }, f.changes.len(), if f.changes.len() == 1 { "" } else { "s" }),
            FontId::proportional(12.5),
            theme::TEXT_MID(),
        );
        let foot_h = if left_out > 0 { 28.0 } else { 0.0 };
        let list = Rect::from_min_max(pos2(rect.left(), head.bottom()), pos2(rect.right(), rect.bottom() - foot_h));
        let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(list));
        lu.set_clip_rect(list);
        let font = FontId::proportional(13.0);
        let small = FontId::proportional(12.0);
        egui::ScrollArea::vertical().id_salt("split_commits").auto_shrink([false, false]).show(&mut lu, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let w = ui.available_width();
            for (i, c) in f.commits.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(w, 46.0), Sense::click());
                let p = ui.painter();
                if f.selected == i {
                    p.rect_filled(r, 0.0, theme::SELECT());
                } else if resp.hovered() {
                    p.rect_filled(r, 0.0, theme::HOVER());
                }
                // Number in a circle, joined by a line like a short history.
                let cx = r.left() + 22.0;
                if i > 0 {
                    p.line_segment([pos2(cx, r.top()), pos2(cx, r.center().y - 9.0)], Stroke::new(1.5, theme::graph_color(0)));
                }
                if i + 1 < n {
                    p.line_segment([pos2(cx, r.center().y + 9.0), pos2(cx, r.bottom())], Stroke::new(1.5, theme::graph_color(0)));
                }
                p.circle_stroke(pos2(cx, r.center().y), 9.0, Stroke::new(1.5, theme::graph_color(0)));
                p.text(pos2(cx, r.center().y), egui::Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(11.0), theme::TEXT());
                let x = r.left() + 42.0;
                let files: HashSet<&str> = c.changes.iter().map(|&k| f.changes[k].path.as_str()).collect();
                let count = format!("{} file{}", files.len(), if files.len() == 1 { "" } else { "s" });
                let cw = theme::text_width(ui, &count, &small);
                let max = r.right() - x - cw - 20.0;
                theme::paint_clipped(p, pos2(x, r.top() + 14.0), &c.title, font.clone(), theme::TEXT(), max);
                theme::paint_clipped(p, pos2(x, r.top() + 32.0), &c.description, small.clone(), theme::TEXT_DIM(), r.right() - x - 12.0);
                p.text(pos2(r.right() - 12.0, r.top() + 14.0), egui::Align2::RIGHT_CENTER, &count, small.clone(), theme::TEXT_DIM());
                let resp = resp.on_hover_text(if c.description.is_empty() { c.title.clone() } else { format!("{}\n\n{}", c.title, c.description) });
                if resp.clicked() && f.selected != i {
                    f.selected = i;
                    f.selected_file = None;
                }
            }
        });
        if left_out > 0 {
            let fr = Rect::from_min_max(pos2(rect.left(), rect.bottom() - foot_h), rect.max);
            theme::hline(ui, fr.top() + 0.5, fr.left(), fr.right());
            ui.painter().text(
                pos2(fr.left() + 14.0, fr.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{left_out} change{} not in the plan stay uncommitted", if left_out == 1 { "" } else { "s" }),
                small,
                theme::UNVERSIONED(),
            );
        }
    }

    fn split_details(ui: &mut Ui, f: &mut SplitForm, rect: Rect) {
        let Some(c) = f.commits.get_mut(f.selected) else { return };
        let inner = rect.shrink2(vec2(14.0, 10.0));
        let mut u = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        u.spacing_mut().item_spacing.y = 6.0;
        u.label(RichText::new(tr("Commit message")).color(theme::TEXT_DIM()).size(11.5));
        u.add(egui::TextEdit::singleline(&mut c.title).desired_width(inner.width()).font(FontId::proportional(13.5)));
        u.add(egui::TextEdit::multiline(&mut c.description).desired_width(inner.width()).desired_rows(2).hint_text(tr("Description")).text_color(theme::TEXT_MID()));
        u.add_space(4.0);
        // Files of the commit with how many of their changes it takes.
        let mut by_file: Vec<(String, Vec<usize>)> = vec![];
        for &k in &c.changes {
            let p = &f.changes[k].path;
            match by_file.iter_mut().find(|(x, _)| x == p) {
                Some((_, v)) => v.push(k),
                None => by_file.push((p.clone(), vec![k])),
            }
        }
        if f.selected_file.as_ref().map(|s| !by_file.iter().any(|(p, _)| p == s)).unwrap_or(true) {
            f.selected_file = by_file.first().map(|(p, _)| p.clone());
        }
        let font = FontId::proportional(13.0);
        let small = FontId::proportional(12.0);
        let w = inner.width();
        u.spacing_mut().item_spacing.y = 0.0;
        for (path, ks) in &by_file {
            let (r, resp) = u.allocate_exact_size(vec2(w, 22.0), Sense::click());
            let p = u.painter();
            if f.selected_file.as_deref() == Some(path.as_str()) {
                p.rect_filled(r, 4.0, theme::SELECT());
            } else if resp.hovered() {
                p.rect_filled(r, 4.0, theme::HOVER());
            }
            let cy = r.center().y;
            theme::paint_icon(p, Rect::from_center_size(pos2(r.left() + 12.0, cy), vec2(14.0, 14.0)), Icon::File, theme::TEXT_MID());
            let e = f.files.iter().find(|e| &e.path == path);
            let col = e.map(|e| status_color(e.kind())).unwrap_or(theme::TEXT());
            let t = p.text(pos2(r.left() + 26.0, cy), egui::Align2::LEFT_CENTER, file_name(path), font.clone(), col);
            let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let after = p.text(pos2(t.right() + 8.0, cy), egui::Align2::LEFT_CENTER, dir, small.clone(), theme::TEXT_DIM());
            let total = f.changes.iter().filter(|x| &x.path == path).count();
            let what = if f.changes[ks[0]].hash.is_none() {
                f.changes[ks[0]].kind.to_string()
            } else if ks.len() == total {
                format!("all {total} change{}", if total == 1 { "" } else { "s" })
            } else {
                format!("{} of {total} changes", ks.len())
            };
            let _ = after;
            p.text(pos2(r.right() - 8.0, cy), egui::Align2::RIGHT_CENTER, what, small.clone(), if ks.len() == total || total == 1 { theme::TEXT_DIM() } else { theme::STAR() });
            if resp.clicked() {
                f.selected_file = Some(path.clone());
            }
        }
        // The selected file's changes in this commit.
        let top = u.cursor().top() + 8.0;
        let pr = Rect::from_min_max(pos2(rect.left(), top), rect.max);
        theme::hline(ui, pr.top() + 0.5, pr.left(), pr.right());
        let Some((_, ks)) = by_file.iter().find(|(p, _)| Some(p) == f.selected_file.as_ref()) else { return };
        let mut pu = ui.new_child(egui::UiBuilder::new().max_rect(pr.shrink2(vec2(0.0, 1.0))));
        pu.set_clip_rect(pr);
        let mono = FontId::monospace(12.0);
        egui::ScrollArea::both().id_salt("split_preview").auto_shrink([false, false]).show(&mut pu, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let w = ui.available_width().max(600.0);
            for &k in ks {
                let ch = &f.changes[k];
                let label = if ch.line > 0 { format!("@@ line {}", ch.line) } else { format!("@@ {}", ch.kind) };
                let (r, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0, theme::rgb(0x22, 0x26, 0x2e));
                ui.painter().text(pos2(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, label, mono.clone(), theme::LINK());
                for (kind, t) in ch.lines.iter().take(400) {
                    let (r, _) = ui.allocate_exact_size(vec2(w, 17.0), Sense::hover());
                    let (bg, fg) = if *kind == '+' {
                        (theme::rgb(0x29, 0x3c, 0x2c), theme::rgb(0xb8, 0xe0, 0xbb))
                    } else {
                        (theme::rgb(0x48, 0x2b, 0x2d), theme::rgb(0xe8, 0xb4, 0xb4))
                    };
                    ui.painter().rect_filled(r, 0.0, bg);
                    ui.painter().text(pos2(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, format!("{kind} {}", t.replace('\t', "    ")), mono.clone(), fg);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: &str, hash: Option<u64>) -> Change {
        let path = id.split('#').next().unwrap().to_string();
        Change { id: id.into(), path, hash, line: 1, kind: "modified", lines: vec![] }
    }

    #[test]
    fn reads_the_plan() {
        let changes = vec![ch("a.py#1", Some(1)), ch("a.py#2", Some(2)), ch("b.md", None), ch("c.py#1", Some(3))];
        let text = "Here is the plan:\n**COMMIT:** [fix] One\nDESCRIPTION: first\nCHANGES: a.py#2, `b.md`\n\n2. COMMIT: [feat] Two\nCHANGES: a.py # 1, a.py#2, nope#1\nCOMMIT: Empty\nCHANGES: zzz\n";
        let plan = parse_plan(text, &changes);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].title, "[fix] One");
        assert_eq!(plan[0].description, "first");
        assert_eq!(plan[0].changes, vec![1, 2]);
        assert_eq!(plan[1].changes, vec![0]);
    }

    #[test]
    fn splits_one_file_between_commits() {
        let dir = std::env::temp_dir().join(format!("ge-split-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let who = [("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")];
        let g = |args: &[&str]| assert!(git::run_env(&dir, &who, args).ok, "{args:?}");
        g(&["init", "-q"]);
        let old: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("f.txt"), &old).unwrap();
        g(&["add", "f.txt"]);
        g(&["commit", "-qm", "base"]);
        let new = old.replace("line 2\n", "LINE TWO\n").replace("line 19\n", "LINE NINETEEN\n");
        std::fs::write(dir.join("f.txt"), &new).unwrap();
        std::fs::write(dir.join("n.txt"), "new\n").unwrap();
        let files = git::status(&dir);
        let changes = collect(&dir, &files, true);
        assert_eq!(changes.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["f.txt#1", "f.txt#2", "n.txt"]);
        let plan = parse_plan("COMMIT: second change\nCHANGES: f.txt#2, n.txt\nCOMMIT: first change\nCHANGES: f.txt#1\n", &changes);
        let cmds = commit_commands(&dir, &files, &changes, &plan, true, false).unwrap();
        for c in &cmds {
            let env: Vec<(&str, &str)> = c.iter().map_while(|s| s.strip_prefix("env:").and_then(|x| x.split_once('='))).chain(who).collect();
            let skip = c.iter().take_while(|s| s.starts_with("env:")).count();
            let args: Vec<&str> = c.iter().skip(skip).map(|s| s.as_str()).collect();
            assert!(git::run_env(&dir, &env, &args).ok, "{args:?}");
        }
        let log = git::run(&dir, &["log", "--format=%s"]).stdout;
        assert_eq!(log, "first change\nsecond change\nbase\n");
        let first = String::from_utf8(git::blob(&dir, "HEAD~1", "f.txt").unwrap()).unwrap();
        assert_eq!(first, old.replace("line 19\n", "LINE NINETEEN\n"));
        assert_eq!(git::blob(&dir, "HEAD", "f.txt").unwrap(), new.as_bytes());
        assert!(git::run(&dir, &["status", "--porcelain"]).stdout.trim().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
