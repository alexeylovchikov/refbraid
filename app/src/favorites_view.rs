//! The Favorites panel: favourite repositories, optionally in groups (drag a repository onto a
//! group to put it there, onto the free space below to take it out), each with its current
//! branch and what there is to push or pull. Clicking one makes it the open repository.
//!
//! The branch and counts are read by background threads, so a long list never slows the start
//! or the window down; until they arrive the rows show the names only.

use crate::app::{App, Dialog};
use crate::i18n::tr;
use crate::git;
use crate::settings::{FavGroup, FavRepo};
use crate::theme::{self, Icon};
use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the panel shows about a repository.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FavInfo {
    /// Current branch; None when detached (then `head` is the short hash).
    pub branch: Option<String>,
    pub head: String,
    /// Commits to push / to pull (compared with the upstream branch, as of the last fetch).
    pub ahead: usize,
    pub behind: usize,
    pub upstream: Option<String>,
    /// Changed tracked files not committed yet.
    pub changes: usize,
    /// The folder is gone or is no longer a repository.
    pub missing: bool,
    /// The last "Fetch All" failed for it (e.g. it needs a password).
    pub fetch_error: Option<String>,
}

pub struct FavState {
    info: HashMap<PathBuf, FavInfo>,
    tx: Sender<(PathBuf, FavInfo)>,
    rx: Receiver<(PathBuf, FavInfo)>,
    /// Repositories being read (or fetched) right now.
    loading: HashSet<PathBuf>,
    loaded_at: Option<Instant>,
    fetching: bool,
}

impl Default for FavState {
    fn default() -> Self {
        let (tx, rx) = channel();
        FavState { info: HashMap::new(), tx, rx, loading: HashSet::new(), loaded_at: None, fetching: false }
    }
}

impl FavState {
    /// What Favorites has read about a repository.
    pub fn info_of(&self, path: &std::path::Path) -> Option<&FavInfo> {
        self.info.get(path)
    }
}

/// Dragged repository.
#[derive(Clone)]
struct FavDrag(PathBuf);

/// Read the branch, the push/pull counts and the number of changed files: a few quick
/// local git commands, no network.
pub fn read_info(dir: &Path) -> FavInfo {
    if !dir.is_dir() || git::find_repo_root(dir).is_none() {
        return FavInfo { missing: true, ..FavInfo::default() };
    }
    let mut i = FavInfo::default();
    let b = git::run(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    if b.ok {
        i.branch = Some(b.stdout.trim().to_string());
    }
    i.head = git::run(dir, &["rev-parse", "--short", "HEAD"]).stdout.trim().to_string();
    let up = git::run(dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]);
    if up.ok && !up.stdout.trim().is_empty() {
        i.upstream = Some(up.stdout.trim().to_string());
        let c = git::run(dir, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]).stdout;
        let mut n = c.split_whitespace().map(|x| x.parse::<usize>().unwrap_or(0));
        i.ahead = n.next().unwrap_or(0);
        i.behind = n.next().unwrap_or(0);
    }
    i.changes = git::run(dir, &["status", "--porcelain", "--untracked-files=no", "--ignore-submodules"]).stdout.lines().filter(|l| !l.trim().is_empty()).count();
    i
}

/// Fetch without ever asking for a password: repositories that need one are skipped and
/// marked, so "Fetch All" can't pop up a dozen prompts.
fn fetch_quietly(dir: &Path) -> Option<String> {
    let env = [("GIT_ASKPASS", ""), ("SSH_ASKPASS", ""), ("SSH_ASKPASS_REQUIRE", "never"), ("GCM_INTERACTIVE", "never")];
    let out = git::run_env(dir, &env, &["fetch", "--all", "--prune", "--quiet"]);
    if out.ok {
        None
    } else {
        Some(out.stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("fetch failed").trim().to_string())
    }
}

/// Rows of the panel: groups first (each followed by its repositories unless folded), then
/// the repositories without a group. Repositories name groups by name; an unknown group
/// counts as none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    Group(usize),
    /// Index into `fav_repos`, and whether it sits inside a group.
    Repo(usize, bool),
}

