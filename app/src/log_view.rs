//! The Log tab: tabs header, filter bar and the commit table with its graph.

use crate::app::{App, CompareSource, DateFilter, DiffTab, Dialog, Job, Tab, format_date, short};
use crate::i18n::tr;
use crate::details_view::{linkified, search_box};
use crate::git::{self, LabelKind, RefKind};
use crate::graph::End;
use crate::rebase::Preset;
use crate::theme::{self, Icon};
use eframe::egui::{self, Color32, FontId, Key, Rect, Sense, Stroke, Ui, pos2, vec2};

const ROW_H: f32 = 24.0;
const LANE: f32 = 14.0;
/// How far a cut graph line goes on past its row's middle before its arrowhead.
const ARROW_LEN: f32 = 9.0;

impl App {
    /// Keep a panel's width after the person drags its edge (saved once the mouse is released).
    pub fn remember_width(&mut self, ui: &Ui, width: f32, field: impl Fn(&mut crate::settings::Settings) -> &mut f32) {
        let dragging = ui.input(|i| i.pointer.any_down());
        let slot = field(&mut self.settings);
        if dragging || (*slot - width).abs() < 1.0 {
            return;
        }
        *slot = width.round();
        if self.screenshot.is_none() {
            self.settings.save();
        }
    }

    pub fn tool_window(&mut self, ui: &mut Ui) {
        // One pixel taller than the Commit / Favorites headers: those panels' frames start their
        // content a pixel lower, and this keeps the bottom lines of all headers level.
        egui::Panel::top("tw_header")
            .exact_size(35.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| self.tool_header(ui));
        self.conflict_banner(ui);
        self.bisect_banner(ui);
        match self.tab {
            Tab::Log => self.log_tab(ui),
            Tab::Console => self.console_tab(ui),
            Tab::Diff(i) => {
                // A diff of a commit's file keeps the commit's files at the right, like the Log.
                let keep = self.diffs.get(i).is_some_and(|d| d.from_commit) && self.show_details && self.repo.as_ref().is_some_and(|r| r.details.is_some());
                if keep {
                    self.details_side(ui);
                }
                self.diff_tab(ui, i)
            }
            Tab::Blame(i) => self.blame_tab(ui, i),
            Tab::File(i) => self.file_tab(ui, i),
            Tab::Reflog => self.reflog_tab(ui),
        }
    }

    fn tool_header(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        theme::hline(ui, rect.bottom() - 0.5, rect.left(), rect.right());
        ui.horizontal_centered(|ui| {
            ui.add_space(12.0);
            ui.label(egui::RichText::new(tr("Git")).strong().color(theme::TEXT()));
            ui.add_space(10.0);
            let mut close = None;
            let mut close_log = None;
            for i in 0..self.log_tabs.len() {
                let title = self.log_tab_title(i);
                let (c, x) = tab_button(ui, &title, self.tab == Tab::Log && self.active_log == i, i > 0);
                if c {
                    self.switch_log(i);
                }
                if x {
                    close_log = Some(i);
                }
            }
            if let Some(i) = close_log {
                self.close_log_tab(i);
            }
            if tab_button(ui, "Console", self.tab == Tab::Console, false).0 {
                self.tab = Tab::Console;
            }
            for i in 0..self.diffs.len() {
                let t = self.diffs[i].tab_title();
                let (c, x) = tab_button(ui, &t, self.tab == Tab::Diff(i), true);
                if c {
                    self.tab = Tab::Diff(i);
                }
                if x {
                    close = Some(i);
                }
            }
            if let Some(i) = close {
                self.close_diff(i);
            }
            let mut close = None;
            for i in 0..self.blames.len() {
                let t = self.blame_tab_title(i);
                let (c, x) = tab_button(ui, &t, self.tab == Tab::Blame(i), true);
                if c {
                    self.tab = Tab::Blame(i);
                }
                if x {
                    close = Some(i);
                }
            }
            if let Some(i) = close {
                self.close_blame(i);
            }
            let mut close = None;
            for i in 0..self.file_tabs.len() {
                let t = self.file_tab_title(i);
                let (c, x) = tab_button(ui, &t, self.tab == Tab::File(i), true);
                if c {
                    self.tab = Tab::File(i);
                }
                if x {
                    close = Some(i);
                }
            }
            if let Some(i) = close {
                self.close_file_tab(i);
            }
            if self.reflog.is_some() {
                let (c, x) = tab_button(ui, tr("Reflog"), self.tab == Tab::Reflog, true);
                if c {
                    self.tab = Tab::Reflog;
                }
                if x {
                    self.close_reflog();
                }
            }
            if theme::icon_button_sized(ui, Icon::Plus, tr("New Log tab"), 22.0, false, true).clicked() {
                self.new_log_tab();
            }
            let resp = theme::icon_button_sized(ui, Icon::ChevronDown, tr("Tabs"), 22.0, false, true);
            egui::Popup::menu(&resp).show(|ui| {
                for i in 0..self.log_tabs.len() {
                    if ui.button(self.log_tab_title(i)).clicked() {
                        self.switch_log(i);
                    }
                }
                if ui.button(tr("Console")).clicked() {
                    self.tab = Tab::Console;
                }
                for i in 0..self.diffs.len() {
                    if ui.button(self.diffs[i].tab_title()).clicked() {
                        self.tab = Tab::Diff(i);
                    }
                }
                for i in 0..self.blames.len() {
                    if ui.button(self.blame_tab_title(i)).clicked() {
                        self.tab = Tab::Blame(i);
                    }
                }
                for i in 0..self.file_tabs.len() {
                    if ui.button(self.file_tab_title(i)).clicked() {
                        self.tab = Tab::File(i);
                    }
                }
                if self.reflog.is_some() && ui.button(tr("Reflog")).clicked() {
                    self.tab = Tab::Reflog;
                }
                if !self.diffs.is_empty() {
                    ui.separator();
                    if ui.button(tr("Close All Diffs")).clicked() {
                        // Files edited and not saved stay, and the first one asks.
                        self.diffs.retain(|d| d.edit.as_ref().is_some_and(|e| e.e.modified()));
                        self.tab = Tab::Log;
                        if let Some(e) = self.diffs.first_mut().and_then(|d| d.edit.as_mut()) {
                            e.ask = Some(crate::files_view::Ask::Close);
                            self.tab = Tab::Diff(0);
                        }
                    }
                }
            });
        });
    }

