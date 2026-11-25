//! Window frame: top toolbar with menus, side stripes, status bar, welcome screen, notifications.

use crate::app::{App, CommitTab, Dialog, Job, Tab, short};
use crate::i18n::tr;
use crate::git::{self, RefKind};
use crate::theme::{self, Icon};
use eframe::egui::{
    self, Color32, FontId, Key, RichText, Sense, Stroke, Ui, pos2, vec2,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Tint at the top-left corner of the window; the title bar and the left stripe fade from it
/// (chosen in Settings → Window).
fn title_tint(s: &crate::settings::Settings) -> Color32 {
    let [r, g, b] = s.title_tint();
    theme::rgb(r, g, b)
}

impl App {
    pub fn handle_keys(&mut self, ctx: &egui::Context) {
        // In the settings a key may be on its way to the Keymap page.
        if self.show_settings {
            return;
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        for (a, shortcut) in crate::keymap::bound(&self.settings.keys) {
            if (a.repo && (self.repo.is_none() || self.dialog.is_some())) || (a.not_typing && typing) {
                continue;
            }
            // The palette is not opened over another window.
            if a.id == "palette" && self.dialog.is_some() {
                continue;
            }
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.run_action(a.id);
            }
        }
    }

    /// Find: in a diff its text is searched, in the merge window its three versions, otherwise
    /// the cursor goes to the search field of the Log.
    pub fn find_action(&mut self) {
        {
            // In a diff (a diff tab, or the diff of a file history): search its text.
            let history = self.tab == Tab::Log && self.repo.as_ref().is_some_and(|r| r.follow && r.path_filter.is_some());
            let diff = match self.tab {
                Tab::Diff(i) => self.diffs.get_mut(i),
                _ if history => self.history_diff.as_mut().and_then(|(_, d)| d.as_mut()),
                _ => None,
            };
            // The merge window is in front of everything else.
            let merge = self.merge_open.is_some().then_some(&mut self.merge_find);
            if let Some(mf) = merge {
                match mf.as_mut() {
                    Some(f) => f.focus = true,
                    None => *mf = Some(crate::find::Find::opened()),
                }
            } else {
            match diff.filter(|d| d.diff.images.is_none() && !d.diff.binary) {
                Some(d) => match d.find.as_mut() {
                    Some(f) => f.focus = true,
                    None => d.find = Some(crate::find::Find::opened()),
                },
                None => {
                    self.tab = Tab::Log;
                    self.focus_search = true;
                }
            }
            }
        }
    }

    /// Add the unversioned files selected in the Changes list to Git (Ctrl+Alt+A).
    pub fn add_selected_to_git(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        if !self.show_commit {
            return;
        }
        let mut chosen = repo.status_multi.clone();
        chosen.extend(repo.status_selected.clone());
        let paths: Vec<String> = repo.status.iter().filter(|e| e.is_untracked() && chosen.contains(&e.path)).map(|e| e.path.clone()).collect();
        if paths.is_empty() {
            self.notify(crate::app::Toast {
                title: tr("Add to Git").into(),
                text: tr("Select new (unversioned) files in the Changes list first.").into(),
                ok: true,
                at: Instant::now(),
            });
            return;
        }
        let n = paths.len();
        let mut a = vec!["add".to_string(), "--".into()];
        a.extend(paths);
        self.run_job(Job::many(if n > 1 { "Add files" } else { "Add file" }, vec![a]));
    }

    pub fn open_commit(&mut self) {
        self.show_commit = true;
        self.commit_tab = CommitTab::Changes;
        self.focus_message = true;
    }

    pub fn watch(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.last_focus {
            self.refresh_soon();
        }
        self.last_focus = focused;
        if self.last_check.elapsed() > Duration::from_secs(2) {
            self.last_check = Instant::now();
            if !self.busy() {
                let changed = self
                    .repo
                    .as_ref()
                    .map(|r| git::fingerprint(&r.path) != r.fingerprint)
                    .unwrap_or(false);
                if changed {
                    self.refresh_soon();
                }
            }
        }
        ctx.request_repaint_after(Duration::from_secs(2));
    }

    pub fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.frame += 1;
        if self.headless {
            return;
        }
        ctx.request_repaint();
        if self.frame == 120 {
            let slot = self.shot.clone();
            let c = ctx.clone();
            ctx.request_screenshot(move |img| {
                *slot.lock().unwrap() = Some(img);
                c.request_repaint();
            });
        }
        let img = self.shot.lock().unwrap().take();
        if let Some(img) = img {
            let [w, h] = img.size;
            let mut buf = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                buf.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
            }
            if let Some(dir) = path.parent() {
                if !dir.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(dir);
                }
            }
            if let Err(e) =
                image::save_buffer(&path, &buf, w as u32, h as u32, image::ExtendedColorType::Rgba8)
            {
                eprintln!("Could not save screenshot: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.frame > 1200 {
            eprintln!("Screenshot timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub fn title_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let left = title_tint(self.settings_draft.as_ref().map(|d| &d.s).unwrap_or(&self.settings));
        let right = theme::BG_BAR();
        ui.painter().rect_filled(rect, 0.0, right);
        let mut mesh = egui::Mesh::default();
        let mid_x = rect.left() + rect.width() * 0.6;
        mesh.colored_vertex(rect.left_top(), left);
        mesh.colored_vertex(pos2(mid_x, rect.top()), right);
        mesh.colored_vertex(pos2(mid_x, rect.bottom()), right);
        mesh.colored_vertex(rect.left_bottom(), left);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(mesh);
        // Border under the title bar, except above the left stripe.
        theme::hline(ui, rect.bottom() - 0.5, rect.left() + 36.0, rect.right());
        // Built-in title bar: this bar is the window's title bar — drag to move,
        // double-click to maximise / restore. Buttons drawn later take clicks first.
        let own_frame = self.own_window_frame();
        if own_frame {
            let bar = ui.interact(rect, egui::Id::unique("title_drag"), Sense::click_and_drag());
            if bar.double_clicked() {
                let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            } else if bar.drag_started_by(egui::PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }

        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            ui.painter().rect_filled(r, 4.0, theme::rgb(0x2f, 0x5f, 0xd8));
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "RB",
                FontId::proportional(10.0),
                Color32::WHITE,
            );
            ui.add_space(8.0);
            // The menu bar takes only as much room as its menus need (measured on the previous frame).
            let start = ui.cursor().min.x;
            let w_id = egui::Id::unique("title_menus_width");
            let menus_w = ui.ctx().data(|d| d.get_temp::<f32>(w_id)).unwrap_or(140.0);
            let right = ui.allocate_ui(vec2(menus_w, 26.0), |ui| self.menus(ui)).inner;
            let used = (right - start).max(40.0);
            if (used - menus_w).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(w_id, used));
                ui.ctx().request_repaint();
            }
            ui.add_space(35.0);
            self.branch_widget(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(if own_frame { 2.0 } else { 8.0 });
                if own_frame {
                    window_buttons(ui);
                    ui.add_space(4.0);
                    let (r, _) = ui.allocate_exact_size(vec2(9.0, 20.0), Sense::hover());
                    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, theme::rgb(0x4e, 0x52, 0x58)));
                    ui.add_space(4.0);
                }
                // After the main buttons, behind a divider: notifications.
                // (Refresh is in the log's toolbar and on F5.)
                self.notifications_button(ui);
                if self.repo.is_some() {
                    // Show / hide the branches (left) and the commit details (right) of the log.
                    let icon = if self.show_details { Icon::PanelRightOn } else { Icon::PanelRight };
                    let tip = if self.show_details { "Hide Details" } else { "Show Details" };
                    if theme::icon_button(ui, icon, tip).clicked() {
                        self.show_details = !self.show_details;
                        if self.show_details {
                            self.tab = crate::app::Tab::Log;
                        }
                    }
                    let icon = if self.show_branches { Icon::PanelLeftOn } else { Icon::PanelLeft };
                    let tip = if self.show_branches { "Hide Branches" } else { "Show Branches" };
                    if theme::icon_button(ui, icon, tip).clicked() {
                        self.show_branches = !self.show_branches;
                        if self.show_branches {
                            self.tab = crate::app::Tab::Log;
                        }
                    }
                }
                if self.repo.is_some() {
                    ui.add_space(4.0);
                    let (r, _) = ui.allocate_exact_size(vec2(9.0, 20.0), Sense::hover());
                    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, theme::rgb(0x4e, 0x52, 0x58)));
                    ui.add_space(4.0);
                    if theme::icon_button(ui, Icon::Push, &self.tip_with_key(tr("Push…"), "push")).clicked() {
                        self.open_push_dialog();
                    }
                    if theme::icon_button(ui, Icon::Pull, &self.tip_with_key(tr("Pull / Update Project…"), "pull")).clicked() {
                        self.open_pull_dialog();
                    }
                    if theme::icon_button(ui, Icon::Fetch, tr("Fetch all remotes")).clicked() {
                        self.fetch();
                    }
                    ui.add_space(8.0);
                    if self.busy() {
                        ui.add(egui::Spinner::new().size(14.0));
                        ui.label(
                            RichText::new(format!("{}…", self.running.join(", ")))
                                .color(theme::TEXT_MID())
                                .size(12.0),
                        );
                    }
                }
            });
        });
    }

    /// The main menus; returns the right edge of the last one.
    fn menus(&mut self, ui: &mut Ui) -> f32 {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("File"), |ui| {
                if ui.button(self.menu_line("open")).clicked() {
                    self.choose_folder();
                }
                if ui.button(tr("Clone…")).on_hover_text(tr("Download a repository from a server (or copy one from a folder) and open it")).clicked() {
                    self.open_clone();
                }
                if ui.button(tr("New Repository…")).on_hover_text(tr("Make a folder a Git repository (git init) and open it")).clicked() {
                    self.new_repository();
                }
                ui.menu_button(tr("Recent Repositories"), |ui| {
                    let recent = self.settings.recent.clone();
                    match recent_list(ui, &recent, &self.settings.fav_repos) {
                        Some(RecentPick::Open(r)) => self.open(&r),
                        Some(RecentPick::Star(r)) => self.toggle_favorite_repo(&r),
                        None => {}
                    }
                });
                if ui.button(tr("Open Sample Repository")).clicked() {
                    if let Some(d) = crate::demo::ensure_demo() {
                        self.open(&d);
                    }
                }
                if ui.button(self.menu_line("settings")).clicked() {
                    self.show_settings = true;
                }
                ui.separator();
                if crate::integrate::SUPPORTED {
                    let label = if cfg!(windows) {
                        if crate::integrate::installed() {
                            "Update \"Open in RefBraid\" in Explorer"
                        } else {
                            "Add \"Open in RefBraid\" to Explorer"
                        }
                    } else if crate::integrate::installed() {
                        "Update \"Open With\" in Files (Nautilus)"
                    } else {
                        "Add to Files (Nautilus) \"Open With\""
                    };
                    if ui.button(label).clicked() {
                        self.install_integration();
                    }
                }
                ui.separator();
                if ui.button(tr("Close Repository")).clicked() {
                    match self.repo.as_ref().map(|r| r.path.clone()) {
                        Some(p) => self.close_repo_tab(&p),
                        None => self.repo = None,
                    }
                }
                if ui.button(tr("Exit")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            let has = self.repo.is_some();
            ui.add_enabled_ui(has, |ui| {
                ui.menu_button(tr("Git"), |ui| self.git_menu(ui));
            });
            ui.menu_button(tr("View"), |ui| {
                let mut fav = self.show_favorites;
                if ui.checkbox(&mut fav, tr("Favorite Repositories")).changed() {
                    self.toggle_favorites();
                }
                ui.checkbox(&mut self.show_commit, tr("Commit Panel"));
                ui.checkbox(&mut self.show_branches, tr("Branches"));
                ui.checkbox(&mut self.show_details, tr("Commit Details"));
                ui.separator();
                if ui.button(tr("Log")).clicked() {
                    self.tab = Tab::Log;
                }
                if ui.button(tr("Console")).clicked() {
                    self.tab = Tab::Console;
                }
                ui.separator();
                if ui.button(tr("Refresh    F5")).clicked() {
                    self.refresh();
                }
            });
            ui.menu_button(tr("Help"), |ui| {
                if ui.button(self.menu_line("palette")).clicked() {
                    self.run_action("palette");
                }
                if ui.button(tr("Keyboard Shortcuts")).clicked() {
                    self.shortcuts();
                }
                if ui.button(tr("Changelog")).clicked() {
                    self.dialog = Some(Dialog::Changelog);
                }
                self.update_menu_item(ui);
                ui.separator();
                if ui.button(tr("Open Merge Conflict Example")).on_hover_text(tr("A sample repository stopped by merge conflicts of every kind, to try resolving them")).clicked() {
                    self.open_conflict_example();
                }
                if ui.button(tr("About")).clicked() {
                    self.open_about();
                }
            })
            .response
            .rect
            .right()
        })
        .inner
    }

    /// The Keyboard Shortcuts window: the actions that have a shortcut now, then the keys that
    /// work in their place.
    pub fn shortcuts(&mut self) {
        let mut text = String::new();
        let mut group = "";
        for a in crate::keymap::ACTIONS {
            let key = self.key_of(a.id);
            if key.is_empty() {
                continue;
            }
            if a.group != group {
                if !text.is_empty() {
                    text.push('\n');
                }
                group = a.group;
            }
            text.push_str(&format!("{key}   {}\n", tr(a.name).trim_end_matches('…')));
        }
        text.push('\n');
        for (key, what) in crate::keymap::FIXED {
            text.push_str(&format!("{}   {}\n", crate::keymap::shown(key), tr(what)));
        }
        text.push_str(&format!("\n{}", tr("Shortcuts are changed in Settings → Keymap; every action is also in Help → Find Action.")));
        self.dialog = Some(Dialog::Message { title: tr("Keyboard shortcuts").into(), text });
    }

    pub fn git_menu(&mut self, ui: &mut Ui) {
        // Undo of the last action (commit, reset, merge, checkout, deleted branch, dropped stash…).
        match (&self.undo, &self.undo_note) {
            (Some(rec), _) => {
                let label = format!("{} {}…", tr(if rec.redo { "Redo" } else { "Undo" }), rec.title);
                if ui.add(egui::Button::new(label).shortcut_text(self.key_of("undo"))).on_hover_text(tr("Put the branches back where they were before the last action; you see what will be done first")).clicked() {
                    self.open_undo();
                }
            }
            (None, Some(pushed)) => {
                ui.add_enabled(false, egui::Button::new(format!("{} {pushed}", tr("Undo")))).on_disabled_hover_text(tr("The last action sent commits to the server; that cannot be undone from here"));
            }
            (None, None) => {
                ui.add_enabled(false, egui::Button::new(tr("Undo"))).on_disabled_hover_text(tr("Nothing to undo yet: the last action that moved a branch, a tag or a stash can be undone here"));
            }
        }
        ui.separator();
        if ui.button(self.menu_line("commit")).clicked() {
            self.open_commit();
        }
        if ui.button(self.menu_line("push")).clicked() {
            self.open_push_dialog();
        }
        if ui.button(self.menu_line("pull")).clicked() {
            self.open_pull_dialog();
        }
        if ui.button(tr("Fetch")).clicked() {
            self.fetch();
        }
        ui.separator();
        if ui.button(tr("New Branch…")).clicked() {
            self.open_new_branch("HEAD");
        }
        if ui.button(tr("New Tag…")).clicked() {
            self.dialog = Some(Dialog::NewTag {
                rev: "HEAD".into(),
                name: String::new(),
                message: String::new(),
            });
        }
        if ui.button(tr("Reflog")).on_hover_text(tr("Where HEAD has been: find and bring back commits lost after a reset, a rebase or a deleted branch")).clicked() {
            self.open_reflog();
        }
        if ui.button(tr("Checkout Tag or Revision…")).clicked() {
            self.dialog = Some(Dialog::Checkout { rev: String::new() });
        }
        if ui.button(tr("Bisect…")).on_hover_text(tr("Find the commit that broke something by trying commits between a good and a bad one")).clicked() {
            self.open_bisect_dialog(None, None);
        }
        ui.separator();
        if ui.button(tr("Stash Changes…")).clicked() {
            self.open_stash_dialog();
        }
        if ui.button(tr("Unstash Changes…")).clicked() {
            self.show_commit = true;
            self.commit_tab = CommitTab::Stash;
        }
        ui.separator();
        let open = self.repo.is_some();
        if ui.add_enabled(open, egui::Button::new(tr("Create Patch…"))).on_hover_text(tr("Save the local changes to a .patch file")).clicked() {
            self.open_create_patch(vec![]);
        }
        if ui.add_enabled(open, egui::Button::new(tr("Apply Patch…"))).on_hover_text(tr("Apply a .patch or .diff file (you can also drop one on the window)")).clicked() {
            self.choose_patch();
        }
        if ui.button(tr("Clean…")).on_hover_text(tr("Delete the files that are not in Git (git clean); the list is shown first")).clicked() {
            self.open_clean();
        }
        ui.add_enabled_ui(open, |ui| ui.menu_button(tr("LFS"), |ui| self.lfs_menu(ui)));
        self.hosting_menu_item(ui);
        if self.repo.as_ref().is_some_and(|r| !r.submodules.is_empty()) && ui.button(tr("Update Submodules")).on_hover_text(tr("Check out the recorded commit in every submodule, fetching the ones that are not there yet (git submodule update --init --recursive)")).clicked() {
            self.confirm_submodule_update(None);
            ui.close();
        }
        ui.separator();
        if ui.add_enabled(self.repo.is_some(), egui::Button::new(tr("Git Config…"))).on_hover_text(tr("Remotes, user name and email, and other git settings")).clicked() {
            self.open_git_config();
        }
        let op = self.repo.as_ref().and_then(|r| r.state.operation);
        if let Some(op) = op {
            ui.separator();
            if op != git::Operation::Merge && ui.button(format!("Continue {}", op.title())).clicked() {
                self.run_job(Job::one(
                    &format!("Continue {}", op.title()),
                    &[op.command(), "--continue"],
                ));
            }
            if ui.button(format!("Abort {}", op.title())).clicked() {
                self.run_job(Job::one(&format!("Abort {}", op.title()), &[op.command(), "--abort"]));
            }
        }
    }

    fn branch_widget(&mut self, ui: &mut Ui) {
        let Some(repo) = self.repo.as_ref() else { return };
        let label = repo
            .state
            .head_branch
            .clone()
            .unwrap_or_else(|| format!("{} (detached)", short(&repo.state.head_hash)));
        let (ahead, behind) = repo
            .current_branch()
            .map(|b| (b.ahead, b.behind))
            .unwrap_or((0, 0));
        // Counts coloured like in the Branches tree: behind blue, ahead green.
        let behind_t = format!("  ↙{behind}");
        let ahead_t = format!("  ↗{ahead}");
        let mut parts: Vec<(&str, egui::Color32)> = vec![(label.as_str(), theme::TEXT())];
        if behind > 0 {
            parts.push((behind_t.as_str(), theme::LINK()));
        }
        if ahead > 0 {
            parts.push((ahead_t.as_str(), theme::GREEN()));
        }
        parts.push(("  ⌄", theme::TEXT()));
        let resp = theme::text_button_parts(ui, Some(Icon::Branch), &parts);
        egui::Popup::menu(&resp)
            .width(340.0)
            .show(|ui| self.branches_popup(ui));
    }

    pub fn branches_popup(&mut self, ui: &mut Ui) {
        ui.set_min_width(320.0);
        if ui.button(tr("Update Project…")).clicked() {
            self.open_pull_dialog();
        }
        if ui.button(tr("Commit…")).clicked() {
            self.open_commit();
        }
        if ui.button(tr("Push…")).clicked() {
            self.open_push_dialog();
        }
        ui.separator();
        if ui.button(tr("New Branch…")).clicked() {
            self.open_new_branch("HEAD");
        }
        if ui.button(tr("Checkout Tag or Revision…")).clicked() {
            self.dialog = Some(Dialog::Checkout { rev: String::new() });
        }
        let Some(repo) = self.repo.as_ref() else { return };
        let branches = repo.state.branches.clone();
        let current = repo.state.head_branch.clone();
        for (title, kind) in [("Local", RefKind::Local), ("Remote", RefKind::Remote)] {
            ui.separator();
            ui.label(RichText::new(tr(title)).color(theme::TEXT_DIM()).size(11.0));
            // Only the root branches (main, master, stage…): no "/" and no digits in the name
            // (for remote ones, after the remote's name). The current branch is always listed.
            let all = branches.iter().filter(|b| b.kind == kind).count();
            let mut list: Vec<&git::Branch> = branches.iter().filter(|b| b.kind == kind && (b.is_head || is_root_branch(&b.name, kind))).collect();
            list.sort_by_key(|b| (!b.is_head, !self.is_favorite(&b.name), b.name.clone()));
            let hidden = all - list.len();
            if list.is_empty() {
                ui.label(RichText::new(tr("No root branches")).color(theme::TEXT_DIM()).size(12.0));
            }
            for b in list.into_iter() {
                let mut t = b.name.clone();
                if b.behind > 0 {
                    t.push_str(&format!("  ↙{}", b.behind));
                }
                if b.ahead > 0 {
                    t.push_str(&format!("  ↗{}", b.ahead));
                }
                let prefix = if b.is_head {
                    "◆ "
                } else if self.is_favorite(&b.name) {
                    "★ "
                } else {
                    "    "
                };
                ui.menu_button(format!("{prefix}{t}"), |ui| {
                    self.branch_actions(ui, &b.name, b.kind, current.as_deref());
                });
            }
            if hidden > 0 {
                ui.label(
                    RichText::new(format!("{hidden} more in the Branches panel"))
                        .color(theme::TEXT_DIM())
                        .size(11.5),
                );
            }
        }
    }


    pub fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(10.0);
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_stroke(
                r.shrink(1.0),
                2.0,
                Stroke::new(1.2, theme::ACCENT()),
                egui::StrokeKind::Middle,
            );
            match &self.repo {
                Some(r) => {
                    ui.label(RichText::new(&r.name).color(theme::TEXT_MID()).size(12.0));
                    ui.label(
                        RichText::new(r.path.display().to_string())
                            .color(theme::TEXT_DIM())
                            .size(11.5),
                    );
                }
                None => {
                    ui.label(RichText::new(tr("No repository")).color(theme::TEXT_MID()).size(12.0));
                }
            }
            // A running command: how far it is, and a way to stop it.
            let mut cancel = false;
            if let Some((line, share)) = self.job_progress() {
                ui.add_space(10.0);
                ui.add(egui::Spinner::new().size(12.0));
                let title = self.live_jobs.last().map(|j| j.title.clone()).unwrap_or_default();
                ui.label(RichText::new(format!("{title}…")).color(theme::TEXT()).size(12.0));
                if let Some(share) = share {
                    let (r, _) = ui.allocate_exact_size(vec2(90.0, 6.0), Sense::hover());
                    ui.painter().rect_filled(r, 3.0, theme::BG_INPUT());
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, vec2(r.width() * share.clamp(0.0, 1.0), r.height())), 3.0, theme::ACCENT());
                }
                if ui.add(egui::Button::new(RichText::new(tr("Cancel")).size(11.5)).small()).on_hover_text(tr("Stop the running git command")).clicked() {
                    cancel = true;
                }
                if !line.is_empty() {
                    ui.add(egui::Label::new(RichText::new(line).color(theme::TEXT_MID()).size(11.5)).truncate());
                }
            }
            if cancel {
                self.cancel_jobs();
            }
            if self.log_loading() {
                ui.add_space(10.0);
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(RichText::new(tr("Loading the log…")).color(theme::TEXT_MID()).size(12.0));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                if let Some(repo) = &self.repo {
                    let (a, b) = repo
                        .current_branch()
                        .map(|b| (b.ahead, b.behind))
                        .unwrap_or((0, 0));
                    let branch = repo.state.head_branch.clone().unwrap_or_else(|| "detached".into());
                    ui.label(RichText::new(branch).color(theme::TEXT_MID()).size(12.0));
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("{a}↑ {b}↓")).color(theme::TEXT_MID()).size(12.0));
                    let changes = repo.status.len();
                    if changes > 0 {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("{changes} {}", crate::i18n::plural(changes, "local change", "local changes")))
                            .color(theme::TEXT_MID())
                            .size(12.0),
                        );
                    }
                    if let Some(op) = repo.state.operation {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("{} in progress", op.title()))
                                .color(theme::STAR())
                                .size(12.0),
                        );
                    }
                }
            });
        });
    }

    pub fn left_stripe(&mut self, ui: &mut Ui) {
        // Same gradient as the title bar, running down instead of across.
        let rect = ui.max_rect();
        let window_h = ui.ctx().content_rect().height();
        let fade_to = (rect.top() + window_h * 0.6).min(rect.bottom());
        let mut mesh = egui::Mesh::default();
        let tint = title_tint(self.settings_draft.as_ref().map(|d| &d.s).unwrap_or(&self.settings));
        mesh.colored_vertex(rect.left_top(), tint);
        mesh.colored_vertex(rect.right_top(), tint);
        mesh.colored_vertex(pos2(rect.right(), fade_to), theme::BG_BAR());
        mesh.colored_vertex(pos2(rect.left(), fade_to), theme::BG_BAR());
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(mesh);
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            if theme::icon_button_sized(ui, Icon::StarOutline, tr("Favorite Repositories"), 26.0, self.show_favorites, true).clicked() {
                self.toggle_favorites();
            }
            ui.add_space(4.0);
            if theme::icon_button_sized(ui, Icon::Folder, tr("Files: the folders and files of the checked-out branch"), 26.0, self.show_files, true).clicked() {
                self.toggle_files();
            }
            // (No "Git Log" button: the log is always on screen.)
            ui.add_space(4.0);
            // Commit and Stashes open the same side panel on its Changes / Stash tab;
            // the button of the open tab is highlighted, clicking it again closes the panel.
            let commit_sel = self.show_commit && self.commit_tab == CommitTab::Changes;
            if theme::icon_button_sized(ui, Icon::Commit, &self.tip_with_key(tr("Commit"), "commit"), 26.0, commit_sel, true)
                .clicked()
            {
                self.show_commit = !commit_sel;
                self.commit_tab = CommitTab::Changes;
            }
            ui.add_space(4.0);
            let stash_sel = self.show_commit && self.commit_tab == CommitTab::Stash;
            if theme::icon_button_sized(ui, Icon::Stash, tr("Stashes"), 26.0, stash_sel, true).clicked() {
                self.show_commit = !stash_sel;
                self.commit_tab = CommitTab::Stash;
            }
            ui.add_space(4.0);
            let more = theme::icon_button_sized(ui, Icon::DotsH, tr("More"), 26.0, false, true);
            egui::Popup::menu(&more).show(|ui| {
                ui.set_min_width(240.0);
                let path = self.repo.as_ref().map(|r| r.path.clone());
                if ui
                    .add_enabled(path.is_some(), egui::Button::new(tr("Open Repository Location")))
                    .on_hover_text(tr("Show the repository folder in the file manager"))
                    .clicked()
                {
                    if let Some(p) = path {
                        open_in_file_manager(&p);
                    }
                    ui.close();
                }
            });
            let rest = ui.available_height();
            if rest > 80.0 {
                ui.add_space(rest - 66.0);
                if theme::icon_button_sized(ui, Icon::Console, tr("Git Console"), 26.0, self.tab == Tab::Console, true)
                    .clicked()
                {
                    self.tab = Tab::Console;
                }
                ui.add_space(4.0);
                if theme::icon_button_sized(ui, Icon::Info, tr("Keyboard shortcuts"), 26.0, false, true).clicked() {
                    self.shortcuts();
                }
            }
        });
    }

    /// The bell: recent notifications (results of push, pull, fetch, commit…).
    fn notifications_button(&mut self, ui: &mut Ui) {
        let tip = if self.unread > 0 { format!("Notifications ({} new)", self.unread) } else { "Notifications".to_string() };
        let r = theme::icon_button(ui, Icon::Bell, &tip);
        if self.unread > 0 {
            ui.painter().circle_filled(r.rect.right_top() + vec2(-5.0, 5.0), 3.5, theme::ACCENT());
        }
        if r.clicked() {
            self.unread = 0;
        }
        egui::Popup::menu(&r)
            .width(380.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(380.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Notifications")).strong().color(theme::TEXT()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.notices.is_empty() && ui.small_button("Clear").clicked() {
                            self.notices.clear();
                        }
                        if ui.small_button("Git Console").on_hover_text(tr("Every git command and its output")).clicked() {
                            self.tab = Tab::Console;
                            ui.close();
                        }
                    });
                });
                ui.separator();
                if self.notices.is_empty() {
                    ui.label(RichText::new(tr("No notifications yet. Results of fetch, pull, push, commit and other actions appear here.")).color(theme::TEXT_DIM()).size(12.5));
                    return;
                }
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for (k, n) in self.notices.iter().enumerate() {
                        if k > 0 {
                            ui.add_space(4.0);
                            ui.separator();
                        }
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(if n.ok { "✓" } else { "✕" }).color(if n.ok { theme::GREEN() } else { theme::RED() }));
                            ui.label(RichText::new(&n.title).color(theme::TEXT()).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(crate::app::format_date(n.when)).color(theme::TEXT_DIM()).size(11.5));
                            });
                        });
                        if !n.text.trim().is_empty() {
                            ui.add(egui::Label::new(RichText::new(n.text.trim()).color(theme::TEXT_MID()).size(12.0)).wrap());
                        }
                    }
                });
            });
    }

    pub fn welcome(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.16).max(20.0));
            let (r, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
            let tex = ui.ctx().load_texture(
                "logo",
                {
                    let d = crate::integrate::icon_data(64);
                    egui::ColorImage::from_rgba_unmultiplied([64, 64], &d.rgba)
                },
                Default::default(),
            );
            ui.painter().image(
                tex.id(),
                r,
                egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            ui.add_space(10.0);
            ui.label(RichText::new(tr("RefBraid")).size(24.0).color(theme::TEXT()));
            ui.label(
                RichText::new(tr("Open a folder with a Git repository to see its history, branches and changes."))
                    .color(theme::TEXT_MID()),
            );
            ui.add_space(18.0);
            static GIT_OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if let Some(e) = &self.error {
                ui.label(RichText::new(e).color(theme::RED()));
                ui.add_space(8.0);
            } else if !*GIT_OK.get_or_init(crate::git::available) {
                let text = if cfg!(windows) {
                    "Git was not found. Install Git for Windows (git-scm.com) to use RefBraid."
                } else {
                    "Git was not found. Install the \"git\" package to use RefBraid."
                };
                ui.label(RichText::new(text).color(theme::RED()));
                ui.add_space(8.0);
            }
            if ui
                .add(
                    egui::Button::new(RichText::new(tr("  Open Repository…  ")).color(Color32::WHITE))
                        .fill(theme::ACCENT()),
                )
                .clicked()
            {
                self.choose_folder();
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let w = 300.0;
                ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
                if ui.add(egui::Button::new(tr("Clone Repository…")).min_size(vec2(146.0, 0.0))).clicked() {
                    self.open_clone();
                }
                if ui.add(egui::Button::new(tr("New Repository…")).min_size(vec2(146.0, 0.0))).clicked() {
                    self.new_repository();
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let w = 520.0;
                ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.open_input)
                        .hint_text(tr("…or paste a folder path and press Enter"))
                        .desired_width(w - 70.0),
                );
                let r = theme::edit_menu(r);
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if ui.button(tr("Open")).clicked() || enter {
                    let p = PathBuf::from(self.open_input.trim());
                    self.open(&p);
                }
            });
            ui.add_space(6.0);
            if ui.link(tr("Open a sample repository to try things out")).clicked() {
                if let Some(d) = crate::demo::ensure_demo() {
                    self.open(&d);
                }
            }
            if crate::integrate::SUPPORTED && !crate::integrate::installed() {
                ui.add_space(4.0);
                if ui
                    .link(if cfg!(windows) {
                        "Add \"Open in RefBraid\" to the folder menu in Explorer"
                    } else {
                        "Add \"Open With → RefBraid\" to folders in the Files app"
                    })
                    .clicked()
                {
                    self.install_integration();
                }
            }
            if !self.settings.recent.is_empty() {
                ui.add_space(22.0);
                ui.label(RichText::new(tr("Recent repositories")).color(theme::TEXT_DIM()));
                ui.add_space(4.0);
                // Rows highlight on hover like the title bar's buttons: name, then the folder.
                let recent: Vec<PathBuf> = self.settings.recent.iter().filter(|r| r.exists()).cloned().collect();
                let font = FontId::proportional(13.0);
                let name_of = |r: &PathBuf| r.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let name_w = recent.iter().map(|r| theme::text_width(ui, &name_of(r), &font)).fold(0.0, f32::max);
                let path_w = recent.iter().map(|r| theme::text_width(ui, &r.display().to_string(), &font)).fold(0.0, f32::max);
                let row_w = (name_w + path_w + 44.0).min(ui.available_width() - 20.0);
                let mut open = None;
                for r in &recent {
                    let (rect, resp) = ui.allocate_exact_size(vec2(row_w, 26.0), Sense::click());
                    let p = ui.painter();
                    if resp.hovered() {
                        p.rect_filled(rect, 5.0, theme::hover_veil());
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let cy = rect.center().y;
                    let name_x = rect.left() + 10.0;
                    theme::paint_clipped(p, pos2(name_x, cy), &name_of(r), font.clone(), theme::TEXT(), name_w + 1.0);
                    let path_x = name_x + name_w + 20.0;
                    theme::paint_clipped(p, pos2(path_x, cy), &r.display().to_string(), font.clone(), theme::TEXT_MID(), rect.right() - path_x - 8.0);
                    if resp.clicked() {
                        open = Some(r.clone());
                    }
                }
                if let Some(r) = open {
                    self.open(&r);
                }
            }
        });
    }

    pub fn toasts(&mut self, ctx: &egui::Context) {
        self.toasts
            .retain(|t| t.at.elapsed() < Duration::from_secs(if t.ok { 5 } else { 12 }));
        let mut y = 0.0;
        let mut close = None;
        let screen = ctx.content_rect();
        let n = self.toasts.len();
        for i in (n.saturating_sub(3)..n).rev() {
            let t = &self.toasts[i];
            let area = egui::Area::new(egui::Id::unique(("toast", t.at)))
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(screen.right() - 400.0, screen.bottom() - 140.0 - y))
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(theme::rgb(0x2b, 0x2d, 0x30))
                        .stroke(Stroke::new(1.0, theme::INPUT_BORDER()))
                        .corner_radius(8)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_width(340.0);
                            ui.horizontal(|ui| {
                                let col = if t.ok { theme::GREEN() } else { theme::RED() };
                                ui.label(RichText::new(if t.ok { "✔" } else { "✖" }).color(col));
                                ui.label(RichText::new(&t.title).strong().color(theme::TEXT()));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if theme::icon_button_sized(ui, Icon::Close, tr("Close"), 18.0, false, true)
                                        .clicked()
                                    {
                                        close = Some(i);
                                    }
                                });
                            });
                            if !t.text.is_empty() {
                                ui.label(RichText::new(&t.text).color(theme::TEXT_MID()).size(12.0));
                            }
                        });
                });
            y += area.response.rect.height() + 8.0;
        }
        if let Some(i) = close {
            self.toasts.remove(i);
        }
        if !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

