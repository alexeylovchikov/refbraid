//! Right side of the Log tab: files changed in the selected commit and its details.

use crate::app::{App, Dialog, Job, file_name, format_full_date, short, status_color};
use crate::i18n::tr;
use crate::git;
use crate::theme::{self, Icon};
use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, text::LayoutJob, vec2};
use std::collections::BTreeMap;
use std::sync::RwLock;

/// Issue patterns of the open project, compiled (File → Settings → Issue Navigation).
static ISSUE_RULES: RwLock<Vec<(regex::Regex, String)>> = RwLock::new(Vec::new());

/// Use these issue patterns for highlighting; patterns that don't compile are skipped.
pub fn set_issue_rules(rules: &[crate::settings::IssueRule]) {
    let compiled = rules
        .iter()
        .filter(|r| !r.pattern.trim().is_empty())
        .filter_map(|r| regex::Regex::new(&r.pattern).ok().map(|re| (re, r.link.trim().to_string())))
        .collect();
    if let Ok(mut g) = ISSUE_RULES.write() {
        *g = compiled;
    }
}

/// Issue references in `text`: byte range and the web address to open (empty when none is set).
pub fn issue_links(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let Ok(rules) = ISSUE_RULES.read() else { return vec![] };
    links_with(&rules, text)
}

pub fn links_with(rules: &[(regex::Regex, String)], text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut all = vec![];
    for (re, link) in rules.iter() {
        for caps in re.captures_iter(text) {
            let m = caps.get(0).unwrap();
            if m.is_empty() {
                continue;
            }
            let mut url = String::new();
            if !link.is_empty() {
                caps.expand(link, &mut url);
            }
            all.push((m.range(), url));
        }
    }
    all.sort_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
    let mut out: Vec<(std::ops::Range<usize>, String)> = vec![];
    for (r, u) in all {
        if out.last().map(|(l, _)| r.start >= l.end).unwrap_or(true) {
            out.push((r, u));
        }
    }
    out
}

/// The issue link under a point of a galley drawn from `text` with [`linkified`].
pub fn link_at(text: &str, galley: &egui::Galley, offset: egui::Vec2) -> Option<String> {
    let ci = galley.cursor_from_pos(offset).index.0;
    let byte = text.char_indices().nth(ci).map(|(b, _)| b)?;
    issue_links(text)
        .into_iter()
        .find(|(r, u)| !u.is_empty() && r.contains(&byte))
        .map(|(_, u)| u)
}

