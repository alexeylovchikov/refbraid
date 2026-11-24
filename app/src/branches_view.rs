//! The branches tree on the left of the Log tab, with its small vertical toolbar.

use crate::app::{App, Dialog, Job};
use crate::i18n::{tr, trf};
use crate::details_view::search_box;
use crate::git::{Branch, RefKind};
use crate::theme::{self, Icon};
use eframe::egui::{self, FontId, Rect, Sense, Ui, pos2, vec2};
use std::collections::BTreeMap;

#[derive(Clone)]
enum TreeRow {
    Head,
    Group { key: String, label: String, depth: usize, open: bool, folder: bool },
    Ref { branch: Branch, label: String, depth: usize },
    /// A working folder of the repository; `current` is the one that is open.
    Worktree { wt: crate::git::Worktree, current: bool },
    Submodule { sub: crate::submodule::Submodule },
}

/// The folder's own name, for the tree.
fn folder_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
}

#[derive(Default)]
struct Node {
    folders: BTreeMap<String, Node>,
    leaves: Vec<(String, Branch)>,
}

impl Node {
    fn insert(&mut self, rel: &str, b: Branch) {
        match rel.split_once('/') {
            Some((head, rest)) => self.folders.entry(head.to_string()).or_default().insert(rest, b),
            None => self.leaves.push((rel.to_string(), b)),
        }
    }
    fn count(&self) -> usize {
        self.leaves.len() + self.folders.values().map(|f| f.count()).sum::<usize>()
    }
}

fn emit(app: &App, node: &Node, prefix: &str, depth: usize, rows: &mut Vec<TreeRow>, open: &dyn Fn(&str) -> bool) {
    let mut leaves: Vec<&(String, Branch)> = node.leaves.iter().collect();
    leaves.sort_by_key(|(l, _)| l.to_lowercase());
    let (fav, rest): (Vec<_>, Vec<_>) = leaves.into_iter().partition(|(_, b)| app.is_favorite(&b.name));
    for (label, b) in fav {
        rows.push(TreeRow::Ref { branch: b.clone(), label: label.clone(), depth });
    }
    for (name, sub) in &node.folders {
        let key = format!("{prefix}/{name}");
        let o = open(&key);
        rows.push(TreeRow::Group { key: key.clone(), label: name.clone(), depth, open: o, folder: true });
        if o {
            emit(app, sub, &key, depth + 1, rows, open);
        }
    }
    for (label, b) in rest {
        rows.push(TreeRow::Ref { branch: b.clone(), label: label.clone(), depth });
    }
}

/// Folders inside Local or inside a remote ("L/fix", "R/origin/fix"); they start closed.
fn nested_folder(key: &str) -> bool {
    key.starts_with("L/") || (key.starts_with("R/") && key.matches('/').count() >= 2)
}

/// Every folder key of the tree, for "Expand all".
fn all_folders(branches: &[Branch]) -> Vec<String> {
    let mut keys = vec![];
    for b in branches {
        let parts: Vec<&str> = b.name.split('/').collect();
        let (mut k, skip) = match b.kind {
            RefKind::Local => ("L".to_string(), 0),
            RefKind::Remote => (format!("R/{}", parts[0]), 1),
            RefKind::Tag => continue,
        };
        for p in parts.iter().take(parts.len().saturating_sub(1)).skip(skip) {
            k = format!("{k}/{p}");
            keys.push(k.clone());
        }
    }
    keys
}

fn rev_of(b: &Branch) -> String {
    if b.kind == RefKind::Tag {
        format!("tags/{}", b.name)
    } else {
        b.name.clone()
    }
}