pub fn rows(repos: &[FavRepo], groups: &[FavGroup]) -> Vec<Row> {
    let mut out = vec![];
    for (gi, g) in groups.iter().enumerate() {
        out.push(Row::Group(gi));
        if !g.collapsed {
            out.extend(repos.iter().enumerate().filter(|(_, r)| r.group == g.name).map(|(i, _)| Row::Repo(i, true)));
        }
    }
    let known: HashSet<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    out.extend(repos.iter().enumerate().filter(|(_, r)| !known.contains(r.group.as_str())).map(|(i, _)| Row::Repo(i, false)));
    out
}

/// Put repository `from` into `group` ("" = none), just before `before` or at the end.
pub fn move_repo(list: &mut Vec<FavRepo>, from: &Path, group: &str, before: Option<&Path>) {
    let Some(i) = list.iter().position(|r| r.path == from) else { return };
    if before == Some(from) {
        list[i].group = group.to_string();
        return;
    }
    let mut r = list.remove(i);
    r.group = group.to_string();
    let at = before.and_then(|b| list.iter().position(|x| x.path == b)).unwrap_or(list.len());
    list.insert(at, r);
}

fn repo_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// A square with the repository's first letter, its colour picked from the name.
fn paint_avatar(p: &egui::Painter, rect: Rect, name: &str) {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let c = theme::graph_color((h % 8) as u16);
    p.rect_filled(rect, 5.0, c.gamma_multiply(0.75));
    let letter: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
    p.text(rect.center(), egui::Align2::CENTER_CENTER, letter, FontId::proportional(rect.height() * 0.55), Color32::WHITE);
}

const ROW_H: f32 = 44.0;
const GROUP_H: f32 = 26.0;

enum Action {
    Open(PathBuf),
    /// One click: in the quick-view tab. A double-click keeps that tab.
    Preview(PathBuf),
    Keep(PathBuf),
    Remove(PathBuf),
    Move { from: PathBuf, group: String, before: Option<PathBuf> },
    Fold(usize),
    RenameGroup(String),
    DeleteGroup(String),
    NewGroup,
    AddCurrent,
    RemoveCurrent,
    Reveal(PathBuf),
}

impl App {
    /// Read the favourites in the background (`fetch`: run "git fetch" for each first).
    pub fn load_favorites(&mut self, fetch: bool) {
        let paths: Vec<PathBuf> = self.settings.fav_repos.iter().map(|r| r.path.clone()).filter(|p| !self.fav.loading.contains(p)).collect();
        self.fav.loaded_at = Some(Instant::now());
        self.fav.fetching |= fetch;
        self.read_favorites(paths, fetch);
    }