    /// The changed files and details of the selected commit, at the right.
    fn details_side(&mut self, ui: &mut Ui) {
        let w = if self.settings.details_width > 0.0 { self.settings.details_width } else { 400.0 };
        let r = egui::Panel::right("details")
            .default_size(w)
            .min_size(240.0)
            // The commit list keeps at least about 300 points.
            .max_size((ui.available_width() - 300.0).max(240.0))
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| self.details_panel(ui));
        let w = r.response.rect.width();
        self.remember_width(ui, w, |s| &mut s.details_width);
    }

    fn log_tab(&mut self, ui: &mut Ui) {
        if self.repo.as_ref().is_some_and(|r| r.follow && r.path_filter.is_some()) {
            self.history_tab(ui);
            return;
        }
        if self.show_branches {
            let w = if self.settings.branches_width > 0.0 { self.settings.branches_width } else { 340.0 };
            let r = egui::Panel::left("branches")
                .default_size(w)
                .min_size(200.0)
                .max_size((ui.available_width() - if self.show_details { 540.0 } else { 300.0 }).clamp(200.0, 640.0))
                .resizable(true)
                .frame(egui::Frame::new().fill(theme::BG()))
                .show(ui, |ui| self.branches_panel(ui));
            let w = r.response.rect.width();
            self.remember_width(ui, w, |s| &mut s.branches_width);
        }
        if self.show_details {
            self.details_side(ui);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| {
                let r = ui.max_rect();
                theme::vline(ui, r.left() + 0.5, r.top(), r.bottom());
                self.filter_bar(ui);
                self.commit_table(ui);
            });
    }

    /// "Show History" of a file: the commits that changed it at the left, and the change the
    /// selected commit made to the file at the right, which follows the selection.
    fn history_tab(&mut self, ui: &mut Ui) {
        let avail = ui.available_width();
        egui::Panel::left("history_commits")
            .default_size((avail * 0.42).clamp(360.0, 760.0))
            .min_size(300.0)
            .max_size((avail - 320.0).max(300.0))
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| {
                self.filter_bar(ui);
                if !self.show_details {
                    self.commit_table(ui);
                    return;
                }
                // The commits above, the selected commit's details below (hidden by the same
                // button as the details of the Log; the height is remembered on its own).
                let total = ui.available_rect_before_wrap();
                let max_h = (total.height() - 90.0).max(80.0);
                let auto_h = (total.height() * 0.3).max(150.0);
                let h = if self.settings.history_info_height > 0.0 { self.settings.history_info_height } else { auto_h }.clamp(80.0, max_h);
                let split = total.bottom() - h;
                let handle = Rect::from_min_max(pos2(total.left(), split - 3.0), pos2(total.right(), split + 3.0));
                let resp = ui.interact(handle, egui::Id::unique("history_info_split"), Sense::drag());
                if resp.hovered() || resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if resp.dragged() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        self.settings.history_info_height = (total.bottom() - p.y).clamp(80.0, max_h);
                    }
                }
                if resp.drag_stopped() && self.screenshot.is_none() {
                    self.settings.save();
                }
                let top = Rect::from_min_max(total.min, pos2(total.right(), split));
                let bottom = Rect::from_min_max(pos2(total.left(), split), total.max);
                let mut tu = ui.new_child(egui::UiBuilder::new().max_rect(top));
                tu.set_clip_rect(top);
                self.commit_table(&mut tu);
                let mut bu = ui.new_child(egui::UiBuilder::new().max_rect(bottom));
                bu.set_clip_rect(bottom);
                self.commit_info(&mut bu);
                let color = if resp.hovered() || resp.dragged() { theme::ACCENT() } else { theme::BORDER() };
                ui.painter().hline(total.x_range(), split, Stroke::new(if color == theme::ACCENT() { 2.0 } else { 1.0 }, color));
            });
        self.sync_history_diff();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| {
                let r = ui.max_rect();
                theme::vline(ui, r.left() + 0.5, r.top(), r.bottom());
                let shown = self.history_diff.as_mut().and_then(|(_, d)| d.take());
                match shown {
                    Some(d) => {
                        // Drawn by the diff viewer as one more (hidden) diff tab.
                        self.diffs.push(d);
                        let k = self.diffs.len() - 1;
                        self.diff_tab(ui, k);
                        if self.tab == Tab::Diff(k) {
                            self.tab = Tab::Log;
                        }
                        if k < self.diffs.len() {
                            let d = self.diffs.remove(k);
                            if let Some((_, slot)) = self.history_diff.as_mut() {
                                *slot = Some(d);
                            }
                        }
                    }
                    None => {
                        let text = if self.repo.as_ref().is_some_and(|r| r.selected.is_some()) {
                            tr("This commit did not change the file itself")
                        } else {
                            tr("Select a commit to see how it changed the file")
                        };
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(13.0), theme::TEXT_DIM());
                    }
                }
            });
    }

    /// Load the file's diff for the selected commit when the selection changed.
    fn sync_history_diff(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(d) = repo.details.as_ref() else {
            self.history_diff = None;
            return;
        };
        // The file's name in this commit (it may have been renamed since).
        let name = repo.file_names.get(&d.hash).cloned().or_else(|| repo.path_filter.clone()).unwrap_or_default();
        // The commit and the file: another history tab may show the same commit for another file.
        let key = format!("{}\0{name}", d.hash);
        if self.history_diff.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let parent = d.parents.first();
        let file = d.files.iter().chain(d.merge_files.iter()).find(|f| f.path == name || f.old_path.as_deref() == Some(name.as_str()));
        // Keep the viewer's choices (side-by-side, folding, divider) from the previous commit.
        let prev = self.history_diff.take().and_then(|(_, d)| d);
        let tab = file.map(|f| {
            let diff = git::commit_file_diff(&repo.path, &d.hash, parent.map(|s| s.as_str()), f);
            let jump = diff.rows.iter().position(|r| r.kind != git::DiffSide::Context);
            DiffTab {
                title: crate::app::file_name(&f.path),
                subtitle: format!("{}  ·  {} vs {}", f.path, short(&d.hash), parent.map(|p| short(p)).unwrap_or("(empty)")),
                diff,
                side_by_side: prev.as_ref().map_or(true, |p| p.side_by_side),
                jump,
                image: None,
                worktree: None,
                collapse: prev.as_ref().map_or(true, |p| p.collapse),
                expanded: Default::default(),
                preview: false,
                preview_followed: None,
                from_commit: true,
                source: Some(CompareSource::Commit { hash: d.hash.clone(), parent: parent.cloned(), path: f.path.clone(), old_path: f.old_path.clone() }),
                split: prev.as_ref().map_or(0.5, |p| p.split),
                hscroll: 0.0,
                text_sel: None,
                blame: None,
                syntax: None,
                reading: false,
                edit: None,
                read_only: None,
                // The search stays open from one commit to the next.
                find: prev.as_ref().and_then(|p| p.find.clone()).map(|mut f| {
                    f.cache = None;
                    f
                }),
            }
        });
        self.history_diff = Some((key, tab));
    }

    fn filter_bar(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        let bar = Rect::from_min_size(full.min, vec2(full.width(), 34.0));
        let cherry_ok = self.can_cherry_pick();
        let focus = std::mem::take(&mut self.focus_search);
        let Some(repo) = self.repo.as_mut() else { return };
        let mut changed = false;
        let mut reload = false;
        // Right-hand icons take a fixed strip; the filters get the rest and are clipped.
        let right_w = if bar.width() > 760.0 { 150.0 } else { 34.0 };
        let left_rect = Rect::from_min_max(bar.min, pos2(bar.right() - right_w, bar.bottom()));
        let mut bui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(left_rect.shrink2(vec2(6.0, 4.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        bui.set_clip_rect(left_rect);
        {
            let ui = &mut bui;
            let before = repo.text_filter.clone();
            let sw = (left_rect.width() * 0.32).clamp(150.0, 230.0);
            let toggles = search_box(ui, &mut repo.text_filter, tr("Text or hash"), sw, Some((&mut repo.regex, &mut repo.case)));
            if focus {
                ui.memory_mut(|m| m.request_focus(egui::Id::unique("search:Text or hash")));
            }
            if repo.deep != 0 && (before != repo.text_filter || toggles) {
                // Another text: search the loaded commits again; the whole history on request.
                if before != repo.text_filter {
                    repo.deep = 0;
                }
                reload = true;
            }
            if before != repo.text_filter || toggles {
                changed = true;
            }
            ui.add_space(4.0);
            let dim = |on: bool| if on { theme::TEXT() } else { theme::TEXT_MID() };
            let history = repo.follow && repo.path_filter.is_some();

            // A file history shows one file along one line of history: no branch and path filters.
            if !history {
                let label = match &repo.branch_filter {
                    Some(b) => format!("{}: {b}  ⌄", tr("Branch")),
                    None => format!("{}  ⌄", tr("Branch")),
                };
                let resp = theme::text_button(ui, None, &label, dim(repo.branch_filter.is_some()));
                egui::Popup::menu(&resp).show(|ui| {
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        if ui.button(tr("All branches")).clicked() {
                            repo.branch_filter = None;
                            reload = true;
                        }
                        if ui.button(tr("HEAD (current branch)")).clicked() {
                            repo.branch_filter = Some("HEAD".into());
                            reload = true;
                        }
                        for kind in [RefKind::Local, RefKind::Remote] {
                            ui.separator();
                            for b in repo.state.branches.iter().filter(|b| b.kind == kind) {
                                if ui.button(&b.name).clicked() {
                                    repo.branch_filter = Some(b.name.clone());
                                    reload = true;
                                }
                            }
                        }
                    });
                });
            }

            let label = match &repo.user_filter {
                Some(u) => format!("{}: {u}  ⌄", tr("User")),
                None => format!("{}  ⌄", tr("User")),
            };
            let resp = theme::text_button(ui, None, &label, dim(repo.user_filter.is_some()));
            egui::Popup::menu(&resp).show(|ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    if ui.button(tr("All users")).clicked() {
                        repo.user_filter = None;
                        changed = true;
                    }
                    if !repo.state.user_name.is_empty()
                        && ui.button(format!("me ({})", repo.state.user_name)).clicked()
                    {
                        repo.user_filter = Some(repo.state.user_name.clone());
                        changed = true;
                    }
                    ui.separator();
                    for a in repo.authors.clone() {
                        if ui.button(&a).clicked() {
                            repo.user_filter = Some(a);
                            changed = true;
                        }
                    }
                });
            });

            let label = if repo.date_filter == DateFilter::All {
                format!("{}  ⌄", tr("Date"))
            } else {
                format!("{}: {}  ⌄", tr("Date"), tr(repo.date_filter.label()))
            };
            let resp = theme::text_button(ui, None, &label, dim(repo.date_filter != DateFilter::All));
            egui::Popup::menu(&resp).show(|ui| {
                for d in [DateFilter::All, DateFilter::Day, DateFilter::Week, DateFilter::Month, DateFilter::Year] {
                    if ui.button(tr(d.label())).clicked() {
                        repo.date_filter = d;
                        changed = true;
                    }
                }
            });

            if !history {
                let label = match &repo.path_filter {
                    Some(p) => format!("{}: {p}  ⌄", tr("Paths")),
                    None => format!("{}  ⌄", tr("Paths")),
                };
                let resp = theme::text_button(ui, None, &label, dim(repo.path_filter.is_some()));
                egui::Popup::menu(&resp)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.label("Show only commits that touch this file or folder:");
                        theme::edit_menu(ui.add(
                            egui::TextEdit::singleline(&mut repo.path_input)
                                .hint_text(tr("e.g. src/ or README.md"))
                                .desired_width(260.0),
                        ));
                        ui.horizontal(|ui| {
                            if ui.button(tr("Apply")).clicked() {
                                let p = repo.path_input.trim().to_string();
                                repo.path_filter = if p.is_empty() { None } else { Some(p) };
                                repo.follow = false;
                                reload = true;
                                ui.close();
                            }
                            if ui.button(tr("Clear")).clicked() {
                                repo.path_filter = None;
                                repo.path_input.clear();
                                repo.follow = false;
                                reload = true;
                                ui.close();
                            }
                        });
                    });
            }

            let any = (!history && (repo.branch_filter.is_some() || repo.path_filter.is_some()))
                || repo.user_filter.is_some()
                || repo.date_filter != DateFilter::All
                || !repo.text_filter.is_empty();
            if any && theme::icon_button_sized(ui, Icon::Close, tr("Clear all filters"), 22.0, false, true).clicked() {
                // In a file history the file (and the commit it is read from) stay.
                if !history {
                    reload |= repo.branch_filter.is_some() || repo.path_filter.is_some();
                    repo.branch_filter = None;
                    repo.path_filter = None;
                }
                repo.user_filter = None;
                repo.date_filter = DateFilter::All;
                repo.text_filter.clear();
                changed = true;
            }
            // A file history: also the commits of the other branches, on request.
            if history && repo.line_range.is_none() && theme::icon_button_sized(ui, Icon::Branch, tr("Show the commits of every branch that changed the file, not only of the current one"), 24.0, repo.history_all, true).clicked() {
                repo.history_all = !repo.history_all;
                reload = true;
            }
            // The log options (sorting, merges, linear branches) do not apply to a file history.
            if !history {
                let sort = theme::icon_button_sized(ui, Icon::Sort, tr("Sort, log options and branch actions"), 24.0, repo.first_parent || repo.no_merges || repo.collapse_linear || repo.not_merged, true);
                egui::Popup::menu(&sort).show(|ui| {
                    if sort_menu(ui, repo) {
                        reload = true;
                    }
                });
            }
        }
        if reload {
            self.settings.log_topo = repo.topo;
            self.settings.log_first_parent = repo.first_parent;
            self.settings.log_no_merges = repo.no_merges;
            if self.screenshot.is_none() {
                self.settings.save();
            }
            App::reload_log(repo);
        } else if changed {
            App::apply_filters(repo);
        }

        let mut rui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(10.0, 4.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if right_w < 100.0 {
            if theme::icon_button(&mut rui, Icon::Refresh, tr("Refresh (F5)")).clicked() {
                self.refresh();
            }
            ui.advance_cursor_after_rect(bar);
            return;
        }
        if theme::icon_button_sized(&mut rui, Icon::Merge, tr("Cherry-pick selected commit"), 24.0, false, cherry_ok).clicked() {
            if let Some(c) = self.repo.as_ref().and_then(|r| r.selected_commit().cloned()) {
                self.cherry_pick(&c);
            }
        }
        if theme::icon_button(&mut rui, Icon::Refresh, tr("Refresh (F5)")).clicked() {
            self.refresh();
        }
        ui.advance_cursor_after_rect(bar);
    }

    fn can_cherry_pick(&self) -> bool {
        let Some(repo) = self.repo.as_ref() else { return false };
        let Some(c) = repo.selected_commit() else { return false };
        match &repo.details {
            Some(d) if d.hash == c.hash => match &repo.state.head_branch {
                Some(h) => !d.containing.iter().any(|b| b == h),
                None => true,
            },
            _ => false,
        }
    }

    fn cherry_pick(&mut self, c: &git::Commit) {
        let mut args = vec!["cherry-pick".to_string()];
        if c.parents.len() > 1 {
            args.push("-m".into());
            args.push("1".into());
        }
        args.push(c.hash.clone());
        self.run_job(Job::many(&format!("Cherry-pick {}", short(&c.hash)), vec![args]));
    }

    fn keyboard_nav(&mut self, ui: &Ui) {
        // A click in the list brings the arrow keys to it.
        if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(ui.max_rect()) {
            self.key_focus = crate::app::KeyFocus::Log;
        }
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if typing || self.dialog.is_some() || self.key_focus != crate::app::KeyFocus::Log || self.show_settings {
            return;
        }
        let Some(repo) = self.repo.as_ref() else { return };
        let n = repo.visible.len();
        if n == 0 {
            return;
        }
        let (up, down, home, end) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
            )
        });
        if !(up || down || home || end) {
            return;
        }
        let pos = repo.selected.and_then(|s| repo.visible.iter().position(|&v| v == s));
        let new = if home {
            0
        } else if end {
            n - 1
        } else {
            match pos {
                Some(p) if up => p.saturating_sub(1),
                Some(p) => (p + 1).min(n - 1),
                None => 0,
            }
        };
        let idx = repo.visible[new];
        self.select_commit(idx);
        if let Some(r) = self.repo.as_mut() {
            r.scroll_to = Some(idx);
        }
    }

    /// Under the filters while a text is searched: how much was found and where, and the
    /// way to search the whole history with git (commit messages, or the changes themselves).
    fn search_scope_bar(&mut self, ui: &mut Ui) {
        let Some(repo) = self.repo.as_ref() else { return };
        if repo.follow || repo.text_filter.trim().is_empty() {
            return;
        }
        let (deep, more, found, loaded, loading) = (repo.deep, repo.more, repo.visible.len(), repo.commits.len(), repo.log_pending.is_some());
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, theme::BG_BAR());
        theme::hline(ui, bar.bottom() - 0.5, bar.left(), bar.right());
        let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(12.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        bui.set_clip_rect(bar);
        let ui = &mut bui;
        let dim = |t: String| egui::RichText::new(t).color(theme::TEXT_MID()).size(12.0);
        let mut set: Option<u8> = None;
        if loading {
            ui.add(egui::Spinner::new().size(12.0));
            ui.label(dim(tr("Searching…").to_string()));
        } else {
            let n = format!("{found} {}", crate::i18n::plural(found, "commit", "commits"));
            ui.label(dim(match deep {
                1 => crate::i18n::trf("Whole history, commit messages: {}", &[&n]),
                2 => crate::i18n::trf("Whole history, commits that added or removed the text: {}", &[&n]),
                _ if more => crate::i18n::trf("{} among the {} loaded (the history is longer)", &[&n, &loaded]),
                _ => n,
            }));
        }
        ui.add_space(10.0);
        let link = |ui: &mut Ui, t: &str, tip: &str| ui.add(egui::Label::new(egui::RichText::new(tr(t)).color(theme::LINK()).size(12.0)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr(tip)).clicked();
        if deep != 1 && (more || deep == 2) && link(ui, "Search the whole history", "Let git look through the messages of every commit, not only the loaded ones (git log --grep)") {
            set = Some(1);
        }
        if deep != 2 {
            ui.add_space(6.0);
            if link(ui, "Search in the changes", "Find the commits that added or removed this text in a file (git log -S). Slow on a big repository") {
                set = Some(2);
            }
        }
        if deep != 0 {
            ui.add_space(6.0);
            if link(ui, "Back to the loaded commits", "Search the subjects of the commits in the list, as usual") {
                set = Some(0);
            }
        }
        if let Some(m) = set {
            self.set_deep_search(m);
        }
    }

    fn commit_table(&mut self, ui: &mut Ui) {
        self.search_scope_bar(ui);
        let area = ui.available_rect_before_wrap();
        {
            let Some(repo) = self.repo.as_ref() else { return };
            let msg = if repo.log_pending.is_some() && (repo.commits.is_empty() || repo.visible.is_empty()) {
                Some("Loading the log…")
            } else if repo.commits.is_empty() {
                Some(if repo.state.head_hash.is_empty() { "No commits yet" } else { "No commits match" })
            } else if repo.visible.is_empty() {
                Some("No commits match the filters")
            } else {
                None
            };
            if let Some(m) = msg {
                ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, tr(m), FontId::proportional(13.0), theme::TEXT_DIM());
                return;
            }
        }
        self.keyboard_nav(ui);

        let scroll_id = egui::Id::unique("log_scroll_y");
        let mut sa = egui::ScrollArea::vertical().id_salt("log_table").auto_shrink([false, false]);
        if let Some(repo) = self.repo.as_mut() {
            if let Some(target) = repo.scroll_to.take() {
                if let Some(pos) = repo.visible.iter().position(|&v| v == target) {
                    let y = pos as f32 * ROW_H;
                    let cur = ui.ctx().data(|d| d.get_temp::<f32>(scroll_id)).unwrap_or(0.0);
                    let h = area.height();
                    if y < cur || y + 2.0 * ROW_H > cur + h {
                        sa = sa.vertical_scroll_offset((y - h / 3.0).max(0.0));
                    }
                }
            }
        }
        let n = self.repo.as_ref().map(|r| r.visible.len()).unwrap_or(0);
        let mut clicked: Option<usize> = None;
        let mut double: Option<usize> = None;
        let mut menu: Option<(egui::Response, usize)> = None;
        // A click on the arrow of a cut graph line: the commit at its other end.
        let mut jump: Option<usize> = None;
        // Rows touch each other; without this the list would count gaps that are never drawn
        // and leave an empty strip at its end.
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut at_end = false;
        let site = self.hosting_tick(ui.ctx());
        let out = sa.show_rows(ui, ROW_H, n, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let repo = self.repo.as_ref().unwrap();
            let ci = site.as_ref().and_then(|s| self.hosting.known(s)).map(|i| &i.ci).filter(|c| !c.is_empty());
            at_end = range.end >= n;
            for vi in range.clone() {
                let idx = repo.visible[vi];
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                paint_row(ui, repo, idx, rect, &resp, &range, ci);
                let arrow = if App::filtered(repo) { None } else { arrow_at(repo, idx, rect, resp.hover_pos()) };
                if let Some(a) = arrow {
                    let tip = match a.target.and_then(|t| repo.commits.get(t)) {
                        Some(c) => format!("Go to the {} commit {} {}", if a.down { "parent" } else { "child" }, crate::app::short(&c.hash), c.subject),
                        None => "The parent commit is older than the loaded history".to_string(),
                    };
                    if a.target.is_some() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::unique("graph_arrow_tip"), egui::PopupAnchor::Pointer).show(|ui| {
                        ui.label(tip);
                    });
                }
                if let (Some(a), true) = (arrow, resp.clicked()) {
                    jump = a.target;
                } else if resp.clicked() || resp.secondary_clicked() {
                    clicked = Some(idx);
                }
                if resp.double_clicked() {
                    double = Some(idx);
                }
                menu.get_or_insert((resp.clone(), idx));
                if resp.secondary_clicked() || resp.context_menu_opened() {
                    menu = Some((resp, idx));
                }
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(scroll_id, out.state.clamped_offset().y));
        // The end of a long history is in view: read the next portion of older commits. Not
        // while a filter hides most of the list (its end is always in view then).
        let grow = self.repo.as_ref().is_some_and(|r| r.more && r.log_pending.is_none() && r.visible.len() == r.commits.len());
        if at_end && grow && n as f32 * ROW_H > area.height() {
            self.load_more_log();
        }
        if let Some(t) = jump {
            if let Some(r) = self.repo.as_mut() {
                r.scroll_to = Some(t);
            }
            self.select_commit(t);
        }
        if let Some(i) = clicked {
            let m = ui.input(|i| i.modifiers);
            let right = ui.input(|i| i.pointer.secondary_released());
            let in_multi = self.repo.as_ref().map(|r| r.multi.contains(&i)).unwrap_or(false);
            if m.shift && !right {
                self.select_more(i, true);
            } else if m.command && !right {
                self.select_more(i, false);
            } else if right && in_multi {
                // Right-click inside a multiple selection keeps it.
            } else if self.repo.as_ref().map(|r| r.selected != Some(i) || r.multi.len() > 1).unwrap_or(false) {
                self.select_commit(i);
            }
        }
        if let Some(i) = double {
            let has_files = self
                .repo
                .as_ref()
                .and_then(|r| r.details.as_ref())
                .map(|d| !d.files.is_empty())
                .unwrap_or(false);
            if has_files {
                // File history: the followed file, not the commit's first one.
                let file = self.repo.as_ref().filter(|r| r.follow && r.details_part == 0).and_then(|r| r.details_file).unwrap_or(0);
                self.open_commit_file_diff(file);
            }
            let _ = i;
        }
        if let Some((resp, idx)) = menu {
            if resp.context_menu_opened() || resp.secondary_clicked() {
                resp.context_menu(|ui| self.commit_actions(ui, idx));
            }
        }
    }

    pub fn commit_actions(&mut self, ui: &mut Ui, idx: usize) {
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(c) = repo.commits.get(idx).cloned() else { return };
        let is_head = c.hash == repo.state.head_hash;
        let head_name = repo.state.head_branch.clone().unwrap_or_else(|| "HEAD".into());
        let child = repo
            .commits
            .iter()
            .find(|x| x.parents.first() == Some(&c.hash))
            .map(|x| x.hash.clone());
        if ui.button(tr("Copy Revision Number")).clicked() {
            ui.ctx().copy_text(c.hash.clone());
        }
        if ui.button(tr("Copy Commit Subject")).clicked() {
            ui.ctx().copy_text(c.subject.clone());
        }
        ui.separator();
        // Several commits selected (Ctrl/Shift+click) and this one among them: all of them.
        let multi: Vec<usize> = self.repo.as_ref().map(|r| r.multi.iter().copied().collect()).unwrap_or_default();
        let picked = if multi.len() > 1 && multi.contains(&idx) { multi } else { vec![idx] };
        self.bisect_items(ui, &picked, &c.hash);
        if picked.len() == 2 {
            if ui.button(tr("Compare Versions")).on_hover_text(tr("The files that differ between the two selected commits")).clicked() {
                self.compare_commits(picked[0], picked[1]);
            }
            ui.separator();
        }
        if ui.add_enabled(!is_head, egui::Button::new(tr("Cherry-Pick"))).clicked() {
            self.cherry_pick(&c);
        }
        if ui.add_enabled(!is_head || picked.len() > 1, egui::Button::new(tr("Cherry-Pick with Options…"))).on_hover_text(tr("Record the original commit (-x), don't commit, choose the parent of a merge commit; also for several selected commits")).clicked() {
            self.open_pick(&picked, false);
        }
        if ui.button(tr("Checkout Revision")).clicked() {
            self.run_job(Job::one(&format!("Checkout {}", short(&c.hash)), &["checkout", &c.hash]));
        }
        // The branches that end at this commit (not the current one): one is checked out right
        // away, several are listed in a submenu.
        let tips: Vec<(String, RefKind)> = {
            let Some(repo) = self.repo.as_ref() else { return };
            let local: Vec<&str> = repo.state.branches.iter().filter(|b| b.kind == RefKind::Local && b.hash == c.hash).map(|b| b.name.as_str()).collect();
            repo.state
                .branches
                .iter()
                .filter(|b| b.hash == c.hash && !b.is_head && !b.name.ends_with("/HEAD"))
                // A remote branch whose local namesake is listed too checks out the same branch.
                .filter(|b| b.kind == RefKind::Local || !local.contains(&b.name.split_once('/').map_or(b.name.as_str(), |x| x.1)))
                .map(|b| (b.name.clone(), b.kind))
                .collect()
        };
        let mut checkout: Option<(String, RefKind)> = None;
        match tips.as_slice() {
            [] => {}
            [(name, kind)] => {
                if ui.button(format!("{} '{name}'", tr("Checkout Branch"))).clicked() {
                    checkout = Some((name.clone(), *kind));
                }
            }
            _ => {
                ui.menu_button(tr("Checkout Branch"), |ui| {
                    for (name, kind) in &tips {
                        if ui.button(name).clicked() {
                            checkout = Some((name.clone(), *kind));
                        }
                    }
                });
            }
        }
        if let Some((name, kind)) = checkout {
            self.checkout_branch(&name, kind);
        }
        if ui.button(format!("Reset Current Branch ('{head_name}') to Here…")).clicked() {
            self.dialog = Some(Dialog::Reset {
                hash: c.hash.clone(),
                subject: c.subject.clone(),
                mode: crate::app::ResetMode::Mixed,
            });
        }
        if ui.button(if picked.len() > 1 { tr("Revert Commits…") } else { tr("Revert Commit…") }).clicked() {
            self.open_pick(&picked, true);
        }
        let label = if picked.len() > 1 { format!("Create Patch ({} commits)…", picked.len()) } else { "Create Patch…".into() };
        if ui.button(label).on_hover_text(tr("Save the commit's changes, author and message to a .patch file")).clicked() {
            self.open_create_patch_commits(picked.clone());
        }
        if is_head && !c.parents.is_empty() {
            if ui.button(tr("Undo Commit (keep changes)")).clicked() {
                self.run_job(Job::one("Undo commit", &["reset", "--soft", "HEAD~1"]));
            }
            if ui.button(tr("Edit Commit Message…")).clicked() {
                let path = self.repo.as_ref().unwrap().path.clone();
                let msg = git::run(&path, &["log", "-1", "--format=%B", &c.hash]).stdout;
                self.dialog = Some(Dialog::EditMessage {
                    hash: c.hash.clone(),
                    message: msg.trim_end().to_string(),
                });
            }
        } else if !is_head && ui.button(tr("Edit Commit Message…")).on_hover_text(tr("Opens Interactive Rebase with this commit's message ready to edit")).clicked() {
            self.open_rebase(&c.hash, Preset::Reword(c.hash.clone()));
        }
        ui.separator();
        if ui.button(tr("Interactively Rebase from Here…")).on_hover_text(tr("Reorder, squash, reword or drop the commits from this one up to the top of the branch")).clicked() {
            self.open_rebase(&c.hash, Preset::Nothing);
        }
        if picked.len() > 1 {
            if ui.button(format!("Squash Commits ({})…", picked.len())).on_hover_text(tr("Join the selected commits into the oldest of them")).clicked() {
                self.open_rebase_for(&picked, Preset::Squash);
            }
            if ui.button(format!("Drop Commits ({})…", picked.len())).clicked() {
                self.open_rebase_for(&picked, Preset::Drop);
            }
        } else {
            if let Some(p) = c.parents.first().filter(|_| c.parents.len() == 1) {
                if ui.button(tr("Squash into Parent…")).on_hover_text(tr("Join this commit into the one before it")).clicked() {
                    self.open_rebase(p, Preset::Squash(vec![p.clone(), c.hash.clone()]));
                }
            }
            if ui.button(tr("Drop Commit…")).on_hover_text(tr("Remove this commit and its changes from the branch")).clicked() {
                self.open_rebase(&c.hash, Preset::Drop(vec![c.hash.clone()]));
            }
        }
        ui.separator();
        if ui.button(tr("New Branch…")).clicked() {
            self.open_new_branch(&c.hash);
        }
        if ui.button(tr("New Tag…")).clicked() {
            self.dialog = Some(Dialog::NewTag {
                rev: c.hash.clone(),
                name: String::new(),
                message: String::new(),
            });
        }
        ui.separator();
        if let Some(p) = c.parents.first().cloned() {
            if ui.button(tr("Go to Parent Commit")).clicked() {
                self.navigate_to(&p);
            }
        }
        if let Some(ch) = child {
            if ui.button(tr("Go to Child Commit")).clicked() {
                self.navigate_to(&ch);
            }
        }
        self.hosting_commit_item(ui, &c.hash);
    }
}

