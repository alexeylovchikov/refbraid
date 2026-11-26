//! Resolving merge conflicts: the Conflicts window, the three-column merge editor
//! (yours · result · theirs) and resolving with an AI agent (rules from Settings or a skill).

use crate::ai;
use crate::i18n::tr;
use crate::app::{App, Dialog, Job, Toast, file_name};
use crate::git::Operation;
use crate::merge::{self, MergeDoc, Method, Rules, Seg, line_count};
use crate::find::Matcher;
use crate::skills;
use crate::syntax;
use crate::theme::{self, Icon};
use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::time::Instant;

pub(crate) const LH: f32 = 18.0;
const GUT: f32 = 34.0;
const NUM: f32 = 38.0;
const C_CONFLICT: Color32 = Color32::from_rgb(0x4a, 0x2f, 0x30);
const C_RESOLVED: Color32 = Color32::from_rgb(0x2a, 0x45, 0x36);
const C_SIDE_DONE: Color32 = Color32::from_rgb(0x25, 0x2b, 0x29);

#[derive(Clone, Copy)]
enum Take {
    Ours,
    Theirs,
    Both,
    BothTheirsFirst,
    Neither,
    Reset,
}

fn take(c: &mut merge::Chunk, t: Take) {
    let join = |a: &str, b: &str| match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a}\n{b}"),
    };
    c.result = match t {
        Take::Reset => None,
        Take::Neither => Some(String::new()),
        Take::Both => Some(join(&c.ours, &c.theirs)),
        Take::BothTheirsFirst => Some(join(&c.theirs, &c.ours)),
        // The arrows add a side to what is already there (like the IDE's merge window).
        Take::Ours | Take::Theirs => {
            let side = if matches!(t, Take::Ours) { &c.ours } else { &c.theirs };
            match &c.result {
                Some(r) if !r.is_empty() && r != side && !r.contains(side.as_str()) => Some(join(r, side)),
                _ => Some(side.clone()),
            }
        }
    };
}

/// A found piece of text to mark: (line of the block, from, to, the current one), in characters.
pub(crate) type Mark = (usize, usize, usize, bool);

/// How a block of text is drawn: the file it belongs to (its language; empty = no colouring),
/// what the lines above left open, and the found text.
#[derive(Clone, Copy)]
pub(crate) struct Look<'a> {
    pub path: &'a str,
    pub st: syntax::State,
    pub marks: &'a [Mark],
}

type JobKey<'a> = (&'a str, Color32, &'a str, syntax::State, &'a [Mark]);

/// Lays a block of code out for drawing: syntax colours and the marks of found text.
#[derive(Default)]
struct JobMaker;

impl egui::cache::ComputerMut<JobKey<'_>, egui::text::LayoutJob> for JobMaker {
    fn compute(&mut self, (text, base, path, st, marks): JobKey<'_>) -> egui::text::LayoutJob {
        code_job(text, base, path, st, marks)
    }
}

/// What a block of code leaves open for the lines after it (a comment, a long string).
#[derive(Default)]
struct EndState;

impl egui::cache::ComputerMut<(&str, &str, syntax::State), syntax::State> for EndState {
    fn compute(&mut self, (text, path, mut st): (&str, &str, syntax::State)) -> syntax::State {
        if let Some(lang) = syntax::lang_for(path) {
            for l in text.split('\n') {
                syntax::line(lang, l, &mut st);
            }
        }
        st
    }
}

fn end_state(ctx: &egui::Context, text: &str, path: &str, st: syntax::State) -> syntax::State {
    if path.is_empty() {
        return st;
    }
    ctx.memory_mut(|m| m.caches.cache::<egui::cache::FrameCache<syntax::State, EndState>>().get((text, path, st)).clone())
}

pub(crate) fn cached_job(ctx: &egui::Context, text: &str, base: Color32, look: Look) -> egui::text::LayoutJob {
    ctx.memory_mut(|m| m.caches.cache::<egui::cache::FrameCache<egui::text::LayoutJob, JobMaker>>().get((text, base, look.path, look.st, look.marks)).clone())
}

fn code_job(text: &str, base: Color32, path: &str, mut st: syntax::State, marks: &[Mark]) -> egui::text::LayoutJob {
    let font = FontId::monospace(13.0);
    let fmt = |color: Color32, background: Color32| egui::TextFormat { font_id: font.clone(), color, background, line_height: Some(LH), ..Default::default() };
    let lang = if path.is_empty() { None } else { syntax::lang_for(path) };
    if text.is_empty() || (lang.is_none() && marks.is_empty()) {
        let mut job = egui::text::LayoutJob::simple(text.to_owned(), font.clone(), base, f32::INFINITY);
        for s in &mut job.sections {
            s.format.line_height = Some(LH);
        }
        return job;
    }
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut mi = 0;
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            job.append("\n", 0.0, fmt(base, Color32::TRANSPARENT));
        }
        let spans = lang.map(|l| syntax::line(l, line, &mut st)).unwrap_or_default();
        while mi < marks.len() && marks[mi].0 < n {
            mi += 1;
        }
        let byte = |col: usize| line.char_indices().nth(col).map(|(b, _)| b).unwrap_or(line.len());
        let mut found: Vec<(usize, usize, bool)> = vec![];
        while mi < marks.len() && marks[mi].0 == n {
            found.push((byte(marks[mi].1), byte(marks[mi].2), marks[mi].3));
            mi += 1;
        }
        if spans.is_empty() && found.is_empty() {
            job.append(line, 0.0, fmt(base, Color32::TRANSPARENT));
            continue;
        }
        // Every place where the colour or the background changes.
        let mut cuts: Vec<usize> = vec![0, line.len()];
        cuts.extend(spans.iter().flat_map(|(r, _)| [r.start, r.end]));
        cuts.extend(found.iter().flat_map(|&(a, b, _)| [a, b]));
        cuts.retain(|&c| c <= line.len() && line.is_char_boundary(c));
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let color = spans.iter().find(|(r, _)| r.start <= a && b <= r.end).map(|(_, k)| theme::syntax_color(*k)).unwrap_or(base);
            let bg = match found.iter().find(|&&(x, y, _)| x <= a && b <= y) {
                Some(&(_, _, true)) => theme::FIND_CURRENT(),
                Some(_) => theme::FIND(),
                None => Color32::TRANSPARENT,
            };
            job.append(&line[a..b], 0.0, fmt(if bg == Color32::TRANSPARENT { color } else { theme::rgb(255, 255, 255) }, bg));
        }
    }
    job
}

/// A place where the searched text was found in the merge window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MHit {
    /// The block (index into the document's segments; 0 for the whole-file view).
    pub seg: usize,
    /// 0 = yours, 1 = the result, 2 = theirs.
    pub col: u8,
    /// Line of the block, and the characters of the line.
    pub line: usize,
    pub from: usize,
    pub to: usize,
}

/// Every place the text occurs in the three columns, top to bottom. Lines both sides agree on
/// are counted once, in the result.
pub fn merge_hits(doc: &MergeDoc, m: &Matcher) -> Vec<MHit> {
    let mut out = vec![];
    let mut block = |seg: usize, texts: [Option<&str>; 3]| {
        let at = out.len();
        for (col, t) in texts.iter().enumerate() {
            if let Some(t) = t {
                out.extend(m.in_text(t).into_iter().map(|(line, from, to)| MHit { seg, col: col as u8, line, from, to }));
            }
        }
        out[at..].sort_by_key(|h| (h.line, h.col, h.from));
    };
    if let Some(whole) = &doc.whole {
        block(0, [Some(doc.ours_file.trim_end_matches('\n')), Some(whole), Some(doc.theirs_file.trim_end_matches('\n'))]);
        return out;
    }
    for (si, seg) in doc.segs.iter().enumerate() {
        match seg {
            Seg::Same { text, .. } => block(si, [None, Some(text), None]),
            Seg::Conflict(c) => block(si, [Some(&c.ours), c.result.as_deref(), Some(&c.theirs)]),
        }
    }
    out
}

/// The search of the merge window while it is drawn.
#[derive(Clone, Copy)]
struct Search<'a> {
    m: &'a Matcher,
    hits: &'a [MHit],
    current: usize,
    /// Bring the current hit into view.
    go: bool,
}

impl Search<'_> {
    /// The marks of one column of a block.
    fn marks(&self, seg: usize, col: u8) -> Vec<Mark> {
        let from = self.hits.partition_point(|h| h.seg < seg);
        self.hits[from..].iter().enumerate().take_while(|(_, h)| h.seg == seg).filter(|(_, h)| h.col == col).map(|(k, h)| (h.line, h.from, h.to, from + k == self.current)).collect()
    }

    /// The marks of a side's copy of lines both sides agree on (never the current one).
    fn marks_in(&self, text: &str) -> Vec<Mark> {
        self.m.in_text(text).into_iter().map(|(l, a, b)| (l, a, b, false)).collect()
    }
}

fn take_menu(ui: &mut Ui, out: &mut Option<Take>) {
    for (label, t) in [
        ("Take Yours", Take::Ours),
        ("Take Theirs", Take::Theirs),
        ("Take Both (yours first)", Take::Both),
        ("Take Both (theirs first)", Take::BothTheirsFirst),
        ("Take Neither", Take::Neither),
    ] {
        if ui.button(label).clicked() {
            *out = Some(t);
        }
    }
    ui.separator();
    if ui.button(tr("Reset to Unresolved")).clicked() {
        *out = Some(Take::Reset);
    }
}