/// A file of the repository as a path of this system (git paths use "/").
pub fn repo_file(root: &std::path::Path, rel: &str) -> PathBuf {
    rel.split('/').filter(|c| !c.is_empty()).fold(root.to_path_buf(), |p, c| p.join(c))
}

/// A Windows path with "\" only. Git gives the repository's folder as "C:/Users/me/repo";
/// Explorer does not take that and opens the Documents folder instead, and other programs
/// stumble over a mix of both.
pub fn backslashed(path: &str) -> String {
    path.replace('/', "\\")
}

/// The path as programs of this system expect it (see [`backslashed`]; unchanged elsewhere).
pub fn native_path(path: &std::path::Path) -> PathBuf {
    if cfg!(windows) { PathBuf::from(backslashed(&path.to_string_lossy())) } else { path.to_path_buf() }
}

/// Show a folder in the system file manager (Files on Linux, Explorer on Windows, Finder on macOS).
pub fn open_in_file_manager(path: &std::path::Path) {
    let path = &native_path(path);
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut c = std::process::Command::new(program);
    crate::git::no_console(&mut c);
    if let Ok(mut child) = c
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        std::thread::spawn(move || child.wait());
    }
}

/// Minimise, maximise / restore and close, for the built-in title bar (right to left).
fn window_buttons(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let line = |hover_red: bool| Stroke::new(1.2, if hover_red { Color32::WHITE } else { theme::ICON() });
    for kind in ["close", "max", "min"] {
        let (r, resp) = ui.allocate_exact_size(vec2(40.0, 30.0), Sense::click());
        let hovered = resp.hovered();
        let red = kind == "close" && hovered;
        if red {
            ui.painter().rect_filled(r, 4.0, theme::rgb(0xc4, 0x2b, 0x1c));
        } else if hovered {
            ui.painter().rect_filled(r, 4.0, theme::hover_veil());
        }
        let c = r.center();
        let p = ui.painter();
        let st = line(red);
        let tip = match kind {
            "close" => {
                let d = 5.0;
                p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], st);
                p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], st);
                "Close"
            }
            "max" if maximized => {
                // Restore: two overlapping squares.
                let a = egui::Rect::from_center_size(c + vec2(-1.5, 1.5), vec2(8.0, 8.0));
                p.rect_stroke(a, 1.0, st, egui::StrokeKind::Middle);
                p.line_segment([pos2(a.left() + 3.0, a.top() - 3.0), pos2(a.right() + 3.0, a.top() - 3.0)], st);
                p.line_segment([pos2(a.right() + 3.0, a.top() - 3.0), pos2(a.right() + 3.0, a.bottom() - 3.0)], st);
                "Restore"
            }
            "max" => {
                p.rect_stroke(egui::Rect::from_center_size(c, vec2(10.0, 10.0)), 1.0, st, egui::StrokeKind::Middle);
                "Maximize"
            }
            _ => {
                p.line_segment([c + vec2(-5.0, 0.5), c + vec2(5.0, 0.5)], st);
                "Minimize"
            }
        };
        if resp.on_hover_text(tip).clicked() {
            match kind {
                "close" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                "max" => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)),
                _ => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            }
        }
    }
}

