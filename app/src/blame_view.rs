//! The "Annotate: file" tab: the file's lines with the commit, author and date of each.

use crate::app::{file_name, format_date, short, App, CompareSource, Tab};
use crate::blame::{self, BlameTab, State};
use crate::i18n::tr;
use crate::theme;
use eframe::egui::{self, pos2, vec2, Color32, FontId, Rect, RichText, Sense, Ui};

const ROW_H: f32 = 18.0;
const GUTTER: f32 = 250.0;
const NUM_W: f32 = 48.0;

/// What a click or a menu item in the tab asks for.
enum Ask {
    ShowInLog(String),
    Previous(String, String),
    History,
}

impl App {
    /// Open an Annotate tab for `path` in `rev` (None = the working folder), at `line` (0-based).
    pub fn open_blame(&mut self, path: &str, rev: Option<&str>, line: Option<usize>) {
        let Some(repo) = self.repo.as_ref() else { return };
        if let Some(i) = self.blames.iter().position(|b| b.path == path && b.rev.as_deref() == rev) {
            self.blames[i].scroll_to = line.or(self.blames[i].scroll_to);
            self.tab = Tab::Blame(i);
            return;
        }
        // git blame can take seconds on a long history: it runs beside the window.
        let (tx, rx) = std::sync::mpsc::channel();
        let (dir, p, r, ctx) = (repo.path.clone(), path.to_string(), rev.map(String::from), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(blame::load(&dir, r.as_deref(), &p));
            ctx.request_repaint();
        });
        self.blames.push(BlameTab { path: path.to_string(), rev: rev.map(String::from), state: State::Loading(rx), scroll_to: line, colors: None });
        self.tab = Tab::Blame(self.blames.len() - 1);
    }

    pub fn close_blame(&mut self, i: usize) {
        if i < self.blames.len() {
            self.blames.remove(i);
        }
        self.tab = match self.tab {
            Tab::Blame(j) if j == i => {
                if self.blames.is_empty() { Tab::Log } else { Tab::Blame(j.min(self.blames.len() - 1)) }
            }
            Tab::Blame(j) if j > i => Tab::Blame(j - 1),
            t => t,
        };
    }

    pub fn blame_tab_title(&self, i: usize) -> String {
        format!("{}: {}", tr("Annotate"), file_name(&self.blames[i].path))
    }

    pub fn blame_tab(&mut self, ui: &mut Ui, i: usize) {
        let syntax_on = !self.settings.syntax_off;
        let Some(tab) = self.blames.get_mut(i) else { return };
        if let State::Loading(rx) = &tab.state {
            match rx.try_recv() {
                Ok(Ok(b)) => tab.state = State::Ready(b),
                Ok(Err(e)) => tab.state = State::Failed(e),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => tab.state = State::Failed("git blame stopped unexpectedly".into()),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut ask: Option<Ask> = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG())).show(ui, |ui| {
            // Header: the file and the version it is shown in.
            let bar = Rect::from_min_size(ui.max_rect().min, vec2(ui.max_rect().width(), 30.0));
            ui.painter().rect_filled(bar, 0.0, theme::BG());
            theme::hline(ui, bar.bottom() - 0.5, bar.left(), bar.right());
            let version = match &tab.rev {
                Some(r) => short(r).to_string(),
                None => tr("working folder").to_string(),
            };
            let w = theme::paint_clipped(ui.painter(), pos2(bar.left() + 12.0, bar.center().y), &tab.path, FontId::proportional(13.0), theme::TEXT(), bar.width() - 200.0);
            ui.painter().text(pos2(bar.left() + 12.0 + w + 10.0, bar.center().y), egui::Align2::LEFT_CENTER, format!("·  {version}"), FontId::proportional(12.0), theme::TEXT_MID());
            ui.advance_cursor_after_rect(bar);

            let area = ui.available_rect_before_wrap();
            if let (true, State::Ready(b), None) = (syntax_on, &tab.state, &tab.colors) {
                tab.colors = Some(crate::syntax::for_lines(&tab.path, b.lines.iter().map(|l| l.text.as_str())));
            }
            let colors = tab.colors.as_deref().unwrap_or(&[]);
            let b = match &tab.state {
                State::Ready(b) => b,
                State::Loading(_) => {
                    ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, tr("Reading the history of every line…"), FontId::proportional(13.0), theme::TEXT_DIM());
                    return;
                }
                State::Failed(e) => {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.add(egui::Label::new(RichText::new(e).color(theme::RED())).wrap());
                    });
                    return;
                }
            };
            if b.lines.is_empty() {
                ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, tr("The file is empty"), FontId::proportional(13.0), theme::TEXT_DIM());
                return;
            }
            let mono = FontId::monospace(12.5);
            let small = FontId::proportional(12.0);
            let char_w = theme::text_width(ui, "0", &mono);
            let longest = b.lines.iter().map(|l| l.text.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>()).max().unwrap_or(0);
            let width = (GUTTER + NUM_W + 12.0 + longest as f32 * char_w + 24.0).max(area.width());
            let mut sa = egui::ScrollArea::both().id_salt(("blame", i)).auto_shrink([false, false]);
            if let Some(line) = tab.scroll_to.take() {
                sa = sa.vertical_scroll_offset((line as f32 * ROW_H - area.height() / 3.0).max(0.0));
            }
            ui.spacing_mut().item_spacing.y = 0.0;
            sa.show_rows(ui, ROW_H, b.lines.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_width(width);
                for n in range {
                    let line = &b.lines[n];
                    let o = &b.origins[line.origin];
                    let (r, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
                    let gr = Rect::from_min_size(r.min, vec2(GUTTER, ROW_H));
                    let resp = ui.interact(gr, ui.scope_id().with(("blame_gutter", n)), Sense::click());
                    let p = ui.painter_at(r);
                    // Newer commits are tinted stronger.
                    let tint = if o.uncommitted() { theme::GREEN() } else { theme::ACCENT() };
                    let strength = if o.uncommitted() { 0.22 } else { 0.05 + 0.25 * b.age[line.origin] };
                    p.rect_filled(gr, 0.0, mix(theme::BG_BAR(), tint, strength + if resp.hovered() { 0.12 } else { 0.0 }));
                    theme::vline(ui, gr.right() - 0.5, r.top(), r.bottom());
                    let cy = r.center().y;
                    // The commit is written on the first line of each run of its lines.
                    let first = n == 0 || b.lines[n - 1].origin != line.origin;
                    if first {
                        if n > 0 {
                            p.line_segment([pos2(gr.left(), r.top() + 0.5), pos2(gr.right(), r.top() + 0.5)], egui::Stroke::new(1.0, theme::BORDER()));
                        }
                        let (date, who) = if o.uncommitted() { (String::new(), tr("Not committed yet").to_string()) } else { (short_date(o.time), o.author.clone()) };
                        p.text(pos2(gr.left() + 8.0, cy), egui::Align2::LEFT_CENTER, &date, small.clone(), theme::TEXT_MID());
                        let gp = p.with_clip_rect(Rect::from_min_max(pos2(gr.left() + 84.0, r.top()), pos2(gr.right() - 66.0, r.bottom())));
                        gp.text(pos2(gr.left() + 84.0, cy), egui::Align2::LEFT_CENTER, &who, small.clone(), theme::TEXT());
                        if !o.uncommitted() {
                            p.text(pos2(gr.right() - 8.0, cy), egui::Align2::RIGHT_CENTER, short(&o.hash), FontId::monospace(11.5), theme::LINK());
                        }
                    }
                    p.text(pos2(gr.right() + NUM_W - 8.0, cy), egui::Align2::RIGHT_CENTER, (n + 1).to_string(), mono.clone(), theme::TEXT_DIM());
                    let shown = line.text.replace('\t', "    ");
                    match colors.get(n).filter(|c| syntax_on && !c.is_empty()) {
                        Some(syn) => {
                            let job = crate::commit_view::diff_line_job(&shown, &[], &mono, egui::Color32::TRANSPARENT, syn, 0, theme::TEXT());
                            let gal = ui.fonts_mut(|f| f.layout_job(job));
                            p.galley(pos2(gr.right() + NUM_W + 12.0, cy - gal.size().y / 2.0), gal, theme::TEXT());
                        }
                        None => {
                            p.text(pos2(gr.right() + NUM_W + 12.0, cy), egui::Align2::LEFT_CENTER, shown, mono.clone(), theme::TEXT());
                        }
                    }

                    if o.uncommitted() {
                        resp.on_hover_text(tr("Changed in the working folder, not committed yet"));
                        continue;
                    }
                    if resp.clicked() {
                        ask = Some(Ask::ShowInLog(o.hash.clone()));
                    }
                    let resp = resp.on_hover_ui(|ui| {
                        ui.label(RichText::new(&o.summary).strong());
                        ui.label(RichText::new(format!("{}  ·  {}  ·  {}", short(&o.hash), o.author, format_date(o.time))).color(theme::TEXT_MID()));
                        if o.filename != tab.path {
                            ui.label(RichText::new(format!("{}: {}", tr("The file was then named"), o.filename)).color(theme::TEXT_DIM()));
                        }
                    });
                    resp.context_menu(|ui| {
                        if ui.button(tr("Show in Log")).clicked() {
                            ask = Some(Ask::ShowInLog(o.hash.clone()));
                        }
                        if ui.button(tr("Copy Revision Number")).clicked() {
                            ui.ctx().copy_text(o.hash.clone());
                        }
                        if ui
                            .add_enabled(o.previous.is_some(), egui::Button::new(tr("Annotate Previous Revision")))
                            .on_hover_text(tr("The file as it was before this commit: who wrote the lines this commit replaced"))
                            .on_disabled_hover_text(tr("The file was added in this commit"))
                            .clicked()
                        {
                            if let Some((h, f)) = &o.previous {
                                ask = Some(Ask::Previous(h.clone(), f.clone()));
                            }
                        }
                        if ui.button(tr("Show History")).clicked() {
                            ask = Some(Ask::History);
                        }
                    });
                }
            });
        });
        let (path, rev) = match self.blames.get(i) {
            Some(t) => (t.path.clone(), t.rev.clone()),
            None => return,
        };
        match ask {
            Some(Ask::ShowInLog(h)) => {
                self.switch_log(0);
                self.navigate_to(&h);
            }
            Some(Ask::Previous(h, f)) => self.open_blame(&f, Some(&h), None),
            Some(Ask::History) => self.open_file_history(&path, rev.as_deref()),
            None => {}
        }
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    // Both colours are of the current theme already.
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

