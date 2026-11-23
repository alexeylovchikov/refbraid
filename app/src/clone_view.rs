//! File → Clone… and File → New Repository…: getting a repository to work with.

use crate::app::{App, Dialog, Job, Then};
use crate::form::{self, Part};
use crate::git;
use crate::i18n::{tr, trf};
use crate::theme;
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};

/// The Clone window.
#[derive(Clone, Debug, Default)]
pub struct CloneForm {
    pub url: String,
    /// The folder the repository's own folder is made in.
    pub parent: String,
    /// The repository's folder; follows the address until it is typed by hand.
    pub name: String,
    pub name_edited: bool,
    /// Check out this branch instead of the default one (empty = default).
    pub branch: String,
    /// Take only the newest commits (empty = the whole history).
    pub depth: String,
    pub submodules: bool,
}

/// The folder name git would choose for an address: the last part without ".git".
pub fn repo_name(url: &str) -> String {
    let u = url.trim().trim_end_matches(['/', '\\']);
    let u = u.strip_suffix(".git").unwrap_or(u).trim_end_matches(['/', '\\']);
    u.rsplit(['/', '\\', ':']).next().unwrap_or("").trim().to_string()
}

impl CloneForm {
    pub fn target(&self) -> PathBuf {
        Path::new(self.parent.trim()).join(self.name.trim())
    }

    /// Why the clone cannot start, if it can't.
    pub fn problem(&self) -> Option<String> {
        if self.url.trim().is_empty() {
            return Some(tr("Enter the repository address.").into());
        }
        if self.parent.trim().is_empty() || self.name.trim().is_empty() {
            return Some(tr("Choose where to put the repository.").into());
        }
        if !self.depth.trim().is_empty() && self.depth.trim().parse::<u32>().map_or(true, |n| n == 0) {
            return Some(tr("Depth is a number of commits, 1 or more.").into());
        }
        let t = self.target();
        if t.is_file() || (t.is_dir() && std::fs::read_dir(&t).map(|mut d| d.next().is_some()).unwrap_or(true)) {
            return Some(trf("The folder {} already exists and is not empty.", &[&t.display()]));
        }
        None
    }

    /// The `git clone` command (run in the parent folder).
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["clone".to_string()];
        if !self.branch.trim().is_empty() {
            a.extend(["--branch".to_string(), self.branch.trim().to_string()]);
        }
        if let Ok(n) = self.depth.trim().parse::<u32>() {
            a.extend(["--depth".to_string(), n.to_string()]);
        }
        if self.submodules {
            a.push("--recurse-submodules".into());
        }
        a.extend(["--".to_string(), self.url.trim().to_string(), self.name.trim().to_string()]);
        a
    }
}