impl App {
    fn tree_rows(&self) -> Vec<TreeRow> {
        let Some(repo) = self.repo.as_ref() else { return vec![] };
        let search = repo.branch_search.trim().to_lowercase();
        let matches = |n: &str| search.is_empty() || n.to_lowercase().contains(&search);
        // "Not Merged into <current>": only the branches that still have such commits.
        let shown = |b: &Branch| {
            !repo.not_merged
                || repo.unmerged.contains(&format!("{}:{}", if b.kind == RefKind::Local { "L" } else { "R" }, b.name))
        };
        let is_open = |k: &str| {
            !search.is_empty()
                || if nested_folder(k) { repo.tree_open.contains(k) } else { !repo.tree_closed.contains(k) }
        };
        let mut rows = vec![];
        if search.is_empty() {
            rows.push(TreeRow::Head);
        }
        let mut local = Node::default();
        for b in repo.state.branches.iter().filter(|b| b.kind == RefKind::Local && matches(&b.name) && shown(b)) {
            local.insert(&b.name, b.clone());
        }
        if search.is_empty() || local.count() > 0 {
            let o = is_open("L");
            rows.push(TreeRow::Group { key: "L".into(), label: "Local".into(), depth: 0, open: o, folder: false });
            if o {
                emit(self, &local, "L", 1, &mut rows, &is_open);
            }
        }
        let mut remotes: BTreeMap<String, Node> = BTreeMap::new();
        for b in repo.state.branches.iter().filter(|b| b.kind == RefKind::Remote && matches(&b.name) && shown(b)) {
            let (r, rest) = b.name.split_once('/').unwrap_or(("", &b.name));
            remotes.entry(r.to_string()).or_default().insert(rest, b.clone());
        }
        if search.is_empty() || !remotes.is_empty() {
            let o = is_open("R");
            rows.push(TreeRow::Group { key: "R".into(), label: "Remote".into(), depth: 0, open: o, folder: false });
            if o {
                for (r, node) in &remotes {
                    let key = format!("R/{r}");
                    let ro = is_open(&key);
                    rows.push(TreeRow::Group { key: key.clone(), label: r.clone(), depth: 1, open: ro, folder: true });
                    if ro {
                        emit(self, node, &key, 2, &mut rows, &is_open);
                    }
                }
            }
        }
        // Worktrees, when the repository has more than its own folder.
        let trees: Vec<&crate::git::Worktree> = repo.state.worktrees.iter().filter(|w| matches(&folder_name(&w.path)) || w.branch.as_deref().is_some_and(matches)).collect();
        if repo.state.worktrees.len() > 1 && !trees.is_empty() {
            let o = is_open("W");
            rows.push(TreeRow::Group { key: "W".into(), label: "Worktrees".into(), depth: 0, open: o, folder: false });
            if o {
                for w in trees {
                    rows.push(TreeRow::Worktree { wt: w.clone(), current: crate::git::same_dir(&w.path, &repo.path) });
                }
            }
        }
        // Submodules, when the repository has any.
        let subs: Vec<&crate::submodule::Submodule> = repo.submodules.iter().filter(|m| matches(&m.path)).collect();
        if !subs.is_empty() {
            let o = is_open("M");
            rows.push(TreeRow::Group { key: "M".into(), label: "Submodules".into(), depth: 0, open: o, folder: false });
            if o {
                for m in subs {
                    rows.push(TreeRow::Submodule { sub: m.clone() });
                }
            }
        }
        let mut tags: Vec<&Branch> = repo.state.tags.iter().filter(|t| matches(&t.name)).collect();
        if search.is_empty() || !tags.is_empty() {
            // Tags start folded, like the IDE.
            let o = !search.is_empty() || repo.tree_closed.contains("T!open");
            rows.push(TreeRow::Group { key: "T".into(), label: "Tags".into(), depth: 0, open: o, folder: false });
            if o {
                tags.sort_by(|a, b| b.name.cmp(&a.name));
                for t in tags {
                    rows.push(TreeRow::Ref { branch: t.clone(), label: t.name.clone(), depth: 1 });
                }
            }
        }
        rows
    }