impl App {
    /// Built-in title bar on Linux: the window has no frame from the desktop, so the app's
    /// title bar carries the window buttons and the edges resize the window.
    pub fn own_window_frame(&self) -> bool {
        cfg!(target_os = "linux") && self.started_builtin_title_bar
    }

    /// Resize the frameless window by dragging its edges and corners.
    pub fn window_edges(&mut self, ctx: &egui::Context) {
        if !self.own_window_frame() || ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let r = ctx.content_rect();
        let m = 5.0;
        let (l, rt, t, b) = (pos.x - r.left() < m, r.right() - pos.x < m, pos.y - r.top() < m, r.bottom() - pos.y < m);
        use egui::viewport::ResizeDirection as D;
        let dir = match (l, rt, t, b) {
            (true, _, true, _) => D::NorthWest,
            (_, true, true, _) => D::NorthEast,
            (true, _, _, true) => D::SouthWest,
            (_, true, _, true) => D::SouthEast,
            (true, ..) => D::West,
            (_, true, ..) => D::East,
            (_, _, true, _) => D::North,
            (_, _, _, true) => D::South,
            _ => return,
        };
        let icon = match dir {
            D::North | D::South => egui::CursorIcon::ResizeVertical,
            D::East | D::West => egui::CursorIcon::ResizeHorizontal,
            D::NorthWest | D::SouthEast => egui::CursorIcon::ResizeNwSe,
            D::NorthEast | D::SouthWest => egui::CursorIcon::ResizeNeSw,
        };
        ctx.set_cursor_icon(icon);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    }
}

