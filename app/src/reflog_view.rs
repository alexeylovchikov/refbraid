//! The Reflog tab: the records of HEAD's moves at the left, the selected record's commit at
//! the right. Commits no branch reaches are marked, and can be given a branch again.

use crate::app::{format_date, short, App, Dialog, Job, ResetMode, Tab};
use crate::i18n::tr;
use crate::reflog::{self, View};
use crate::theme;
use eframe::egui::{self, pos2, vec2, FontId, Rect, RichText, Sense, Ui};

const ROW_H: f32 = 24.0;
const LIMIT: usize = 1000;

enum Ask {
    ShowInLog(String),
    Checkout(String),
    NewBranch(String),
    Reset(String, String),
}

impl App {
    /// Git → Reflog: open the tab (or come back to it).
    pub fn open_reflog(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        if self.reflog.is_none() {
            self.reflog = Some(View { entries: reflog::load(&repo.path, LIMIT), selected: 0, only_lost: false, details: None, fingerprint: repo.fingerprint });
        }
        self.tab = Tab::Reflog;
    }

    pub fn close_reflog(&mut self) {
        self.reflog = None;
        if self.tab == Tab::Reflog {
            self.tab = Tab::Log;
        }
    }

    pub fn reflog_tab(&mut self, ui: &mut Ui) {
        let (Some(repo), Some(view)) = (self.repo.as_ref(), self.reflog.as_mut()) else {
            self.tab = Tab::Log;
            return;
        };
        // The repository changed (a commit, a reset, a checkout…): read the records again.
        if view.fingerprint != repo.fingerprint {
            let keep = view.entries.get(view.selected).map(|e| (e.hash.clone(), e.time));
            view.entries = reflog::load(&repo.path, LIMIT);
            view.fingerprint = repo.fingerprint;
            view.selected = keep.and_then(|k| view.entries.iter().position(|e| (e.hash.clone(), e.time) == k)).unwrap_or(0);
            view.details = None;
        }
        let shown: Vec<usize> = (0..view.entries.len()).filter(|&i| !view.only_lost || view.entries[i].lost).collect();
        if !shown.contains(&view.selected) {
            view.selected = shown.first().copied().unwrap_or(0);
            view.details = None;
        }
        let want = view.entries.get(view.selected).filter(|_| !shown.is_empty()).map(|e| e.hash.clone());
        if view.details.as_ref().map(|d| &d.hash) != want.as_ref() {
            view.details = want.as_ref().and_then(|h| crate::app::details_of(&repo.path, h, None));
        }
        let head_name = repo.state.head_branch.clone().unwrap_or_else(|| "HEAD".into());
        let busy = repo.state.operation.is_some();
        let lost_total = view.entries.iter().filter(|e| e.lost).count();

        // Right: the commit of the selected record.
        let avail = ui.available_width();
        egui::Panel::right("reflog_details")
            .default_size((avail * 0.34).clamp(320.0, 560.0))
            .min_size(260.0)
            .max_size((avail - 360.0).max(260.0))
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| self.reflog_details(ui));