    pub fn branches_panel(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        ui.expand_to_include_rect(full);
        let bar = Rect::from_min_max(full.min, pos2(full.left() + 32.0, full.bottom()));
        theme::vline(ui, bar.right() - 0.5, bar.top(), bar.bottom());
        let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(4.0, 6.0))));
        self.branch_toolbar(&mut bar_ui);

        let area = Rect::from_min_max(pos2(bar.right() + 8.0, full.top() + 6.0), pos2(full.right() - 6.0, full.bottom()));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        // The search field; its clear button sits inside it while it has text.
        ui.horizontal(|ui| {
            if let Some(repo) = self.repo.as_mut() {
                search_box(ui, &mut repo.branch_search, tr("Branch or tag"), area.width().max(80.0), None);
            }
        });
        ui.add_space(6.0);
        // Open the folder that holds the current branch (once per branch change).
        if let Some(r) = self.repo.as_mut() {
            if r.tree_head != r.state.head_branch {
                r.tree_head = r.state.head_branch.clone();
                if let Some(h) = &r.tree_head {
                    let parts: Vec<&str> = h.split('/').collect();
                    let mut k = "L".to_string();
                    for p in &parts[..parts.len().saturating_sub(1)] {
                        k = format!("{k}/{p}");
                        r.tree_open.insert(k.clone());
                    }
                }
            }
        }
        let rows = self.tree_rows();
        let current = self.repo.as_ref().and_then(|r| r.state.head_branch.clone());
        // A click in the tree brings the arrow keys to it.
        if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(full) {
            self.key_focus = crate::app::KeyFocus::Branches;
        }
        let row_key = |row: &TreeRow| match row {
            TreeRow::Head => "HEAD".to_string(),
            TreeRow::Group { key, .. } => key.clone(),
            TreeRow::Ref { branch, .. } => format!("{:?}:{}", branch.kind, branch.name),
            TreeRow::Worktree { wt, .. } => format!("W:{}", wt.path.display()),
            TreeRow::Submodule { sub } => format!("M:{}", sub.path),
        };
        // The arrow keys: up and down through the rows, left and right fold and unfold a
        // group, Enter shows the branch in the Log.
        let mut moved_to: Option<String> = None;
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if self.key_focus == crate::app::KeyFocus::Branches && !typing && self.dialog.is_none() && !self.show_settings && !rows.is_empty() {
            let (up, down, left, right, enter) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Enter)));
            let sel = self.repo.as_ref().and_then(|r| r.tree_selected.clone());
            let at = sel.as_ref().and_then(|s| rows.iter().position(|r| row_key(r) == *s));
            if up || down {
                let next = match at {
                    Some(k) if up => k.saturating_sub(1),
                    Some(k) => (k + 1).min(rows.len() - 1),
                    None => 0,
                };
                moved_to = Some(row_key(&rows[next]));
                if let Some(r) = self.repo.as_mut() {
                    r.tree_selected = moved_to.clone();
                }
            } else if let Some(row) = at.map(|k| rows[k].clone()) {
                match row {
                    TreeRow::Group { key, open, .. } if (left && open) || (right && !open) => self.fold_tree_group(&key, open),
                    TreeRow::Ref { branch, .. } if enter => self.navigate_to(&rev_of(&branch)),
                    TreeRow::Head if enter => self.navigate_to("HEAD"),
                    TreeRow::Worktree { wt, current } if enter && !current && !wt.prunable => self.open(&wt.path),
                    TreeRow::Submodule { sub } if enter => self.open_submodule(&sub),
                    _ => {}
                }
            }
        }
        let rows = if moved_to.is_some() { rows } else { self.tree_rows() };
        let selected = self.repo.as_ref().and_then(|r| r.tree_selected.clone());
        egui::ScrollArea::both()
            .id_salt("branch_tree")
            .auto_shrink([false, false])
            .show(&mut ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in rows.iter() {
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width().max(200.0), 22.0), Sense::click());
                    let key = row_key(row);
                    if moved_to.as_ref() == Some(&key) {
                        ui.scroll_to_rect(rect, None);
                    }
                    self.paint_tree_row(ui, rect, &resp, row, selected.as_deref() == Some(key.as_str()), current.as_deref());
                    if resp.clicked() || resp.secondary_clicked() {
                        if let Some(r) = self.repo.as_mut() {
                            r.tree_selected = Some(key.clone());
                        }
                    }
                    match row {
                        TreeRow::Head => {
                            if resp.double_clicked() {
                                if let Some(r) = self.repo.as_mut() {
                                    r.branch_filter = Some("HEAD".into());
                                    App::reload_log(r);
                                }
                            } else if resp.clicked() {
                                self.navigate_to("HEAD");
                            }
                        }
                        TreeRow::Group { key, open, .. } => {
                            if resp.clicked() {
                                self.fold_tree_group(key, *open);
                            }
                        }
                        TreeRow::Worktree { wt, current } => {
                            if resp.double_clicked() && !*current && !wt.prunable {
                                self.open(&wt.path);
                            }
                            let (w, cur) = (wt.clone(), *current);
                            resp.context_menu(|ui| self.worktree_actions(ui, &w, cur));
                        }
                        TreeRow::Submodule { sub } => {
                            if resp.double_clicked() {
                                self.open_submodule(sub);
                            }
                            let m = sub.clone();
                            resp.context_menu(|ui| self.submodule_actions(ui, &m));
                        }
                        TreeRow::Ref { branch, .. } => {
                            if resp.double_clicked() {
                                if let Some(r) = self.repo.as_mut() {
                                    r.branch_filter = Some(rev_of(branch));
                                    App::reload_log(r);
                                }
                            } else if resp.clicked() {
                                self.navigate_to(&rev_of(branch));
                            }
                            let b = branch.clone();
                            resp.context_menu(|ui| {
                                if b.kind == RefKind::Tag {
                                    self.tag_actions(ui, &b.name);
                                } else {
                                    self.branch_actions(ui, &b.name, b.kind, current.as_deref());
                                }
                            });
                        }
                    }
                }
            });
    }

    fn paint_tree_row(&self, ui: &Ui, rect: Rect, resp: &egui::Response, row: &TreeRow, is_sel: bool, current: Option<&str>) {
        let p = ui.painter();
        if is_sel {
            p.rect_filled(rect, 4.0, theme::SELECT());
        } else if resp.hovered() {
            p.rect_filled(rect, 4.0, theme::HOVER());
        }
        let depth = match row {
            TreeRow::Head => 0,
            TreeRow::Group { depth, .. } | TreeRow::Ref { depth, .. } => *depth,
            TreeRow::Worktree { .. } | TreeRow::Submodule { .. } => 1,
        };
        let x0 = rect.left() + 18.0 + depth as f32 * 18.0;
        let cy = rect.center().y;
        let font = FontId::proportional(13.0);
        match row {
            TreeRow::Head => {
                p.text(pos2(x0, cy), egui::Align2::LEFT_CENTER, tr("HEAD (Current Branch)"), font, theme::TEXT());
            }
            TreeRow::Group { label, open, folder, .. } => {
                let chev = Rect::from_center_size(pos2(x0 - 9.0, cy), vec2(12.0, 12.0));
                theme::paint_icon(p, chev, if *open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_MID());
                let mut x = x0 + 2.0;
                if *folder {
                    theme::paint_icon(p, Rect::from_center_size(pos2(x + 7.0, cy), vec2(15.0, 15.0)), Icon::Folder, theme::ICON());
                    x += 20.0;
                }
                p.text(pos2(x, cy), egui::Align2::LEFT_CENTER, if *folder { label.as_str() } else { tr(label) }, font, theme::TEXT());
            }
            TreeRow::Worktree { wt, current } => {
                theme::paint_icon(p, Rect::from_center_size(pos2(x0 + 8.0, cy), vec2(15.0, 15.0)), Icon::Folder, if *current { theme::STAR() } else { theme::ICON() });
                let x = x0 + 21.0;
                let right = ui.clip_rect().right().min(rect.right()) - 8.0;
                let name = folder_name(&wt.path);
                let note = if wt.prunable {
                    tr("folder is missing").to_string()
                } else {
                    let at = wt.branch.clone().unwrap_or_else(|| crate::app::short(&wt.head).to_string());
                    if wt.locked { format!("{at}, {}", tr("locked")) } else { at }
                };
                let w = theme::paint_clipped(p, pos2(x, cy), &name, font.clone(), if wt.prunable { theme::TEXT_DIM() } else { theme::TEXT() }, (right - x).max(20.0));
                theme::paint_clipped(p, pos2(x + w + 8.0, cy), &note, FontId::proportional(11.5), theme::TEXT_DIM(), (right - x - w - 8.0).max(0.0));
            }
            TreeRow::Submodule { sub } => {
                use crate::submodule::State;
                let off = sub.state == State::NotInit;
                theme::paint_icon(p, Rect::from_center_size(pos2(x0 + 8.0, cy), vec2(15.0, 15.0)), Icon::Folder, if sub.state == State::Moved || sub.state == State::Conflict { theme::STAR() } else { theme::ICON() });
                let x = x0 + 21.0;
                let right = ui.clip_rect().right().min(rect.right()) - 8.0;
                let w = theme::paint_clipped(p, pos2(x, cy), &sub.path, font.clone(), if off { theme::TEXT_DIM() } else { theme::TEXT() }, (right - x).max(20.0));
                theme::paint_clipped(p, pos2(x + w + 8.0, cy), &submodule_note(sub), FontId::proportional(11.5), theme::TEXT_DIM(), (right - x - w - 8.0).max(0.0));
            }
            TreeRow::Ref { branch, label, .. } => {
                let is_head = branch.kind == RefKind::Local && current == Some(branch.name.as_str());
                let fav = self.is_favorite(&branch.name);
                let icon_rect = Rect::from_center_size(pos2(x0 + 8.0, cy), vec2(15.0, 15.0));
                let (icon, col) = if branch.kind == RefKind::Tag {
                    (Icon::Tag, theme::ICON())
                } else if is_head {
                    (Icon::Head, theme::STAR())
                } else if fav {
                    (Icon::Star, theme::STAR())
                } else {
                    (Icon::Branch, theme::ICON())
                };
                theme::paint_icon(p, icon_rect, icon, col);
                let x = x0 + 21.0;
                // Ahead / behind counts go right after the name; the name gets all the room
                // they don't need and is cut with "…" just before the edge.
                let small = FontId::proportional(11.5);
                let behind = (branch.behind > 0).then(|| format!("↙ {}", branch.behind));
                let ahead = (branch.ahead > 0).then(|| format!("↗ {}", branch.ahead));
                let counts_w: f32 = [&behind, &ahead]
                    .iter()
                    .filter_map(|t| t.as_ref())
                    .map(|t| theme::text_width(ui, t, &small) + 6.0)
                    .sum::<f32>()
                    + if behind.is_some() || ahead.is_some() { 2.0 } else { 0.0 };
                let right = ui.clip_rect().right().min(rect.right()) - 8.0;
                let w = theme::paint_clipped(p, pos2(x, cy), label, font.clone(), theme::TEXT(), (right - x - counts_w).max(20.0));
                let mut x = x + w + 8.0;
                if let Some(t) = behind {
                    p.text(pos2(x, cy), egui::Align2::LEFT_CENTER, &t, small.clone(), theme::LINK());
                    x += theme::text_width(ui, &t, &small) + 6.0;
                }
                if let Some(t) = ahead {
                    p.text(pos2(x, cy), egui::Align2::LEFT_CENTER, &t, small.clone(), theme::GREEN());
                    x += theme::text_width(ui, &t, &small) + 6.0;
                }
                // The branch is open in another working folder.
                if let Some(wt) = (branch.kind == RefKind::Local).then(|| self.worktree_with(&branch.name)).flatten() {
                    theme::paint_clipped(p, pos2(x, cy), &format!("⌂ {}", folder_name(&wt.path)), small, theme::TEXT_DIM(), (right - x).max(0.0));
                }
            }
        }
    }

    /// Fold an open group of the tree, or unfold a folded one.
    fn fold_tree_group(&mut self, key: &str, open: bool) {
        let Some(r) = self.repo.as_mut() else { return };
        if key == "T" {
            if open {
                r.tree_closed.remove("T!open");
            } else {
                r.tree_closed.insert("T!open".into());
            }
        } else if nested_folder(key) {
            if open {
                r.tree_open.remove(key);
            } else {
                r.tree_open.insert(key.to_string());
            }
        } else if open {
            r.tree_closed.insert(key.to_string());
        } else {
            r.tree_closed.remove(key);
        }
    }

    /// The menu of a worktree in the tree.
    fn worktree_actions(&mut self, ui: &mut Ui, wt: &crate::git::Worktree, current: bool) {
        ui.set_min_width(230.0);
        let there = !wt.prunable;
        if ui.add_enabled(there && !current, egui::Button::new(tr("Open"))).on_hover_text(tr("Open this working folder in this window")).clicked() {
            self.open(&wt.path);
            ui.close();
        }
        if ui.add_enabled(there, egui::Button::new(tr("Open in New Window"))).clicked() {
            if let Some(exe) = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok()) {
                if let Err(e) = std::process::Command::new(exe).arg(&wt.path).spawn() {
                    self.dialog = Some(Dialog::Message { title: tr("Open in New Window").into(), text: e.to_string() });
                }
            }
            ui.close();
        }
        if ui.add_enabled(there, egui::Button::new(tr("Copy Path"))).clicked() {
            ui.ctx().copy_text(crate::chrome::native_path(&wt.path).display().to_string());
            ui.close();
        }
        ui.separator();
        let can_remove = !wt.main && !current;
        let why = if wt.main { tr("This is the repository's own folder") } else { tr("Open another working folder first: the open one cannot be removed") };
        if ui.add_enabled(can_remove, egui::Button::new(tr("Remove Worktree…"))).on_disabled_hover_text(why).clicked() {
            self.confirm_remove_worktree(wt);
            ui.close();
        }
        let stale = self.repo.as_ref().is_some_and(|r| r.state.worktrees.iter().any(|w| w.prunable));
        if ui.add_enabled(stale, egui::Button::new(tr("Remove Stale Records"))).on_hover_text(tr("Forget the worktrees whose folders are gone (git worktree prune)")).clicked() {
            self.run_job(Job::one("Prune worktrees", &["worktree", "prune"]));
            ui.close();
        }
    }

    /// Open a submodule as a repository of its own, in a tab.
    pub fn open_submodule(&mut self, sub: &crate::submodule::Submodule) {
        let Some(repo) = self.repo.as_ref() else { return };
        let dir = repo.path.join(&sub.path);
        if sub.state == crate::submodule::State::NotInit || !dir.join(".git").exists() {
            self.dialog = Some(Dialog::Confirm {
                title: tr("Submodule").into(),
                text: crate::i18n::trf("'{}' is not initialised: its folder is empty. Fetch it now?", &[&sub.path]),
                ok: tr("Init").into(),
                job: Some(Job::many("Init submodule", crate::submodule::update_args(Some(&sub.path), false))),
            });
            return;
        }
        self.open(&dir);
    }

    /// The menu of a submodule in the tree.
    fn submodule_actions(&mut self, ui: &mut Ui, sub: &crate::submodule::Submodule) {
        use crate::submodule::{State, sync_args, update_args};
        ui.set_min_width(250.0);
        let there = sub.state != State::NotInit;
        if ui.add_enabled(there, egui::Button::new(tr("Open"))).on_hover_text(tr("Open the submodule as a repository of its own")).clicked() {
            self.open_submodule(sub);
            ui.close();
        }
        ui.separator();
        if !there {
            if ui.button(tr("Init")).on_hover_text(tr("Fetch the submodule into its folder (git submodule update --init)")).clicked() {
                self.run_job(Job::many("Init submodule", update_args(Some(&sub.path), false)));
                ui.close();
            }
        } else {
            let hint = tr("Check out the commit the repository has recorded for it (git submodule update)");
            if ui.add_enabled(sub.state != State::Same, egui::Button::new(tr("Update"))).on_hover_text(hint).on_disabled_hover_text(tr("The recorded commit is already checked out")).clicked() {
                self.confirm_submodule_update(Some(sub));
                ui.close();
            }
        }
        if ui.button(tr("Update with Nested Submodules")).on_hover_text(tr("Also the submodules inside it (--recursive)")).clicked() {
            self.run_job(Job::many("Update submodule", update_args(Some(&sub.path), true)));
            ui.close();
        }
        if ui.button(tr("Sync Address")).on_hover_text(tr("Take the address from .gitmodules again, after it was changed there (git submodule sync)")).clicked() {
            self.run_job(Job::many("Sync submodule", sync_args(Some(&sub.path))));
            ui.close();
        }
        ui.separator();
        if ui.button(tr("Update All Submodules")).clicked() {
            self.confirm_submodule_update(None);
            ui.close();
        }
        ui.separator();
        if ui.button(tr("Copy Path")).clicked() {
            if let Some(repo) = self.repo.as_ref() {
                ui.ctx().copy_text(crate::chrome::native_path(&repo.path.join(&sub.path)).display().to_string());
            }
            ui.close();
        }
        if ui.add_enabled(!sub.url.is_empty(), egui::Button::new(tr("Copy Address"))).clicked() {
            ui.ctx().copy_text(sub.url.clone());
            ui.close();
        }
    }

    /// Update one submodule or all of them; when a commit of its own is checked out there or
    /// files are changed, say first what the update does to them.
    pub fn confirm_submodule_update(&mut self, one: Option<&crate::submodule::Submodule>) {
        let Some(repo) = self.repo.as_ref() else { return };
        let subs: Vec<&crate::submodule::Submodule> = match one {
            Some(m) => vec![m],
            None => repo.submodules.iter().collect(),
        };
        let job = Job::many(if one.is_some() { "Update submodule" } else { "Update submodules" }, crate::submodule::update_args(one.map(|m| m.path.as_str()), one.is_none()));
        match update_warning(&subs) {
            Some(text) => self.dialog = Some(Dialog::Confirm { title: tr("Update Submodules").into(), text, ok: tr("Update").into(), job: Some(job) }),
            None => self.run_job(job),
        }
    }

    /// Ask before removing a worktree; one with changes in it is removed only by force.
    fn confirm_remove_worktree(&mut self, wt: &crate::git::Worktree) {
        let path = wt.path.display().to_string();
        if wt.prunable {
            self.run_job(Job::one("Prune worktrees", &["worktree", "prune"]));
            return;
        }
        let changed = crate::git::status(&wt.path).len();
        let (text, args) = remove_worktree_plan(&path, wt.branch.as_deref(), changed, wt.locked);
        self.dialog = Some(Dialog::Confirm { title: tr("Remove Worktree").into(), text, ok: tr("Remove").into(), job: Some(Job::many("Remove worktree", vec![args])) });
    }

    fn tag_actions(&mut self, ui: &mut Ui, name: &str) {
        let cur = self
            .repo
            .as_ref()
            .and_then(|r| r.state.head_branch.clone())
            .unwrap_or_else(|| "HEAD".into());
        if ui.button(tr("Checkout Tag")).clicked() {
            self.checkout_branch(name, RefKind::Tag);
        }
        if ui.button(format!("New Branch from '{name}'…")).clicked() {
            self.open_new_branch(&format!("tags/{name}"));
        }
        if ui.button(format!("Merge '{name}' into '{cur}'")).clicked() {
            self.run_job(Job::one(&format!("Merge {name}"), &["merge", "--no-edit", &format!("tags/{name}")]));
        }
        if ui.button(tr("Merge with Options…")).on_hover_text(tr("Choose how to merge: always a merge commit, fast-forward only or squash")).clicked() {
            self.open_merge_options(&format!("tags/{name}"), name);
        }
        ui.separator();
        if ui.button(tr("Show in Log")).clicked() {
            self.navigate_to(&format!("tags/{name}"));
        }
        let remotes: Vec<String> = self.repo.as_ref().map(|r| r.state.remotes.clone()).unwrap_or_default();
        for remote in &remotes {
            if ui.button(trf("Push Tag to {}", &[remote])).clicked() {
                self.run_job(Job::one(&format!("Push tag {name}"), &["push", remote, &format!("refs/tags/{name}")]));
            }
        }
        ui.separator();
        if ui.button(tr("Delete Tag")).clicked() {
            self.confirm_delete_tag(name);
        }
        for remote in &remotes {
            if ui.button(trf("Delete Tag on {}…", &[remote])).on_hover_text(tr("Remove the tag from the server; the local tag stays")).clicked() {
                self.dialog = Some(Dialog::Confirm {
                    title: tr("Delete Tag on Remote").into(),
                    text: trf("Delete the tag '{}' on {}? Other people who already fetched it keep their copy.", &[&name, remote]),
                    ok: tr("Delete").into(),
                    job: Some(Job::one(&format!("Delete tag {name} on {remote}"), &["push", remote, "--delete", &format!("refs/tags/{name}")])),
                });
            }
        }
    }

    fn confirm_delete_tag(&mut self, name: &str) {
        self.dialog = Some(Dialog::Confirm {
            title: tr("Delete Tag").into(),
            text: format!("Delete the local tag '{name}'?"),
            ok: "Delete".into(),
            job: Some(Job::one(&format!("Delete tag {name}"), &["tag", "-d", name])),
        });
    }

    pub fn selected_tree_branch(&self) -> Option<Branch> {
        let repo = self.repo.as_ref()?;
        let sel = repo.tree_selected.as_ref()?;
        let (kind, name) = sel.split_once(':')?;
        repo.state
            .branches
            .iter()
            .chain(repo.state.tags.iter())
            .find(|b| format!("{:?}", b.kind) == kind && b.name == name)
            .cloned()
    }

    fn branch_toolbar(&mut self, ui: &mut Ui) {
        let sel = self.selected_tree_branch();
        let cur = self.repo.as_ref().and_then(|r| r.state.head_branch.clone());
        let is_cur = sel
            .as_ref()
            .map(|b| b.kind == RefKind::Local && Some(&b.name) == cur.as_ref())
            .unwrap_or(false);
        let has = sel.is_some();
        // Tooltips name the branches the button acts on.
        let name = sel.as_ref().map(|b| b.name.clone()).unwrap_or_default();
        let what = sel
            .as_ref()
            .map(|b| match b.kind {
                RefKind::Tag => format!("tag {}", b.name),
                _ => b.name.clone(),
            })
            .unwrap_or_default();
        let cur_name = cur.clone().unwrap_or_else(|| "HEAD".into());
        let pick = "Select a branch first";
        let new_tip = if has { format!("New branch from {what}…") } else { format!("New branch from {cur_name}…") };
        let delete_tip = match sel.as_ref() {
            None => pick.to_string(),
            Some(_) if is_cur => format!("{name} is the current branch and can't be deleted"),
            Some(b) if b.kind == RefKind::Remote => format!("Delete {name} on the remote"),
            Some(_) => format!("Delete {what}"),
        };
        let merge_tip = match sel.as_ref() {
            None => pick.to_string(),
            Some(_) if is_cur => format!("{name} is the current branch"),
            Some(_) => format!("Merge {what} into {cur_name}"),
        };
        let show_tip = if has { format!("Show {what} in the log") } else { pick.to_string() };
        let checkout_tip = match sel.as_ref() {
            None => pick.to_string(),
            Some(_) if is_cur => format!("{name} is already checked out"),
            Some(_) => format!("Checkout {what}"),
        };
        let only_tip = if has { format!("Show only {what} in the log (or double-click it)") } else { pick.to_string() };
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if theme::icon_button(ui, Icon::Plus, &new_tip).clicked() {
                let from = sel.as_ref().map(rev_of).unwrap_or_else(|| "HEAD".into());
                self.open_new_branch(&from);
            }
            // Update the selected branch from its remote (not the current one, unless it is selected).
            let can_update = sel
                .as_ref()
                .map(|b| match b.kind {
                    RefKind::Local => b.upstream.as_deref().map(|u| u.contains('/')).unwrap_or(false),
                    RefKind::Remote => b.name.contains('/'),
                    _ => false,
                })
                .unwrap_or(false);
            let tip = match sel.as_ref() {
                Some(b) if can_update && b.kind == RefKind::Remote => format!("Update {} from the remote", b.name),
                Some(b) if can_update => format!("Update {} from {}", b.name, b.upstream.clone().unwrap_or_default()),
                Some(_) => "The selected branch has no remote branch to update from".to_string(),
                None => pick.to_string(),
            };
            if theme::icon_button_sized(ui, Icon::Pull, &tip, 24.0, false, can_update).clicked() {
                if let Some(b) = &sel {
                    match b.kind {
                        RefKind::Local if is_cur => {
                            // The checked-out branch: pull into the working tree.
                            self.run_job(Job::one(&format!("Update {}", b.name), &["pull", "--no-rebase", "--no-edit"]));
                        }
                        RefKind::Local => {
                            // Another local branch: move it to its remote branch (fast-forward only).
                            if let Some((remote, rb)) = b.upstream.as_deref().and_then(|u| u.split_once('/')) {
                                let spec = format!("{rb}:{}", b.name);
                                self.run_job(Job::one(&format!("Update {}", b.name), &["fetch", remote, &spec]));
                            }
                        }
                        RefKind::Remote => {
                            if let Some((remote, rb)) = b.name.split_once('/') {
                                self.run_job(Job::one(&format!("Update {}", b.name), &["fetch", remote, rb]));
                            }
                        }
                        _ => {}
                    }
                }
            }
            if theme::icon_button_sized(ui, Icon::Trash, &delete_tip, 24.0, false, has && !is_cur).clicked() {
                if let Some(b) = &sel {
                    if b.kind == RefKind::Tag {
                        self.confirm_delete_tag(&b.name);
                    } else {
                        self.dialog = Some(Dialog::DeleteBranch {
                            name: b.name.clone(),
                            remote: b.kind == RefKind::Remote,
                            force: false,
                        });
                    }
                }
            }
            if theme::icon_button_sized(ui, Icon::Merge, &merge_tip, 24.0, false, has && !is_cur)
                .clicked()
            {
                if let Some(b) = &sel {
                    self.run_job(Job::one(&format!("Merge {}", b.name), &["merge", "--no-edit", &rev_of(b)]));
                }
            }
            if theme::icon_button_sized(ui, Icon::Search, &show_tip, 24.0, false, has).clicked() {
                if let Some(b) = &sel {
                    self.navigate_to(&rev_of(b));
                }
            }
            let fav_on = sel.as_ref().map(|b| self.is_favorite(&b.name)).unwrap_or(false);
            let fav_tip = match (has, fav_on) {
                (false, _) => pick.to_string(),
                (true, true) => format!("Remove {what} from favorites"),
                (true, false) => format!("Mark {what} as favorite"),
            };
            if theme::icon_button_sized(
                ui,
                if fav_on { Icon::Star } else { Icon::StarOutline },
                &fav_tip,
                24.0,
                false,
                has,
            )
            .clicked()
            {
                if let Some(b) = &sel {
                    self.toggle_favorite(&b.name);
                }
            }
            if theme::icon_button_sized(ui, Icon::Branch, &checkout_tip, 24.0, false, has && !is_cur).clicked() {
                if let Some(b) = &sel {
                    self.checkout_branch(&b.name, b.kind);
                }
            }
            ui.add_space(14.0);
            if theme::icon_button_sized(ui, Icon::Log, &only_tip, 24.0, false, has)
                .clicked()
            {
                if let (Some(b), Some(r)) = (&sel, self.repo.as_mut()) {
                    r.branch_filter = Some(rev_of(b));
                    App::reload_log(r);
                }
            }
            if theme::icon_button(ui, Icon::Expand, tr("Expand all branch folders")).clicked() {
                if let Some(r) = self.repo.as_mut() {
                    r.tree_closed.clear();
                    r.tree_closed.insert("T!open".into());
                    r.tree_open = all_folders(&r.state.branches).into_iter().collect();
                }
            }
            if theme::icon_button(ui, Icon::Collapse, tr("Collapse all branch folders")).clicked() {
                if let Some(r) = self.repo.as_mut() {
                    let mut keys = vec!["L".to_string(), "R".to_string()];
                    for b in &r.state.branches {
                        let base = if b.kind == RefKind::Local { "L" } else { "R" };
                        let parts: Vec<&str> = b.name.split('/').collect();
                        let mut k = base.to_string();
                        for p in &parts[..parts.len().saturating_sub(1)] {
                            k = format!("{k}/{p}");
                            keys.push(k.clone());
                        }
                    }
                    r.tree_closed.extend(keys);
                    r.tree_closed.remove("T!open");
                    r.tree_open.clear();
                }
            }
        });
    }
}