/// One side's text (yours or theirs): line numbers and read-only text that can be selected and
/// copied. A right-click offers Copy, and for a conflict block the ways to take it.
fn side_view(ui: &mut Ui, col: Rect, text: &str, first_no: usize, color: Color32, id: egui::Id, mut menu: Option<&mut Option<Take>>, hs: Hs, look: Look) {
    let clip = col.intersect(ui.clip_rect());
    if !clip.is_positive() {
        return;
    }
    paint_numbers(ui, col, line_count(text).max(1), first_no);
    let rect = Rect::from_min_max(pos2(col.left() + NUM, col.top()), col.max);
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _w: f32| {
        let job = cached_job(ui.ctx(), buf.as_str(), color, look);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let wide = hs.wide(rect);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(wide));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let mut buf: &str = text;
    let r = child.add(
        egui::TextEdit::multiline(&mut buf)
            .id(id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_rows(1)
            .desired_width(wide.width())
            .min_size(wide.size())
            .layouter(&mut layouter),
    );
    let rid = r.id;
    r.context_menu(|ui| {
        if ui.button(tr("Copy")).clicked() {
            let has_sel = egui::TextEdit::load_state(ui.ctx(), rid).and_then(|s| s.cursor.char_range()).is_some_and(|c| !c.is_empty());
            if has_sel {
                ui.ctx().memory_mut(|m| m.request_focus(rid));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestCopy);
            } else {
                ui.ctx().copy_text(text.to_string());
            }
            ui.close();
        }
        if let Some(out) = menu.as_deref_mut() {
            ui.separator();
            take_menu(ui, out);
        }
    });
}

fn paint_numbers(ui: &Ui, col: Rect, n: usize, first_no: usize) {
    let clip = col.intersect(ui.clip_rect());
    if !clip.is_positive() {
        return;
    }
    let p = ui.painter().with_clip_rect(clip);
    for k in 0..n {
        let y = col.top() + k as f32 * LH + LH / 2.0;
        if y < clip.top() - LH || y > clip.bottom() + LH {
            continue;
        }
        p.text(pos2(col.left() + NUM - 8.0, y), egui::Align2::RIGHT_CENTER, (first_no + k).to_string(), FontId::monospace(11.5), theme::TEXT_DIM());
    }
}

/// An editable block of code without wrapping, one line every LH points. Returns true when edited.
fn code_edit(ui: &mut Ui, rect: Rect, text: &mut String, id: egui::Id, hint: &str, hs: Hs, look: Look) -> bool {
    let clip = rect.intersect(ui.clip_rect());
    if !clip.is_positive() {
        return false;
    }
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _w: f32| {
        let job = cached_job(ui.ctx(), buf.as_str(), theme::TEXT(), look);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let wide = hs.wide(rect);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(wide));
    child.set_clip_rect(clip);
    let r = child.add(
        egui::TextEdit::multiline(text)
            .id(id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_rows(1)
            .desired_width(wide.width())
            .min_size(wide.size())
            .hint_text(RichText::new(hint).color(theme::TEXT_DIM()).monospace())
            .layouter(&mut layouter),
    );
    theme::edit_menu(r).changed()
}

fn divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 20.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, theme::rgb(0x4e, 0x52, 0x58)));
}

impl App {
    fn repo_key_string(&self) -> Option<String> {
        self.repo.as_ref().map(|r| r.path.display().to_string())
    }

    /// Look for skills again (repository, personal and added folders).
    pub fn refresh_skills(&mut self) {
        let repo = self.repo.as_ref().map(|r| r.path.clone());
        self.skills = skills::find(repo.as_deref(), !self.settings.no_personal_skills, &self.settings.skill_dirs);
        self.skills_for = repo;
    }

    fn ensure_skills(&mut self) {
        let repo = self.repo.as_ref().map(|r| r.path.clone());
        if self.skills_for != repo {
            self.refresh_skills();
        }
    }

    /// How this repository resolves conflicts with AI, and why it was chosen.
    pub fn conflict_method(&self) -> (Method, &'static str) {
        if let Some(m) = self
            .repo_key_string()
            .and_then(|k| self.settings.conflict_method.get(&k).cloned())
            .and_then(|k| Method::from_key(&k))
        {
            match &m {
                Method::Skill(p) if !p.is_file() => {}
                _ => return (m, "remembered for this repository"),
            }
        }
        if let Some(s) = self.skills.iter().find(|s| s.project && s.about_conflicts) {
            return (Method::Skill(s.file.clone()), "chosen automatically: a conflict skill in this repository");
        }
        (Method::Prompt, "")
    }

