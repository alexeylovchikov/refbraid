//! Git → Create Patch / Apply Patch: save local changes or commits to a .patch file, and apply
//! such a file to the working folder, to the staging area too, or as commits.

use crate::app::{App, Dialog, Job, Toast, short};
use crate::i18n::tr;
use crate::form::{self, Part};
use crate::git;
use crate::theme;
use eframe::egui::{self, RichText, Ui};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// What goes into a new patch.
#[derive(Clone, Debug)]
pub enum PatchFrom {
    /// The local changes (against the last commit); only these files when not empty.
    Local { paths: Vec<String> },
    /// Commits (hash, subject), oldest first.
    Commits(Vec<(String, String)>),
}

pub struct CreatePatch {
    pub from: PatchFrom,
    pub untracked: bool,
    pub path: String,
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApplyMode {
    /// Change the files only.
    WorkTree,
    /// Change the files and stage the changes.
    Stage,
    /// Make the commits the patch holds (git am), with their authors and messages.
    Commits,
}

pub struct ApplyPatch {
    pub path: String,
    /// (added, removed, file)
    pub files: Vec<(String, String, String)>,
    /// Commits in the file (made by "Create Patch" from commits, or git format-patch).
    pub commits: usize,
    pub mode: ApplyMode,
    pub three_way: bool,
    /// Whether it applies with the chosen options, and the options it was checked with.
    pub check: Result<(), String>,
    pub checked: Option<(ApplyMode, bool)>,
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// A file name from a commit subject: "Fix the waybill (APP-1)" → "Fix-the-waybill-APP-1".
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.chars().count() >= 50 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Run git and keep its output as bytes (a patch may hold text in any encoding).
fn git_bytes(dir: &Path, env: &[(&str, &str)], args: &[&str]) -> Result<Vec<u8>, String> {
    let mut c = git::command(dir, args);
    c.envs(env.iter().copied());
    let o = c.output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(o.stdout)
    } else {
        let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
        Err(if e.is_empty() { format!("git {} failed", args.first().unwrap_or(&"")) } else { e })
    }
}

/// The patch text for `from`.
pub fn make_patch(dir: &Path, from: &PatchFrom, untracked: bool) -> Result<Vec<u8>, String> {
    match from {
        PatchFrom::Local { paths } => {
            let has_head = git::run(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok;
            let mut limit: Vec<&str> = vec!["--"];
            limit.extend(paths.iter().map(|s| s.as_str()));
            if !untracked && has_head {
                let mut args = vec!["diff", "--binary", "HEAD"];
                args.extend(&limit);
                return git_bytes(dir, &[], &args);
            }
            // New files too: add everything to a copy of the staging area and compare that
            // with the last commit. The real staging area is not touched.
            let index = git::run(dir, &["rev-parse", "--git-path", "index"]).stdout.trim().to_string();
            let index = dir.join(index);
            let tmp = std::env::temp_dir().join(format!("refbraid-patch-index-{}", std::process::id()));
            if index.is_file() {
                std::fs::copy(&index, &tmp).map_err(|e| format!("could not copy the staging area: {e}"))?;
            }
            let tmp_s = tmp.display().to_string();
            let env = [("GIT_INDEX_FILE", tmp_s.as_str())];
            let mut add = vec!["add", "-A"];
            if !untracked {
                add = vec!["add", "-u"];
            }
            add.extend(if paths.is_empty() { vec!["--", "."] } else { limit.clone() });
            let r = git_bytes(dir, &env, &add).and_then(|_| {
                let mut args = vec!["diff", "--cached", "--binary"];
                if has_head {
                    args.push("HEAD");
                }
                args.extend(&limit);
                git_bytes(dir, &env, &args)
            });
            let _ = std::fs::remove_file(&tmp);
            r
        }
        PatchFrom::Commits(list) => {
            let mut out = vec![];
            for (h, _) in list {
                out.extend(git_bytes(dir, &[], &["format-patch", "-1", "--stdout", "--binary", h])?);
            }
            Ok(out)
        }
    }
}

/// How many commits a patch file holds (mailbox format, as git format-patch writes).
pub fn commits_in(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            l.strip_prefix("From ")
                .and_then(|r| r.split(' ').next())
                .is_some_and(|h| h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit()))
        })
        .count()
}

fn apply_args(path: &str, mode: ApplyMode, three_way: bool, check: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["apply".into()];
    if mode == ApplyMode::Stage {
        a.push("--index".into());
    }
    if three_way {
        a.push("--3way".into());
    }
    if check {
        a.push("--check".into());
    }
    a.push(path.into());
    a
}