pub fn short_date(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local.timestamp_opt(ts, 0).single().map(|d| d.format("%d.%m.%Y").to_string()).unwrap_or_default()
}

/// Width of the blame column inside a diff.
pub const DIFF_BLAME_W: f32 = 200.0;

impl App {
    /// Diff `i` with "Annotate with Git Blame": start reading the blame of its new side, or take
    /// the finished one. False when the diff can't be annotated (nothing to read it from).
    pub fn diff_blame_tick(&mut self, i: usize) -> bool {
        let Some(repo) = self.repo.as_ref() else { return false };
        let Some(d) = self.diffs.get_mut(i) else { return false };
        if d.diff.binary {
            return false;
        }
        let (rev, path) = match &d.source {
            Some(CompareSource::Commit { hash, path, .. }) => (Some(hash.clone()), path.clone()),
            Some(CompareSource::Worktree { path, .. }) => (None, path.clone()),
            None => return false,
        };
        if !self.diff_annotate {
            return true;
        }
        let key = format!("{}\u{1}{path}", rev.as_deref().unwrap_or(""));
        if d.blame.as_ref().map(|(k, _)| k) != Some(&key) {
            let (tx, rx) = std::sync::mpsc::channel();
            let (dir, ctx) = (repo.path.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(blame::load(&dir, rev.as_deref(), &path));
                ctx.request_repaint();
            });
            d.blame = Some((key, State::Loading(rx)));
        }
        if let Some((_, st)) = d.blame.as_mut() {
            if let State::Loading(rx) = st {
                match rx.try_recv() {
                    Ok(Ok(b)) => *st = State::Ready(b),
                    Ok(Err(e)) => *st = State::Failed(e),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => *st = State::Failed("git blame stopped unexpectedly".into()),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        }
        true
    }
}

/// One row of the blame column of a diff: the date and author of the commit that last changed
/// line `line` (1-based) of the new side, tinted stronger for newer commits. Returns the commit
/// when the row is clicked (to show it in the Log).
pub fn diff_blame_cell(ui: &Ui, rect: Rect, state: Option<&State>, line: Option<usize>, id: egui::Id) -> Option<String> {
    let p = ui.painter_at(rect);
    let small = FontId::proportional(12.0);
    let cy = rect.center().y;
    let found = match (state, line) {
        (Some(State::Ready(b)), Some(n)) => b.lines.get(n.wrapping_sub(1)).map(|l| (&b.origins[l.origin], b.age[l.origin])),
        _ => None,
    };
    let Some((o, age)) = found else {
        p.rect_filled(rect, 0.0, theme::BG_BAR());
        match (state, line) {
            (Some(State::Loading(_)), Some(_)) => {
                p.text(pos2(rect.left() + 8.0, cy), egui::Align2::LEFT_CENTER, "…", small, theme::TEXT_DIM());
            }
            (Some(State::Failed(e)), Some(_)) => {
                let resp = ui.interact(rect, id, Sense::hover());
                p.text(pos2(rect.left() + 8.0, cy), egui::Align2::LEFT_CENTER, tr("not annotated"), small, theme::TEXT_DIM());
                resp.on_hover_text(e.as_str());
            }
            _ => {}
        }
        return None;
    };
    let resp = ui.interact(rect, id, Sense::click());
    let tint = if o.uncommitted() { theme::GREEN() } else { theme::ACCENT() };
    let strength = if o.uncommitted() { 0.22 } else { 0.06 + 0.34 * age } + if resp.hovered() { 0.12 } else { 0.0 };
    p.rect_filled(rect, 0.0, mix(theme::BG_BAR(), tint, strength));
    if o.uncommitted() {
        p.text(pos2(rect.left() + 8.0, cy), egui::Align2::LEFT_CENTER, tr("Not committed yet"), small, theme::TEXT_MID());
        resp.on_hover_text(tr("Changed in the working folder, not committed yet"));
        return None;
    }
    p.text(pos2(rect.left() + 8.0, cy), egui::Align2::LEFT_CENTER, short_date(o.time), small.clone(), theme::TEXT_MID());
    let gp = p.with_clip_rect(Rect::from_min_max(pos2(rect.left() + 80.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom())));
    gp.text(pos2(rect.left() + 80.0, cy), egui::Align2::LEFT_CENTER, &o.author, small, theme::TEXT());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let resp = resp.on_hover_ui(|ui| {
        ui.label(RichText::new(&o.summary).strong());
        ui.label(RichText::new(format!("{}  ·  {}  ·  {}", short(&o.hash), o.author, format_date(o.time))).color(theme::TEXT_MID()));
        ui.label(RichText::new(tr("Click to show the commit in the Log")).color(theme::TEXT_DIM()).size(11.5));
    });
    resp.clicked().then(|| o.hash.clone())
}
