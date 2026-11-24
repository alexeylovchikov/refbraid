//! The right-click menu for branches (same order as the IDE), branch comparison and worktrees.

use crate::app::{App, Dialog, Job, Tab, file_name, short, status_color};
use crate::i18n::tr;
use crate::git::{self, ChangedFile, RefKind};
use crate::theme;
use eframe::egui::{self, RichText, Ui};

pub struct CompareView {
    pub title: String,
    /// Older side of the comparison.
    pub base: String,
    /// Newer side; `None` means the working tree.
    pub head: Option<String>,
    /// Commits in `head` that are not in `base`, and the other way round.
    pub only_head: Vec<(String, String)>,
    pub only_base: Vec<(String, String)>,
    pub files: Vec<ChangedFile>,
    pub show_files: bool,
}

pub struct WorktreeForm {
    pub from: String,
    pub path: String,
    pub new_branch: String,
    pub create_branch: bool,
}

fn log_range(dir: &std::path::Path, range: &str) -> Vec<(String, String)> {
    git::run(dir, &["log", "--format=%H%x00%s", "-n", "300", range])
        .stdout
        .lines()
        .filter_map(|l| l.split_once('\0').map(|(a, b)| (a.to_string(), b.to_string())))
        .collect()
}

fn item(ui: &mut Ui, text: impl Into<String>, shortcut: &str) -> bool {
    let b = egui::Button::new(text.into());
    let b = if shortcut.is_empty() { b } else { b.shortcut_text(RichText::new(shortcut).color(theme::TEXT_DIM())) };
    ui.add(b).clicked()
}

impl App {
    fn cur_upstream(&self) -> Option<String> {
        self.repo.as_ref()?.current_branch()?.upstream.clone()
    }

    fn branch_upstream(&self, name: &str) -> Option<String> {
        self.repo
            .as_ref()?
            .state
            .branches
            .iter()
            .find(|b| b.kind == RefKind::Local && b.name == name)?
            .upstream
            .clone()
    }