impl App {
    /// Git → Create Patch… for the local changes (all of them, or the files given).
    pub fn open_create_patch(&mut self, paths: Vec<String>) {
        let Some(repo) = self.repo.as_ref() else { return };
        let what = if paths.len() == 1 { slug(&crate::app::file_name(&paths[0])) } else { "changes".into() };
        let path = home().join(format!("{}-{what}.patch", repo.name)).display().to_string();
        let untracked = paths.is_empty() || paths.iter().any(|p| repo.status.iter().any(|e| &e.path == p && e.is_untracked()));
        self.dialog = Some(Dialog::CreatePatch(CreatePatch { from: PatchFrom::Local { paths }, untracked, path, error: None }));
    }

    /// Create Patch… for commits of the log (indexes into the commit list).
    pub fn open_create_patch_commits(&mut self, mut idx: Vec<usize>) {
        let Some(repo) = self.repo.as_ref() else { return };
        // The log lists the newest first; a patch holds them oldest first.
        idx.sort_unstable_by(|a, b| b.cmp(a));
        let list: Vec<(String, String)> = idx.iter().filter_map(|&i| repo.commits.get(i)).map(|c| (c.hash.clone(), c.subject.clone())).collect();
        let Some((h, s)) = list.first().cloned() else { return };
        let name = if list.len() == 1 { format!("{}-{}.patch", short(&h), slug(&s)) } else { format!("{}-{}-commits.patch", repo.name, list.len()) };
        let path = home().join(name).display().to_string();
        self.dialog = Some(Dialog::CreatePatch(CreatePatch { from: PatchFrom::Commits(list), untracked: false, path, error: None }));
    }

    /// Git → Apply Patch…: choose the file, then show what it changes.
    pub fn choose_patch(&mut self) {
        let dir = self.repo.as_ref().map(|r| r.path.clone());
        let mut dlg = rfd::FileDialog::new().set_title("Apply Patch").add_filter("Patches", &["patch", "diff", "mbox", "txt"]).add_filter("All files", &["*"]);
        if let Some(d) = dir {
            dlg = dlg.set_directory(d);
        }
        if let Some(p) = dlg.pick_file() {
            self.open_apply_patch(&p);
        }
    }