/// A tab in the tool window header. Returns (clicked, close clicked).
fn tab_button(ui: &mut Ui, text: &str, selected: bool, closable: bool) -> (bool, bool) {
    let text = tr(text);
    let font = FontId::proportional(13.0);
    let tw = theme::text_width(ui, text, &font);
    let w = tw + 20.0 + if closable { 18.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(r, 5.0, theme::TAB_SEL());
    } else if resp.hovered() {
        p.rect_filled(r, 5.0, theme::HOVER());
    }
    p.text(pos2(r.left() + 10.0, r.center().y), egui::Align2::LEFT_CENTER, text, font, theme::TEXT());
    let mut closed = false;
    if closable {
        let cr = Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(14.0, 14.0));
        let hov = ui.rect_contains_pointer(cr);
        theme::paint_icon(p, cr, Icon::Close, if hov { theme::TEXT() } else { theme::TEXT_DIM() });
        if resp.clicked() && hov {
            closed = true;
        }
    }
    (resp.clicked() && !closed, closed)
}

/// The arrow of a cut graph line under the pointer, if any.
fn arrow_at(repo: &crate::app::Repo, idx: usize, rect: Rect, pointer: Option<egui::Pos2>) -> Option<crate::graph::Arrow> {
    let p = pointer?;
    let gx = rect.left() + 12.0;
    let cy = rect.center().y;
    repo.rows.get(idx)?.arrows.iter().copied().find(|a| {
        let x = gx + a.col * LANE;
        (p.x - x).abs() <= LANE * 0.5 && (p.y - (cy + if a.down { ARROW_LEN / 2.0 } else { -ARROW_LEN / 2.0 })).abs() <= 9.0
    })
}