    fn read_favorites(&mut self, paths: Vec<PathBuf>, fetch: bool) {
        if paths.is_empty() {
            return;
        }
        self.fav.loading.extend(paths.iter().cloned());
        let queue = Arc::new(Mutex::new(paths));
        let workers = queue.lock().map(|q| q.len()).unwrap_or(1).min(4);
        for _ in 0..workers {
            let queue = queue.clone();
            let tx = self.fav.tx.clone();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                loop {
                    let next = queue.lock().ok().and_then(|mut q| q.pop());
                    let Some(p) = next else { break };
                    let err = if fetch && p.is_dir() { fetch_quietly(&p) } else { None };
                    let mut info = read_info(&p);
                    info.fetch_error = err;
                    if tx.send((p, info)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
    }

    /// The open repository changed (commit, pull, outside change…): update its row.
    pub fn refresh_favorite(&mut self, p: &Path) {
        if self.settings.is_fav_repo(p) && !self.fav.loading.contains(p) {
            self.read_favorites(vec![p.to_path_buf()], false);
        }
    }

    pub fn poll_favorites(&mut self) {
        while let Ok((p, mut info)) = self.fav.rx.try_recv() {
            self.fav.loading.remove(&p);
            if info.fetch_error.is_none() && !self.fav.fetching {
                // A plain re-read keeps the last fetch result.
                info.fetch_error = self.fav.info.get(&p).and_then(|i| i.fetch_error.clone());
            }
            self.fav.info.insert(p, info);
        }
        if self.fav.loading.is_empty() {
            self.fav.fetching = false;
        }
        // Keep the counts fresh while the panel is open.
        if self.show_favorites && self.fav.loading.is_empty() && self.fav.loaded_at.is_none_or(|t| t.elapsed() > Duration::from_secs(120)) {
            self.load_favorites(false);
        }
    }

    pub fn toggle_favorites(&mut self) {
        self.show_favorites = !self.show_favorites;
        if self.show_favorites && self.fav.loaded_at.is_none_or(|t| t.elapsed() > Duration::from_secs(15)) {
            self.load_favorites(false);
        }
    }

    pub fn add_current_to_favorites(&mut self) {
        let Some(p) = self.repo.as_ref().map(|r| r.path.clone()) else { return };
        if !self.settings.is_fav_repo(&p) {
            self.settings.toggle_fav_repo(&p);
            self.save_settings();
            self.read_favorites(vec![p], false);
        }
    }

    pub fn toggle_favorite_repo(&mut self, p: &Path) {
        self.settings.toggle_fav_repo(p);
        self.save_settings();
        if self.settings.is_fav_repo(p) {
            self.read_favorites(vec![p.to_path_buf()], false);
        }
    }

    fn save_settings(&self) {
        if self.screenshot.is_none() {
            self.settings.save();
        }
    }

    /// Create a group, or rename `old` (the repositories in it follow).
    pub fn save_fav_group(&mut self, old: Option<&str>, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        match old {
            Some(o) => {
                for g in self.settings.fav_groups.iter_mut().filter(|g| g.name == o) {
                    g.name = name.to_string();
                }
                for r in self.settings.fav_repos.iter_mut().filter(|r| r.group == o) {
                    r.group = name.to_string();
                }
            }
            None => {
                if !self.settings.fav_groups.iter().any(|g| g.name == name) {
                    self.settings.fav_groups.push(FavGroup { name: name.to_string(), collapsed: false });
                }
            }
        }
        self.save_settings();
    }

    pub fn favorites_panel(&mut self, ui: &mut Ui) {
        let current = self.repo.as_ref().map(|r| r.path.clone());
        let mut act: Option<Action> = None;
        egui::Panel::top("fav_header")
            .exact_size(34.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| {
                let r = ui.max_rect();
                theme::hline(ui, r.bottom() - 0.5, r.left(), r.right());
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(tr("Favorites")).strong().color(theme::TEXT()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        if theme::icon_button_sized(ui, Icon::Minus, tr("Hide"), 22.0, false, true).clicked() {
                            self.show_favorites = false;
                        }
                        let busy = !self.fav.loading.is_empty();
                        let any = !self.settings.fav_repos.is_empty();
                        if theme::icon_button_sized(ui, Icon::Fetch, tr("Fetch All: get the news of every repository from its server (repositories that ask for a password are skipped)"), 22.0, false, any && !busy).clicked() {
                            self.load_favorites(true);
                        }
                        if theme::icon_button_sized(ui, Icon::Refresh, tr("Refresh branches and counts"), 22.0, false, any && !busy).clicked() {
                            self.load_favorites(false);
                        }
                        // Same as File → Open Repository… (new groups: right-click in the list).
                        if theme::icon_button_sized(ui, Icon::Folder, tr("Open Repository…  (Ctrl+O)"), 22.0, false, true).clicked() {
                            self.choose_folder();
                        }
                        // The open repository: outlined star = not a favourite (click to add),
                        // filled star = a favourite (click to take it out).
                        let fav_now = current.as_ref().is_some_and(|p| self.settings.is_fav_repo(p));
                        let name = current.as_deref().map(repo_name).unwrap_or_default();
                        let tip = match (&current, fav_now) {
                            (None, _) => "Open a repository to add it to Favorites".to_string(),
                            (Some(_), false) => format!("Add the open repository ({name}) to Favorites"),
                            (Some(_), true) => format!("Remove the open repository ({name}) from Favorites"),
                        };
                        let star = theme::icon_button_sized(ui, if fav_now { Icon::Star } else { Icon::StarOutline }, &tip, 22.0, false, current.is_some());
                        if star.clicked() {
                            act = Some(if fav_now { Action::RemoveCurrent } else { Action::AddCurrent });
                        }
                        // Loading indicator before (to the left of) the action buttons.
                        if busy {
                            ui.add_space(2.0);
                            ui.add(egui::Spinner::new().size(14.0)).on_hover_text(tr("Reading branches and counts…"));
                        }
                    });
                });
            });
        let rect = ui.available_rect_before_wrap();
        if self.settings.fav_repos.is_empty() && self.settings.fav_groups.is_empty() {
            let mut u = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(20.0, 30.0))));
            u.vertical_centered(|ui| {
                ui.add_space(40.0);
                theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(ui.max_rect().center().x, ui.cursor().top() + 14.0), vec2(28.0, 28.0)), Icon::StarOutline, theme::TEXT_DIM());
                ui.add_space(38.0);
                ui.label(RichText::new(tr("No favorite repositories yet")).color(theme::TEXT_MID()));
                ui.add_space(6.0);
                ui.label(RichText::new(tr("Add the open repository with the star above\nor in the repository menu.")).color(theme::TEXT_DIM()).size(12.0));
                if current.is_some() {
                    ui.add_space(12.0);
                    if ui.button(tr("Add the Open Repository")).clicked() {
                        act = Some(Action::AddCurrent);
                    }
                }
            });
            self.fav_action(act);
            return;
        }
        let list = rows(&self.settings.fav_repos, &self.settings.fav_groups);
        let dragging = egui::DragAndDrop::has_payload_of_type::<FavDrag>(ui.ctx());
        let mut u = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        u.set_clip_rect(rect);
        egui::ScrollArea::vertical().id_salt("fav_list").auto_shrink([false, false]).show(&mut u, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.add_space(4.0);
            let w = ui.available_width();
            let font = FontId::proportional(13.0);
            let small = FontId::proportional(12.0);
            for row in &list {
                match *row {
                    Row::Group(gi) => {
                        let g = self.settings.fav_groups[gi].clone();
                        let count = self.settings.fav_repos.iter().filter(|r| r.group == g.name).count();
                        let (r, resp) = ui.allocate_exact_size(vec2(w, GROUP_H), Sense::click());
                        let p = ui.painter();
                        let hover_drop = resp.dnd_hover_payload::<FavDrag>().is_some();
                        if hover_drop {
                            p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 4.0, theme::SELECT_INACTIVE());
                            p.rect_stroke(r.shrink2(vec2(4.0, 1.0)), 4.0, Stroke::new(1.0, theme::ACCENT()), egui::StrokeKind::Inside);
                        } else if resp.hovered() {
                            p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 4.0, theme::HOVER());
                        }
                        let cy = r.center().y;
                        let chev = if g.collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
                        theme::paint_icon(p, Rect::from_center_size(pos2(r.left() + 14.0, cy), vec2(11.0, 11.0)), chev, theme::TEXT_MID());
                        theme::paint_icon(p, Rect::from_center_size(pos2(r.left() + 32.0, cy), vec2(15.0, 15.0)), Icon::Folder, theme::TEXT_MID());
                        let t = p.text(pos2(r.left() + 46.0, cy), egui::Align2::LEFT_CENTER, &g.name, font.clone(), theme::TEXT());
                        p.text(pos2(t.right() + 7.0, cy), egui::Align2::LEFT_CENTER, format!("{count}"), small.clone(), theme::TEXT_DIM());
                        if let Some(d) = resp.dnd_release_payload::<FavDrag>() {
                            act = Some(Action::Move { from: d.0.clone(), group: g.name.clone(), before: None });
                        } else if resp.clicked() {
                            act = Some(Action::Fold(gi));
                        }
                        resp.context_menu(|ui| {
                            if ui.button(tr("Rename Group…")).clicked() {
                                act = Some(Action::RenameGroup(g.name.clone()));
                            }
                            if ui.button(tr("Delete Group")).on_hover_text(tr("The repositories in it stay in Favorites")).clicked() {
                                act = Some(Action::DeleteGroup(g.name.clone()));
                            }
                            ui.separator();
                            if ui.button(tr("New Group…")).clicked() {
                                act = Some(Action::NewGroup);
                            }
                        });
                    }
                    Row::Repo(ri, in_group) => {
                        let fav = self.settings.fav_repos[ri].clone();
                        let name = repo_name(&fav.path);
                        let info = self.fav.info.get(&fav.path);
                        let loading = self.fav.loading.contains(&fav.path);
                        let (r, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click_and_drag());
                        resp.dnd_set_drag_payload(FavDrag(fav.path.clone()));
                        let active = current.as_deref() == Some(fav.path.as_path());
                        let p = ui.painter();
                        let bg = r.shrink2(vec2(4.0, 1.0));
                        if active {
                            p.rect_filled(bg, 5.0, theme::SELECT());
                        } else if resp.hovered() && !dragging {
                            p.rect_filled(bg, 5.0, theme::HOVER());
                        }
                        if resp.dnd_hover_payload::<FavDrag>().is_some_and(|d| d.0 != fav.path) {
                            // Dropped here: it goes before this repository, in its group.
                            p.line_segment([pos2(r.left() + 8.0, r.top() + 0.5), pos2(r.right() - 8.0, r.top() + 0.5)], Stroke::new(2.0, theme::ACCENT()));
                        }
                        let x0 = r.left() + if in_group { 30.0 } else { 12.0 };
                        let missing = info.is_some_and(|i| i.missing);
                        paint_avatar(p, Rect::from_min_size(pos2(x0, r.center().y - 13.0), vec2(26.0, 26.0)), &name);
                        let tx = x0 + 36.0;
                        // Right side of the first line: what there is to push / pull, local changes.
                        let mut right = r.right() - 12.0;
                        let y1 = r.top() + 14.0;
                        if let Some(i) = info.filter(|i| !i.missing) {
                            let parts = [
                                (i.ahead > 0, format!("↗{}", i.ahead), theme::GREEN()),
                                (i.behind > 0, format!("↙{}", i.behind), theme::LINK()),
                            ];
                            for (show, t, c) in parts.iter() {
                                if *show {
                                    let g = p.text(pos2(right, y1), egui::Align2::RIGHT_CENTER, t, small.clone(), *c);
                                    right = g.left() - 6.0;
                                }
                            }
                            if i.changes > 0 {
                                p.circle_filled(pos2(right - 3.0, y1), 3.5, theme::BLUE_FILE());
                                right -= 13.0;
                            }
                            if i.fetch_error.is_some() {
                                let g = p.text(pos2(right, y1), egui::Align2::RIGHT_CENTER, "!", FontId::proportional(13.0), theme::UNVERSIONED());
                                right = g.left() - 6.0;
                            }
                        }
                        let name_col = if missing { theme::TEXT_DIM() } else { theme::TEXT() };
                        theme::paint_clipped(p, pos2(tx, y1), &name, font.clone(), name_col, right - tx - 4.0);
                        // Second line: the branch.
                        let y2 = r.top() + 31.0;
                        match info {
                            Some(i) if i.missing => {
                                p.text(pos2(tx, y2), egui::Align2::LEFT_CENTER, tr("Folder not found"), small.clone(), theme::RED());
                            }
                            Some(i) => {
                                theme::paint_icon(p, Rect::from_center_size(pos2(tx + 5.0, y2), vec2(12.0, 12.0)), Icon::Branch, theme::TEXT_DIM());
                                let (t, c) = match &i.branch {
                                    Some(b) => (b.clone(), if active { theme::TEXT() } else { theme::TEXT_MID() }),
                                    None => (format!("{} (detached)", i.head), theme::TEXT_DIM()),
                                };
                                theme::paint_clipped(p, pos2(tx + 14.0, y2), &t, small.clone(), c, r.right() - tx - 26.0);
                            }
                            None => {
                                let t = if loading { "Loading…" } else { "" };
                                p.text(pos2(tx, y2), egui::Align2::LEFT_CENTER, t, small.clone(), theme::TEXT_DIM());
                            }
                        }
                        let tip = {
                            let mut t = fav.path.display().to_string();
                            if let Some(i) = info {
                                if i.missing {
                                    t += "\nThe folder is not there or is not a Git repository.";
                                } else {
                                    if let Some(b) = &i.branch {
                                        t += &format!("\nBranch: {b}");
                                    }
                                    match &i.upstream {
                                        Some(u) if i.ahead == 0 && i.behind == 0 => t += &format!("\nUp to date with {u} (as of the last fetch)"),
                                        Some(u) => t += &format!("\n{} to push, {} to pull ({u}, as of the last fetch)", commits(i.ahead), commits(i.behind)),
                                        None => t += "\nThe branch is not on the server yet",
                                    }
                                    if i.changes > 0 {
                                        t += &format!("\n{} changed file{} not committed", i.changes, if i.changes == 1 { "" } else { "s" });
                                    }
                                    if let Some(e) = &i.fetch_error {
                                        t += &format!("\nFetch failed: {e}");
                                    }
                                }
                            }
                            t
                        };
                        let resp = if dragging { resp } else { resp.on_hover_text(tip) };
                        if let Some(d) = resp.dnd_release_payload::<FavDrag>() {
                            let group = if in_group { fav.group.clone() } else { String::new() };
                            act = Some(Action::Move { from: d.0.clone(), group, before: Some(fav.path.clone()) });
                        } else if resp.double_clicked() {
                            act = Some(Action::Keep(fav.path.clone()));
                        } else if resp.clicked() && !active {
                            act = Some(Action::Preview(fav.path.clone()));
                        }
                        let groups: Vec<String> = self.settings.fav_groups.iter().map(|g| g.name.clone()).collect();
                        resp.context_menu(|ui| {
                            ui.set_min_width(200.0);
                            if ui.add_enabled(!active && !missing, egui::Button::new(tr("Open"))).clicked() {
                                act = Some(Action::Open(fav.path.clone()));
                            }
                            if ui.add_enabled(!missing, egui::Button::new(tr("Open in File Manager"))).clicked() {
                                act = Some(Action::Reveal(fav.path.clone()));
                            }
                            ui.separator();
                            ui.menu_button(tr("Move to Group"), |ui| {
                                for g in &groups {
                                    if ui.add_enabled(*g != fav.group, egui::Button::new(g)).clicked() {
                                        act = Some(Action::Move { from: fav.path.clone(), group: g.clone(), before: None });
                                    }
                                }
                                if !groups.is_empty() {
                                    ui.separator();
                                }
                                if ui.add_enabled(in_group, egui::Button::new(tr("No Group"))).clicked() {
                                    act = Some(Action::Move { from: fav.path.clone(), group: String::new(), before: None });
                                }
                                if ui.button(tr("New Group…")).clicked() {
                                    act = Some(Action::NewGroup);
                                }
                            });
                            if ui.button(tr("Remove from Favorites")).clicked() {
                                act = Some(Action::Remove(fav.path.clone()));
                            }
                        });
                    }
                }
            }
            // The free space below: drop a repository here to take it out of its group.
            let rest = ui.available_height().max(40.0);
            let (r, resp) = ui.allocate_exact_size(vec2(w, rest), Sense::click());
            if dragging {
                let hover = resp.dnd_hover_payload::<FavDrag>().is_some();
                let zone = Rect::from_min_size(r.min + vec2(8.0, 6.0), vec2(r.width() - 16.0, 34.0));
                ui.painter().rect_stroke(zone, 5.0, Stroke::new(1.0, if hover { theme::ACCENT() } else { theme::BORDER() }), egui::StrokeKind::Inside);
                ui.painter().text(zone.center(), egui::Align2::CENTER_CENTER, tr("Drop here to take it out of the group"), FontId::proportional(12.0), if hover { theme::TEXT() } else { theme::TEXT_DIM() });
            }
            if let Some(d) = resp.dnd_release_payload::<FavDrag>() {
                act = Some(Action::Move { from: d.0.clone(), group: String::new(), before: None });
            }
            resp.context_menu(|ui| {
                if ui.button(tr("New Group…")).clicked() {
                    act = Some(Action::NewGroup);
                }
                if ui.add_enabled(current.as_ref().is_some_and(|p| !self.settings.is_fav_repo(p)), egui::Button::new(tr("Add the Open Repository"))).clicked() {
                    act = Some(Action::AddCurrent);
                }
            });
        });
        self.fav_action(act);
    }

    fn fav_action(&mut self, act: Option<Action>) {
        let Some(act) = act else { return };
        match act {
            Action::Open(p) => {
                self.open(&p);
                self.keep_repo_tab(&p);
                self.refresh_favorite(&p);
            }
            Action::Preview(p) => {
                self.open_preview(&p);
                self.refresh_favorite(&p);
            }
            Action::Keep(p) => {
                // The first click of the double-click has opened it already.
                if self.repo.as_ref().is_none_or(|r| !crate::git::same_dir(&r.path, &p)) {
                    self.open_preview(&p);
                }
                self.keep_repo_tab(&p);
            }
            Action::Reveal(p) => crate::chrome::open_in_file_manager(&p),
            Action::Remove(p) => {
                self.settings.fav_repos.retain(|r| r.path != p);
                self.save_settings();
            }
            Action::Move { from, group, before } => {
                move_repo(&mut self.settings.fav_repos, &from, &group, before.as_deref());
                if let Some(g) = self.settings.fav_groups.iter_mut().find(|g| g.name == group) {
                    g.collapsed = false;
                }
                self.save_settings();
            }
            Action::Fold(gi) => {
                if let Some(g) = self.settings.fav_groups.get_mut(gi) {
                    g.collapsed = !g.collapsed;
                }
                self.save_settings();
            }
            Action::RenameGroup(name) => self.dialog = Some(Dialog::FavGroup { old: Some(name.clone()), name }),
            Action::DeleteGroup(name) => {
                self.settings.fav_groups.retain(|g| g.name != name);
                for r in self.settings.fav_repos.iter_mut().filter(|r| r.group == name) {
                    r.group.clear();
                }
                self.save_settings();
            }
            Action::NewGroup => self.dialog = Some(Dialog::FavGroup { old: None, name: String::new() }),
            Action::AddCurrent => self.add_current_to_favorites(),
            Action::RemoveCurrent => {
                if let Some(p) = self.repo.as_ref().map(|r| r.path.clone()) {
                    self.settings.fav_repos.retain(|r| r.path != p);
                    self.save_settings();
                }
            }
        }
    }

    /// Screenshot helper: a few favourites in a group, read synchronously.
    pub fn demo_favorites(&mut self, paths: &[PathBuf]) {
        self.settings.fav_groups = vec![FavGroup { name: "Work".into(), collapsed: false }, FavGroup { name: "Archive".into(), collapsed: true }];
        self.settings.fav_repos = paths
            .iter()
            .enumerate()
            .map(|(i, p)| FavRepo { path: p.clone(), group: if i == 0 { String::new() } else { "Work".into() } })
            .collect();
        self.settings.fav_repos.push(FavRepo { path: PathBuf::from("/tmp/refbraid-demo/old-prototype"), group: String::new() });
        for p in self.settings.fav_repos.iter().map(|r| r.path.clone()).collect::<Vec<_>>() {
            let i = read_info(&p);
            self.fav.info.insert(p, i);
        }
        self.fav.loaded_at = Some(Instant::now());
        self.show_favorites = true;
    }
}