        let Some(view) = self.reflog.as_mut() else { return };
        let mut ask: Option<Ask> = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG())).show(ui, |ui| {
            // Header.
            let bar = Rect::from_min_size(ui.max_rect().min, vec2(ui.max_rect().width(), 34.0));
            theme::hline(ui, bar.bottom() - 0.5, bar.left(), bar.right());
            let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(12.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
            bui.label(RichText::new(tr("Where HEAD has been, newest first")).color(theme::TEXT_MID()));
            bui.add_space(12.0);
            bui.add_enabled(lost_total > 0, egui::Checkbox::new(&mut view.only_lost, format!("{} ({lost_total})", tr("Only lost commits"))))
                .on_hover_text(tr("Commits that no branch or tag reaches any more — after a reset, a rebase, an amended commit or a deleted branch. Only the reflog still knows them."));
            ui.advance_cursor_after_rect(bar);

            let area = ui.available_rect_before_wrap();
            if shown.is_empty() {
                let text = if view.entries.is_empty() { tr("The reflog is empty") } else { tr("No lost commits") };
                ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(13.0), theme::TEXT_DIM());
                return;
            }
            // Arrow keys move the selection.
            let typing = ui.ctx().memory(|m| m.focused().is_some());
            let mut scroll = false;
            if !typing && self.dialog.is_none() {
                let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
                let pos = shown.iter().position(|&i| i == view.selected).unwrap_or(0);
                let new = if up { pos.saturating_sub(1) } else if down { (pos + 1).min(shown.len() - 1) } else { pos };
                if new != pos {
                    view.selected = shown[new];
                    scroll = true;
                }
            }
            let font = FontId::proportional(13.0);
            let small = FontId::proportional(12.0);
            let mono = FontId::monospace(12.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().id_salt("reflog_rows").auto_shrink([false, false]).show_rows(ui, ROW_H, shown.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for k in range {
                    let i = shown[k];
                    let e = &view.entries[i];
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                    let selected = i == view.selected;
                    if selected && scroll {
                        ui.scroll_to_rect(r, None);
                    }
                    let p = ui.painter_at(r);
                    if selected {
                        p.rect_filled(r, 0.0, theme::SELECT());
                    } else if resp.hovered() {
                        p.rect_filled(r, 0.0, theme::HOVER());
                    }
                    let cy = r.center().y;
                    p.text(pos2(r.left() + 12.0, cy), egui::Align2::LEFT_CENTER, format_date(e.time), small.clone(), theme::TEXT_MID());
                    p.text(pos2(r.left() + 150.0, cy), egui::Align2::LEFT_CENTER, short(&e.hash), mono.clone(), theme::LINK());
                    let mut x = r.left() + 232.0;
                    if e.lost {
                        let g = p.layout_no_wrap(tr("lost").to_string(), FontId::proportional(11.0), theme::STAR());
                        let br = Rect::from_min_size(pos2(x, cy - 8.0), vec2(g.size().x + 10.0, 16.0));
                        p.rect_stroke(br, 3.0, egui::Stroke::new(1.0, theme::STAR()), egui::StrokeKind::Inside);
                        p.galley(pos2(br.left() + 5.0, cy - g.size().y / 2.0), g, theme::STAR());
                        x = br.right() + 8.0;
                    }
                    // What happened, then the commit HEAD came to.
                    let w = theme::paint_clipped(&p, pos2(x, cy), &e.action, font.clone(), action_color(&e.action), 170.0);
                    x += w + 10.0;
                    // "commit: <subject>" would say the subject twice.
                    let what = if e.detail.is_empty() || e.detail == e.subject { e.subject.clone() } else { format!("{}  ·  {}", e.detail, e.subject) };
                    theme::paint_clipped(&p, pos2(x, cy), &what, font.clone(), theme::TEXT(), r.right() - 10.0 - x);

                    if resp.clicked() || resp.secondary_clicked() {
                        view.selected = i;
                    }
                    if resp.double_clicked() && !e.lost {
                        ask = Some(Ask::ShowInLog(e.hash.clone()));
                    }
                    let (hash, subject, lost) = (e.hash.clone(), e.subject.clone(), e.lost);
                    resp.context_menu(|ui| {
                        if ui.button(tr("New Branch…")).on_hover_text(tr("A branch on this commit keeps it: this is how a lost commit is brought back")).clicked() {
                            ask = Some(Ask::NewBranch(hash.clone()));
                        }
                        if ui.add_enabled(!busy, egui::Button::new(format!("Reset Current Branch ('{head_name}') to Here…"))).clicked() {
                            ask = Some(Ask::Reset(hash.clone(), subject.clone()));
                        }
                        if ui.add_enabled(!busy, egui::Button::new(tr("Checkout Revision"))).clicked() {
                            ask = Some(Ask::Checkout(hash.clone()));
                        }
                        ui.separator();
                        if ui.add_enabled(!lost, egui::Button::new(tr("Show in Log"))).on_disabled_hover_text(tr("No branch reaches this commit, so the Log does not show it")).clicked() {
                            ask = Some(Ask::ShowInLog(hash.clone()));
                        }
                        if ui.button(tr("Copy Revision Number")).clicked() {
                            ui.ctx().copy_text(hash.clone());
                        }
                    });
                }
            });
        });
        match ask {
            Some(Ask::ShowInLog(h)) => {
                self.switch_log(0);
                self.navigate_to(&h);
            }
            Some(Ask::Checkout(h)) => self.run_job(Job::one(&format!("Checkout {}", short(&h)), &["checkout", &h])),
            Some(Ask::NewBranch(h)) => self.open_new_branch(&h),
            Some(Ask::Reset(hash, subject)) => self.dialog = Some(Dialog::Reset { hash, subject, mode: ResetMode::Mixed }),
            None => {}
        }
    }

    /// The right side: the commit's message, author and branches, and the files it changed.
    fn reflog_details(&mut self, ui: &mut Ui) {
        let total = ui.max_rect();
        ui.expand_to_include_rect(total);
        theme::vline(ui, total.left() + 0.5, total.top(), total.bottom());
        let Some(details) = self.reflog.as_mut().and_then(|v| v.details.take()) else {
            ui.painter().text(total.center(), egui::Align2::CENTER_CENTER, tr("Commit details"), FontId::proportional(13.0), theme::TEXT_MID());
            return;
        };
        let files = details.files.clone();
        // The files, read only: up to the lower third.
        let fh = (total.height() * 0.34).min(36.0 + files.len().max(1) as f32 * 22.0).max(60.0);
        let top = Rect::from_min_max(total.min, pos2(total.right(), total.bottom() - fh));
        let bottom = Rect::from_min_max(pos2(total.left(), total.bottom() - fh), total.max);

        // The commit is drawn by the Log's own details view: it reads the repository's selected
        // commit, so the record's commit stands in for it while it draws.
        let Some(repo) = self.repo.as_mut() else { return };
        let log_details = std::mem::replace(&mut repo.details, Some(details));
        let log_multi = std::mem::take(&mut repo.multi);
        let log_selected = repo.selected;
        let mut tu = ui.new_child(egui::UiBuilder::new().max_rect(top));
        tu.set_clip_rect(top);
        self.commit_info(&mut tu);
        let Some(repo) = self.repo.as_mut() else { return };
        if repo.selected != log_selected {
            // A click on a parent went to it in the Log: that selection stays.
            self.tab = Tab::Log;
            return;
        }
        let details = std::mem::replace(&mut repo.details, log_details);
        repo.multi = log_multi;
        if let Some(v) = self.reflog.as_mut() {
            v.details = details;
        }

        theme::hline(ui, bottom.top() + 0.5, bottom.left(), bottom.right());
        let mut bu = ui.new_child(egui::UiBuilder::new().max_rect(bottom.shrink2(vec2(14.0, 8.0))));
        bu.set_clip_rect(bottom);
        let n = files.len();
        bu.label(RichText::new(format!("{n} {}", crate::i18n::plural(n, "file", "files"))).color(theme::TEXT_MID()).size(12.0));
        egui::ScrollArea::vertical().id_salt("reflog_files").auto_shrink([false, false]).show(&mut bu, |ui| {
            for f in &files {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(f.status.to_string()).monospace().color(crate::app::status_color(f.status)));
                    ui.add(egui::Label::new(RichText::new(&f.path).color(theme::TEXT())).truncate()).on_hover_text(&f.path);
                });
            }
        });
    }
}

/// Actions that throw commits away or rewrite them stand out.
fn action_color(action: &str) -> egui::Color32 {
    let a = action.split_whitespace().next().unwrap_or("");
    match a {
        "reset" | "rebase" => theme::UNVERSIONED(),
        "commit" | "cherry-pick" | "revert" | "merge" | "pull" => theme::GREEN(),
        _ => theme::TEXT_MID(),
    }
}
