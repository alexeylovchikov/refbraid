//! The Push window: the commits to push on the left, the files of the selected commit and its
//! details on the right, "Push tags" and a Push button with Force Push in its drop-down.

use crate::app::{App, Dialog, Job, Tab, file_name, format_full_date, short, status_color};
use crate::i18n::{tr, trf};
use crate::form::{self, Part};
use crate::git::{self, ChangedFile, CommitDetails};
use crate::theme::{self, Icon};
use eframe::egui::{self, Color32, FontId, Key, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::collections::{BTreeMap, HashMap, HashSet};

pub struct PushCommit {
    pub hash: String,
    pub subject: String,
}

pub struct PushForm {
    head: String,
    remote: String,
    /// Branch on the server; click it to type another one.
    branch: String,
    editing_branch: bool,
    commits: Vec<PushCommit>,
    selected: Option<usize>,
    details: HashMap<String, Option<CommitDetails>>,
    selected_file: Option<String>,
    collapsed: HashSet<String>,
    flat: bool,
    show_details: bool,
    tags: bool,
    /// Push tags: all of them (--tags) or those of the pushed commits (--follow-tags).
    tags_all: bool,
    /// The diff shown inside the window (index into App::diffs) and the tab it replaced.
    diff: Option<(usize, Tab)>,
    /// Open the diff of the first file as soon as it is known (screenshots).
    pub pending_diff: bool,
    /// Open the Create Pull Request window after the push; what the hosting calls it (None:
    /// nothing to offer for this branch).
    request: bool,
    request_word: Option<&'static str>,
    /// The remote and the branch `request_word` was worked out for.
    request_for: (String, String),
}

const W: f32 = 820.0;
const H: f32 = 440.0;
const LEFT_W: f32 = 400.0;
const ROW: f32 = 22.0;

/// Rows of the right-hand file tree.
enum FRow {
    Folder { dir: String, count: usize, open: bool },
    File { file: ChangedFile, depth: usize },
}

fn file_rows(files: &[ChangedFile], flat: bool, collapsed: &HashSet<String>) -> Vec<FRow> {
    if flat {
        let mut v: Vec<&ChangedFile> = files.iter().collect();
        v.sort_by_key(|f| f.path.to_lowercase());
        return v.into_iter().map(|f| FRow::File { file: f.clone(), depth: 0 }).collect();
    }
    let mut dirs: BTreeMap<String, Vec<&ChangedFile>> = BTreeMap::new();
    let mut root: Vec<&ChangedFile> = vec![];
    for f in files {
        match f.path.rsplit_once('/') {
            Some((d, _)) => dirs.entry(d.to_string()).or_default().push(f),
            None => root.push(f),
        }
    }
    let mut rows = vec![];
    for (dir, mut list) in dirs {
        let open = !collapsed.contains(&dir);
        rows.push(FRow::Folder { dir: dir.clone(), count: list.len(), open });
        if open {
            list.sort_by_key(|f| file_name(&f.path).to_lowercase());
            rows.extend(list.into_iter().map(|f| FRow::File { file: f.clone(), depth: 1 }));
        }
    }
    root.sort_by_key(|f| f.path.to_lowercase());
    rows.extend(root.into_iter().map(|f| FRow::File { file: f.clone(), depth: 0 }));
    rows
}

/// A toolbar toggle drawn with a soft background when on (like "Show Details").
fn toggle_icon(ui: &mut Ui, icon: Icon, tip: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 5.0, theme::rgb(0x43, 0x45, 0x4a));
    } else if resp.hovered() {
        p.rect_filled(rect, 5.0, theme::rgb(0x39, 0x3b, 0x40));
    }
    theme::paint_icon(p, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)), icon, theme::ICON());
    resp.on_hover_text(tip)
}