/// Recent repositories as rows: the name stands out, the folder is dimmed and lined up
/// in a second column. Returns the one that was clicked (and closes the menu).
enum RecentPick {
    Open(PathBuf),
    /// The star at the end of the row: add to / remove from Favorites.
    Star(PathBuf),
}

fn recent_list(ui: &mut Ui, recent: &[PathBuf], favs: &[crate::settings::FavRepo]) -> Option<RecentPick> {
    if recent.is_empty() {
        ui.label(RichText::new(tr("Nothing yet")).color(theme::TEXT_DIM()));
        return None;
    }
    let name_font = FontId::proportional(13.0);
    let path_font = FontId::proportional(12.0);
    let name_of = |r: &PathBuf| r.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name_w = recent.iter().map(|r| theme::text_width(ui, &name_of(r), &name_font)).fold(0.0, f32::max).min(260.0);
    let path_w = recent.iter().map(|r| theme::text_width(ui, &r.display().to_string(), &path_font)).fold(0.0, f32::max);
    let row_w = (name_w + path_w + 70.0).min(790.0);
    let mut open = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    for r in recent {
        let path = r.display().to_string();
        let (rect, resp) = ui.allocate_exact_size(vec2(row_w, 26.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 4.0, theme::HOVER());
        }
        let p = ui.painter();
        let cy = rect.center().y;
        let name_x = rect.left() + 8.0;
        theme::paint_clipped(p, pos2(name_x, cy), &name_of(r), name_font.clone(), theme::TEXT(), name_w + 1.0);
        let path_x = name_x + name_w + 20.0;
        let star_r = egui::Rect::from_center_size(pos2(rect.right() - 16.0, cy), vec2(20.0, 20.0));
        theme::paint_clipped(p, pos2(path_x, cy), &path, path_font.clone(), theme::TEXT_DIM(), star_r.left() - path_x - 6.0);
        // Star: filled for favourites, shown on hover for the others.
        let is_fav = favs.iter().any(|f| &f.path == r);
        let star = ui.interact(star_r, ui.scope_id().with(("recent_star", &path)), Sense::click());
        if is_fav || resp.hovered() || star.hovered() {
            if star.hovered() {
                ui.painter().rect_filled(star_r, 4.0, theme::SELECT_INACTIVE());
            }
            let (icon, col) = if is_fav { (Icon::Star, theme::STAR()) } else { (Icon::StarOutline, theme::TEXT_MID()) };
            theme::paint_icon(ui.painter(), star_r.shrink(3.0), icon, col);
        }
        let star = star.on_hover_text(if is_fav { "Remove from Favorites" } else { "Add to Favorites" });
        let resp = if theme::text_width(ui, &path, &path_font) > star_r.left() - path_x - 6.0 { resp.on_hover_text(&path) } else { resp };
        if star.clicked() {
            open = Some(RecentPick::Star(r.clone()));
        } else if resp.clicked() {
            open = Some(RecentPick::Open(r.clone()));
            ui.close();
        }
    }
    open
}