/// Text with issue keys (ABC-123) drawn as underlined links, as the IDE does.
pub fn linkified(text: &str, size: f32, color: Color32, max_w: Option<f32>) -> LayoutJob {
    let mut job = LayoutJob::default();
    let normal = egui::TextFormat {
        font_id: FontId::proportional(size),
        color,
        ..Default::default()
    };
    let link = egui::TextFormat {
        font_id: FontId::proportional(size),
        color: theme::LINK(),
        underline: Stroke::new(1.0, theme::LINK()),
        ..Default::default()
    };
    let mut last = 0;
    for (r, _) in issue_links(text) {
        if r.start > last {
            job.append(&text[last..r.start], 0.0, normal.clone());
        }
        job.append(&text[r.clone()], 0.0, link.clone());
        last = r.end;
    }
    if last < text.len() {
        job.append(&text[last..], 0.0, normal);
    }
    if let Some(w) = max_w {
        job.wrap = egui::text::TextWrapping {
            max_width: w.max(1.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
    }
    job
}

/// A rounded search field with a magnifier, a clear button inside its right end while it has
/// text, and, optionally, regex / match-case toggles. Returns true when a toggle changed.
pub fn search_box(ui: &mut Ui, text: &mut String, hint: &str, width: f32, toggles: Option<(&mut bool, &mut bool)>) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
    let edit_id = egui::Id::unique(format!("search:{hint}"));
    let focused = ui.memory(|m| m.has_focus(edit_id));
    ui.painter().rect(
        rect,
        5.0,
        theme::BG_INPUT(),
        Stroke::new(1.0, if focused { theme::ACCENT() } else { theme::INPUT_BORDER() }),
        egui::StrokeKind::Inside,
    );
    theme::paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(14.0, 14.0)),
        Icon::Search,
        theme::TEXT_MID(),
    );
    let tw = if toggles.is_some() { 52.0 } else { 0.0 };
    // The clear button takes its room only while there is something to clear.
    let cw = if text.is_empty() { 0.0 } else { 22.0 };
    let edit_rect = Rect::from_min_max(
        pos2(rect.left() + 26.0, rect.top() + 4.0),
        pos2(rect.right() - tw - cw - 4.0, rect.bottom() - 4.0),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect));
    let er = child.add(
        egui::TextEdit::singleline(text)
            .id(edit_id)
            .hint_text(RichText::new(crate::i18n::tr(hint)).color(theme::TEXT_DIM()))
            .frame(egui::Frame::NONE)
            .text_color(theme::TEXT())
            .desired_width(edit_rect.width()),
    );
    theme::edit_menu(er);
    if !text.is_empty() {
        let r = Rect::from_center_size(pos2(rect.right() - tw - 14.0, rect.center().y), vec2(18.0, 18.0));
        let resp = ui.interact(r, edit_id.with("clear"), Sense::click()).on_hover_text(crate::i18n::tr("Clear"));
        if resp.hovered() {
            ui.painter().rect_filled(r, 9.0, theme::HOVER());
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        theme::paint_icon(ui.painter(), r.shrink(4.0), Icon::Close, if resp.hovered() { theme::TEXT() } else { theme::TEXT_MID() });
        if resp.clicked() {
            text.clear();
            ui.memory_mut(|m| m.request_focus(edit_id));
        }
    }
    let mut changed = false;
    if let Some((re, case)) = toggles {
        for (i, (label, val, tip)) in [(".*", re, "Regex"), ("Cc", case, "Match case")].into_iter().enumerate() {
            let r = Rect::from_center_size(pos2(rect.right() - 38.0 + i as f32 * 24.0, rect.center().y), vec2(22.0, 20.0));
            let resp = ui.interact(r, egui::Id::unique(("toggle", hint, i)), Sense::click()).on_hover_text(tip);
            if *val {
                ui.painter().rect_filled(r, 4.0, theme::TAB_SEL());
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 4.0, theme::HOVER());
            }
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(12.0),
                if *val { theme::TEXT() } else { theme::TEXT_MID() },
            );
            if resp.clicked() {
                *val = !*val;
                changed = true;
            }
        }
    }
    changed
}

fn revert_args(c: &git::Commit) -> Vec<String> {
    let mut args = vec!["revert".to_string(), "--no-edit".to_string()];
    if c.parents.len() > 1 {
        args.push("-m".into());
        args.push("1".into());
    }
    args.push(c.hash.clone());
    args
}