/// The graph part of one row: its lines, cut-line arrows and the commit dot. `bg` is the
/// row's background (the hole in the HEAD commit's ring).
fn paint_graph(gp: &egui::Painter, row: &crate::graph::Row, rect: Rect, gx: f32, head: bool, bg: Color32) {
    let cy = rect.center().y;
    let colx = |col: f32| gx + col * LANE;
    let yof = |e: End| match e {
        End::Top => rect.top(),
        End::Middle => cy,
        End::Bottom => rect.bottom(),
    };
    for s in &row.segments {
        let a = pos2(colx(s.from_col), yof(s.from));
        let b = pos2(colx(s.to_col), yof(s.to));
        let st = Stroke::new(1.4, theme::graph_color(s.color));
        if s.dashed {
            gp.extend(egui::Shape::dashed_line(&[a, b], st, 3.0, 3.0));
        } else {
            gp.line_segment([a, b], st);
        }
    }
    // Cut ends of long lines: the line goes on straight for a bit and ends in an arrowhead
    // ("↓" below the commit it leaves, "↑" above the commit it reaches).
    for a in &row.arrows {
        let x = colx(a.col);
        let st = Stroke::new(1.4, theme::graph_color(a.color));
        let dir = if a.down { 1.0 } else { -1.0 };
        let tip = pos2(x, cy + dir * ARROW_LEN);
        gp.line_segment([pos2(x, cy), tip], st);
        gp.add(egui::Shape::line(vec![pos2(x - 4.0, tip.y - dir * 4.0), tip, pos2(x + 4.0, tip.y - dir * 4.0)], st));
    }
    let cpos = pos2(colx(row.col as f32), cy);
    let col = theme::graph_color(row.color);
    if head {
        gp.circle_filled(cpos, 5.0, col);
        gp.circle_filled(cpos, 2.2, bg);
    } else {
        gp.circle_filled(cpos, 3.6, col);
    }
}