    pub fn method_label(&self, m: &Method) -> String {
        match m {
            Method::Prompt => tr("Rules from Settings").into(),
            Method::Skill(p) => {
                let name = self
                    .skills
                    .iter()
                    .find(|s| &s.file == p)
                    .map(|s| s.name.clone())
                    .or_else(|| p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()))
                    .unwrap_or_else(|| "skill".into());
                format!("Skill: {name}")
            }
        }
    }

    fn remember_method(&mut self, m: &Method) {
        let Some(k) = self.repo_key_string() else { return };
        self.settings.conflict_method.insert(k, m.key());
        if self.screenshot.is_none() {
            self.settings.save();
        }
    }

    /// The "AI uses" drop-down: the rules from Settings or one of the skills found.
    fn method_picker(&mut self, ui: &mut Ui, width: f32) {
        let (cur, why) = self.conflict_method();
        let mut pick: Option<Method> = None;
        let mut to_settings = false;
        let mut again = false;
        egui::ComboBox::from_id_salt("conflict_method")
            .width(width)
            .selected_text(self.method_label(&cur))
            .show_ui(ui, |ui| {
                ui.set_min_width(360.0);
                if ui.selectable_label(cur == Method::Prompt, tr("Rules from Settings")).clicked() {
                    pick = Some(Method::Prompt);
                }
                if self.skills.is_empty() {
                    ui.label(RichText::new(tr("No skills found (.claude/skills/<name>/SKILL.md, …)")).color(theme::TEXT_DIM()).size(12.0));
                }
                let mut last_group = None;
                for s in &self.skills {
                    let group = if s.about_conflicts { "Skills about conflicts" } else { "Other skills" };
                    if last_group != Some(group) {
                        ui.label(RichText::new(group).color(theme::TEXT_DIM()).size(11.5));
                        last_group = Some(group);
                    }
                    let m = Method::Skill(s.file.clone());
                    let text = format!("{}   ·  {}", s.name, s.source);
                    let r = ui.selectable_label(cur == m, text).on_hover_text(if s.description.is_empty() {
                        s.file.display().to_string()
                    } else {
                        format!("{}\n\n{}", s.description, s.file.display())
                    });
                    if r.clicked() {
                        pick = Some(m);
                    }
                }
                ui.separator();
                if ui.button(tr("Search for skills again")).clicked() {
                    again = true;
                }
                if ui.button(tr("Edit rules and skill folders…")).clicked() {
                    to_settings = true;
                }
            });
        if !why.is_empty() {
            ui.label(RichText::new(format!("({why})")).color(theme::TEXT_DIM()).size(11.5));
        }
        if let Some(m) = pick {
            self.remember_method(&m);
        }
        if again {
            self.refresh_skills();
        }
        if to_settings {
            self.open_settings(Some(crate::settings_view::Page::MergeConflicts));
        }
    }

    pub fn conflicted_files(&self) -> Vec<crate::git::StatusEntry> {
        self.repo.as_ref().map(|r| r.status.iter().filter(|e| e.is_conflict()).cloned().collect()).unwrap_or_default()
    }

    /// Open the merge editor for a file (or switch to it).
    pub fn open_merge(&mut self, path: &str, show: bool) -> Option<usize> {
        self.ensure_skills();
        let i = match self.merges.iter().position(|d| d.path == path) {
            Some(i) => i,
            None => {
                let repo = self.repo.as_ref()?;
                self.merges.push(MergeDoc::load(&repo.path, path));
                self.merges.len() - 1
            }
        };
        if show {
            self.merge_open = Some(path.to_string());
            self.merge_confirm = None;
        }
        Some(i)
    }

    pub fn close_merge(&mut self, i: usize) {
        if i < self.merges.len() {
            let path = self.merges[i].path.clone();
            self.conflict_ai.retain(|(p, job)| {
                if *p == path {
                    ai::cancel(job);
                }
                *p != path
            });
            self.merges.remove(i);
        }
    }

    pub fn ai_running_for(&self, path: &str) -> bool {
        self.conflict_ai.iter().any(|(p, _)| p == path)
    }

    /// Ask the AI agent to resolve the conflicts of one file; the answer fills the merge editor.
    pub fn resolve_with_ai(&mut self, path: &str, show: bool) {
        let Some(h) = ai::harness(&self.settings.agent) else {
            self.open_settings(Some(crate::settings_view::Page::AiAgents));
            return;
        };
        let Some((_, program)) = self.agents.iter().find(|(id, _)| *id == h.id).cloned() else {
            self.open_settings(Some(crate::settings_view::Page::AiAgents));
            return;
        };
        if self.ai_running_for(path) {
            return;
        }
        let Some(i) = self.open_merge(path, show) else { return };
        if self.merges[i].no_text {
            self.merges[i].note = Some(("This file can't be merged line by line; choose Accept Yours or Accept Theirs.".into(), true));
            return;
        }
        let (method, _) = self.conflict_method();
        let rules = match &method {
            Method::Prompt => Rules::Prompt(self.settings.conflict_prompt.clone()),
            Method::Skill(file) => Rules::Skill {
                name: self.method_label(&method).trim_start_matches("Skill: ").to_string(),
                file: file.clone(),
                body: skills::body(file),
            },
        };
        let Some(repo) = self.repo.as_ref() else { return };
        let branch = repo.state.head_branch.clone().unwrap_or_else(|| "HEAD".into());
        let prompt = merge::build_prompt(&self.merges[i], &rules, repo.state.operation, &branch);
        let job = ai::start_with(h, &program, &repo.path, prompt, merge::clean_answer);
        self.merges[i].note = Some((format!("{} is resolving the conflicts ({})…", h.name, self.method_label(&method)), false));
        self.conflict_ai.push((path.to_string(), job));
        self.remember_method(&method);
    }

    pub fn poll_conflict_ai(&mut self) {
        if self.conflict_ai.is_empty() {
            return;
        }
        self.ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let mut done = vec![];
        for (k, (p, job)) in self.conflict_ai.iter().enumerate() {
            if let Ok(res) = job.rx.try_recv() {
                done.push((k, p.clone(), job.agent.clone(), res));
            }
        }
        for (k, ..) in done.iter().rev() {
            self.conflict_ai.remove(*k);
        }
        for (_, path, agent, res) in done {
            let Some(i) = self.open_merge(&path, false) else { continue };
            let doc = &mut self.merges[i];
            let (note, ok) = match res {
                Ok(text) if merge::has_markers(&text) => (format!("{agent} left conflict markers in its answer; nothing was changed."), false),
                Ok(text) => {
                    let n = doc.conflicts();
                    if doc.apply_ai(&text) {
                        (format!("{agent} filled in {n} conflict{} — review the result, edit it if needed and press Apply.", if n == 1 { "" } else { "s" }), true)
                    } else {
                        (format!("{agent} also changed lines outside the conflicts, so its whole answer is shown in the middle — review it and press Apply."), true)
                    }
                }
                Err(e) => (format!("{agent} could not resolve the conflicts: {}", e.lines().next().unwrap_or("")), false),
            };
            doc.note = Some((note.clone(), !ok));
            self.notify(Toast {
                title: if ok { format!("Conflicts resolved in {}", file_name(&path)) } else { format!("{} not resolved", file_name(&path)) },
                text: note,
                ok,
                at: Instant::now(),
            });
        }
    }

    /// Take one side for the whole file and mark it resolved.
    pub fn accept_side(&mut self, path: &str, ours: bool) {
        let side = if ours { "--ours" } else { "--theirs" };
        let deleted = self
            .repo
            .as_ref()
            .and_then(|r| r.status.iter().find(|e| e.path == path).cloned())
            .map(|e| if ours { e.x == 'D' } else { e.y == 'D' })
            .unwrap_or(false);
        let cmds = if deleted {
            vec![vec!["rm".into(), "--".into(), path.to_string()]]
        } else {
            vec![
                vec!["checkout".into(), side.into(), "--".into(), path.to_string()],
                vec!["add".into(), "--".into(), path.to_string()],
            ]
        };
        self.run_job(Job::many(if ours { "Accept yours" } else { "Accept theirs" }, cmds));
        self.mark_resolved(path);
        if let Some(i) = self.merges.iter().position(|d| d.path == path) {
            self.close_merge(i);
        }
    }

    /// Conflicted files not resolved yet (a file just resolved can still be listed by git until
    /// the status is read again).
    pub fn unresolved_files(&self) -> Vec<crate::git::StatusEntry> {
        self.conflicted_files().into_iter().filter(|f| !self.conflicts_done.iter().any(|d| d.path == f.path)).collect()
    }

    /// (conflicts, simple ones) of a file: from its open merge document, or worked out once.
    fn conflict_info_of(&self, path: &str) -> Option<(usize, usize)> {
        if let Some(d) = self.merges.iter().find(|d| d.path == path) {
            return Some(if d.no_text { (0, 0) } else { (d.conflicts(), d.simple_count()) });
        }
        self.conflict_info.get(path).copied()
    }

    /// Read one more file's conflicts (one per frame, so a long list doesn't stall the window).
    fn fill_conflict_info(&mut self, files: &[crate::git::StatusEntry]) {
        let Some(f) = files.iter().find(|f| self.conflict_info_of(&f.path).is_none()) else { return };
        let Some(repo) = self.repo.as_ref() else { return };
        let d = MergeDoc::load(&repo.path, &f.path);
        let v = if d.no_text { (0, 0) } else { (d.conflicts(), d.simple_count()) };
        if !d.theirs_label.is_empty() {
            self.conflict_theirs = d.theirs_label.clone();
        }
        self.conflict_info.insert(f.path.clone(), v);
        self.ctx.request_repaint();
    }

    /// Put a file under "Resolved" in the Conflicts window.
    fn mark_resolved(&mut self, path: &str) {
        if self.conflicts_done.iter().any(|d| d.path == path) {
            return;
        }
        let e = self.conflicted_files().into_iter().find(|f| f.path == path);
        let n = self.conflict_info_of(path).map(|x| x.0).unwrap_or(0);
        self.conflicts_done.push(DoneFile { path: path.to_string(), x: e.as_ref().map_or('M', |e| e.x), y: e.as_ref().map_or('M', |e| e.y), conflicts: n });
        self.conflict_info.remove(path);
    }

    /// After a file is resolved in the merge window: the next unresolved one opens, or the
    /// Conflicts window comes back when none is left.
    fn open_next_conflict(&mut self, after: &str) {
        let next = self.unresolved_files().into_iter().map(|f| f.path).find(|p| p != after);
        match next {
            Some(p) => {
                self.open_merge(&p, true);
            }
            None => {
                self.merge_open = None;
                self.show_conflicts = true;
            }
        }
    }

    /// Write the merge editor's result to the file and forget the document; the caller adds
    /// the file to git (one job for several files keeps git's index lock free).
    fn save_merge(&mut self, i: usize) -> Option<String> {
        let repo = self.repo.as_ref()?;
        let doc = &self.merges[i];
        let path = doc.path.clone();
        if let Err(e) = std::fs::write(repo.path.join(&path), doc.result_text()) {
            self.dialog = Some(Dialog::Message { title: tr("Merge").into(), text: format!("Could not save {path}: {e}") });
            return None;
        }
        self.mark_resolved(&path);
        self.close_merge(i);
        Some(path)
    }

    /// Resolve every simple conflict of every unresolved file; files left with no conflicts
    /// are saved and marked resolved.
    fn resolve_all_simple(&mut self) {
        let mut fixed = 0;
        let mut saved: Vec<String> = vec![];
        for f in self.unresolved_files() {
            if self.conflict_info_of(&f.path).is_some_and(|x| x.1 == 0) {
                continue;
            }
            let Some(i) = self.open_merge(&f.path, false) else { continue };
            let n = self.merges[i].resolve_simple();
            fixed += n;
            if n > 0 && self.merges[i].unresolved() == 0 {
                if let Some(p) = self.save_merge(i) {
                    saved.push(p);
                }
            }
        }
        if !saved.is_empty() {
            let mut add = vec!["add".to_string(), "--".to_string()];
            add.extend(saved.iter().cloned());
            self.run_job(Job::many("Resolve simple conflicts", vec![add]));
        }
        self.notify(Toast {
            title: tr("Simple conflicts").to_string(),
            text: format!("{fixed} conflict(s) resolved; {} file(s) fully resolved.", saved.len()),
            ok: true,
            at: Instant::now(),
        });
    }

    /// "Accept and Finish": go on with the merge, rebase, cherry-pick or revert.
    fn finish_conflicts(&mut self) {
        self.show_conflicts = false;
        let Some(op) = self.repo.as_ref().and_then(|r| r.state.operation) else { return };
        let cmd: Vec<String> = match op {
            Operation::Merge => vec!["commit".into(), "--no-edit".into()],
            o => vec!["env:GIT_EDITOR=true".into(), o.command().into(), "--continue".into()],
        };
        self.run_job(Job::many(&format!("Continue {}", op.title()), vec![cmd]));
    }

    /// Opens the Conflicts window when a merge (or rebase…) stops with conflicts, and forgets
    /// the resolved list once the operation is over.
    pub fn watch_conflicts(&mut self) {
        let has = self.repo.as_ref().is_some_and(|r| r.status.iter().any(|e| e.is_conflict()));
        if has && !self.had_conflicts {
            self.conflict_info.clear();
            self.conflict_theirs.clear();
            self.conflicts_done.clear();
            if self.screenshot.is_none() {
                self.show_conflicts = true;
            }
        }
        if !has && self.repo.as_ref().is_none_or(|r| r.state.operation.is_none()) {
            self.conflicts_done.clear();
        }
        self.had_conflicts = has;
    }

    /// A strip above the Log while conflicts wait and their windows are closed.
    pub fn conflict_banner(&mut self, ui: &mut Ui) {
        if self.show_conflicts || self.merge_open.is_some() {
            return;
        }
        let n = self.unresolved_files().len();
        if n == 0 {
            return;
        }
        let op = self.repo.as_ref().and_then(|r| r.state.operation);
        let mut open = false;
        let mut abort = None;
        egui::Panel::top("conflict_banner")
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::rgb(0x3d, 0x32, 0x23)).inner_margin(egui::Margin::symmetric(12, 6)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let what = op.map(|o| crate::i18n::trf("{} is stopped by conflicts.", &[&tr(o.title())])).unwrap_or_else(|| tr("There are conflicts.").to_string());
                    ui.label(RichText::new(format!("⚠  {what} {}", crate::i18n::trf("Resolve them to continue: {} file(s) left.", &[&n]))).color(theme::STAR()));
                    if ui.button(tr("Resolve Conflicts…")).clicked() {
                        open = true;
                    }
                    if let Some(o) = op {
                        if ui.button(tr("Abort")).clicked() {
                            abort = Some(o);
                        }
                    }
                });
            });
        if open {
            self.show_conflicts = true;
        }
        if let Some(o) = abort {
            self.run_job(Job::one(&format!("Abort {}", o.title()), &[o.command(), "--abort"]));
        }
    }

    /// The Conflicts window: unresolved and resolved files, "Accept" for either side, and
    /// "Resolve Manually" for the merge window.
    pub fn conflicts_window(&mut self, ctx: &egui::Context) {
        if !self.show_conflicts || self.merge_open.is_some() {
            return;
        }
        self.ensure_skills();
        let open = self.unresolved_files();
        let done = self.conflicts_done.clone();
        if open.is_empty() && done.is_empty() && self.screenshot.is_none() {
            self.show_conflicts = false;
            return;
        }
        self.fill_conflict_info(&open);
        if self.conflicts_sel.as_ref().is_none_or(|s| !open.iter().any(|f| &f.path == s) && !done.iter().any(|d| &d.path == s)) {
            self.conflicts_sel = open.first().map(|f| f.path.clone()).or_else(|| done.first().map(|d| d.path.clone()));
        }
        let head = self.repo.as_ref().and_then(|r| r.state.head_branch.clone()).unwrap_or_else(|| "HEAD".into());
        let theirs = self.merges.iter().find(|d| !d.theirs_label.is_empty()).map(|d| d.theirs_label.clone()).or_else(|| (!self.conflict_theirs.is_empty()).then(|| self.conflict_theirs.clone()));
        let simple: usize = open.iter().filter_map(|f| self.conflict_info_of(&f.path)).map(|x| x.1).sum();
        let info: Vec<(usize, usize)> = open.iter().map(|f| self.conflict_info_of(&f.path).unwrap_or((0, 0))).collect();
        let ai_running: Vec<bool> = open.iter().map(|f| self.ai_running_for(&f.path)).collect();
        let sel = self.conflicts_sel.clone();
        let sel_open = sel.as_ref().is_some_and(|s| open.iter().any(|f| &f.path == s));
        #[derive(PartialEq)]
        enum Do {
            Close,
            Finish,
            Manual(String),
            Accept(String, bool),
            Simple,
            AiAll,
            External(String, crate::settings::ExtTool),
            MarkResolved(String),
        }
        let ext_merge: Vec<crate::settings::ExtTool> = if self.settings.ext_enabled {
            self.settings.ext_tools.iter().filter(|t| t.kind == crate::settings::ExtKind::Merge).cloned().collect()
        } else {
            vec![]
        };
        let mut todo: Vec<Do> = vec![];
        let fold_id = egui::Id::unique("conflicts_fold");
        let mut fold: [bool; 2] = ctx.data(|d| d.get_temp(fold_id)).unwrap_or_default();
        let closed = crate::form::show(ctx, "conflicts_window", "Conflicts", 880.0, |ui, part| {
            if part == crate::form::Part::Footer {
                if open.is_empty() {
                    // Everything resolved: finishing is the main action.
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if crate::form::primary(ui, tr("Accept and Finish"), true, enter) {
                        todo.push(Do::Finish);
                    }
                } else {
                    if crate::form::primary(ui, tr("Resolve Manually"), sel_open, false) {
                        if let Some(s) = &sel {
                            todo.push(Do::Manual(s.clone()));
                        }
                    }
                    let b = egui::Button::new(tr("Accept and Finish")).min_size(vec2(84.0, 28.0));
                    if ui.add_enabled(false, b).on_disabled_hover_text(tr("Resolve every file first")).clicked() {
                        todo.push(Do::Finish);
                    }
                }
                if crate::form::button(ui, tr("Close")) {
                    todo.push(Do::Close);
                }
                return;
            }
            // What is going on.
            ui.horizontal(|ui| {
                if open.is_empty() {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 7.5, theme::GREEN());
                    theme::paint_icon(ui.painter(), r.shrink(4.0), Icon::Check, theme::BG());
                    ui.label(RichText::new(tr("All conflicts have been resolved")).color(theme::TEXT()));
                } else {
                    let t = if open.len() == 1 { tr("The following file has conflicts:") } else { tr("The following files have conflicts:") };
                    ui.label(RichText::new(t).color(theme::TEXT()));
                }
            });
            ui.add_space(8.0);
            // Toolbar.
            ui.horizontal(|ui| {
                let b = ui.add_enabled(simple > 0, egui::Button::new(tr("Resolve All Simple Conflicts")));
                let b = if simple > 0 {
                    b.on_hover_text(crate::i18n::trf("{} conflict(s) where the two sides changed different lines (or only one side changed them) can be resolved without you", &[&simple]))
                } else {
                    b.on_disabled_hover_text(tr("No conflict can be resolved without you: both sides changed the same lines"))
                };
                if b.clicked() {
                    todo.push(Do::Simple);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = self.agent_ready();
                    let b = ui.add_enabled(!open.is_empty(), egui::Button::new(RichText::new(format!("      {}", tr("Resolve All with AI"))).color(theme::TEXT())));
                    theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(b.rect.left() + 14.0, b.rect.center().y), vec2(14.0, 14.0)), Icon::Spiral, if ready { theme::LINK() } else { theme::TEXT_DIM() });
                    if b.on_hover_text(tr("The AI agent fills in the merge window of every file; nothing is saved until you apply it")).clicked() {
                        todo.push(Do::AiAll);
                    }
                    self.method_picker(ui, 200.0);
                });
            });
            ui.add_space(6.0);
            // The files.
            let w = ui.available_width();
            let (cy_, ct_) = (w - 330.0, w - 165.0);
            egui::Frame::new().stroke(Stroke::new(1.0, theme::BORDER())).show(ui, |ui| {
                ui.set_width(w - 2.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (hr, _) = ui.allocate_exact_size(vec2(w - 2.0, 26.0), Sense::hover());
                theme::hline(ui, hr.bottom() - 0.5, hr.left(), hr.right());
                for (x, t) in [(cy_, format!("{} ({head})", tr("Yours"))), (ct_, theirs.clone().map(|t| format!("{} ({t})", tr("Theirs"))).unwrap_or_else(|| tr("Theirs").to_string()))] {
                    theme::vline(ui, hr.left() + x - 0.5, hr.top(), hr.bottom());
                    theme::paint_clipped(ui.painter(), pos2(hr.left() + x + 8.0, hr.center().y), &t, FontId::proportional(12.5), theme::TEXT_MID(), 150.0);
                }
                let h = 250.0 + crate::form::grow(ui);
                egui::ScrollArea::vertical().max_height(h).min_scrolled_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width(w - 2.0);
                    let groups: [(bool, usize); 2] = [(false, open.len()), (true, done.len())];
                    for (g, (resolved, n)) in groups.into_iter().enumerate() {
                        if n == 0 {
                            continue;
                        }
                        // Group row: ✗ Unresolved / ✓ Resolved, with the count.
                        let (r, resp) = ui.allocate_exact_size(vec2(w - 2.0, 26.0), Sense::click());
                        let cy = r.center().y;
                        theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 12.0, cy), vec2(11.0, 11.0)), if fold[g] { Icon::ChevronRight } else { Icon::ChevronDown }, theme::TEXT_MID());
                        theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 30.0, cy), vec2(12.0, 12.0)), if resolved { Icon::Check } else { Icon::Close }, if resolved { theme::GREEN() } else { theme::RED() });
                        let label = if resolved { tr("Resolved") } else { tr("Unresolved") };
                        let lw = theme::text_width(ui, label, &FontId::proportional(13.0));
                        ui.painter().text(pos2(r.left() + 44.0, cy), egui::Align2::LEFT_CENTER, label, FontId::proportional(13.0), theme::TEXT());
                        ui.painter().text(pos2(r.left() + 52.0 + lw, cy), egui::Align2::LEFT_CENTER, format!("{n} {}", crate::i18n::plural(n, "file", "files")), FontId::proportional(12.0), theme::TEXT_DIM());
                        if resp.clicked() {
                            fold[g] = !fold[g];
                        }
                        if fold[g] {
                            continue;
                        }
                        for k in 0..n {
                            let (path, x, y, badge) = if resolved {
                                let d = &done[k];
                                (d.path.clone(), d.x, d.y, (d.conflicts, d.conflicts))
                            } else {
                                let f = &open[k];
                                (f.path.clone(), f.x, f.y, (info[k].0 - info[k].0.min(self.merges.iter().find(|d| d.path == f.path).map_or(info[k].0, |d| d.unresolved())), info[k].0))
                            };
                            let (r, resp) = ui.allocate_exact_size(vec2(w - 2.0, 26.0), Sense::click());
                            let selected = sel.as_deref() == Some(path.as_str());
                            if selected {
                                ui.painter().rect_filled(r, 0.0, theme::SELECT());
                            } else if resp.hovered() {
                                ui.painter().rect_filled(r, 0.0, theme::HOVER());
                            }
                            let cy = r.center().y;
                            theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 52.0, cy), vec2(13.0, 13.0)), Icon::File, theme::TEXT_MID());
                            let name = file_name(&path);
                            let nf = FontId::proportional(13.0);
                            let nw = theme::text_width(ui, &name, &nf);
                            ui.painter().text(pos2(r.left() + 64.0, cy), egui::Align2::LEFT_CENTER, &name, nf, theme::TEXT());
                            let mut xx = r.left() + 72.0 + nw;
                            if badge.1 > 0 {
                                let t = format!("{}/{}", badge.0, badge.1);
                                let g = ui.painter().layout_no_wrap(t, FontId::proportional(11.5), if resolved { theme::TEXT() } else { theme::TEXT_MID() });
                                let br = Rect::from_min_size(pos2(xx, cy - 9.0), vec2(g.size().x + 12.0, 18.0));
                                ui.painter().rect_filled(br, 9.0, if resolved { theme::rgb(0x2f, 0x4a, 0x7a) } else { theme::rgb(0x3a, 0x3d, 0x42) });
                                ui.painter().galley(pos2(br.left() + 6.0, cy - g.size().y / 2.0), g, theme::TEXT());
                                xx = br.right() + 8.0;
                            }
                            let dir = path.rsplit_once('/').map(|x| x.0).unwrap_or("");
                            theme::paint_clipped(ui.painter(), pos2(xx, cy), dir, FontId::proportional(12.0), theme::TEXT_DIM(), (r.left() + cy_ - 10.0 - xx).max(0.0));
                            if !resolved && ai_running[k] {
                                egui::Spinner::new().size(12.0).paint_at(ui, Rect::from_center_size(pos2(r.left() + cy_ - 16.0, cy), vec2(12.0, 12.0)));
                            }
                            for (col_x, c, ours) in [(cy_, x, true), (ct_, y, false)] {
                                ui.painter().text(pos2(r.left() + col_x + 8.0, cy), egui::Align2::LEFT_CENTER, tr(side_word(c)), FontId::proportional(12.5), theme::TEXT());
                                // "Accept" takes that side for the whole file.
                                if !resolved && (selected || resp.hovered()) {
                                    let br = Rect::from_min_size(pos2(r.left() + col_x + 80.0, cy - 10.0), vec2(74.0, 20.0));
                                    let tip = if ours { tr("Keep your version of the whole file") } else { tr("Keep their version of the whole file") };
                                    if ui.put(br, egui::Button::new(RichText::new(tr("Accept")).size(12.5))).on_hover_text(tip).clicked() {
                                        todo.push(Do::Accept(path.clone(), ours));
                                    }
                                }
                            }
                            if resp.clicked() {
                                self.conflicts_sel = Some(path.clone());
                            }
                            if !resolved && resp.double_clicked() {
                                todo.push(Do::Manual(path.clone()));
                            }
                            if !resolved {
                                resp.context_menu(|ui| {
                                    if ui.button(tr("Resolve Manually")).clicked() {
                                        todo.push(Do::Manual(path.clone()));
                                    }
                                    for t in &ext_merge {
                                        if ui.button(crate::i18n::trf("Open in {}", &[&t.name])).clicked() {
                                            todo.push(Do::External(path.clone(), t.clone()));
                                        }
                                    }
                                    ui.separator();
                                    if ui.button(tr("Accept Yours")).clicked() {
                                        todo.push(Do::Accept(path.clone(), true));
                                    }
                                    if ui.button(tr("Accept Theirs")).clicked() {
                                        todo.push(Do::Accept(path.clone(), false));
                                    }
                                    ui.separator();
                                    if ui.button(tr("Mark as Resolved")).on_hover_text(tr("The file is already merged (e.g. in an external tool): add it to git as it is now")).clicked() {
                                        todo.push(Do::MarkResolved(path.clone()));
                                    }
                                });
                            }
                        }
                    }
                });
            });
        });
        ctx.data_mut(|d| d.insert_temp(fold_id, fold));
        if closed {
            todo.push(Do::Close);
        }
        for t in todo {
            match t {
                Do::Close => self.show_conflicts = false,
                Do::Finish => self.finish_conflicts(),
                Do::Manual(p) => {
                    self.conflicts_sel = Some(p.clone());
                    self.open_merge(&p, true);
                }
                Do::Accept(p, ours) => self.accept_side(&p, ours),
                Do::Simple => self.resolve_all_simple(),
                Do::External(p, t) => {
                    if let Some(i) = self.merges.iter().position(|d| d.path == p) {
                        self.close_merge(i);
                    }
                    self.external_merge_with(&p, &t);
                }
                Do::MarkResolved(p) => {
                    if let Some(i) = self.merges.iter().position(|d| d.path == p) {
                        self.close_merge(i);
                    }
                    self.mark_resolved(&p);
                    self.run_job(Job::many("Mark resolved", vec![vec!["add".into(), "--".into(), p]]));
                }
                Do::AiAll => {
                    let all: Vec<String> = self.unresolved_files().into_iter().map(|f| f.path).collect();
                    for p in &all {
                        self.resolve_with_ai(p, false);
                    }
                    if let Some(first) = all.first() {
                        self.open_merge(first, true);
                    }
                }
            }
        }
    }

    /// The merge window of one file: yours · result · theirs, the AI, and Apply.
    pub fn merge_window(&mut self, ctx: &egui::Context) {
        let Some(path) = self.merge_open.clone() else { return };
        let Some(i) = self.open_merge(&path, false) else {
            self.merge_open = None;
            return;
        };
        let running = self.ai_running_for(&path);
        let agent = self.agent_name();
        let ready = self.agent_ready();
        let full = self.repo.as_ref().map(|r| r.path.join(&path).display().to_string()).unwrap_or_else(|| path.clone());
        let screen = ctx.content_rect();
        let width = (screen.width() - 60.0).clamp(700.0, 1500.0);
        let height = (screen.height() - 230.0).clamp(240.0, 900.0);
        let title = format!("{} {full}", tr("Merge Revisions for"));
        let ext_tools: Vec<crate::settings::ExtTool> = if self.settings.ext_enabled {
            self.settings.ext_tools.iter().filter(|t| t.kind == crate::settings::ExtKind::Merge).cloned().collect()
        } else {
            vec![]
        };
        let ext_default = crate::external::tool_for(&self.settings, &path, crate::settings::ExtKind::Merge).cloned().or_else(|| ext_tools.first().cloned());
        let mut act: Option<MAct> = None;
        let closed = crate::form::show_titled(ctx, "merge_window", "Merge Revisions", &title, width, false, |ui, part| {
            if part == crate::form::Part::Footer {
                if crate::form::button(ui, tr("Save and Close")) {
                    act = Some(MAct::SaveClose);
                }
                let no_text = self.merges[i].no_text;
                if crate::form::primary(ui, tr("Apply Changes"), !no_text, false) {
                    act = Some(MAct::Apply);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if ui.button(tr("Accept Left")).on_hover_text(tr("Keep your version of the whole file")).clicked() {
                        act = Some(MAct::Side(true));
                    }
                    if ui.button(tr("Accept Right")).on_hover_text(tr("Keep their version of the whole file")).clicked() {
                        act = Some(MAct::Side(false));
                    }
                });
                return;
            }
            // Toolbar.
            egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let doc = &self.merges[i];
                    let text = !doc.no_text;
                    if theme::icon_button_sized(ui, Icon::ChevronUp, tr("Previous conflict"), 24.0, false, text).clicked() {
                        act = Some(MAct::Go(false));
                    }
                    if theme::icon_button_sized(ui, Icon::ChevronDown, tr("Next conflict"), 24.0, false, text).clicked() {
                        act = Some(MAct::Go(true));
                    }
                    if text {
                        divider(ui);
                        let simple = doc.simple_count();
                        let b = ui.add_enabled(simple > 0, egui::Button::new(tr("Resolve Simple Conflicts")));
                        let b = if simple > 0 { b.on_hover_text(tr("Resolve the conflicts where the two sides changed different lines")) } else { b.on_disabled_hover_text(tr("Both sides changed the same lines in every conflict left")) };
                        if b.clicked() {
                            act = Some(MAct::Simple);
                        }
                        if ui.button(tr("» All Yours")).on_hover_text(tr("Fill every open conflict with your side")).clicked() {
                            act = Some(MAct::All(true));
                        }
                        if ui.button(tr("All Theirs «")).on_hover_text(tr("Fill every open conflict with their side")).clicked() {
                            act = Some(MAct::All(false));
                        }
                        divider(ui);
                        let label = if running { format!("      {}", tr("Stop")) } else { format!("      {}", tr("Resolve with AI")) };
                        let tip = match (running, agent) {
                            (true, _) => tr("Stop the AI agent").to_string(),
                            (false, Some(n)) if ready => crate::i18n::trf("Let {} resolve the conflicts; you review the result before applying", &[&n]),
                            _ => tr("Choose an AI agent in Settings first").to_string(),
                        };
                        let b = ui.add(egui::Button::new(RichText::new(label).color(theme::TEXT())));
                        let ir = Rect::from_center_size(pos2(b.rect.left() + 15.0, b.rect.center().y), vec2(15.0, 15.0));
                        theme::paint_icon(ui.painter(), ir, if running { Icon::Close } else { Icon::Spiral }, if ready || running { theme::LINK() } else { theme::TEXT_DIM() });
                        if b.on_hover_text(tip).clicked() {
                            act = Some(if running { MAct::StopAi } else { MAct::Ai });
                        }
                        if running {
                            ui.add(egui::Spinner::new().size(14.0));
                        } else {
                            self.method_picker(ui, 200.0);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // The external merge tool from Settings (the one for this type of file first).
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if ext_tools.len() > 1 {
                            let more = theme::icon_button_sized(ui, Icon::ChevronDown, tr("Choose the external tool"), 16.0, false, true);
                            egui::Popup::menu(&more).show(|ui| {
                                ui.set_min_width(180.0);
                                for t in &ext_tools {
                                    if ui.button(crate::i18n::trf("Open in {}", &[&t.name])).clicked() {
                                        act = Some(MAct::External(Some(t.clone())));
                                    }
                                }
                            });
                        }
                        let tip = match &ext_default {
                            Some(t) => crate::i18n::trf("Open in External Merge Tool ({})", &[&t.name]),
                            None if !self.settings.ext_enabled => tr("External tools are turned off — set them up in Settings").to_string(),
                            None => tr("No external merge tool yet — add one in Settings").to_string(),
                        };
                        if theme::icon_button_sized(ui, Icon::External, &tip, 24.0, false, true).clicked() {
                            act = Some(MAct::External(ext_default.clone()));
                        }
                        ui.spacing_mut().item_spacing.x = 8.0;
                        divider(ui);
                        let doc = &self.merges[i];
                        if !doc.no_text {
                            let left = doc.unresolved();
                            let (t, c) = if left == 0 {
                                (tr("All conflicts resolved").to_string(), theme::GREEN())
                            } else {
                                (crate::i18n::trf("{} of {} conflict(s) left", &[&left, &doc.conflicts()]), theme::TEXT_MID())
                            };
                            ui.label(RichText::new(t).color(c).size(12.5));
                        }
                    });
                });
            });
            // The search line (Ctrl+F).
            let matcher = self.merge_find.as_ref().and_then(|f| Matcher::new(&f.query, f.regex, f.case));
            let hits = match (&matcher, self.merges[i].no_text) {
                (Some(m), false) => merge_hits(&self.merges[i], m),
                _ => vec![],
            };
            if let Some(f) = self.merge_find.as_mut() {
                if f.current >= hits.len() {
                    f.current = 0;
                }
                let mut close = false;
                egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
                    ui.horizontal(|ui| close = crate::commit_view::find_line(ui, f, "Search in the three versions", (f.current + 1, hits.len())));
                });
                // Esc closes the search, not the window.
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) || close {
                    self.merge_find = None;
                }
            }
            let search = match (&matcher, self.merge_find.as_mut()) {
                (Some(m), Some(f)) => Some(Search { m, hits: &hits, current: f.current, go: std::mem::take(&mut f.go) }),
                _ => None,
            };
            let colours = !self.settings.syntax_off;
            if let Some((note, err)) = self.merges[i].note.clone() {
                egui::Frame::new()
                    .fill(if err { theme::rgb(0x3d, 0x26, 0x26) } else { theme::rgb(0x23, 0x30, 0x3d) })
                    .inner_margin(egui::Margin::symmetric(12, 5))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(note).color(theme::TEXT()).size(12.5));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if theme::icon_button_sized(ui, Icon::Close, tr("Hide"), 18.0, false, true).clicked() {
                                    self.merges[i].note = None;
                                }
                                if self.merges[i].whole.is_some() && ui.small_button(tr("Back to conflict blocks")).clicked() {
                                    self.merges[i].whole = None;
                                    self.merges[i].note = None;
                                }
                            });
                        });
                    });
            }
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height + crate::form::grow(ui)), Sense::hover());
            let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            cu.set_clip_rect(rect);
            let doc = &mut self.merges[i];
            if doc.no_text {
                cu.add_space(40.0);
                cu.vertical_centered(|ui| {
                    ui.label(RichText::new(crate::i18n::trf("{} can't be merged line by line", &[&doc.path])).color(theme::TEXT()).size(14.0));
                    ui.label(RichText::new(tr("It is binary, or it was deleted on one side. Choose which version to keep.")).color(theme::TEXT_MID()));
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("Use Accept Left (yours) or Accept Right (theirs) below.")).color(theme::TEXT_DIM()));
                });
            } else {
                merge_columns(&mut cu, doc, colours, search);
            }
        });
        if closed && act.is_none() {
            act = Some(MAct::Close);
        }
        // "Apply Changes" asks first: everything resolved, or what is left goes in as it is.
        if let Some(all) = self.merge_confirm {
            let n = self.merges.get(i).map(|d| d.unresolved()).unwrap_or(0);
            let title = if all { "Finish Resolving" } else { "Unprocessed Changes" };
            let mut answer: Option<bool> = None;
            let shut = crate::form::show(ctx, "merge_confirm", title, 460.0, |ui, part| {
                if part == crate::form::Part::Footer {
                    if crate::form::button(ui, tr("Back to Resolving")) {
                        answer = Some(false);
                    }
                    if crate::form::primary(ui, if all { tr("Save and Finish") } else { tr("Save Current Result") }, true, ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        answer = Some(true);
                    }
                    return;
                }
                ui.horizontal_top(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                    let c = if all { theme::GREEN() } else { theme::STAR() };
                    let pts = vec![pos2(r.center().x, r.top() + 2.0), pos2(r.right() - 1.0, r.bottom() - 3.0), pos2(r.left() + 1.0, r.bottom() - 3.0)];
                    if all {
                        ui.painter().circle_filled(r.center(), 12.0, c);
                        theme::paint_icon(ui.painter(), r.shrink(8.0), Icon::Check, theme::BG());
                    } else {
                        ui.painter().add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
                        ui.painter().text(pos2(r.center().x, r.center().y + 3.0), egui::Align2::CENTER_CENTER, "!", FontId::proportional(14.0), theme::BG());
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.add_space(2.0);
                        let text = if all {
                            tr("All conflicts in this file are resolved. The result is saved and the file is marked as resolved.").to_string()
                        } else {
                            crate::i18n::trf("{} unresolved conflict(s) still remain. The file will be saved exactly as shown in the Result pane (conflict markers stay where nothing was chosen) and marked as resolved.", &[&n])
                        };
                        ui.add(egui::Label::new(RichText::new(text).color(theme::TEXT_MID())).wrap());
                    });
                });
            });
            if shut && answer.is_none() {
                answer = Some(false);
            }
            match answer {
                Some(true) => {
                    self.merge_confirm = None;
                    act = Some(MAct::Save);
                }
                Some(false) => self.merge_confirm = None,
                None => {}
            }
        }
        match act {
            None => {}
            Some(MAct::Apply) => {
                let left = self.merges[i].unresolved();
                self.merge_confirm = Some(left == 0);
            }
            Some(MAct::Save) => {
                if let Some(p) = self.save_merge(i) {
                    self.run_job(Job::many("Resolve conflict", vec![vec!["add".into(), "--".into(), p.clone()]]));
                    self.open_next_conflict(&p);
                }
            }
            Some(MAct::SaveClose) => {
                // Saved as it is (open conflicts keep their markers); the edits stay for later.
                if let Some(repo) = self.repo.as_ref() {
                    let doc = &self.merges[i];
                    if let Err(e) = std::fs::write(repo.path.join(&doc.path), doc.result_text()) {
                        self.dialog = Some(Dialog::Message { title: tr("Merge").into(), text: format!("Could not save {}: {e}", doc.path) });
                    }
                }
                self.merge_open = None;
                self.show_conflicts = true;
            }
            Some(MAct::Close) => {
                // The edits stay in memory: opening the file again shows them.
                self.merge_open = None;
                self.merge_confirm = None;
                self.show_conflicts = true;
            }
            Some(MAct::Side(ours)) => {
                self.accept_side(&path, ours);
                self.open_next_conflict(&path);
            }
            Some(MAct::All(ours)) => self.merges[i].take_all(ours),
            Some(MAct::Simple) => {
                let n = self.merges[i].resolve_simple();
                self.merges[i].note = Some((crate::i18n::trf("{} simple conflict(s) resolved — check them in the middle.", &[&n]), false));
            }
            Some(MAct::Go(down)) => {
                let d = &mut self.merges[i];
                if let Some(k) = d.next_open(down) {
                    d.jump = Some(k);
                    d.cursor = Some(k);
                }
            }
            Some(MAct::Ai) => self.resolve_with_ai(&path, false),
            Some(MAct::StopAi) => {
                self.conflict_ai.retain(|(p, job)| {
                    if *p == path {
                        ai::cancel(job);
                    }
                    *p != path
                });
                self.merges[i].note = Some((tr("Stopped.").into(), false));
            }
            Some(MAct::External(None)) => {
                // No tool yet: to the settings page where tools are added.
                self.merge_open = None;
                self.open_settings(Some(crate::settings_view::Page::ExternalTools));
            }
            Some(MAct::External(Some(tool))) => {
                // The tool writes the file itself; this window's edits would be stale.
                self.external_merge_with(&path, &tool);
                self.close_merge(i);
                self.merge_open = None;
                self.show_conflicts = true;
            }
        }
    }
}