impl App {
    pub fn details_panel(&mut self, ui: &mut Ui) {
        let total = ui.max_rect();
        ui.expand_to_include_rect(total);
        theme::vline(ui, total.left() + 0.5, total.top(), total.bottom());
        // Split between changed files (top) and commit details (bottom); the height is remembered.
        let max_h = (total.height() - 90.0).max(80.0);
        let auto_h = (total.height() * 0.42).max(160.0);
        let h = if self.settings.info_height > 0.0 { self.settings.info_height } else { auto_h }.clamp(80.0, max_h);
        let split = total.bottom() - h;
        let handle = Rect::from_min_max(pos2(total.left(), split - 3.0), pos2(total.right(), split + 3.0));
        let resp = ui.interact(handle, egui::Id::unique("info_split"), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.settings.info_height = (total.bottom() - p.y).clamp(80.0, max_h);
            }
        }
        if resp.drag_stopped() && self.screenshot.is_none() {
            self.settings.save();
        }
        let top = Rect::from_min_max(total.min, pos2(total.right(), split));
        let bottom = Rect::from_min_max(pos2(total.left(), split), total.max);
        let mut tu = ui.new_child(egui::UiBuilder::new().max_rect(top));
        tu.set_clip_rect(top);
        self.changes_tree(&mut tu);
        let mut bu = ui.new_child(egui::UiBuilder::new().max_rect(bottom));
        bu.set_clip_rect(bottom);
        self.commit_info(&mut bu);
        if resp.hovered() || resp.dragged() {
            ui.painter().hline(total.x_range(), split, Stroke::new(2.0, theme::ACCENT()));
        }
    }

    pub fn confirm_revert(&mut self, c: &git::Commit) {
        self.dialog = Some(Dialog::Confirm {
            title: tr("Revert Commit").into(),
            text: format!("Create a new commit that undoes \"{}\"?", c.subject),
            ok: "Revert".into(),
            job: Some(Job::many(&format!("Revert {}", short(&c.hash)), vec![revert_args(c)])),
        });
    }

    /// The lists shown in the changes tree. A merge commit gets the files it changed against
    /// every parent, then "Changes to <parent>" for each parent (like the IDE).
    fn detail_sections(&self) -> Vec<Section> {
        let Some(repo) = self.repo.as_ref() else { return vec![] };
        if repo.multi.len() > 1 {
            let files: Vec<git::ChangedFile> = repo.multi_files.iter().map(|m| m.file.clone()).collect();
            let n = files.len();
            return vec![Section {
                part: 0,
                title: repo.name.clone(),
                count: format!("{n} {}, {} {}", crate::i18n::plural(n, "file", "files"), repo.multi.len(), crate::i18n::plural(repo.multi.len(), "commit", "commits")),
                is_header: false,
                files,
            }];
        }
        let Some(d) = repo.details.as_ref() else { return vec![] };
        if !d.is_merge() {
            let n = d.files.len();
            return vec![Section { part: 0, title: repo.name.clone(), count: format!("{n} {}", crate::i18n::plural(n, "file", "files")), is_header: false, files: d.files.clone() }];
        }
        let mut out = vec![];
        if !d.merge_files.is_empty() {
            let n = d.merge_files.len();
            out.push(Section { part: 0, title: repo.name.clone(), count: format!("{n} {}", crate::i18n::plural(n, "file", "files")), is_header: false, files: d.merge_files.clone() });
        }
        for (k, files) in d.parent_files.iter().enumerate() {
            let n = files.len();
            let subject = d.parent_subjects.get(k).cloned().unwrap_or_default();
            out.push(Section {
                part: k + 1,
                title: format!("Changes to {} {subject}", short(&d.parents[k])),
                count: format!("{n} {}", crate::i18n::plural(n, "file", "files")),
                is_header: true,
                files: files.clone(),
            });
        }
        out
    }

    fn changes_tree(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 34.0));
        let has_sel = self.repo.as_ref().map(|r| r.details.is_some()).unwrap_or(false);
        let file_sel = self.repo.as_ref().and_then(|r| r.details_file.map(|f| (r.details_part, f)));
        let multi = self.repo.as_ref().map(|r| r.multi.len() > 1).unwrap_or(false);
        let sections = self.detail_sections();
        let mut tb = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(8.0, 4.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        if theme::icon_button_sized(&mut tb, Icon::Diff, tr("Show Diff (double-click a file)"), 24.0, false, file_sel.is_some()).clicked() {
            if let Some((part, f)) = file_sel {
                if multi {
                    self.open_multi_file_diff(f);
                } else {
                    self.open_commit_part_diff(part, f);
                }
            }
        }
        if theme::icon_button_sized(&mut tb, Icon::Undo, tr("Revert selected commit"), 24.0, false, has_sel).clicked() {
            if let Some(c) = self.repo.as_ref().and_then(|r| r.selected_commit().cloned()) {
                self.confirm_revert(&c);
            }
        }
        if theme::icon_button_sized(&mut tb, Icon::Branch, tr("New branch from this commit"), 24.0, false, has_sel).clicked() {
            if let Some(h) = self.repo.as_ref().and_then(|r| r.selected_commit().map(|c| c.hash.clone())) {
                self.open_new_branch(&h);
            }
        }
        tb.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Expand everything; when everything is already open, collapse the folders.
            let all_open = self.details_collapsed.is_empty();
            let (icon, tip) = if all_open { (Icon::Collapse, "Collapse All") } else { (Icon::Expand, "Expand All") };
            if theme::icon_button(ui, icon, tip).clicked() {
                if all_open {
                    let keys: Vec<String> = tree_rows(&sections, &Default::default())
                        .into_iter()
                        .filter_map(|r| match r {
                            DRow::Folder { key, .. } => Some(key),
                            _ => None,
                        })
                        .collect();
                    self.details_collapsed.extend(keys);
                } else {
                    self.details_collapsed.clear();
                }
            }
        });

        let body = Rect::from_min_max(pos2(rect.left(), bar.bottom()), rect.max);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        let Some(repo) = self.repo.as_ref() else { return };
        let root = repo.path.clone();
        if repo.details.is_none() && !multi {
            ui.painter().text(
                body.center(),
                egui::Align2::CENTER_CENTER,
                tr("Select commit to view changes"),
                FontId::proportional(13.0),
                theme::TEXT_MID(),
            );
            return;
        };
        let sel = repo.details_file.map(|f| (repo.details_part, f));
        let rows = tree_rows(&sections, &self.details_collapsed);
        let mut open_diff: Option<(usize, usize)> = None;
        let mut select: Option<(usize, usize)> = None;
        let mut fold: Option<String> = None;
        let mut open_file: Option<std::path::PathBuf> = None;
        let mut history: Option<String> = None;
        let mut annotate: Option<String> = None;
        let font = FontId::proportional(13.0);
        let small = FontId::proportional(12.0);
        egui::ScrollArea::both().id_salt("changes_tree").auto_shrink([false, false]).show(&mut ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let w = ui.available_width().max(200.0);
            for row in &rows {
                let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
                let cy = r.center().y;
                match row {
                    DRow::Section { key, title, count, open, is_header } | DRow::Folder { key, label: title, count, open, depth: _, is_header } => {
                        let depth = if let DRow::Folder { depth, .. } = row { *depth } else { 0 };
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 0.0, theme::HOVER());
                        }
                        if resp.clicked() {
                            fold = Some(key.clone());
                        }
                        let x = r.left() + 14.0 + 18.0 * depth as f32;
                        let p = ui.painter();
                        let chev = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
                        theme::paint_icon(p, Rect::from_center_size(pos2(x, cy), vec2(12.0, 12.0)), chev, theme::TEXT_MID());
                        let tx = if *is_header {
                            x + 12.0
                        } else {
                            theme::paint_icon(p, Rect::from_center_size(pos2(x + 16.0, cy), vec2(15.0, 15.0)), Icon::Folder, theme::ICON());
                            x + 28.0
                        };
                        p.text(pos2(tx, cy), egui::Align2::LEFT_CENTER, title, font.clone(), theme::TEXT());
                        let tw = theme::text_width(ui, title, &font);
                        ui.painter().text(pos2(tx + tw + 8.0, cy), egui::Align2::LEFT_CENTER, count, small.clone(), theme::TEXT_DIM());
                    }
                    DRow::File { part, idx, depth, file } => {
                        let p = ui.painter();
                        if sel == Some((*part, *idx)) {
                            p.rect_filled(r, 0.0, theme::SELECT());
                        } else if resp.hovered() {
                            p.rect_filled(r, 0.0, theme::HOVER());
                        }
                        let x = r.left() + 14.0 + 18.0 * *depth as f32;
                        theme::paint_icon(p, Rect::from_center_size(pos2(x + 16.0, cy), vec2(14.0, 14.0)), Icon::File, theme::TEXT_MID());
                        let mut name = file_name(&file.path);
                        if let Some(o) = &file.old_path {
                            name = format!("{name}  ← {o}");
                        }
                        p.text(pos2(x + 28.0, cy), egui::Align2::LEFT_CENTER, &name, font.clone(), status_color(file.status));
                        if resp.double_clicked() {
                            open_diff = Some((*part, *idx));
                        } else if resp.clicked() {
                            select = Some((*part, *idx));
                        }
                        let path = file.path.clone();
                        let at = (*part, *idx);
                        resp.on_hover_text(&file.path).context_menu(|ui| {
                            if ui.button(tr("Show Diff")).clicked() {
                                open_diff = Some(at);
                            }
                            if ui.button(tr("Copy Path")).clicked() {
                                ui.ctx().copy_text(path.clone());
                            }
                            // The file as it is now in the working folder.
                            let local = crate::chrome::repo_file(&root, &path);
                            if ui.add_enabled(local.is_file(), egui::Button::new(tr("Open in Default Application"))).on_disabled_hover_text(tr("The file is not in the working folder")).clicked() {
                                open_file = Some(local.clone());
                            }
                            if ui.button(tr("Show History")).on_hover_text(tr("The commits that changed this file, followed through its renames")).clicked() {
                                history = Some(path.clone());
                            }
                            crate::dialogs::compare_file_items(ui, &path, local.is_file());
                            if ui.button(tr("Annotate")).on_hover_text(tr("Who last changed each line of the file, and in which commit")).clicked() {
                                annotate = Some(path.clone());
                            }
                        });
                    }
                }
            }
        });
        if let Some(f) = open_file {
            self.open_file_at(&f, None);
        }
        if let Some(p) = history {
            let rev = self.repo.as_ref().and_then(|r| r.selected_commit()).map(|c| c.hash.clone());
            self.open_file_history(&p, rev.as_deref());
        }
        if let Some(p) = annotate {
            let rev = self.repo.as_ref().and_then(|r| r.selected_commit()).map(|c| c.hash.clone());
            self.open_blame(&p, rev.as_deref(), None);
        }
        if let Some(k) = fold {
            if !self.details_collapsed.remove(&k) {
                self.details_collapsed.insert(k);
            }
        }
        if let Some((part, i)) = select.or(open_diff) {
            if let Some(r) = self.repo.as_mut() {
                r.details_part = part;
                r.details_file = Some(i);
            }
        }
        if let Some((part, i)) = open_diff {
            if multi {
                if !self.try_external_multi_diff(i) {
                    self.open_multi_file_diff(i);
                }
            } else {
                self.open_commit_part_diff_as(part, i, crate::app::OpenDiff::DoubleClick);
            }
        } else if let (Some((part, i)), false) = (select, multi) {
            // A click on another file shows it in the quick-view diff tab, if one is open.
            self.open_commit_part_diff_as(part, i, crate::app::OpenDiff::Follow);
        }
    }

    pub fn commit_info(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        theme::hline(ui, rect.top() + 0.5, rect.left(), rect.right());
        let Some(repo) = self.repo.as_ref() else { return };
        if repo.multi.len() > 1 {
            let list = repo.multi_commits.clone();
            let mut nav: Option<String> = None;
            egui::ScrollArea::vertical().id_salt("commit_info_multi").auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (k, d) in list.iter().enumerate() {
                        if k > 0 {
                            ui.add_space(6.0);
                            ui.separator();
                            ui.add_space(4.0);
                        }
                        message_with_links(ui, &d.message);
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            let r = ui.add(egui::Label::new(RichText::new(short(&d.hash)).color(theme::LINK())).sense(Sense::click()));
                            if r.on_hover_text(tr("Go to this commit")).clicked() {
                                nav = Some(d.hash.clone());
                            }
                            ui.label(RichText::new(format!("{} <{}>", d.author, d.email)).color(theme::TEXT_MID()));
                            ui.label(RichText::new(format!("on {}", format_full_date(d.author_time))).color(theme::TEXT_MID()));
                        });
                    }
                });
            });
            if let Some(h) = nav {
                self.navigate_to(&h);
            }
            return;
        }
        let Some(d) = repo.details.clone() else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr("Commit details"),
                FontId::proportional(13.0),
                theme::TEXT_MID(),
            );
            return;
        };
        // JIRA status of the issues the message and the branches name.
        let jira = crate::jira::Config::from(&self.settings).map(|cfg| {
            let mut keys = crate::jira::commit_keys(&d.message, &d.containing);
            keys.truncate(8);
            let states = self.jira.states(&cfg, &keys, ui.ctx());
            (cfg, states)
        });
        let mut nav: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("commit_info").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                message_with_links(ui, &d.message);
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    let r = ui.add(egui::Label::new(RichText::new(short(&d.hash)).color(theme::LINK())).sense(Sense::click()));
                    if r.on_hover_text(tr("Click to copy the full hash")).clicked() {
                        ui.ctx().copy_text(d.hash.clone());
                    }
                    ui.label(RichText::new(format!("{} <{}>", d.author, d.email)).color(theme::TEXT_MID()));
                    ui.label(RichText::new(format!("on {}", format_full_date(d.author_time))).color(theme::TEXT_MID()));
                });
                if d.committer != d.author || d.commit_time != d.author_time {
                    ui.label(
                        RichText::new(crate::i18n::trf("committed by {} on {}", &[&d.committer, &format_full_date(d.commit_time)]))
                            .color(theme::TEXT_DIM())
                            .size(12.0),
                    );
                }
                if !d.parents.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(tr(if d.parents.len() > 1 { "Parents:" } else { "Parent:" }))
                                .color(theme::TEXT_DIM())
                                .size(12.0),
                        );
                        for p in &d.parents {
                            let l = egui::Label::new(RichText::new(short(p)).color(theme::LINK()).size(12.0)).sense(Sense::click());
                            if ui.add(l).clicked() {
                                nav = Some(p.clone());
                            }
                        }
                    });
                }
                if let Some((state, signer)) = &d.signature {
                    let (words, good) = crate::git::signature_words(*state);
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        let color = if good { theme::GREEN() } else if *state == 'B' { theme::RED() } else { theme::STAR() };
                        ui.label(RichText::new(tr(words)).color(color).size(12.0));
                        if !signer.is_empty() {
                            ui.label(RichText::new(signer.as_str()).color(theme::TEXT_DIM()).size(12.0));
                        }
                    });
                }
                for (name, tagger, time, msg) in &d.tags {
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{} {name}", tr("Tag"))).color(theme::STAR()).size(12.0));
                        if !tagger.is_empty() {
                            ui.label(RichText::new(format!("{tagger}, {}", format_full_date(*time))).color(theme::TEXT_DIM()).size(12.0));
                        }
                    });
                    if !msg.is_empty() {
                        ui.label(RichText::new(msg.as_str()).color(theme::TEXT_MID()).size(12.5));
                    }
                }
                ui.add_space(6.0);
                if d.refs_loading && d.containing.is_empty() {
                    ui.label(RichText::new(tr("Looking for the branches with this commit…")).color(theme::TEXT_DIM()).size(12.0));
                }
                if !d.containing.is_empty() {
                    // Root branches (main, master, stage…) first and in their own colour; the
                    // others after them, cut to a dozen.
                    let n = d.containing.len();
                    let remotes = &repo.state.remotes;
                    let (roots, others): (Vec<&String>, Vec<&String>) =
                        d.containing.iter().partition(|b| crate::chrome::is_root_ref(b, remotes));
                    let room = 12usize.saturating_sub(roots.len()).max(4);
                    let more = others.len().saturating_sub(room);
                    let small = |c: Color32| egui::TextFormat { font_id: egui::FontId::proportional(12.0), color: c, ..Default::default() };
                    let mut job = egui::text::LayoutJob::default();
                    job.append(&format!("{} {n} {}: ", tr("In"), crate::i18n::plural(n, "branch", "branches")), 0.0, small(theme::TEXT_MID()));
                    let list: Vec<(&String, bool)> = roots.iter().map(|b| (*b, true)).chain(others.iter().take(room).map(|b| (*b, false))).collect();
                    for (i, (b, root)) in list.iter().enumerate() {
                        if i > 0 {
                            job.append(", ", 0.0, small(theme::TEXT_MID()));
                        }
                        // master / main (also origin/master…) in green, other root branches in gold.
                        let trunk = matches!(b.rsplit('/').next(), Some("master" | "main"));
                        let col = match (*root, trunk) {
                            (true, true) => theme::GREEN(),
                            (true, false) => theme::STAR(),
                            _ => theme::TEXT_MID(),
                        };
                        job.append(b, 0.0, small(col));
                    }
                    if more > 0 {
                        job.append(&format!(" and {more} more"), 0.0, small(theme::TEXT_MID()));
                    }
                    ui.add(egui::Label::new(job).wrap()).on_hover_text(tr("Branches without \"/\" and digits in the name are highlighted"));
                }
                if let Some((cfg, states)) = &jira {
                    jira_rows(ui, cfg, states);
                }
            });
        });
        if let Some(h) = nav {
            self.navigate_to(&h);
        }
    }
}