/// A small made-up history drawn in one graph style (Settings → Log). Clickable.
pub fn graph_preview(ui: &mut Ui, fixed: bool, chosen: bool) -> egui::Response {
    use crate::git::Commit;
    let c = |h: &str, p: &[&str]| Commit { hash: h.into(), parents: p.iter().map(|s| s.to_string()).collect(), author: String::new(), email: String::new(), time: 0, subject: String::new() };
    let commits = [
        c("m4", &["m3", "a2"]),
        c("b2", &["b1"]),
        c("a2", &["a1"]),
        c("m3", &["m2"]),
        c("c1", &["m2"]),
        c("a1", &["m2"]),
        c("b1", &["m1"]),
        c("m2", &["m1", "c1"]),
        c("m1", &["m0"]),
        c("m0", &[]),
    ];
    let main: std::collections::HashSet<String> = ["m4", "m3", "m2", "m1", "m0"].iter().map(|s| s.to_string()).collect();
    let none = std::collections::HashSet::new();
    let rows = if fixed { crate::graph::layout_fixed(&commits, &none, &main) } else { crate::graph::layout(&commits, &none, &main) };
    let row_h = 16.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(250.0), rows.len() as f32 * row_h + 8.0), Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, theme::BG());
    let border = if chosen { theme::ACCENT() } else if resp.hovered() { theme::INPUT_BORDER() } else { theme::BORDER() };
    p.rect_stroke(rect, 4.0, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    let inner = p.with_clip_rect(rect.shrink(1.0));
    let lanes = rows.iter().map(|r| r.width).max().unwrap_or(1) as f32;
    for (i, row) in rows.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left(), rect.top() + 4.0 + i as f32 * row_h), vec2(rect.width(), row_h));
        paint_graph(&inner, row, r, rect.left() + 14.0, i == 0, theme::BG());
        // Grey bars where the commit messages would be.
        let x = rect.left() + 14.0 + lanes * LANE;
        let w: f32 = [90.0, 70.0, 110.0, 80.0, 60.0, 100.0, 75.0, 95.0, 65.0, 85.0][i % 10];
        inner.rect_filled(Rect::from_min_size(pos2(x, r.center().y - 2.5), vec2(w.min(rect.right() - x - 10.0).max(0.0), 5.0)), 2.5, theme::BORDER());
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_row(ui: &Ui, repo: &crate::app::Repo, idx: usize, rect: Rect, resp: &egui::Response, range: &std::ops::Range<usize>, ci: Option<&std::collections::HashMap<String, crate::hosting::CiRun>>) {
    let c = &repo.commits[idx];
    let row = &repo.rows[idx];
    let filtered = App::filtered(repo);
    let width = rect.width();
    let max_lanes = range
        .clone()
        .map(|vi| repo.rows[repo.visible[vi]].width)
        .max()
        .unwrap_or(1) as f32;
    let graph_w = if filtered { 24.0 } else { (max_lanes * LANE + 14.0).min(width * 0.35).max(34.0) };
    // (min/max rather than clamp: the table can be squeezed to no width at all.)
    let hash_w = 86.0;
    let date_w = 120.0;
    let narrow = width < 700.0;
    // A file history is read by its commit numbers too: keep them while there is any room.
    let show_hash = !narrow || (repo.follow && repo.path_filter.is_some() && width >= 500.0);
    let (hash_w, date_w) = if show_hash && narrow { (72.0, 112.0) } else if show_hash { (hash_w, date_w) } else { (0.0, date_w) };
    let author_w = (width * 0.13).clamp(if narrow { 90.0 } else { 110.0 }, 190.0);
    let subj_w = (width - graph_w - hash_w - date_w - author_w - 20.0).max(80.0);
    let p = ui.painter_at(rect);
    let selected = repo.selected == Some(idx) || repo.multi.contains(&idx);
    if selected {
        p.rect_filled(rect, 0.0, theme::SELECT());
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, theme::HOVER());
    }
    let cy = rect.center().y;
    let gx = rect.left() + 12.0;
    if filtered {
        p.circle_filled(pos2(gx, cy), 3.5, theme::graph_color(row.color));
    } else {
        let gp = ui.painter_at(Rect::from_min_max(rect.min, pos2(rect.left() + graph_w, rect.bottom())));
        let head = c.hash == repo.state.head_hash;
        paint_graph(&gp, row, rect, gx, head, if selected { theme::SELECT() } else { theme::BG() });
    }

    let merge = c.parents.len() > 1;
    let text_col = if merge { theme::TEXT_DIM() } else { theme::TEXT() };
    let sx = rect.left() + graph_w + 6.0;
    let mut labels_w = 0.0;
    // A file history has no branch and tag labels: its rows are narrow, and the details under
    // the list name the branches.
    let history = repo.follow && repo.path_filter.is_some();
    if let Some(labels) = repo.state.labels.get(&c.hash).filter(|_| !history) {
        let font = FontId::proportional(12.0);
        let mut x_right = sx + subj_w - 4.0;
        let max_total = subj_w * 0.45;
        let extra = labels.len().saturating_sub(2);
        if extra > 0 {
            let t = format!("+{extra}");
            let w = theme::text_width(ui, &t, &font);
            p.text(pos2(x_right - w, cy), egui::Align2::LEFT_CENTER, &t, font.clone(), theme::TEXT_MID());
            x_right -= w + 6.0;
            labels_w += w + 6.0;
        }
        let compact = subj_w < 380.0;
        for l in labels.iter().take(2).collect::<Vec<_>>().into_iter().rev() {
            let avail = (max_total - labels_w - 18.0).max(30.0);
            let tw = if compact { 0.0 } else { theme::text_width(ui, &l.text, &font).min(avail) };
            let start = x_right - tw - 16.0;
            let (icon, col) = match l.kind {
                LabelKind::Head => (Icon::Head, theme::STAR()),
                LabelKind::Local => (Icon::Label, theme::GREEN()),
                LabelKind::Remote => (Icon::Label, theme::rgb(0xa8, 0x8b, 0xe8)),
                LabelKind::Tag => (Icon::Tag, theme::TEXT_MID()),
                LabelKind::Bisect => (Icon::Label, bisect_color(&l.text)),
            };
            theme::paint_icon(&p, Rect::from_center_size(pos2(start + 6.0, cy), vec2(12.0, 12.0)), icon, col);
            if !compact {
                theme::paint_clipped(&p, pos2(start + 15.0, cy), &l.text, font.clone(), theme::TEXT_MID(), tw + 1.0);
            }
            labels_w += tw + if compact { 16.0 } else { 22.0 };
            x_right = start - 6.0;
        }
        // Pointing at the labels names them at once: in a narrow table they are only icons,
        // the text may be cut, and "+N" hides the rest.
        let zone = Rect::from_min_max(pos2(x_right, rect.top()), pos2(sx + subj_w, rect.bottom()));
        if resp.hover_pos().is_some_and(|hp| zone.contains(hp)) {
            egui::Tooltip::always_open(ui.ctx().clone(), resp.layer_id, resp.id.with(("labels", idx)), egui::PopupAnchor::Pointer).show(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for l in labels {
                    let (icon, col, kind) = match l.kind {
                        LabelKind::Head => (Icon::Head, theme::STAR(), "current branch"),
                        LabelKind::Local => (Icon::Label, theme::GREEN(), "local branch"),
                        LabelKind::Remote => (Icon::Label, theme::rgb(0xa8, 0x8b, 0xe8), "remote branch"),
                        LabelKind::Tag => (Icon::Tag, theme::TEXT_MID(), "tag"),
                        LabelKind::Bisect => (Icon::Label, bisect_color(&l.text), "bisect"),
                    };
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        theme::paint_icon(ui.painter(), r, icon, col);
                        ui.label(egui::RichText::new(&l.text).color(theme::TEXT()));
                        ui.label(egui::RichText::new(kind).color(theme::TEXT_DIM()).size(11.5));
                    });
                }
            });
        }
    }
    // The result of CI before the subject: its mark, named when pointed at.
    let mark = ci.and_then(|m| m.get(&c.hash));
    // Room for the mark in every row once the hosting told of any, so the subjects stay in line.
    let mark_w = if ci.is_some() { 16.0 } else { 0.0 };
    if let Some(run) = mark {
        let at = pos2(sx + 6.0, cy);
        crate::hosting_view::paint_ci(&p, at, run.state);
        if resp.hover_pos().is_some_and(|hp| Rect::from_center_size(at, vec2(16.0, rect.height())).contains(hp)) {
            egui::Tooltip::always_open(ui.ctx().clone(), resp.layer_id, resp.id.with(("ci", idx)), egui::PopupAnchor::Pointer).show(|ui| {
                ui.label(crate::hosting_view::ci_tip(run));
            });
        }
    }
    let job = linkified(&c.subject, 13.0, text_col, Some(subj_w - labels_w - 8.0 - mark_w));
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let gpos = pos2(sx + mark_w, cy - g.size().y / 2.0);
    // Issue links open in the browser when clicked.
    if let Some(hp) = resp.hover_pos() {
        let in_text = Rect::from_min_size(gpos, g.size()).contains(hp);
        if in_text {
            if let Some(url) = crate::details_view::link_at(&c.subject, &g, hp - gpos) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() && !ui.input(|i| i.modifiers.ctrl || i.modifiers.shift || i.modifiers.command) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        }
    }
    p.galley(gpos, g, text_col);
    let font = FontId::proportional(13.0);
    let ax = sx + subj_w + 4.0;
    theme::paint_clipped(&p, pos2(ax, cy), &c.author, font.clone(), text_col, author_w - 8.0);
    let dx = ax + author_w;
    p.text(pos2(dx, cy), egui::Align2::LEFT_CENTER, format_date(c.time), font.clone(), text_col);
    if show_hash {
        p.text(pos2(dx + date_w, cy), egui::Align2::LEFT_CENTER, short(&c.hash), font, text_col);
    }
}