impl App {
    pub fn open_push_window(&mut self, commits: Vec<PushCommit>, target: String, head: String) {
        let (remote, branch) = target.split_once('/').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or(("origin".into(), head.clone()));
        let selected = if commits.is_empty() { None } else { Some(0) };
        self.push_form = Some(PushForm {
            head,
            remote,
            branch,
            editing_branch: false,
            commits,
            selected,
            details: HashMap::new(),
            selected_file: None,
            collapsed: HashSet::new(),
            flat: self.settings.changes_flat,
            show_details: true,
            tags: false,
            tags_all: true,
            diff: None,
            pending_diff: false,
            request: false,
            request_word: None,
            request_for: Default::default(),
        });
    }

    fn push_args(f: &PushForm, force: bool) -> Vec<String> {
        let mut args: Vec<String> = vec!["push".into(), "-u".into()];
        if force {
            args.push("--force-with-lease".into());
        }
        if f.tags {
            args.push(if f.tags_all { "--tags".into() } else { "--follow-tags".into() });
        }
        args.push(f.remote.clone());
        args.push(format!("HEAD:refs/heads/{}", f.branch.trim()));
        args
    }

    pub fn push_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.push_form.take() else { return };
        let Some(dir) = self.repo.as_ref().map(|r| r.path.clone()) else { return };
        let name = self.repo.as_ref().map(|r| r.name.clone()).unwrap_or_default();
        // Load the selected commit's files once.
        if let Some(c) = f.selected.and_then(|i| f.commits.get(i)) {
            if !f.details.contains_key(&c.hash) {
                f.details.insert(c.hash.clone(), git::commit_details_light(&dir, &c.hash));
            }
        }
        let mut keep = true;
        let mut push: Option<bool> = None; // Some(force)
        let mut show_diff = false;
        let mut jump = false;
        let mut back = false;
        let (ctrl_enter, ctrl_d, f4, ctrl_alt_p) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, Key::Enter),
                i.consume_key(egui::Modifiers::COMMAND, Key::D),
                i.consume_key(egui::Modifiers::NONE, Key::F4),
                i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::ALT, Key::P),
            )
        });
        show_diff |= ctrl_d;
        if f.pending_diff && f.selected_file.is_some() {
            f.pending_diff = false;
            show_diff = true;
        }
        jump |= f4;
        if ctrl_alt_p {
            f.flat = !f.flat;
        }
        let valid = !f.branch.trim().is_empty() && !f.branch.contains(char::is_whitespace);
        let target = (f.remote.clone(), f.branch.trim().to_string());
        if f.request_for != target {
            // Start reading the open requests, so the one this branch may have is known by the push.
            if let Some(site) = self.site_of(&target.0) {
                self.hosting.info(&site, ctx);
            }
            f.request_word = self.request_after_push(&target.0, &target.1);
            f.request_for = target;
        }
        if ctrl_enter && valid {
            push = Some(false);
        }
        let closed = form::show_with(ctx, "push_window", &format!("Push Commits to {name}"), W, false, |ui, part| {
            if part == Part::Footer {
                Self::push_footer(ui, &mut f, valid, &mut push, &mut keep);
                return;
            }
            // Fills the window, which can be made bigger; the divider between the panes can be dragged.
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(W * 0.7), H + form::grow(ui)), Sense::hover());
            if let Some((i, _)) = f.diff {
                // The diff of a file, inside the window, with a way back.
                let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 30.0));
                let mut bu = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(12.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                if bu.link(tr("← Back to commits")).clicked() || bu.input(|i| i.key_pressed(Key::Backspace) && !i.focused) {
                    back = true;
                }
                let body = Rect::from_min_max(pos2(rect.left(), bar.bottom()), rect.max);
                let mut du = ui.new_child(egui::UiBuilder::new().max_rect(body));
                du.set_clip_rect(body);
                self.diff_tab(&mut du, i);
                return;
            }
            let lw = form::split_width(ui.ctx(), rect, "push_window", LEFT_W, 220.0);
            let left = Rect::from_min_max(rect.min, pos2(rect.left() + lw, rect.bottom()));
            let right = Rect::from_min_max(pos2(left.right() + 1.0, rect.top()), rect.max);
            let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(left));
            lu.set_clip_rect(left);
            Self::push_commits(&mut lu, &mut f, left);
            let mut ru = ui.new_child(egui::UiBuilder::new().max_rect(right));
            ru.set_clip_rect(right);
            Self::push_files(&mut ru, &mut f, right, &mut show_diff, &mut jump);
            form::splitter(ui, rect, "push_window", LEFT_W, 220.0);
        });
        if closed {
            keep = false;
        }
        if show_diff && f.diff.is_none() {
            let sel = f.selected.and_then(|i| f.commits.get(i)).and_then(|c| f.details.get(&c.hash).cloned().flatten());
            let file = sel.as_ref().and_then(|d| d.files.iter().find(|x| Some(&x.path) == f.selected_file.as_ref()).cloned());
            if let (Some(d), Some(file)) = (sel, file) {
                let diff = git::commit_file_diff(&dir, &d.hash, d.parents.first().map(|s| s.as_str()), &file);
                let before = self.tab;
                let subtitle = format!("{}  ·  {} vs {}", file.path, short(&d.hash), d.parents.first().map(|p| short(p)).unwrap_or("(empty)"));
                self.push_diff(file_name(&file.path), subtitle, diff);
                if let Tab::Diff(i) = self.tab {
                    f.diff = Some((i, before));
                }
                self.tab = before;
            }
        }
        if back || (!keep && f.diff.is_some()) {
            if let Some((i, before)) = f.diff.take() {
                self.close_diff(i);
                self.tab = match before {
                    Tab::Diff(j) if j >= i => Tab::Diff(j.saturating_sub(1)),
                    t => t,
                };
                if let Tab::Diff(j) = self.tab {
                    if j >= self.diffs.len() {
                        self.tab = Tab::Log;
                    }
                }
            }
            if back {
                keep = true;
            }
        }
        if jump {
            if let Some(p) = &f.selected_file {
                let path = dir.join(p);
                if path.exists() {
                    crate::chrome::open_in_file_manager(&path);
                } else {
                    self.dialog = Some(Dialog::Message { title: tr("Jump to Source").into(), text: format!("{p} is not in the working folder (deleted or renamed since).") });
                }
            }
        }
        if let Some(force) = push {
            let args = Self::push_args(&f, force);
            let mut job = Job::many(if force { "Force push" } else { "Push" }, vec![args]);
            if f.request && f.request_word.is_some() {
                self.pending_request = Some((f.remote.clone(), f.branch.trim().to_string()));
                job = job.then(crate::app::Then::NewRequest);
            }
            self.run_job(job);
            keep = false;
        }
        if keep {
            self.push_form = Some(f);
        }
    }

    fn push_commits(ui: &mut Ui, f: &mut PushForm, rect: Rect) {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        // "master → origin : master"; click the server branch to push somewhere else.
        let head_r = Rect::from_min_size(rect.min + vec2(0.0, 6.0), vec2(rect.width(), ROW + 2.0));
        let font = FontId::proportional(13.0);
        let p = ui.painter().clone();
        let mut x = head_r.left() + 14.0;
        let cy = head_r.center().y;
        let txt = |p: &egui::Painter, x: &mut f32, t: &str, c: Color32| {
            let r = p.text(pos2(*x, cy), egui::Align2::LEFT_CENTER, t, font.clone(), c);
            *x = r.right();
        };
        txt(&p, &mut x, &format!("{} → ", f.head), theme::TEXT());
        txt(&p, &mut x, &f.remote, theme::LINK());
        txt(&p, &mut x, " : ", theme::TEXT_MID());
        if f.editing_branch {
            let er = Rect::from_min_max(pos2(x, head_r.top() + 1.0), pos2((x + 200.0).min(rect.right() - 8.0), head_r.bottom() - 1.0));
            let mut eu = ui.new_child(egui::UiBuilder::new().max_rect(er));
            let r = eu.add(egui::TextEdit::singleline(&mut f.branch).desired_width(er.width()));
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                f.editing_branch = false;
            }
        } else {
            let bw = theme::text_width(ui, &f.branch, &font);
            let br = Rect::from_min_size(pos2(x, head_r.top()), vec2(bw, head_r.height()));
            let resp = ui.interact(br, ui.scope_id().with("push_target"), Sense::click()).on_hover_text(tr("Click to push to another branch"));
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            p.text(pos2(x, cy), egui::Align2::LEFT_CENTER, &f.branch, font.clone(), theme::LINK());
            if resp.clicked() {
                f.editing_branch = true;
            }
        }
        let list = Rect::from_min_max(pos2(rect.left(), head_r.bottom()), rect.max);
        let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(list));
        if f.commits.is_empty() {
            lu.add_space(6.0);
            lu.horizontal(|ui| {
                ui.add_space(30.0);
                ui.label(RichText::new(tr("No new commits. Push anyway to create or update the branch on the server.")).color(theme::TEXT_DIM()).size(12.5));
            });
            return;
        }
        egui::ScrollArea::both().id_salt("push_commits").auto_shrink([false, false]).show(&mut lu, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let w = list.width().max(
                f.commits.iter().map(|c| theme::text_width(ui, &c.subject, &font) + 50.0).fold(0.0, f32::max),
            );
            for (i, c) in f.commits.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(w, ROW - 2.0), Sense::click());
                if f.selected == Some(i) {
                    ui.painter().rect_filled(r, 0.0, theme::SELECT());
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, theme::HOVER());
                }
                ui.painter().text(pos2(r.left() + 30.0, r.center().y), egui::Align2::LEFT_CENTER, &c.subject, font.clone(), theme::TEXT());
                let resp = resp.on_hover_text(format!("{}  {}", short(&c.hash), c.subject));
                if resp.clicked() && f.selected != Some(i) {
                    f.selected = Some(i);
                    f.selected_file = None;
                }
            }
        });
    }

    fn push_files(ui: &mut Ui, f: &mut PushForm, rect: Rect, show_diff: &mut bool, jump: &mut bool) {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        let details = f.selected.and_then(|i| f.commits.get(i)).and_then(|c| f.details.get(&c.hash).cloned().flatten());
        let files: Vec<ChangedFile> = details.as_ref().map(|d| if d.is_merge() { d.merge_files.clone() } else { d.files.clone() }).unwrap_or_default();
        if f.selected_file.is_none() {
            f.selected_file = files.first().map(|x| x.path.clone());
        }
        // Toolbar
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 32.0));
        let mut bu = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let has_file = f.selected_file.is_some();
        if theme::icon_button_sized(&mut bu, Icon::Diff, tr("Show Diff  Ctrl+D"), 24.0, false, has_file).clicked() {
            *show_diff = true;
        }
        let (sep, _) = bu.allocate_exact_size(vec2(9.0, 18.0), Sense::hover());
        bu.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, theme::BORDER()));
        let eye = theme::icon_button(&mut bu, Icon::Eye, tr("View Options"));
        egui::Popup::menu(&eye).show(|ui| {
            ui.set_min_width(200.0);
            ui.label(RichText::new(tr("Group By")).color(theme::TEXT_DIM()).size(11.5));
            let label = if f.flat { "     Directory" } else { "✓  Directory" };
            if ui.add(egui::Button::new(label).shortcut_text(RichText::new(tr("Ctrl+Alt+P")).color(theme::TEXT_DIM())).frame_when_inactive(!f.flat)).clicked() {
                f.flat = !f.flat;
                ui.close();
            }
        });
        if theme::icon_button_sized(&mut bu, Icon::Pencil, tr("Jump to Source  F4"), 24.0, false, has_file).clicked() {
            *jump = true;
        }
        if toggle_icon(&mut bu, Icon::PanelBottom, "Show Details", f.show_details).clicked() {
            f.show_details = !f.show_details;
        }
        bu.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::icon_button(ui, Icon::Collapse, tr("Collapse All")).clicked() {
                for x in &files {
                    if let Some((d, _)) = x.path.rsplit_once('/') {
                        f.collapsed.insert(d.to_string());
                    }
                }
            }
            if theme::icon_button(ui, Icon::Expand, tr("Expand All")).clicked() {
                f.collapsed.clear();
            }
        });
        let details_h = if f.show_details && details.is_some() { 150.0 } else { 0.0 };
        let tree = Rect::from_min_max(pos2(rect.left(), bar.bottom()), pos2(rect.right(), rect.bottom() - details_h));
        let mut tu = ui.new_child(egui::UiBuilder::new().max_rect(tree.shrink2(vec2(8.0, 2.0))));
        tu.set_clip_rect(tree);
        if details.is_none() {
            tu.painter().text(tree.center(), egui::Align2::CENTER_CENTER, tr("Select a commit to see its files"), FontId::proportional(13.0), theme::TEXT_DIM());
        } else {
            let rows = file_rows(&files, f.flat, &f.collapsed);
            let mut fold: Option<String> = None;
            egui::ScrollArea::vertical().id_salt("push_files").auto_shrink([false, false]).show(&mut tu, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let w = ui.available_width();
                let font = FontId::proportional(13.0);
                for row in &rows {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, ROW), Sense::click());
                    let cy = r.center().y;
                    match row {
                        FRow::Folder { dir, count, open } => {
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 4.0, theme::HOVER());
                            }
                            let chev = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
                            theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 8.0, cy), vec2(11.0, 11.0)), chev, theme::TEXT_MID());
                            theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 26.0, cy), vec2(14.0, 14.0)), Icon::Folder, theme::TEXT_MID());
                            let t = ui.painter().text(pos2(r.left() + 38.0, cy), egui::Align2::LEFT_CENTER, dir, font.clone(), theme::TEXT());
                            ui.painter().text(pos2(t.right() + 6.0, cy), egui::Align2::LEFT_CENTER, format!("{count} file{}", if *count == 1 { "" } else { "s" }), FontId::proportional(12.0), theme::TEXT_DIM());
                            if resp.clicked() {
                                fold = Some(dir.clone());
                            }
                        }
                        FRow::File { file, depth } => {
                            if f.selected_file.as_deref() == Some(file.path.as_str()) {
                                ui.painter().rect_filled(r, 4.0, theme::SELECT());
                            } else if resp.hovered() {
                                ui.painter().rect_filled(r, 4.0, theme::HOVER());
                            }
                            let x = r.left() + 20.0 + 18.0 * *depth as f32;
                            theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(x + 6.0, cy), vec2(14.0, 14.0)), Icon::File, theme::TEXT_MID());
                            let name = if f.flat { file.path.clone() } else { file_name(&file.path) };
                            ui.painter().text(pos2(x + 18.0, cy), egui::Align2::LEFT_CENTER, &name, font.clone(), status_color(file.status));
                            if resp.clicked() {
                                f.selected_file = Some(file.path.clone());
                            }
                            if resp.double_clicked() {
                                f.selected_file = Some(file.path.clone());
                                *show_diff = true;
                            }
                        }
                    }
                }
            });
            if let Some(d) = fold {
                if !f.collapsed.remove(&d) {
                    f.collapsed.insert(d);
                }
            }
        }
        // Details of the selected commit
        if let (true, Some(d)) = (details_h > 0.0, &details) {
            let dr = Rect::from_min_max(pos2(rect.left(), rect.bottom() - details_h), rect.max);
            theme::hline(ui, dr.top() + 0.5, dr.left(), dr.right());
            let mut du = ui.new_child(egui::UiBuilder::new().max_rect(dr.shrink2(vec2(14.0, 12.0))));
            du.set_clip_rect(dr);
            egui::ScrollArea::vertical().id_salt("push_details").auto_shrink([false, false]).show(&mut du, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let mut lines = d.message.lines();
                let subject = lines.next().unwrap_or("");
                ui.label(RichText::new(subject).strong().size(14.0).color(theme::TEXT()));
                let body: Vec<&str> = lines.skip_while(|l| l.trim().is_empty()).collect();
                if !body.is_empty() {
                    ui.label(RichText::new(body.join("\n")).color(theme::TEXT_MID()));
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(short(&d.hash)).color(theme::TEXT_MID()));
                    ui.label(RichText::new(&d.author).color(theme::TEXT_MID()));
                    ui.label(RichText::new(format!("<{}>", d.email)).color(theme::LINK()));
                    ui.label(RichText::new(format!("on {}", format_full_date(d.author_time))).color(theme::TEXT_MID()));
                });
            });
        }
    }

    fn push_footer(ui: &mut Ui, f: &mut PushForm, valid: bool, push: &mut Option<bool>, keep: &mut bool) {
        // Right side (right to left): Cancel, then the Push button with its drop-down.
        if form::button(ui, tr("Cancel")) {
            *keep = false;
        }
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.x = 1.0;
        let arrow = ui.add_enabled(
            valid,
            egui::Button::new(RichText::new("⌄").color(Color32::WHITE)).fill(theme::ACCENT()).min_size(vec2(26.0, 28.0)),
        );
        let arrow = arrow.on_hover_text(tr("More push options"));
        egui::Popup::menu(&arrow).show(|ui| {
            if ui.button(tr("Force Push")).on_hover_text(tr("git push --force-with-lease: overwrite the server branch unless someone else pushed to it")).clicked() {
                *push = Some(true);
            }
        });
        if form::primary(ui, if f.commits.is_empty() { "Push Anyway" } else { "Push" }, valid, false) {
            *push = Some(false);
        }
        ui.spacing_mut().item_spacing.x = 8.0;
        // Left side
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add_space(2.0);
            let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
            ui.painter().circle_stroke(r.center(), 10.5, Stroke::new(1.0, theme::TEXT_DIM()));
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "?", FontId::proportional(13.0), theme::TEXT_MID());
            resp.on_hover_text(
                tr("Push sends the commits on the left to the branch on the server.\nClick the branch name after the remote to push to another branch.\nCtrl+Enter pushes; the arrow next to Push has Force Push."),
            );
            ui.add_space(14.0);
            ui.checkbox(&mut f.tags, tr("Push tags:"));
            ui.add_enabled_ui(f.tags, |ui| {
                egui::ComboBox::from_id_salt("push_tags")
                    .width(130.0)
                    .selected_text(if f.tags_all { "All" } else { "Current Branch" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.tags_all, true, tr("All")).on_hover_text(tr("git push --tags"));
                        ui.selectable_value(&mut f.tags_all, false, tr("Current Branch")).on_hover_text(tr("git push --follow-tags: tags of the pushed commits"));
                    });
            });
            if let Some(word) = f.request_word {
                ui.add_space(10.0);
                ui.checkbox(&mut f.request, trf("Create {}", &[&tr(word)])).on_hover_text(tr("After the push, open the window that creates the request for this branch"));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_rows() {
        let f = |p: &str| ChangedFile { status: 'M', path: p.into(), old_path: None };
        let files = vec![f("src/b.py"), f("README.md"), f("src/a.py")];
        let rows = file_rows(&files, false, &HashSet::new());
        let names: Vec<String> = rows
            .iter()
            .map(|r| match r {
                FRow::Folder { dir, count, .. } => format!("{dir}/ {count}"),
                FRow::File { file, .. } => file.path.clone(),
            })
            .collect();
        assert_eq!(names, vec!["src/ 2", "src/a.py", "src/b.py", "README.md"]);
    }
}