/// A file resolved while the Conflicts window is in use (listed under "Resolved").
#[derive(Clone, Debug)]
pub struct DoneFile {
    pub path: String,
    pub x: char,
    pub y: char,
    pub conflicts: usize,
}

fn side_word(c: char) -> &'static str {
    match c {
        'D' => "Deleted",
        'A' => "Added",
        _ => "Modified",
    }
}

/// What the merge window asks for.
enum MAct {
    Apply,
    Save,
    SaveClose,
    Close,
    Side(bool),
    All(bool),
    Simple,
    Go(bool),
    /// Open the file in an external merge tool (None: none set up yet).
    External(Option<crate::settings::ExtTool>),
    Ai,
    StopAi,
}


/// Yours · result · theirs, aligned block by block, in one scrolling area.
fn merge_columns(ui: &mut Ui, doc: &mut MergeDoc, colours: bool, search: Option<Search>) {
    let full = ui.max_rect();
    let colw = ((full.width() - 2.0 * GUT) / 3.0).max(120.0);
    let xs = [full.left(), full.left() + colw + GUT, full.left() + 2.0 * (colw + GUT)];
    // Column titles.
    let (hr, _) = ui.allocate_exact_size(vec2(full.width(), 26.0), Sense::hover());
    ui.painter().rect_filled(hr, 0.0, theme::BG_BAR());
    theme::hline(ui, hr.bottom() - 0.5, hr.left(), hr.right());
    let theirs = if doc.theirs_label.is_empty() { tr("incoming").to_string() } else { doc.theirs_label.clone() };
    let name = file_name(&doc.path);
    for (k, (t, sub)) in [(tr("Your version"), doc.ours_label.clone()), (tr("Result"), name), (tr("Changes from"), theirs)].iter().enumerate() {
        let x = xs[k] + NUM;
        ui.painter().text(pos2(x, hr.center().y), egui::Align2::LEFT_CENTER, *t, FontId::proportional(13.0), theme::TEXT());
        if !sub.is_empty() {
            let w = theme::text_width(ui, t, &FontId::proportional(13.0));
            theme::paint_clipped(ui.painter(), pos2(x + w + 8.0, hr.center().y), sub, FontId::proportional(12.0), theme::TEXT_DIM(), (colw - NUM - w - 12.0).max(0.0));
        }
    }
    let jump = doc.jump.take();
    let path = doc.path.clone();
    // The language of the file, for colouring ("" = plain text).
    let lang_path: &str = if colours && syntax::lang_for(&path).is_some() { &path } else { "" };
    let goal = search.filter(|s| s.go).and_then(|s| s.hits.get(s.current).copied());
    // Long lines: the three columns scroll sideways together (bar at the bottom, Shift+wheel).
    let char_w = ui.painter().layout_no_wrap("0".repeat(20), FontId::monospace(13.0), theme::TEXT()).size().x / 20.0;
    let content = longest_line(doc) as f32 * char_w + 24.0;
    let view = colw - NUM;
    let max_off = (content - view).max(0.0);
    let hid = egui::Id::unique(("merge_hscroll", &path));
    let mut off: f32 = ui.ctx().data(|d| d.get_temp(hid)).unwrap_or(0.0f32).clamp(0.0, max_off);
    let bar_h = if max_off > 0.0 { 14.0 } else { 0.0 };
    if let Some(h) = goal {
        // The found text comes into view sideways too.
        let x = h.from as f32 * char_w;
        if x < off + 16.0 || x > off + view - 80.0 {
            off = (x - view / 3.0).clamp(0.0, max_off);
            ui.ctx().request_repaint();
        }
    }
    let hs = Hs { off, content };
    let ctx = ui.ctx().clone();
    let none: &[Mark] = &[];
    let marks = |seg: usize, col: u8| search.map(|s| s.marks(seg, col)).unwrap_or_default();
    // The line of a block the search goes to.
    let show_hit = |ui: &Ui, row: Rect, seg: usize, pad: f32| {
        if let Some(h) = goal.filter(|h| h.seg == seg) {
            let y = row.top() + pad + h.line as f32 * LH;
            ui.scroll_to_rect(Rect::from_min_max(pos2(row.left(), y - LH), pos2(row.right(), y + 2.0 * LH)), Some(egui::Align::Center));
        }
    };
    egui::ScrollArea::vertical().id_salt(("merge_scroll", &path)).max_height((ui.available_height() - bar_h).max(40.0)).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let w = ui.available_width();
        let left0 = ui.max_rect().left();
        let col = |row: Rect, k: usize| Rect::from_min_size(pos2(left0 + (xs[k] - full.left()), row.top()), vec2(colw, row.height()));
        ui.add_space(4.0);
        if let Some(whole) = doc.whole.as_mut() {
            let n = line_count(&doc.ours_file).max(line_count(&doc.theirs_file)).max(line_count(whole)).max(1) + 1;
            let (row, _) = ui.allocate_exact_size(vec2(w, n as f32 * LH), Sense::hover());
            show_hit(ui, row, 0, 0.0);
            let (ml, mc, mr) = (marks(0, 0), marks(0, 1), marks(0, 2));
            let look = |marks| Look { path: lang_path, st: syntax::State::default(), marks };
            side_view(ui, col(row, 0), doc.ours_file.trim_end_matches('\n'), 1, theme::TEXT_MID(), egui::Id::unique(("merge_ours_file", &path)), None, hs, look(&ml));
            side_view(ui, col(row, 2), doc.theirs_file.trim_end_matches('\n'), 1, theme::TEXT_MID(), egui::Id::unique(("merge_theirs_file", &path)), None, hs, look(&mr));
            let c = col(row, 1);
            paint_numbers(ui, c, line_count(whole).max(1), 1);
            ui.painter().rect_filled(Rect::from_min_max(pos2(c.left() + NUM - 4.0, c.top()), c.max), 0.0, theme::rgb(0x1f, 0x26, 0x24));
            code_edit(ui, Rect::from_min_max(pos2(c.left() + NUM, c.top()), c.max), whole, egui::Id::unique(("merge_whole", &path)), "", hs, look(&mc));
            return;
        }
        let mut no = [1usize; 3];
        // What the lines above left open in each column (a comment, a long string).
        let mut st = [syntax::State::default(); 3];
        for (si, seg) in doc.segs.iter_mut().enumerate() {
            match seg {
                Seg::Same { orig, text } => {
                    let side_n = line_count(orig).max(1);
                    let mid_n = line_count(text).max(1);
                    let n = side_n.max(mid_n);
                    let (row, _) = ui.allocate_exact_size(vec2(w, n as f32 * LH), Sense::hover());
                    show_hit(ui, row, si, 0.0);
                    let before = st;
                    if ui.is_rect_visible(row) {
                        let side = search.map(|s| s.marks_in(orig)).unwrap_or_default();
                        let mid = marks(si, 1);
                        side_view(ui, col(row, 0), orig, no[0], theme::TEXT_MID(), egui::Id::unique(("merge_same_l", &path, si)), None, hs, Look { path: lang_path, st: before[0], marks: &side });
                        side_view(ui, col(row, 2), orig, no[2], theme::TEXT_MID(), egui::Id::unique(("merge_same_r", &path, si)), None, hs, Look { path: lang_path, st: before[2], marks: &side });
                        let c = col(row, 1);
                        paint_numbers(ui, c, mid_n, no[1]);
                        code_edit(ui, Rect::from_min_max(pos2(c.left() + NUM, c.top()), c.max), text, egui::Id::unique(("merge_same", &path, si)), "", hs, Look { path: lang_path, st: before[1], marks: &mid });
                    }
                    st[0] = end_state(&ctx, orig, lang_path, st[0]);
                    st[2] = end_state(&ctx, orig, lang_path, st[2]);
                    st[1] = end_state(&ctx, text, lang_path, st[1]);
                    no[0] += side_n;
                    no[2] += side_n;
                    no[1] += mid_n;
                }
                Seg::Conflict(ch) => {
                    let (nl, nr) = (line_count(&ch.ours), line_count(&ch.theirs));
                    let nc = ch.result.as_deref().map(line_count).unwrap_or(0).max(1);
                    let n = nl.max(nr).max(nc).max(2);
                    let (row, _) = ui.allocate_exact_size(vec2(w, n as f32 * LH + 6.0), Sense::hover());
                    let inner = |r: Rect| Rect::from_min_max(pos2(r.left(), r.top() + 3.0), pos2(r.right(), r.bottom() - 3.0));
                    let open = ch.result.is_none();
                    let mut action: Option<Take> = None;
                    if jump == Some(si) {
                        ui.scroll_to_rect(row, Some(egui::Align::Center));
                    }
                    show_hit(ui, row, si, 3.0);
                    let before = st;
                    if ui.is_rect_visible(row) {
                        let (l, c, r) = (col(row, 0), col(row, 1), col(row, 2));
                        let side_bg = if open { theme::adapt(C_CONFLICT) } else { theme::adapt(C_SIDE_DONE) };
                        ui.painter().rect_filled(l, 0.0, side_bg);
                        ui.painter().rect_filled(r, 0.0, side_bg);
                        ui.painter().rect_filled(c, 0.0, if open { theme::adapt(C_CONFLICT) } else { theme::adapt(C_RESOLVED) });
                        // The connecting bands in the gutters.
                        let band = if open { theme::rgb(0x3a, 0x27, 0x28) } else { theme::rgb(0x22, 0x30, 0x2a) };
                        ui.painter().rect_filled(Rect::from_min_max(pos2(l.right(), row.top()), pos2(c.left(), row.bottom())), 0.0, band);
                        ui.painter().rect_filled(Rect::from_min_max(pos2(c.right(), row.top()), pos2(r.left(), row.bottom())), 0.0, band);
                        let dim = if open { theme::TEXT() } else { theme::TEXT_MID() };
                        // Selectable text; right-click a side to copy or for more choices.
                        let (ml, mc, mr) = (marks(si, 0), marks(si, 1), marks(si, 2));
                        side_view(ui, inner(l), &ch.ours, no[0], dim, egui::Id::unique(("merge_side", &path, si, 0)), Some(&mut action), hs, Look { path: lang_path, st: before[0], marks: &ml });
                        side_view(ui, inner(r), &ch.theirs, no[2], dim, egui::Id::unique(("merge_side", &path, si, 2)), Some(&mut action), hs, Look { path: lang_path, st: before[2], marks: &mr });
                        // Arrows: » takes yours, « takes theirs (both pressed = both kept).
                        let arrow = |ui: &mut Ui, x: f32, text: &str, tip: &str, salt: u8| {
                            let b = Rect::from_center_size(pos2(x, row.top() + 3.0 + LH / 2.0), vec2(24.0, 18.0));
                            let resp = ui.interact(b, egui::Id::unique(("merge_arrow", &path, si, salt)), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(b, 4.0, theme::rgb(0x45, 0x48, 0x4e));
                            }
                            ui.painter().text(b.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(16.0), theme::ICON());
                            resp.on_hover_text(tip).clicked()
                        };
                        if arrow(ui, (l.right() + c.left()) / 2.0, "»", "Take yours (adds to the result)", 0) {
                            action = Some(Take::Ours);
                        }
                        if arrow(ui, (c.right() + r.left()) / 2.0, "«", "Take theirs (adds to the result)", 1) {
                            action = Some(Take::Theirs);
                        }
                        if !open {
                            let b = Rect::from_center_size(pos2((l.right() + c.left()) / 2.0, row.top() + 3.0 + LH * 1.5), vec2(24.0, 18.0));
                            let resp = ui.interact(b, egui::Id::unique(("merge_reset", &path, si)), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(b, 4.0, theme::rgb(0x45, 0x48, 0x4e));
                            }
                            theme::paint_icon(ui.painter(), Rect::from_center_size(b.center(), vec2(13.0, 13.0)), Icon::Undo, theme::TEXT_MID());
                            if resp.on_hover_text(tr("Back to unresolved")).clicked() {
                                action = Some(Take::Reset);
                            }
                        }
                        let ci = inner(c);
                        paint_numbers(ui, ci, if open { 0 } else { nc }, no[1]);
                        let edit_rect = Rect::from_min_max(pos2(ci.left() + NUM, ci.top()), ci.max);
                        match ch.result.as_mut() {
                            Some(t) => {
                                code_edit(ui, edit_rect, t, egui::Id::unique(("merge_res", &path, si)), "(nothing — both sides dropped)", hs, Look { path: lang_path, st: before[1], marks: &mc });
                            }
                            None => {
                                let mut buf = String::new();
                                if code_edit(ui, edit_rect, &mut buf, egui::Id::unique(("merge_res", &path, si)), "Unresolved: use » / « or type here", hs, Look { path: lang_path, st: before[1], marks: none }) {
                                    ch.result = Some(buf);
                                }
                            }
                        }
                    }
                    if let Some(t) = action {
                        take(ch, t);
                    }
                    st[0] = end_state(&ctx, &ch.ours, lang_path, st[0]);
                    st[2] = end_state(&ctx, &ch.theirs, lang_path, st[2]);
                    if let Some(t) = &ch.result {
                        st[1] = end_state(&ctx, t, lang_path, st[1]);
                    }
                    no[0] += nl;
                    no[2] += nr;
                    no[1] += if open { 0 } else { ch.result.as_deref().map(line_count).unwrap_or(0) };
                }
            }
        }
        ui.add_space(40.0);
    });
    if max_off > 0.0 {
        // Shift+wheel (or a sideways swipe) over the columns.
        if ui.rect_contains_pointer(full) {
            let dx = ui.input(|i| i.smooth_scroll_delta.x);
            if dx != 0.0 {
                off = (off - dx).clamp(0.0, max_off);
            }
        }
        let (track, resp) = ui.allocate_exact_size(vec2(full.width(), bar_h), Sense::click_and_drag());
        let track = track.shrink2(vec2(4.0, 3.0));
        let thumb_w = (track.width() * view / content).clamp(30.0, track.width());
        let room = (track.width() - thumb_w).max(1.0);
        if resp.dragged() {
            off = (off + resp.drag_delta().x * max_off / room).clamp(0.0, max_off);
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                off = ((p.x - track.left() - thumb_w / 2.0) / room * max_off).clamp(0.0, max_off);
            }
        }
        let thumb = Rect::from_min_size(pos2(track.left() + room * off / max_off, track.top()), vec2(thumb_w, track.height()));
        let c = if resp.hovered() || resp.dragged() { theme::rgb(0x6a, 0x6e, 0x75) } else { theme::rgb(0x4e, 0x52, 0x58) };
        ui.painter().rect_filled(thumb, 4.0, c);
        ui.ctx().data_mut(|d| d.insert_temp(hid, off));
    }
}