    pub fn open_apply_patch(&mut self, file: &Path) {
        let Some(repo) = self.repo.as_ref() else { return };
        let path = file.display().to_string();
        let text = match std::fs::read(file) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                self.notify(Toast { title: tr("Apply Patch").into(), text: format!("Could not read {path}: {e}"), ok: false, at: Instant::now() });
                return;
            }
        };
        let files: Vec<(String, String, String)> = git::run(&repo.path, &["apply", "--numstat", &path])
            .stdout
            .lines()
            .filter_map(|l| {
                let mut it = l.splitn(3, '\t');
                Some((it.next()?.to_string(), it.next()?.to_string(), it.next()?.to_string()))
            })
            .collect();
        if files.is_empty() && commits_in(&text) == 0 {
            self.notify(Toast { title: tr("Apply Patch").into(), text: format!("{} is not a patch file.", crate::app::file_name(&path)), ok: false, at: Instant::now() });
            return;
        }
        let commits = commits_in(&text);
        let mode = if commits > 0 { ApplyMode::Commits } else { ApplyMode::WorkTree };
        self.dialog = Some(Dialog::ApplyPatch(ApplyPatch { path, files, commits, mode, three_way: false, check: Ok(()), checked: None }));
    }

    /// Patch files dropped on the window open the Apply Patch window.
    pub fn take_dropped_patch(&mut self, ctx: &egui::Context) {
        if self.repo.is_none() || self.dialog.is_some() {
            return;
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        let patch = dropped.into_iter().find(|p| {
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            matches!(ext.as_str(), "patch" | "diff" | "mbox")
        });
        if let Some(p) = patch {
            self.open_apply_patch(&p);
        }
    }

    /// Create Patch and Apply Patch windows.
    pub fn patch_dialog(&mut self, ctx: &egui::Context, mut d: Dialog) {
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        let mut keep = true;
        let mut job: Option<Job> = None;
        let title = if matches!(d, Dialog::CreatePatch(_)) { "Create Patch" } else { "Apply Patch" };
        let closed = form::show(ctx, "patch_dialog", title, 520.0, |ui, part| match &mut d {
            Dialog::CreatePatch(st) => keep &= self.create_patch_form(ui, part, st, enter),
            Dialog::ApplyPatch(st) => keep &= self.apply_patch_form(ui, part, st, enter, &mut job),
            _ => {}
        });
        if keep && !closed && self.dialog.is_none() {
            self.dialog = Some(d);
        }
        if let Some(j) = job {
            self.run_job(j);
        }
    }

    /// The body and footer of the Create Patch window. Returns false when it closes.
    pub fn create_patch_form(&mut self, ui: &mut Ui, part: Part, st: &mut CreatePatch, enter: bool) -> bool {
        let Some(repo) = self.repo.as_ref() else { return false };
        let dir = repo.path.clone();
        let all = repo.status.iter().filter(|e| st.untracked || !e.is_untracked()).count();
        if part == Part::Body {
            match &st.from {
                PatchFrom::Local { paths } if paths.is_empty() => {
                    let n = all;
                    ui.label(format!("All local changes against the last commit ({n} file{}).", if n == 1 { "" } else { "s" }));
                }
                PatchFrom::Local { paths } => {
                    ui.label(format!("Local changes in {} selected file{}:", paths.len(), if paths.len() == 1 { "" } else { "s" }));
                    for p in paths.iter().take(6) {
                        ui.label(RichText::new(p).color(theme::TEXT_MID()).size(12.0));
                    }
                    if paths.len() > 6 {
                        ui.label(RichText::new(format!("and {} more", paths.len() - 6)).color(theme::TEXT_DIM()).size(12.0));
                    }
                }
                PatchFrom::Commits(list) => {
                    ui.label(if list.len() == 1 { "The commit, with its author and message:".to_string() } else { format!("{} commits, oldest first, with their authors and messages:", list.len()) });
                    for (h, s) in list.iter().take(6) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(short(h)).color(theme::LINK()).size(12.0));
                            ui.add(egui::Label::new(RichText::new(s).color(theme::TEXT_MID()).size(12.0)).truncate());
                        });
                    }
                    if list.len() > 6 {
                        ui.label(RichText::new(format!("and {} more", list.len() - 6)).color(theme::TEXT_DIM()).size(12.0));
                    }
                }
            }
            if matches!(st.from, PatchFrom::Local { .. }) {
                ui.add_space(4.0);
                ui.checkbox(&mut st.untracked, tr("Include unversioned files")).on_hover_text(tr("New files that are not added to Git yet. Your staging area is not changed."));
            }
            ui.add_space(8.0);
            ui.label("Save to:");
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 90.0).max(120.0);
                theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut st.path).desired_width(w)));
                if ui.button(tr("Browse…")).clicked() {
                    let p = PathBuf::from(&st.path);
                    let mut dlg = rfd::FileDialog::new().set_title("Save Patch").add_filter("Patches", &["patch", "diff"]);
                    if let Some(d) = p.parent().filter(|d| d.is_dir()) {
                        dlg = dlg.set_directory(d);
                    }
                    if let Some(n) = p.file_name() {
                        dlg = dlg.set_file_name(n.to_string_lossy());
                    }
                    if let Some(f) = dlg.save_file() {
                        st.path = f.display().to_string();
                    }
                }
            });
            if let Some(e) = &st.error {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(e).color(theme::RED())).wrap());
            }
            return true;
        }
        let (ok, cancel) = form::ok_cancel(ui, tr("Create Patch"), !st.path.trim().is_empty(), enter);
        if cancel {
            return false;
        }
        if ok {
            let file = PathBuf::from(st.path.trim());
            match make_patch(&dir, &st.from, st.untracked) {
                Ok(b) if b.is_empty() => st.error = Some("There is nothing to put in the patch: no changes.".into()),
                Ok(b) => match std::fs::write(&file, &b) {
                    Ok(()) => {
                        self.notify(Toast { title: tr("Patch created").into(), text: file.display().to_string(), ok: true, at: Instant::now() });
                        return false;
                    }
                    Err(e) => st.error = Some(format!("Could not save it: {e}")),
                },
                Err(e) => st.error = Some(e),
            }
        }
        true
    }

    /// The body and footer of the Apply Patch window. Returns false when it closes.
    pub fn apply_patch_form(&mut self, ui: &mut Ui, part: Part, st: &mut ApplyPatch, enter: bool, job: &mut Option<Job>) -> bool {
        let Some(dir) = self.repo.as_ref().map(|r| r.path.clone()) else { return false };
        if st.checked != Some((st.mode, st.three_way)) {
            let args = if st.mode == ApplyMode::Commits { apply_args(&st.path, ApplyMode::WorkTree, st.three_way, true) } else { apply_args(&st.path, st.mode, st.three_way, true) };
            let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let r = git::run(&dir, &args);
            st.check = if r.ok { Ok(()) } else { Err(r.stderr.lines().filter(|l| !l.trim().is_empty()).take(8).collect::<Vec<_>>().join("\n")) };
            st.checked = Some((st.mode, st.three_way));
        }
        if part == Part::Body {
            ui.label(RichText::new(crate::app::file_name(&st.path)).strong());
            ui.label(RichText::new(&st.path).color(theme::TEXT_DIM()).size(12.0));
            ui.add_space(6.0);
            let n = st.files.len();
            ui.label(format!("Changes {n} file{}{}:", if n == 1 { "" } else { "s" }, if st.commits > 0 { format!(" in {} commit{}", st.commits, if st.commits == 1 { "" } else { "s" }) } else { String::new() }));
            egui::ScrollArea::vertical().id_salt("patch_files").max_height(140.0 + form::grow(ui)).show(ui, |ui| {
                for (a, d, f) in &st.files {
                    ui.horizontal(|ui| {
                        ui.add_sized([44.0, 16.0], egui::Label::new(RichText::new(if a == "-" { "bin".to_string() } else { format!("+{a}") }).color(theme::GREEN()).size(12.0)));
                        ui.add_sized([44.0, 16.0], egui::Label::new(RichText::new(if d == "-" { String::new() } else { format!("−{d}") }).color(theme::RED()).size(12.0)));
                        ui.add(egui::Label::new(RichText::new(f).size(12.0)).truncate());
                    });
                }
            });
            ui.add_space(8.0);
            if st.commits > 0 {
                ui.radio_value(&mut st.mode, ApplyMode::Commits, tr("Make the commits (keeps their authors and messages)"));
            }
            ui.radio_value(&mut st.mode, ApplyMode::WorkTree, tr("Change the files only"));
            ui.radio_value(&mut st.mode, ApplyMode::Stage, tr("Change the files and stage the changes"));
            ui.add_space(4.0);
            ui.checkbox(&mut st.three_way, tr("Use a 3-way merge when it doesn't apply cleanly"))
                .on_hover_text(tr("Parts that clash with your changes become conflicts you can resolve, instead of failing."));
            ui.add_space(8.0);
            match &st.check {
                Ok(()) => {
                    ui.label(RichText::new(tr("✓ The patch applies")).color(theme::GREEN()));
                }
                Err(e) => {
                    ui.label(RichText::new(tr("The patch does not apply as it is:")).color(theme::RED()));
                    ui.add(egui::Label::new(RichText::new(e).color(theme::TEXT_MID()).size(12.0).monospace()).wrap());
                    if !st.three_way {
                        ui.label(RichText::new(tr("Try a 3-way merge: the clashing parts become conflicts to resolve.")).color(theme::TEXT_DIM()).size(12.0));
                    }
                }
            }
            return true;
        }
        let (ok, cancel) = form::ok_cancel(ui, tr("Apply"), st.check.is_ok() || st.three_way, enter && st.check.is_ok());
        if ok {
            let name = crate::app::file_name(&st.path);
            *job = Some(if st.mode == ApplyMode::Commits {
                let mut am: Vec<String> = vec!["am".into()];
                if st.three_way {
                    am.push("--3way".into());
                }
                am.push(st.path.clone());
                // A failed "git am" would stay half done: undo it.
                Job::many(&format!("Apply {name}"), vec![am]).rescue(0, vec![vec!["am".into(), "--abort".into()]])
            } else {
                Job::many(&format!("Apply {name}"), vec![apply_args(&st.path, st.mode, st.three_way, false)])
            });
        }
        !(ok || cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_commit_count() {
        assert_eq!(slug("[fix] Waybill: number (APP-1308)"), "fix-Waybill-number-APP-1308");
        let mbox = "From 0123456789abcdef0123456789abcdef01234567 Mon Sep 17 00:00:00 2001\nFrom: A <a@b>\nSubject: [PATCH] x\n\n---\nFrom here on\n";
        assert_eq!(commits_in(mbox), 1);
        assert_eq!(commits_in("diff --git a/x b/x\n"), 0);
    }

    #[test]
    fn makes_and_applies_a_patch() {
        let dir = std::env::temp_dir().join(format!("refbraid-patch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| assert!(git::run(&dir, args).ok, "git {args:?}");
        g(&["init", "-q"]);
        g(&["config", "user.email", "t@example.com"]);
        g(&["config", "user.name", "T"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        g(&["add", "a.txt"]);
        g(&["commit", "-qm", "first"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("new.txt"), "new\n").unwrap();
        let p = make_patch(&dir, &PatchFrom::Local { paths: vec![] }, true).unwrap();
        let text = String::from_utf8(p.clone()).unwrap();
        assert!(text.contains("+two") && text.contains("new.txt"), "{text}");
        // The real staging area is untouched: new.txt is still unversioned.
        assert!(git::run(&dir, &["status", "--porcelain"]).stdout.contains("?? new.txt"));
        let without = String::from_utf8(make_patch(&dir, &PatchFrom::Local { paths: vec![] }, false).unwrap()).unwrap();
        assert!(!without.contains("new.txt"));
        // Put the changes back and apply the patch.
        g(&["checkout", "--", "a.txt"]);
        std::fs::remove_file(dir.join("new.txt")).unwrap();
        let file = dir.join("x.patch");
        std::fs::write(&file, &p).unwrap();
        let f = file.display().to_string();
        g(&["apply", "--check", &f]);
        g(&["apply", &f]);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\ntwo\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