/// A menu row with a check mark in front when `on`. Returns true when clicked.
fn check_item(ui: &mut Ui, on: bool, text: &str, help: &str) -> bool {
    let label = if on { format!("✓  {text}") } else { format!("     {text}") };
    let r = ui.add(egui::Button::new(egui::RichText::new(label).color(theme::TEXT())).frame_when_inactive(on));
    let clicked = r.clicked();
    r.on_hover_text(help);
    if clicked {
        ui.close();
    }
    clicked
}

/// The ⇅ menu of the log: sort order, log options and collapsing linear branches.
pub fn sort_menu(ui: &mut Ui, repo: &mut crate::app::Repo) -> bool {
    let mut reload = false;
        ui.set_min_width(230.0);
        let head = |ui: &mut Ui, t: &str| {
            ui.label(egui::RichText::new(t).color(theme::TEXT_DIM()).size(11.5));
        };
        head(ui, "Sort");
        if check_item(ui, !repo.topo, "By Commit Date", "Newest commits first, whatever branch they are on.") && repo.topo {
            repo.topo = false;
            reload = true;
        }
        if check_item(ui, repo.topo, "Topologically", "Keep the commits of each branch together, so the graph is easier to follow.") && !repo.topo {
            repo.topo = true;
            reload = true;
        }
        ui.separator();
        head(ui, "Options");
        if check_item(ui, repo.first_parent, "First Parent", "Follow only the first parent of merge commits: commits that came in from merged branches are hidden.") {
            repo.first_parent = !repo.first_parent;
            reload = true;
        }
        if check_item(ui, repo.no_merges, "No Merges", "Hide merge commits from the log.") {
            repo.no_merges = !repo.no_merges;
            reload = true;
        }
        let cur = repo.state.head_branch.clone().unwrap_or_else(|| "HEAD".into());
        if check_item(
            ui,
            repo.not_merged,
            &format!("Not Merged into {cur}"),
            &format!("Show only the branches whose commits are not in {cur} yet, with just those commits."),
        ) {
            repo.not_merged = !repo.not_merged;
            reload = true;
        }
        ui.separator();
        head(ui, "Branch Actions");
        if check_item(ui, false, "Collapse Linear Branches", "Hide the commits in the middle of side branches; a dashed line marks where commits were folded away.") {
            repo.collapse_linear = true;
            reload = true;
        }
        if check_item(ui, false, "Expand Linear Branches", "Show every commit of every branch again.") {
            repo.collapse_linear = false;
            reload = true;
        }
    reload
}

/// The colour of a bisect mark in the Log.
fn bisect_color(word: &str) -> Color32 {
    match word {
        "good" => theme::GREEN(),
        "skipped" => theme::TEXT_DIM(),
        _ => theme::RED(),
    }
}