/// The sideways scroll of the merge columns: how far it is moved and how wide the text is.
#[derive(Clone, Copy)]
struct Hs {
    off: f32,
    content: f32,
}

impl Hs {
    /// Where a column's text goes: moved left by the scroll, as wide as the longest line.
    fn wide(&self, rect: Rect) -> Rect {
        Rect::from_min_size(pos2(rect.left() - self.off, rect.top()), vec2(rect.width().max(self.content), rect.height()))
    }
}

/// The longest line of all three columns, in characters (a tab counts as four).
fn longest_line(doc: &MergeDoc) -> usize {
    let width = |t: &str| t.split('\n').map(|l| l.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>()).max().unwrap_or(0);
    let mut n = 0;
    if let Some(w) = &doc.whole {
        n = width(w).max(width(&doc.ours_file)).max(width(&doc.theirs_file));
    }
    for s in &doc.segs {
        n = n.max(match s {
            Seg::Same { orig, text } => width(orig).max(width(text)),
            Seg::Conflict(c) => width(&c.ours).max(width(&c.theirs)).max(c.result.as_deref().map(width).unwrap_or(0)),
        });
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "def total(items):\n<<<<<<< HEAD\n    return sum(items)  # total\n=======\n    return round(sum(items), 2)\n>>>>>>> feature\n\ndef Total():\n    pass\n";

    #[test]
    fn finds_text_in_the_three_columns() {
        let mut d = MergeDoc::parse("a.py", FILE);
        let m = Matcher::new("total", false, false).unwrap();
        let at = |h: &[MHit]| h.iter().map(|h| (h.seg, h.col, h.line, h.from)).collect::<Vec<_>>();
        // Lines both sides agree on are counted once (in the result); an open conflict has
        // nothing in the middle.
        assert_eq!(at(&merge_hits(&d, &m)), [(0, 1, 0, 4), (1, 0, 0, 25), (2, 1, 1, 4)]);
        d.take_all(true);
        assert_eq!(at(&merge_hits(&d, &m)), [(0, 1, 0, 4), (1, 0, 0, 25), (1, 1, 0, 25), (2, 1, 1, 4)]);
        let hits = merge_hits(&d, &Matcher::new("Total", false, true).unwrap());
        assert_eq!(at(&hits), [(2, 1, 1, 4)]);
        // The marks of a column of a block, the current hit told apart.
        let s = Search { m: &m, hits: &merge_hits(&d, &m), current: 2, go: false };
        assert_eq!(s.marks(1, 0), [(0, 25, 30, false)]);
        assert_eq!(s.marks(1, 1), [(0, 25, 30, true)]);
        assert!(s.marks(1, 2).is_empty());
        // The whole-file view (an AI answer that didn't fit the blocks) is searched as three texts.
        d.whole = Some("total = 1\n".into());
        d.ours_file = "x\ntotal\n".into();
        d.theirs_file = "nothing\n".into();
        assert_eq!(at(&merge_hits(&d, &m)), [(0, 1, 0, 0), (0, 0, 1, 0)]);
    }

    #[test]
    fn colours_code_and_marks_found_text() {
        let text = "x = 1  # set\nreturn \"ю\" + x";
        let piece = |job: &egui::text::LayoutJob, what: &str| {
            let s = job.sections.iter().find(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0] == what).unwrap_or_else(|| panic!("no piece {what:?}"));
            (s.format.color, s.format.background)
        };
        let job = code_job(text, theme::TEXT(), "a.py", syntax::State::default(), &[(1, 13, 14, true), (0, 0, 1, false)][1..]);
        assert_eq!(job.text, text);
        assert_eq!(piece(&job, "# set").0, theme::syntax_color(syntax::Kind::Comment));
        assert_eq!(piece(&job, "x"), (Color32::WHITE, theme::FIND()));
        let job = code_job(text, theme::TEXT(), "a.py", syntax::State::default(), &[(1, 13, 14, true)]);
        assert_eq!(piece(&job, "return").0, theme::syntax_color(syntax::Kind::Keyword));
        // Columns are characters: the letter before the found one takes two bytes.
        assert_eq!(job.sections.last().map(|s| (&job.text[s.byte_range.start.0..s.byte_range.end.0], s.format.background)), Some(("x", theme::FIND_CURRENT())));
        // Without a known language and without marks the text is one plain piece.
        let plain = code_job(text, theme::TEXT(), "", syntax::State::default(), &[]);
        assert_eq!((plain.sections.len(), plain.text.as_str()), (1, text));
        // A comment left open by the lines above goes on.
        let mut st = syntax::State::default();
        syntax::line(syntax::lang_for("a.rs").unwrap(), "/* open", &mut st);
        let job = code_job("still a comment", theme::TEXT(), "a.rs", st, &[]);
        assert_eq!(piece(&job, "still a comment").0, theme::syntax_color(syntax::Kind::Comment));
    }
}