/// One line per JIRA issue: its key (a link), the status as a coloured badge and the summary.
fn jira_rows(ui: &mut Ui, cfg: &crate::jira::Config, states: &[(String, crate::jira::State)]) {
    use crate::jira::State;
    if states.is_empty() {
        return;
    }
    ui.add_space(8.0);
    for (key, st) in states {
        let issue = match st {
            State::Found(i) => Some(i),
            State::Loading => None,
            State::Missing | State::Failed(_) => continue,
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let url = cfg.browse(key);
            ui.add(egui::Hyperlink::from_label_and_url(RichText::new(key).size(12.0).color(theme::LINK()), &url)).on_hover_text(format!("Open in JIRA: {url}"));
            match issue {
                Some(i) => {
                    let mut tip = format!("Status: {}", i.status);
                    if !i.kind.is_empty() {
                        tip.push_str(&format!("\nType: {}", i.kind));
                    }
                    tip.push_str(&format!("\nAssignee: {}", if i.assignee.is_empty() { "Unassigned" } else { &i.assignee }));
                    lozenge(ui, if i.status.is_empty() { "?" } else { &i.status }, &i.category).on_hover_text(tip);
                    if !i.summary.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&i.summary).size(12.0).color(theme::TEXT_MID())).truncate());
                    }
                }
                None => {
                    ui.spinner();
                    ui.label(RichText::new(tr("reading JIRA…")).size(12.0).color(theme::TEXT_DIM()));
                }
            }
        });
    }
    // A wrong address or token fails every issue alike: say it once.
    if let Some(e) = states.iter().find_map(|(_, s)| if let State::Failed(e) = s { Some(e) } else { None }) {
        ui.add(egui::Label::new(RichText::new(format!("JIRA: {e}")).size(12.0).color(theme::RED())).wrap())
            .on_hover_text(tr("Check the address and the token in File → Settings → JIRA"));
    }
}