/// A "root" branch such as main, master or stage: no folder ("/") and no digits in its name.
/// For a remote branch the remote's name ("origin/") is not counted.
pub fn is_root_branch(name: &str, kind: RefKind) -> bool {
    let short = if kind == RefKind::Remote { name.split_once('/').map(|x| x.1).unwrap_or(name) } else { name };
    !short.is_empty() && short != "HEAD" && !short.contains('/') && !short.chars().any(|c| c.is_ascii_digit())
}

/// Like [`is_root_branch`] for a name that may start with one of `remotes` ("origin/main").
pub fn is_root_ref(name: &str, remotes: &[String]) -> bool {
    let remote = remotes.iter().any(|r| name.strip_prefix(r.as_str()).is_some_and(|rest| rest.starts_with('/')));
    is_root_branch(name, if remote { RefKind::Remote } else { RefKind::Local })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_refs() {
        let remotes = vec!["origin".to_string()];
        assert!(is_root_ref("origin/main", &remotes));
        assert!(is_root_ref("stage", &remotes));
        assert!(!is_root_ref("origin/fix/APP-1308", &remotes));
        assert!(!is_root_ref("feat/V-1", &remotes));
    }

    #[test]
    fn root_branches() {
        assert!(is_root_branch("stage", RefKind::Local));
        assert!(is_root_branch("main-merge", RefKind::Local));
        assert!(!is_root_branch("fix/footer-links-main", RefKind::Local));
        assert!(!is_root_branch("release-2", RefKind::Local));
        assert!(is_root_branch("origin/master", RefKind::Remote));
        assert!(!is_root_branch("origin/IDW-13312", RefKind::Remote));
        assert!(!is_root_branch("origin/bugfix/x", RefKind::Remote));
    }
}