impl App {
    /// Screenshot helper: the favourites still loading (the spinner in the header stays on).
    pub fn demo_favorites_loading(&mut self) {
        self.fav.loading.extend(self.settings.fav_repos.iter().map(|r| r.path.clone()));
    }
}

fn commits(n: usize) -> String {
    format!("{n} commit{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fav(p: &str, g: &str) -> FavRepo {
        FavRepo { path: PathBuf::from(p), group: g.into() }
    }

    #[test]
    fn groups_come_first_and_fold() {
        let repos = vec![fav("/a", ""), fav("/b", "Work"), fav("/c", "Gone"), fav("/d", "Work")];
        let mut groups = vec![FavGroup { name: "Work".into(), collapsed: false }];
        assert_eq!(rows(&repos, &groups), vec![Row::Group(0), Row::Repo(1, true), Row::Repo(3, true), Row::Repo(0, false), Row::Repo(2, false)]);
        groups[0].collapsed = true;
        assert_eq!(rows(&repos, &groups), vec![Row::Group(0), Row::Repo(0, false), Row::Repo(2, false)]);
    }

    #[test]
    fn moves_between_groups() {
        let mut list = vec![fav("/a", ""), fav("/b", "Work"), fav("/c", "Work")];
        move_repo(&mut list, Path::new("/a"), "Work", Some(Path::new("/c")));
        assert_eq!(list, vec![fav("/b", "Work"), fav("/a", "Work"), fav("/c", "Work")]);
        move_repo(&mut list, Path::new("/c"), "", None);
        assert_eq!(list, vec![fav("/b", "Work"), fav("/a", "Work"), fav("/c", "")]);
    }

    #[test]
    fn reads_branch_and_counts() {
        let dir = std::env::temp_dir().join(format!("ge-fav-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let who = [("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")];
        let g = |args: &[&str]| assert!(git::run_env(&dir, &who, args).ok, "{args:?}");
        g(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("f.txt"), "1\n").unwrap();
        g(&["add", "f.txt"]);
        g(&["commit", "-qm", "one"]);
        g(&["branch", "base"]);
        g(&["branch", "--set-upstream-to=base"]);
        std::fs::write(dir.join("f.txt"), "2\n").unwrap();
        g(&["commit", "-qam", "two"]);
        std::fs::write(dir.join("f.txt"), "3\n").unwrap();
        let i = read_info(&dir);
        assert_eq!(i.branch.as_deref(), Some("main"));
        assert_eq!((i.ahead, i.behind, i.changes), (1, 0, 1));
        assert!(read_info(&dir.join("nope")).missing);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