/// A JIRA-style status badge, coloured by the status category (to do, in progress, done).
fn lozenge(ui: &mut Ui, text: &str, category: &str) -> egui::Response {
    let (bg, fg) = match category {
        "done" => (theme::rgb(0x1c, 0x45, 0x2a), theme::rgb(0x7e, 0xd3, 0x87)),
        "indeterminate" => (theme::rgb(0x1d, 0x3a, 0x6b), theme::rgb(0x8c, 0xb4, 0xff)),
        _ => (theme::rgb(0x3a, 0x3d, 0x44), theme::rgb(0xc4, 0xc7, 0xce)),
    };
    let g = ui.painter().layout_no_wrap(text.to_uppercase(), FontId::proportional(10.5), fg);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 12.0, 17.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, bg);
    ui.painter().galley(pos2(rect.left() + 6.0, rect.center().y - g.size().y / 2.0), g, fg);
    resp
}

/// A commit message with its issue references as links that open in the browser.
pub fn message_with_links(ui: &mut Ui, message: &str) {
    for line in message.trim_end().lines() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if line.is_empty() {
                ui.label(" ");
                return;
            }
            let mut last = 0;
            let plain = |ui: &mut Ui, t: &str| {
                ui.add(egui::Label::new(RichText::new(t).size(13.0).color(theme::TEXT())).wrap());
            };
            for (r, url) in issue_links(line) {
                if r.start > last {
                    plain(ui, &line[last..r.start]);
                }
                let text = RichText::new(&line[r.clone()]).size(13.0).color(theme::LINK()).underline();
                if url.is_empty() {
                    ui.label(text);
                } else {
                    ui.add(egui::Hyperlink::from_label_and_url(text, &url)).on_hover_text(&url);
                }
                last = r.end;
            }
            if last < line.len() {
                plain(ui, &line[last..]);
            }
        });
    }
}