    /// Actions for one branch, shared by the branch popup in the toolbar and the branches tree.
    pub fn branch_actions(&mut self, ui: &mut Ui, name: &str, kind: RefKind, current: Option<&str>) {
        ui.set_min_width(270.0);
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            self.branch_actions_inner(ui, name, kind, current)
        });
    }

    fn branch_actions_inner(&mut self, ui: &mut Ui, name: &str, kind: RefKind, current: Option<&str>) {
        let is_current = kind == RefKind::Local && current == Some(name);
        let cur = current.unwrap_or("HEAD").to_string();

        if !is_current && item(ui, "Checkout", "") {
            self.checkout_branch(name, kind);
        }
        if item(ui, format!("New Branch from '{name}'…"), "") {
            self.open_new_branch(name);
        }
        if !is_current && item(ui, format!("Checkout and Rebase onto '{cur}'"), "") {
            self.checkout_and_rebase(name, kind, &cur);
        }
        ui.separator();
        if !is_current && item(ui, format!("Compare with '{cur}'"), "") {
            self.open_compare(&cur, Some(name));
        }
        if item(ui, "Show Diff with Working Tree", "") {
            self.open_compare(name, None);
        }
        ui.separator();
        if !is_current {
            if item(ui, format!("Rebase '{cur}' onto '{name}'"), "") {
                self.run_job(Job::one(&format!("Rebase onto {name}"), &["rebase", name]));
            }
            if item(ui, format!("Merge '{name}' into '{cur}'"), "") {
                self.run_job(Job::one(&format!("Merge {name}"), &["merge", "--no-edit", name]));
            }
            if ui.button(tr("Merge with Options…")).on_hover_text(tr("Choose how to merge: always a merge commit, fast-forward only or squash")).clicked() {
                self.open_merge_options(name, name);
            }
            ui.separator();
        }
        if item(ui, format!("New Worktree from '{name}'…"), "") {
            self.open_worktree(name, kind, is_current);
        }
        ui.separator();

        if kind == RefKind::Local {
            let up = self.branch_upstream(name);
            if item(ui, "Update", if is_current { "Ctrl+T" } else { "" }) {
                match (&up, is_current) {
                    (Some(_), true) => self.run_job(Job::one(&format!("Update {name}"), &["pull", "--no-edit"])),
                    (Some(up), false) => {
                        if let Some((remote, rb)) = up.split_once('/') {
                            self.run_job(Job::one(&format!("Update {name}"), &["fetch", remote, &format!("{rb}:{name}")]));
                        }
                    }
                    (None, _) => self.open_pull_dialog(),
                }
            }
            if item(ui, "Push…", if is_current { "Ctrl+Shift+K" } else { "" }) {
                if is_current {
                    self.open_push_dialog();
                } else {
                    let remote = self
                        .repo
                        .as_ref()
                        .and_then(|r| r.state.remotes.first().cloned())
                        .unwrap_or_else(|| "origin".into());
                    self.run_job(Job::one(&format!("Push {name}"), &["push", "-u", &remote, name]));
                }
            }
            if let Some(up) = up {
                ui.menu_button(format!("Tracked Branch '{up}'"), |ui| {
                    self.remote_actions(ui, &up, &cur, current.is_some());
                });
            }
            ui.separator();
            if item(ui, "Rename…", "F2") {
                self.dialog = Some(Dialog::RenameBranch { old: name.to_string(), new: name.to_string() });
            }
        } else {
            self.pull_items(ui, name, &cur, current.is_some());
            ui.separator();
        }

        let fav = self.is_favorite(name);
        if item(ui, if fav { "Remove from Favorites" } else { "Add to Favorites" }, "") {
            self.toggle_favorite(name);
        }
        if item(ui, "Show in Log", "") {
            self.tab = Tab::Log;
            self.navigate_to(name);
        }
        if item(ui, "Show Only This Branch in Log", "") {
            if let Some(r) = self.repo.as_mut() {
                r.branch_filter = Some(name.to_string());
                App::reload_log(r);
            }
            self.tab = Tab::Log;
        }
        if !is_current {
            ui.separator();
            let tracked_by_current = kind == RefKind::Remote && self.cur_upstream().as_deref() == Some(name);
            if ui.add_enabled(!tracked_by_current, egui::Button::new(tr("Delete"))).clicked() {
                self.dialog = Some(Dialog::DeleteBranch {
                    name: name.to_string(),
                    remote: kind == RefKind::Remote,
                    force: false,
                });
            }
        }
    }

    /// The submenu for the remote branch the current branch tracks.
    fn remote_actions(&mut self, ui: &mut Ui, up: &str, cur: &str, on_branch: bool) {
        ui.set_min_width(270.0);
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            self.remote_actions_inner(ui, up, cur, on_branch)
        });
    }

    fn remote_actions_inner(&mut self, ui: &mut Ui, up: &str, cur: &str, on_branch: bool) {
        if item(ui, "Checkout", "") {
            self.checkout_branch(up, RefKind::Remote);
        }
        if item(ui, format!("New Branch from '{up}'…"), "") {
            self.open_new_branch(up);
        }
        if item(ui, format!("Checkout and Rebase onto '{cur}'"), "") {
            self.checkout_and_rebase(up, RefKind::Remote, cur);
        }
        ui.separator();
        if item(ui, format!("Compare with '{cur}'"), "") {
            self.open_compare(cur, Some(up));
        }
        if item(ui, "Show Diff with Working Tree", "") {
            self.open_compare(up, None);
        }
        ui.separator();
        if item(ui, format!("Rebase '{cur}' onto '{up}'"), "") {
            self.run_job(Job::one(&format!("Rebase onto {up}"), &["rebase", up]));
        }
        if item(ui, format!("Merge '{up}' into '{cur}'"), "") {
            self.run_job(Job::one(&format!("Merge {up}"), &["merge", "--no-edit", up]));
        }
        if ui.button(tr("Merge with Options…")).on_hover_text(tr("Choose how to merge: always a merge commit, fast-forward only or squash")).clicked() {
            self.open_merge_options(up, up);
        }
        ui.separator();
        if item(ui, format!("New Worktree from '{cur}'…"), "") {
            self.open_worktree(cur, RefKind::Local, true);
        }
        ui.separator();
        self.pull_items(ui, up, cur, on_branch);
        ui.separator();
        let _ = ui.add_enabled(false, egui::Button::new(tr("Delete")));
    }

    fn pull_items(&mut self, ui: &mut Ui, remote_branch: &str, cur: &str, on_branch: bool) {
        let (remote, rb) = remote_branch.split_once('/').unwrap_or(("origin", remote_branch));
        let (remote, rb) = (remote.to_string(), rb.to_string());
        if ui.add_enabled(on_branch, egui::Button::new(format!("Pull into '{cur}' Using Rebase"))).clicked() {
            self.run_job(Job::one("Pull (rebase)", &["pull", "--no-edit", "--rebase", &remote, &rb]));
        }
        if ui.add_enabled(on_branch, egui::Button::new(format!("Pull into '{cur}' Using Merge"))).clicked() {
            self.run_job(Job::one("Pull (merge)", &["pull", "--no-edit", "--no-rebase", &remote, &rb]));
        }
    }

    fn checkout_and_rebase(&mut self, name: &str, kind: RefKind, onto: &str) {
        let Some(repo) = self.repo.as_ref() else { return };
        if kind == RefKind::Local {
            self.run_job(Job::one(&format!("Rebase {name}"), &["rebase", onto, name]));
            return;
        }
        let local = name.split_once('/').map(|x| x.1).unwrap_or(name).to_string();
        let exists = repo.state.branches.iter().any(|b| b.kind == RefKind::Local && b.name == local);
        let first = if exists {
            vec!["checkout".to_string(), local.clone()]
        } else {
            vec!["checkout".to_string(), "--track".into(), name.to_string()]
        };
        self.run_job(Job::many(
            &format!("Checkout and rebase {local}"),
            vec![first, vec!["rebase".into(), onto.to_string()]],
        ));
    }

    pub fn open_compare(&mut self, base: &str, head: Option<&str>) {
        let Some(repo) = self.repo.as_ref() else { return };
        let dir = repo.path.clone();
        let (title, only_head, only_base, files) = match head {
            Some(h) => {
                let files = git::parse_name_status(&git::run(&dir, &["diff", "--name-status", "-z", "-M", base, h]).stdout);
                (
                    format!("Compare '{h}' with '{base}'"),
                    log_range(&dir, &format!("{base}..{h}")),
                    log_range(&dir, &format!("{h}..{base}")),
                    files,
                )
            }
            None => {
                let files = git::parse_name_status(&git::run(&dir, &["diff", "--name-status", "-z", "-M", base]).stdout);
                (format!("Diff between '{base}' and the working tree"), vec![], vec![], files)
            }
        };
        self.compare = Some(CompareView {
            title,
            base: base.to_string(),
            head: head.map(String::from),
            show_files: head.is_none(),
            only_head,
            only_base,
            files,
        });
    }

    pub fn open_compare_file(&mut self, base: &str, head: Option<&str>, f: &ChangedFile) {
        let Some(repo) = self.repo.as_ref() else { return };
        let mut args = vec!["diff", "--no-ext-diff", "--submodule=short", "-M", "--unified=100000"];
        args.extend(git::ws_flag(git::diff_ws()));
        args.push(base);
        if let Some(h) = head {
            args.push(h);
        }
        args.push("--");
        if let Some(o) = &f.old_path {
            args.push(o);
        }
        args.push(&f.path);
        let diff = git::parse_diff(&git::run(&repo.path, &args).stdout);
        let diff = crate::submodule::replace_diff(&repo.path, &f.path, &diff).unwrap_or(diff);
        let diff = {
            use crate::lfs::Side;
            let old = f.old_path.as_deref().unwrap_or(&f.path);
            let after = match head {
                Some(h) => Side::Rev(h, &f.path),
                None if repo.path.join(&f.path).is_file() => Side::File(&f.path),
                None => Side::Nothing,
            };
            crate::lfs::replace_diff(&repo.path, &f.path, &diff, Side::Rev(base, old), after).unwrap_or(diff)
        };
        let other = head.unwrap_or("Working Tree");
        self.push_diff(file_name(&f.path), format!("{}  ·  {base} vs {other}", f.path), diff);
    }

    fn open_worktree(&mut self, from: &str, kind: RefKind, is_current: bool) {
        let Some(repo) = self.repo.as_ref() else { return };
        let safe: String = from
            .rsplit('/')
            .next()
            .unwrap_or(from)
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
            .collect();
        let parent = repo.path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| repo.path.clone());
        let path = parent.join(format!("{}-{safe}", repo.name)).display().to_string();
        // A branch can only be checked out in one place: the current one needs a new branch.
        let local_name = from.split_once('/').map(|x| x.1).unwrap_or(from).to_string();
        let local_exists = repo.state.branches.iter().any(|b| b.kind == RefKind::Local && b.name == local_name);
        let (create, new_branch) = if is_current {
            (true, format!("{from}-worktree"))
        } else if kind == RefKind::Remote && !local_exists {
            (true, local_name)
        } else {
            (false, String::new())
        };
        self.worktree = Some(WorktreeForm { from: from.to_string(), path, new_branch, create_branch: create });
    }

    pub fn branch_windows(&mut self, ctx: &egui::Context) {
        if self.worktree.is_some() {
            let mut keep = true;
            let mut job = None;
            let title = format!("New Worktree from '{}'", self.worktree.as_ref().unwrap().from);
            let closed = crate::form::show(ctx, "worktree", &title, 540.0, |ui, part| {
                let f = self.worktree.as_mut().unwrap();
                let valid = !f.path.trim().is_empty() && (!f.create_branch || !f.new_branch.trim().is_empty());
                if part == crate::form::Part::Footer {
                    let (ok, cancel) = crate::form::ok_cancel(ui, tr("Create"), valid, false);
                    if ok {
                        let mut args = vec!["worktree".to_string(), "add".into()];
                        if f.create_branch {
                            args.push("-b".into());
                            args.push(f.new_branch.trim().to_string());
                        }
                        args.push(f.path.trim().to_string());
                        args.push(f.from.clone());
                        job = Some(Job::many("New worktree", vec![args]));
                    }
                    keep &= !(ok || cancel);
                    return;
                }
                ui.label(
                    RichText::new(tr("A second folder with its own checkout of this repository, so you can work on two branches at once."))
                        .color(theme::TEXT_DIM())
                        .size(12.0),
                );
                ui.add_space(10.0);
                ui.label("Folder:");
                theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.path).desired_width(f32::INFINITY)));
                ui.add_space(6.0);
                ui.checkbox(&mut f.create_branch, tr("Create a new branch"));
                ui.add_enabled_ui(f.create_branch, |ui| {
                    theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.new_branch).desired_width(f32::INFINITY)));
                });
            });
            if closed {
                keep = false;
            }
            if !keep {
                self.worktree = None;
            }
            if let Some(j) = job {
                self.run_job(j);
            }
        }

        if self.compare.is_some() {
            let mut keep = true;
            let mut open_file: Option<ChangedFile> = None;
            let mut nav: Option<String> = None;
            let title = self.compare.as_ref().unwrap().title.clone();
            let closed = crate::form::show(ctx, "compare", &title, 680.0, |ui, part| {
                let c = self.compare.as_mut().unwrap();
                if part == crate::form::Part::Footer {
                    if crate::form::primary(ui, tr("Close"), true, false) {
                        keep = false;
                    }
                    return;
                }
                if c.head.is_some() {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut c.show_files, false, tr("Commits"));
                        ui.selectable_value(&mut c.show_files, true, format!("Files ({})", c.files.len()));
                    });
                    ui.add_space(4.0);
                }
                let h = (ui.ctx().content_rect().height() * 0.55).clamp(200.0, 520.0);
                egui::Frame::new()
                    .fill(theme::BG())
                    .stroke(egui::Stroke::new(1.0, theme::BORDER()))
                    .inner_margin(8)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(h + crate::form::grow(ui)).auto_shrink([false, true]).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if c.show_files {
                                if c.files.is_empty() {
                                    ui.label(RichText::new(tr("No differences")).color(theme::TEXT_DIM()));
                                }
                                for f in &c.files {
                                    let label = match &f.old_path {
                                        Some(o) => format!("{}   ← {o}", f.path),
                                        None => f.path.clone(),
                                    };
                                    let r = ui.add(
                                        egui::Button::new(RichText::new(label).color(status_color(f.status))).frame(false),
                                    );
                                    if r.on_hover_text(tr("Click to show the diff")).clicked() {
                                        open_file = Some(f.clone());
                                    }
                                }
                            } else {
                                let head = c.head.clone().unwrap_or_default();
                                for (title, list) in [
                                    (format!("Commits in '{head}' but not in '{}'", c.base), &c.only_head),
                                    (format!("Commits in '{}' but not in '{head}'", c.base), &c.only_base),
                                ] {
                                    ui.label(RichText::new(format!("{title}: {}", list.len())).color(theme::TEXT_MID()).strong());
                                    if list.is_empty() {
                                        ui.label(RichText::new(tr("none")).color(theme::TEXT_DIM()));
                                    }
                                    for (hash, subj) in list {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(short(hash)).monospace().color(theme::TEXT_DIM()));
                                            if ui.add(egui::Button::new(RichText::new(subj).color(theme::TEXT())).frame(false)).clicked() {
                                                nav = Some(hash.clone());
                                            }
                                        });
                                    }
                                    ui.add_space(8.0);
                                }
                            }
                        });
                    });
            });
            if closed {
                keep = false;
            }
            if let Some(f) = open_file {
                let (base, head) = {
                    let c = self.compare.as_ref().unwrap();
                    (c.base.clone(), c.head.clone())
                };
                self.open_compare_file(&base, head.as_deref(), &f);
                keep = false;
            }
            if let Some(h) = nav {
                self.tab = Tab::Log;
                self.navigate_to(&h);
                keep = false;
            }
            if !keep {
                self.compare = None;
            }
        }
    }
}