impl App {
    /// Where new repositories go by default: beside the last cloned one, or the home folder.
    fn default_clone_parent(&self) -> String {
        if !self.settings.clone_dir.trim().is_empty() && Path::new(self.settings.clone_dir.trim()).is_dir() {
            return self.settings.clone_dir.trim().to_string();
        }
        self.repo
            .as_ref()
            .and_then(|r| r.path.parent().map(|p| p.to_path_buf()))
            .or_else(git::home_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    pub fn open_clone(&mut self) {
        // An address on the clipboard is most likely the one to clone.
        self.dialog = Some(Dialog::Clone(CloneForm { parent: self.default_clone_parent(), ..Default::default() }));
    }

    /// File → New Repository…: choose a folder and run `git init` in it.
    pub fn new_repository(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title("Folder for the New Repository");
        if let Some(s) = self.repo.as_ref().and_then(|r| r.path.parent().map(|p| p.to_path_buf())).or_else(git::home_dir) {
            dlg = dlg.set_directory(s);
        }
        let Some(p) = dlg.pick_folder() else { return };
        if git::find_repo_root(&p).is_some_and(|root| root == git::plain_path(std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone()))) {
            // Already a repository: just open it.
            self.open(&p);
            return;
        }
        self.init_repository(&p);
    }

    /// `git init` in `dir`, then open it.
    pub fn init_repository(&mut self, dir: &Path) {
        self.pending_open = Some(dir.to_path_buf());
        self.run_job(Job::one("Create repository", &["init"]).in_dir(dir).then(Then::OpenRepo));
    }

    /// The folder is not a repository: offer to make it one.
    pub fn offer_init(&mut self, dir: &Path) {
        self.pending_open = Some(dir.to_path_buf());
        self.dialog = Some(Dialog::Confirm {
            title: tr("Create Git Repository").into(),
            text: trf("\"{}\" is not a Git repository yet. Create one in this folder (git init)? The files in it stay as they are.", &[&dir.display()]),
            ok: tr("Create").into(),
            job: Some(Job::one("Create repository", &["init"]).in_dir(dir).then(Then::OpenRepo)),
        });
    }

    pub fn clone_dialog(&mut self, ctx: &egui::Context, d: Dialog) {
        let Dialog::Clone(mut f) = d else { return };
        let mut keep = true;
        let mut start = false;
        let mut browse = false;
        let problem = f.problem();
        let closed = form::show(ctx, "clone", "Clone Repository", 560.0, |ui, part| {
            if part == Part::Body {
                let label = |ui: &mut egui::Ui, t: &str| {
                    ui.label(RichText::new(tr(t)).color(theme::TEXT_MID()));
                };
                label(ui, "Repository address (HTTPS, SSH or a folder):");
                let r = theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.url).hint_text("https://example.com/team/project.git").desired_width(f32::INFINITY)));
                if !r.has_focus() && f.url.is_empty() && ui.memory(|m| m.focused().is_none()) {
                    r.request_focus();
                }
                if r.changed() && !f.name_edited {
                    f.name = repo_name(&f.url);
                }
                ui.add_space(8.0);
                label(ui, "Put it into the folder:");
                ui.horizontal(|ui| {
                    if ui.button(tr("Browse…")).clicked() {
                        browse = true;
                    }
                    theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.parent).desired_width(f32::INFINITY)));
                });
                ui.add_space(8.0);
                label(ui, "Folder name:");
                if theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(f32::INFINITY))).changed() {
                    f.name_edited = !f.name.trim().is_empty();
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    label(ui, "Branch:");
                    theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.branch).hint_text(tr("default")).desired_width(150.0)))
                        .on_hover_text(tr("Check out this branch instead of the repository's default one"));
                    ui.add_space(12.0);
                    label(ui, "Depth:");
                    theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.depth).hint_text(tr("all history")).desired_width(90.0)))
                        .on_hover_text(tr("Take only this many newest commits (a shallow clone): faster for a big repository, but the older history is not there"));
                });
                ui.add_space(6.0);
                ui.checkbox(&mut f.submodules, tr("Also clone submodules (--recurse-submodules)"));
                ui.add_space(6.0);
                match &problem {
                    Some(p) if !f.url.trim().is_empty() => ui.label(RichText::new(p).color(theme::RED()).size(12.0)),
                    _ => ui.label(RichText::new(trf("Will be cloned into {}", &[&f.target().display()])).color(theme::TEXT_DIM()).size(12.0)),
                };
            } else {
                let (ok, cancel) = form::ok_cancel(ui, tr("Clone"), problem.is_none(), false);
                start = ok;
                keep &= !(ok || cancel);
            }
        });
        if browse {
            let mut dlg = rfd::FileDialog::new().set_title("Folder for the Repository");
            if Path::new(f.parent.trim()).is_dir() {
                dlg = dlg.set_directory(f.parent.trim());
            }
            if let Some(p) = dlg.pick_folder() {
                f.parent = p.display().to_string();
            }
        }
        if start {
            let parent = PathBuf::from(f.parent.trim());
            if let Err(e) = std::fs::create_dir_all(&parent) {
                self.dialog = Some(Dialog::Message { title: tr("Clone Repository").into(), text: format!("{}: {e}", parent.display()) });
                return;
            }
            self.settings.clone_dir = f.parent.trim().to_string();
            if self.screenshot.is_none() {
                self.settings.save();
            }
            self.pending_open = Some(f.target());
            self.run_job(Job::many(&format!("Clone {}", f.name.trim()), vec![f.args()]).in_dir(&parent).then(Then::OpenRepo));
            return;
        }
        if keep && !closed {
            self.dialog = Some(Dialog::Clone(f));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_folder_after_the_address() {
        assert_eq!(repo_name("https://example.com/team/project.git"), "project");
        assert_eq!(repo_name("git@example.com:team/project.git"), "project");
        assert_eq!(repo_name("git@example.com:project"), "project");
        assert_eq!(repo_name("https://example.com/team/project/"), "project");
        assert_eq!(repo_name(r"C:\work\repo.git\"), "repo");
        assert_eq!(repo_name(""), "");
    }

    #[test]
    fn builds_the_command_and_clones() {
        let base = std::env::temp_dir().join(format!("ge-clone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("source repo");
        std::fs::create_dir_all(&src).unwrap();
        let g = |args: &[&str]| assert!(git::run_env(&src, &[("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")], args).ok, "{args:?}");
        g(&["init", "-q", "-b", "main"]);
        for m in ["one", "two", "three"] {
            std::fs::write(src.join("f.txt"), m).unwrap();
            g(&["add", "-A"]);
            g(&["commit", "-qm", m]);
        }
        g(&["branch", "side", "HEAD~1"]);

        let mut f = CloneForm { url: format!("file://{}", src.display()), parent: base.display().to_string(), ..Default::default() };
        assert!(f.problem().is_some());
        f.name = "copy".into();
        f.branch = "side".into();
        f.depth = "1".into();
        assert_eq!(f.problem(), None);
        let args = f.args();
        assert_eq!(args[..5], ["clone", "--branch", "side", "--depth", "1"].map(String::from));
        let live = git::Live::default();
        let out = git::run_live(&base, &[], &git::with_progress(&args).iter().map(String::as_str).collect::<Vec<_>>(), &live, &|| {});
        assert!(out.ok, "{out:?}");
        let copy = f.target();
        assert_eq!(git::run(&copy, &["log", "--format=%s"]).stdout.trim(), "two");
        // The folder is there now, so the same clone is refused.
        assert!(f.problem().is_some());
        f.depth = "0".into();
        f.name = "other".into();
        assert!(f.problem().is_some());
        let _ = std::fs::remove_dir_all(&base);
    }
}