/// The small text after a submodule's name in the tree: where it is against what is recorded.
pub fn submodule_note(sub: &crate::submodule::Submodule) -> String {
    use crate::submodule::State;
    let short = |h: &str| crate::app::short(h).to_string();
    let mut note = match (sub.state, &sub.actual) {
        (State::NotInit, _) => tr("not initialised").to_string(),
        (State::Conflict, _) => tr("conflict").to_string(),
        (State::Moved, Some(a)) => format!("{} → {}", short(&sub.recorded), short(a)),
        _ => short(&sub.recorded),
    };
    if sub.modified || sub.untracked {
        note = format!("{note}, {}", tr("has changes"));
    }
    note
}

/// What to say before updating these submodules, when the update takes something away from
/// view: a commit of its own that is checked out there, or changed files that may stop it.
pub fn update_warning(subs: &[&crate::submodule::Submodule]) -> Option<String> {
    let moved: Vec<&str> = subs.iter().filter(|m| m.pointer_moved()).map(|m| m.path.as_str()).collect();
    let dirty: Vec<&str> = subs.iter().filter(|m| m.modified).map(|m| m.path.as_str()).collect();
    if moved.is_empty() && dirty.is_empty() {
        return None;
    }
    let mut text = String::new();
    if !moved.is_empty() {
        text.push_str(&crate::i18n::trf("Another commit than the recorded one is checked out in: {}.\nThe update checks out the recorded commit there; a commit that is on no branch of the submodule stays reachable only through its reflog.", &[&moved.join(", ")]));
    }
    if !dirty.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&crate::i18n::trf("Files are changed and not committed in: {}.\nGit stops the update when it would overwrite them.", &[&dirty.join(", ")]));
    }
    Some(text)
}