/// One list of the changes tree (see `App::detail_sections`).
pub struct Section {
    /// 0 = the commit's own files, k = "Changes to" parent k.
    part: usize,
    title: String,
    count: String,
    /// A "Changes to …" line (no folder icon) rather than the repository row.
    is_header: bool,
    files: Vec<git::ChangedFile>,
}

enum DRow {
    Section { key: String, title: String, count: String, open: bool, is_header: bool },
    Folder { key: String, label: String, count: String, open: bool, depth: usize, is_header: bool },
    File { part: usize, idx: usize, depth: usize, file: git::ChangedFile },
}

#[derive(Default)]
struct TreeDir {
    dirs: BTreeMap<String, TreeDir>,
    files: Vec<usize>,
}

impl TreeDir {
    fn count(&self) -> usize {
        self.files.len() + self.dirs.values().map(|d| d.count()).sum::<usize>()
    }
}

/// Rows of the changes tree: each section, its folders nested (single-folder chains joined,
/// "templates/referral") and files, skipping what is collapsed.
fn tree_rows(sections: &[Section], collapsed: &std::collections::HashSet<String>) -> Vec<DRow> {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let mut rows = vec![];
    for s in sections {
        let key = format!("{}:", s.part);
        let open = !collapsed.contains(&key);
        rows.push(DRow::Section { key, title: s.title.clone(), count: s.count.clone(), open, is_header: s.is_header });
        if !open {
            continue;
        }
        let mut root = TreeDir::default();
        for (i, f) in s.files.iter().enumerate() {
            let mut d = &mut root;
            if let Some((dir, _)) = f.path.rsplit_once('/') {
                for part in dir.split('/') {
                    d = d.dirs.entry(part.to_string()).or_default();
                }
            }
            d.files.push(i);
        }
        fn walk(d: &TreeDir, prefix: &str, depth: usize, s: &Section, collapsed: &std::collections::HashSet<String>, rows: &mut Vec<DRow>, plural: &dyn Fn(usize) -> &'static str) {
            for (name, sub) in &d.dirs {
                let mut label = name.clone();
                let mut full = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
                let mut cur = sub;
                while cur.files.is_empty() && cur.dirs.len() == 1 {
                    let (n, next) = cur.dirs.iter().next().unwrap();
                    label = format!("{label}/{n}");
                    full = format!("{full}/{n}");
                    cur = next;
                }
                let key = format!("{}:{full}", s.part);
                let open = !collapsed.contains(&key);
                let n = cur.count();
                rows.push(DRow::Folder { key, label, count: format!("{n} {}", crate::i18n::plural(n, "file", "files")), open, depth, is_header: false });
                if open {
                    walk(cur, &full, depth + 1, s, collapsed, rows, plural);
                }
            }
            let mut files = d.files.clone();
            files.sort_by_key(|&i| file_name(&s.files[i].path).to_lowercase());
            for i in files {
                rows.push(DRow::File { part: s.part, idx: i, depth, file: s.files[i].clone() });
            }
        }
        walk(&root, "", 1, s, collapsed, &mut rows, &plural);
    }
    rows
}