/// What removing a worktree will do, in words, and the command for it. A worktree with
/// changes or a locked one is only removed by force.
pub fn remove_worktree_plan(path: &str, branch: Option<&str>, changed: usize, locked: bool) -> (String, Vec<String>) {
    let mut text = trf("Remove the working folder {}?", &[&path]);
    if let Some(b) = branch {
        text.push_str(&format!("\n{}", trf("The branch '{}' stays in the repository.", &[&b])));
    }
    let mut args = vec!["worktree".to_string(), "remove".into()];
    if changed > 0 {
        text.push_str(&format!("\n\n{}", trf("It has {} changed or new file(s) that are not committed. They will be lost.", &[&changed])));
    }
    if changed > 0 || locked {
        args.push("--force".into());
        if locked {
            // A locked worktree needs the option twice.
            args.push("--force".into());
        }
    }
    args.push(path.to_string());
    (text, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_from_git_get_backslashes() {
        use crate::chrome::{backslashed, native_path, repo_file};
        assert_eq!(backslashed("C:/Users/me/OneDrive/Documents/repo"), "C:\\Users\\me\\OneDrive\\Documents\\repo");
        assert_eq!(backslashed("C:/Users/me/repo\\src\\a.rs"), "C:\\Users\\me\\repo\\src\\a.rs");
        // On this system the path is left as it is unless this is Windows.
        let p = repo_file(std::path::Path::new("/work/repo"), "src/a.rs");
        if cfg!(windows) {
            assert!(!native_path(&p).to_string_lossy().contains('/'));
        } else {
            assert_eq!(native_path(&p), p);
        }
    }

    #[test]
    fn a_worktree_with_changes_is_removed_by_force() {
        let (text, args) = remove_worktree_plan("/w/app-fix", Some("fix/login"), 0, false);
        assert_eq!(args, ["worktree", "remove", "/w/app-fix"]);
        assert!(text.contains("/w/app-fix") && text.contains("fix/login") && !text.contains("lost"));
        let (text, args) = remove_worktree_plan("/w/app-fix", None, 3, false);
        assert_eq!(args, ["worktree", "remove", "--force", "/w/app-fix"]);
        assert!(text.contains('3'));
        assert_eq!(remove_worktree_plan("/w/x", None, 0, true).1, ["worktree", "remove", "--force", "--force", "/w/x"]);
    }
}
