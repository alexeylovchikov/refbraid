//! Commit panel (local changes, message, commit/push), stashes, diff viewer and console.

use std::collections::{HashMap, HashSet};
use crate::i18n::tr;
use crate::app::PartialSel;
use crate::partial::{Disp, fold_rows};
use crate::app::{App, CommitTab, Dialog, Job, Tab, Then, file_name, format_date, status_color};
use crate::ai;
use crate::git::{DiffSide, Operation, StatusEntry};
use crate::theme::{self, Icon};
use eframe::egui::{self, Color32, FontId, Key, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

const DIFF_ROW: f32 = 18.0;
const C_ADDED: Color32 = Color32::from_rgb(0x2a, 0x45, 0x36);
const C_REMOVED: Color32 = Color32::from_rgb(0x4a, 0x2f, 0x30);
const C_CHANGED: Color32 = Color32::from_rgb(0x2f, 0x43, 0x5e);
/// The words that changed inside a changed line: removed (old side) and added (new side).
const W_REMOVED: Color32 = Color32::from_rgb(0x7d, 0x3b, 0x3e);
const W_ADDED: Color32 = Color32::from_rgb(0x2e, 0x6c, 0x45);

/// Text selected in a diff with the mouse: rows are shown rows, columns are characters of the
/// line as shown (tabs as four spaces). `side`: 0 left, 1 right, 2 the unified view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSel {
    pub side: u8,
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl TextSel {
    fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }
    fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
    /// The selected characters of row `k` (a line of `n` characters).
    fn cols(&self, k: usize, n: usize) -> Option<(usize, usize)> {
        let (a, b) = self.ordered();
        if k < a.0 || k > b.0 {
            return None;
        }
        let c0 = if k == a.0 { a.1.min(n) } else { 0 };
        let c1 = if k == b.0 { b.1.min(n) } else { n };
        Some((c0, c1.max(c0)))
    }
}

const SEL_BG: Color32 = Color32::from_rgba_premultiplied(0x1e, 0x41, 0x88, 0x90);

/// Paint the selection of row `k` behind its text (`x0`: where the text starts, `cw`: width of a
/// character).
fn paint_text_sel(p: &egui::Painter, s: &TextSel, k: usize, n: usize, x0: f32, cw: f32, rect: Rect) {
    let Some((c0, c1)) = s.cols(k, n) else { return };
    // A line selected up to its end shows a bit of its line break too.
    let extra = if k < s.ordered().1 .0 { cw * 0.6 } else { 0.0 };
    if c1 == c0 && extra == 0.0 {
        return;
    }
    let r = Rect::from_min_max(pos2(x0 + c0 as f32 * cw, rect.top() + 1.0), pos2(x0 + c1 as f32 * cw + extra, rect.bottom() - 1.0));
    p.rect_filled(r, 2.0, SEL_BG);
}

/// Where the text cursor of a diff goes on a key press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    WordLeft,
    WordRight,
    /// A screenful (so many lines) up or down.
    PageUp(usize),
    PageDown(usize),
}

/// The key press as a cursor movement: the arrows, Home, End, Page Up / Down; with Ctrl the
/// left and right arrows go by words and Home / End to the ends of the text.
pub fn key_move(key: Key, ctrl: bool, page: usize) -> Option<Move> {
    Some(match (key, ctrl) {
        (Key::ArrowLeft, false) => Move::Left,
        (Key::ArrowRight, false) => Move::Right,
        (Key::ArrowLeft, true) => Move::WordLeft,
        (Key::ArrowRight, true) => Move::WordRight,
        (Key::ArrowUp, _) => Move::Up,
        (Key::ArrowDown, _) => Move::Down,
        (Key::Home, false) => Move::LineStart,
        (Key::End, false) => Move::LineEnd,
        (Key::Home, true) => Move::DocStart,
        (Key::End, true) => Move::DocEnd,
        (Key::PageUp, _) => Move::PageUp(page.max(1)),
        (Key::PageDown, _) => Move::PageDown(page.max(1)),
        _ => return None,
    })
}

/// Move the text cursor at `pos` (line, character). `line` gives the text of a line of this
/// side, or None where the side has no line (the gap beside an added line, a hunk header):
/// such lines are stepped over.
pub fn move_caret(pos: (usize, usize), m: Move, total: usize, line: &dyn Fn(usize) -> Option<String>) -> (usize, usize) {
    let len = |k: usize| line(k).map(|t| t.chars().count());
    let above = |k: usize| (0..k).rev().find(|&j| line(j).is_some());
    let below = |k: usize| (k + 1..total).find(|&j| line(j).is_some());
    let (k, c) = pos;
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    match m {
        Move::Left if c > 0 => (k, c.min(len(k).unwrap_or(0)).saturating_sub(1)),
        // At the start of a line: to the end of the line above.
        Move::Left => above(k).map(|j| (j, len(j).unwrap_or(0))).unwrap_or(pos),
        Move::WordLeft if c == 0 => above(k).map(|j| (j, len(j).unwrap_or(0))).unwrap_or(pos),
        Move::Right if c < len(k).unwrap_or(0) => (k, c + 1),
        Move::Right => below(k).map(|j| (j, 0)).unwrap_or(pos),
        Move::WordLeft => {
            let chars: Vec<char> = line(k).unwrap_or_default().chars().collect();
            let mut i = c.min(chars.len());
            while i > 0 && !is_word(chars[i - 1]) {
                i -= 1;
            }
            while i > 0 && is_word(chars[i - 1]) {
                i -= 1;
            }
            (k, i)
        }
        Move::WordRight => {
            let chars: Vec<char> = line(k).unwrap_or_default().chars().collect();
            let mut i = c.min(chars.len());
            if i >= chars.len() {
                return below(k).map(|j| (j, 0)).unwrap_or(pos);
            }
            while i < chars.len() && is_word(chars[i]) {
                i += 1;
            }
            while i < chars.len() && !is_word(chars[i]) {
                i += 1;
            }
            (k, i)
        }
        Move::Up => above(k).map(|j| (j, c.min(len(j).unwrap_or(0)))).unwrap_or((k, 0)),
        Move::Down => below(k).map(|j| (j, c.min(len(j).unwrap_or(0)))).unwrap_or((k, len(k).unwrap_or(0))),
        Move::LineStart => (k, 0),
        Move::LineEnd => (k, len(k).unwrap_or(0)),
        Move::DocStart => (0..total).find(|&j| line(j).is_some()).map(|j| (j, 0)).unwrap_or(pos),
        Move::DocEnd => (0..total).rev().find(|&j| line(j).is_some()).map(|j| (j, len(j).unwrap_or(0))).unwrap_or(pos),
        Move::PageUp(n) => (0..n).fold(pos, |p, _| move_caret(p, Move::Up, total, line)),
        Move::PageDown(n) => (0..n).fold(pos, |p, _| move_caret(p, Move::Down, total, line)),
    }
}

/// The text cursor: a thin line at the selection's moving end (`x0`: where the text starts).
fn paint_caret(p: &egui::Painter, s: &TextSel, k: usize, x0: f32, cw: f32, rect: Rect) {
    if s.head.0 == k {
        let x = x0 + s.head.1 as f32 * cw;
        p.line_segment([pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 2.0)], Stroke::new(1.5, theme::TEXT()));
    }
}

/// The word around character `col` of `line` (for a double-click).
fn word_at(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let col = col.min(chars.len().saturating_sub(1));
    if chars.is_empty() || !is_word(chars[col]) {
        return (col, (col + 1).min(chars.len()));
    }
    let mut a = col;
    while a > 0 && is_word(chars[a - 1]) {
        a -= 1;
    }
    let mut b = col;
    while b < chars.len() && is_word(chars[b]) {
        b += 1;
    }
    (a, b)
}

/// Mark the found text of a line: `marks` are (from, to, current) in characters.
fn paint_find(p: &egui::Painter, marks: &[(usize, usize, bool)], x0: f32, cw: f32, rect: Rect) {
    for &(a, b, current) in marks {
        let r = Rect::from_min_max(pos2(x0 + a as f32 * cw, rect.top() + 1.0), pos2(x0 + b as f32 * cw, rect.bottom() - 1.0));
        p.rect_filled(r, 2.0, if current { theme::FIND_CURRENT() } else { theme::FIND() });
        if current {
            p.rect_stroke(r, 2.0, Stroke::new(1.0, theme::STAR()), egui::StrokeKind::Outside);
        }
    }
}

/// Options of one commit that are not kept after it.
#[derive(Clone, Default, Debug)]
pub struct CommitOpts {
    /// Don't run the pre-commit and commit-msg hooks.
    pub no_verify: bool,
    /// "Name <email>" when the commit is somebody else's work.
    pub author: String,
    /// The author date, in any form git understands.
    pub date: String,
}

impl CommitOpts {
    /// How many of them are set.
    pub fn count(&self) -> usize {
        self.no_verify as usize + !self.author.trim().is_empty() as usize + !self.date.trim().is_empty() as usize
    }
}

/// The options of `git commit` chosen in the Commit panel.
pub fn commit_flags(signoff: bool, sign: bool, o: &CommitOpts) -> Vec<String> {
    let mut f = vec![];
    if signoff {
        f.push("--signoff".to_string());
    }
    if sign {
        f.push("-S".to_string());
    }
    if o.no_verify {
        f.push("--no-verify".to_string());
    }
    if !o.author.trim().is_empty() {
        f.push(format!("--author={}", o.author.trim()));
    }
    if !o.date.trim().is_empty() {
        f.push(format!("--date={}", o.date.trim()));
    }
    f
}

/// Put the options into every `git commit` of a job (after "commit", before its own arguments;
/// commands may start with `env:` items).
pub fn add_commit_flags(cmds: &mut [Vec<String>], flags: &[String]) {
    for c in cmds.iter_mut() {
        let at = c.iter().position(|a| !a.starts_with("env:"));
        if let Some(at) = at.filter(|&at| c[at] == "commit") {
            c.splice(at + 1..at + 1, flags.iter().cloned());
        }
    }
}

/// The search line of a text view: the field, the steps, "N of M" and the cross. `count` is
/// (the current match, 1-based; how many). True when the line is to be closed.
pub fn find_line(ui: &mut Ui, f: &mut crate::find::Find, hint: &str, count: (usize, usize)) -> bool {
    let mut close = false;
    let edit_id = egui::Id::unique(format!("search:{hint}"));
    let had_focus = ui.memory(|m| m.has_focus(edit_id));
    let before = (f.query.clone(), f.regex, f.case);
    crate::details_view::search_box(ui, &mut f.query, hint, 320.0, Some((&mut f.regex, &mut f.case)));
    if std::mem::take(&mut f.focus) {
        ui.memory_mut(|m| m.request_focus(edit_id));
    }
    if before != (f.query.clone(), f.regex, f.case) {
        f.current = 0;
        f.go = true;
    }
    let (cur, total) = count;
    let (enter, f3, shift, esc) = ui.input_mut(|i| (i.key_pressed(Key::Enter), i.consume_key(egui::Modifiers::NONE, Key::F3) || i.consume_key(egui::Modifiers::SHIFT, Key::F3), i.modifiers.shift, i.key_pressed(Key::Escape)));
    if f3 || (enter && had_focus) {
        f.step(total, !shift);
        if enter {
            ui.memory_mut(|m| m.request_focus(edit_id));
        }
    }
    if theme::icon_button_sized(ui, Icon::Expand, tr("Previous match (Shift+F3)"), 24.0, false, total > 0).clicked() {
        f.step(total, false);
    }
    if theme::icon_button_sized(ui, Icon::ChevronDown, tr("Next match (F3 or Enter)"), 24.0, false, total > 0).clicked() {
        f.step(total, true);
    }
    ui.add_space(6.0);
    let (text, col) = if f.query.is_empty() {
        (String::new(), theme::TEXT_DIM())
    } else if total == 0 {
        (tr("No matches").to_string(), theme::RED())
    } else {
        (crate::i18n::trf("{} of {}", &[&cur, &total]), theme::TEXT_MID())
    };
    ui.label(RichText::new(text).color(col).size(12.0));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if theme::icon_button_sized(ui, Icon::Close, tr("Close the search (Esc)"), 22.0, false, true).clicked() || (esc && (had_focus || ui.memory(|m| m.focused().is_none()))) {
            close = true;
        }
    });
    close
}

/// A line of diff text: the changed words (`ranges`) on a stronger background, the code
/// coloured by `syntax` (pieces of the line after its first `shift` bytes).
pub fn diff_line_job(text: &str, ranges: &[std::ops::Range<usize>], font: &FontId, word_bg: Color32, syntax: &[crate::syntax::Span], shift: usize, base: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    // Every place where the colour or the background changes.
    let mut cuts: Vec<usize> = vec![0, text.len()];
    cuts.extend(ranges.iter().flat_map(|r| [r.start, r.end]));
    cuts.extend(syntax.iter().flat_map(|(r, _)| [r.start + shift, r.end + shift]));
    cuts.retain(|&c| c <= text.len() && text.is_char_boundary(c));
    cuts.sort_unstable();
    cuts.dedup();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let marked = ranges.iter().any(|r| r.start <= a && b <= r.end);
        let kind = syntax.iter().find(|(r, _)| r.start + shift <= a && b <= r.end + shift).map(|(_, k)| *k);
        let color = match kind {
            Some(k) => theme::syntax_color(k),
            None if marked => theme::rgb(255, 255, 255),
            None => base,
        };
        job.append(&text[a..b], 0.0, egui::TextFormat { font_id: font.clone(), color, background: if marked { word_bg } else { Color32::TRANSPARENT }, ..Default::default() });
    }
    job
}

/// Unified view: the line of the new file at row `k` (from the hunk header "@@ -a,b +c,d @@").
fn unified_new_line(u: &[(char, String)], k: usize) -> Option<usize> {
    let k = k.min(u.len().checked_sub(1)?);
    let h = (0..=k).rev().find(|&j| u[j].0 == '@')?;
    let start: usize = u[h].1.split_whitespace().find_map(|w| w.strip_prefix('+'))?.split(',').next()?.parse().ok()?;
    if k <= h {
        return Some(start);
    }
    Some(start + u[h + 1..k].iter().filter(|(t, _)| *t != '-').count())
}

/// In the unified view, the line a removed line was changed into (or the other way round): the
/// n-th line of a run of "-" lines pairs with the n-th line of the "+" run right after it.
fn unified_partner(u: &[(char, String)], k: usize) -> Option<usize> {
    let run = |tag: char, from: usize| {
        let (mut s, mut e) = (from, from);
        while s > 0 && u[s - 1].0 == tag {
            s -= 1;
        }
        while e + 1 < u.len() && u[e + 1].0 == tag {
            e += 1;
        }
        (s, e)
    };
    match u[k].0 {
        '-' => {
            let (s, e) = run('-', k);
            let j = e + 1 + (k - s);
            (e + 1 < u.len() && u[e + 1].0 == '+' && j <= run('+', e + 1).1).then_some(j)
        }
        '+' => {
            let (s, _) = run('+', k);
            if s == 0 || u[s - 1].0 != '-' {
                return None;
            }
            let (ms, _) = run('-', s - 1);
            let j = ms + (k - s);
            (j < s).then_some(j)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Conflicts,
    Changes,
    Unversioned,
    Ignored,
}

/// Ignored files are drawn in a muted olive, like in the IDE.
const C_IGNORED: Color32 = Color32::from_rgb(0x9a, 0x94, 0x5c);

/// One line of the Changes view.
enum Row {
    Group { g: Group, title: &'static str, key: String, paths: Vec<String>, open: bool },
    Folder { g: Group, key: String, label: String, depth: usize, paths: Vec<String>, open: bool },
    File { e: StatusEntry, depth: usize, g: Group, show_dir: bool },
}

#[derive(Default)]
struct Dir {
    dirs: std::collections::BTreeMap<String, Dir>,
    files: Vec<usize>,
}

impl Dir {
    fn all_files(&self, out: &mut Vec<usize>) {
        for d in self.dirs.values() {
            d.all_files(out);
        }
        out.extend(&self.files);
    }
}

/// Folder tree rows for a group: folders first, single-folder chains joined ("src/main/java").
fn tree_rows(items: &[StatusEntry], title: &str, g: Group, collapsed: &std::collections::HashSet<String>, rows: &mut Vec<Row>) {
    let mut root = Dir::default();
    for (i, e) in items.iter().enumerate() {
        let p = e.path.trim_end_matches('/');
        let mut d = &mut root;
        if let Some((dir, _)) = p.rsplit_once('/') {
            for part in dir.split('/') {
                d = d.dirs.entry(part.to_string()).or_default();
            }
        }
        d.files.push(i);
    }
    fn walk(d: &Dir, prefix: &str, depth: usize, items: &[StatusEntry], title: &str, g: Group, collapsed: &std::collections::HashSet<String>, rows: &mut Vec<Row>) {
        for (name, sub) in &d.dirs {
            let mut label = name.clone();
            let mut full = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let mut cur = sub;
            while cur.files.is_empty() && cur.dirs.len() == 1 {
                let (n, s) = cur.dirs.iter().next().unwrap();
                label = format!("{label}/{n}");
                full = format!("{full}/{n}");
                cur = s;
            }
            let key = format!("{title}:{full}");
            let open = !collapsed.contains(&key);
            let mut idx = vec![];
            cur.all_files(&mut idx);
            let paths = idx.iter().map(|&i| items[i].path.clone()).collect();
            rows.push(Row::Folder { g, key, label, depth, paths, open });
            if open {
                walk(cur, &full, depth + 1, items, title, g, collapsed, rows);
            }
        }
        for &i in &d.files {
            rows.push(Row::File { e: items[i].clone(), depth, g, show_dir: false });
        }
    }
    walk(&root, "", 0, items, title, g, collapsed, rows);
}

impl App {
    pub fn commit_panel(&mut self, ui: &mut Ui) {
        egui::Panel::top("commit_header")
            .exact_size(34.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| {
                let r = ui.max_rect();
                theme::hline(ui, r.bottom() - 0.5, r.left(), r.right());
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(tr("Commit")).strong().color(theme::TEXT()));
                    ui.add_space(8.0);
                    for (t, label) in [(CommitTab::Changes, "Changes"), (CommitTab::Stash, "Stash")] {
                        let sel = self.commit_tab == t;
                        let resp = ui.add(
                            egui::Button::new(RichText::new(tr(label)).color(theme::TEXT()))
                                .fill(if sel { theme::TAB_SEL() } else { Color32::TRANSPARENT })
                                .stroke(Stroke::NONE),
                        );
                        if resp.clicked() {
                            self.commit_tab = t;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        if theme::icon_button_sized(ui, Icon::Minus, tr("Hide"), 22.0, false, true).clicked() {
                            self.show_commit = false;
                        }
                    });
                });
            });
        match self.commit_tab {
            CommitTab::Changes => self.changes_tab(ui),
            CommitTab::Stash => self.stash_tab(ui),
        }
    }

    fn changes_tab(&mut self, ui: &mut Ui) {
        let op = self.repo.as_ref().and_then(|r| r.state.operation);
        if let Some(op) = op {
            egui::Panel::top("op_banner")
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::rgb(0x3d, 0x32, 0x23)).inner_margin(8))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{} in progress", op.title())).color(theme::STAR()));
                        if self.repo.as_ref().map(|r| r.status.iter().any(|e| e.is_conflict())).unwrap_or(false)
                            && ui.button(tr("Resolve…")).clicked()
                        {
                            self.show_conflicts = true;
                        }
                        if op != Operation::Merge && ui.button(tr("Continue")).clicked() {
                            self.run_job(Job::one(&format!("Continue {}", op.title()), &[op.command(), "--continue"]));
                        }
                        if ui.button(tr("Abort")).clicked() {
                            self.run_job(Job::one(&format!("Abort {}", op.title()), &[op.command(), "--abort"]));
                        }
                    });
                });
        }
        egui::Panel::top("changes_toolbar")
            .exact_size(32.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG()).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if theme::icon_button(ui, Icon::Refresh, tr("Refresh")).clicked() {
                        self.refresh();
                    }
                    let sel = self.repo.as_ref().and_then(|r| r.status_selected.clone());
                    if theme::icon_button(ui, Icon::Undo, tr("Rollback checked changes…")).clicked() {
                        self.confirm_rollback();
                    }
                    if theme::icon_button_sized(ui, Icon::Diff, tr("Show Diff"), 24.0, false, sel.is_some()).clicked() {
                        if let Some(p) = sel {
                            self.open_worktree_diff(&p);
                        }
                    }
                    if theme::icon_button(ui, Icon::Stash, tr("Stash Changes…")).clicked() {
                        self.open_stash_dialog();
                    }
                    let eye = theme::icon_button_sized(ui, Icon::Eye, tr("View Options"), 24.0, self.show_view_options, true);
                    if self.show_view_options {
                        // Screenshot helper: show the menu without a click.
                        let pos = eye.rect.left_bottom() + vec2(0.0, 4.0);
                        egui::Area::new(egui::Id::unique("changes_view_menu_demo"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(pos)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::menu(ui.style()).show(ui, |ui| {
                                    egui::containers::menu::menu_style(ui.style_mut());
                                    self.changes_view_menu(ui);
                                });
                            });
                    } else {
                        egui::Popup::menu(&eye).show(|ui| self.changes_view_menu(ui));
                    }
                    if theme::icon_button(ui, Icon::Expand, tr("Expand All")).clicked() {
                        self.collapse_changes(false);
                    }
                    if theme::icon_button(ui, Icon::Collapse, tr("Collapse All")).clicked() {
                        self.collapse_changes(true);
                    }
                    // AI tasks for the checked changes (for now: split them into atomic commits).
                    let on = self.show_ai_tasks || self.split_running();
                    let stars = theme::icon_button_sized(ui, Icon::Spiral, tr("AI Tasks"), 24.0, on, true);
                    if self.show_ai_tasks {
                        let pos = stars.rect.left_bottom() + vec2(0.0, 4.0);
                        egui::Area::new(egui::Id::unique("changes_ai_menu_demo"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(pos)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::menu(ui.style()).show(ui, |ui| {
                                    egui::containers::menu::menu_style(ui.style_mut());
                                    self.ai_tasks_menu(ui);
                                });
                            });
                    } else {
                        egui::Popup::menu(&stars).show(|ui| self.ai_tasks_menu(ui));
                    }
                });
            });
        if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::ALT, Key::P))) {
            self.toggle_group_by_dir();
        }
        // The split between the changes and the message is remembered.
        let h = if self.settings.message_height > 0.0 { self.settings.message_height } else { 200.0 };
        let r = egui::Panel::bottom("commit_message")
            .default_size(h)
            .min_size(150.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::BG()).inner_margin(egui::Margin::symmetric(10, 8)))
            .show(ui, |ui| self.message_area(ui));
        let h = r.response.rect.height();
        self.remember_width(ui, h, |s| &mut s.message_height);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| self.changes_list(ui));
    }

    /// The groups of the Changes view, each with its files.
    fn change_groups(&self) -> Vec<(Group, &'static str, Vec<StatusEntry>)> {
        let Some(repo) = self.repo.as_ref() else { return vec![] };
        let mut groups = vec![
            (Group::Conflicts, "Merge Conflicts", repo.status.iter().filter(|e| e.is_conflict()).cloned().collect()),
            (
                Group::Changes,
                "Changes",
                repo.status.iter().filter(|e| !e.is_conflict() && !e.is_untracked()).cloned().collect(),
            ),
            (Group::Unversioned, "Unversioned Files", repo.status.iter().filter(|e| e.is_untracked()).cloned().collect()),
        ];
        if self.settings.changes_show_ignored {
            groups.push((
                Group::Ignored,
                "Ignored Files",
                repo.ignored.iter().map(|p| StatusEntry { path: p.clone(), old_path: None, x: '!', y: '!' }).collect(),
            ));
        }
        groups
    }

    /// Eye button menu of the Changes view: group by directory, show ignored files.
    pub fn changes_view_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(230.0);
        ui.label(RichText::new(tr("Group By")).color(theme::TEXT_DIM()).size(11.5));
        let on = !self.settings.changes_flat;
        let label = if on { "✓  Directory" } else { "     Directory" };
        let r = ui.add(
            egui::Button::new(RichText::new(label).color(theme::TEXT()))
                .shortcut_text(RichText::new(tr("Ctrl+Alt+P")).color(theme::TEXT_DIM()))
                .frame_when_inactive(on),
        );
        if r.on_hover_text(tr("Show the changed files in a tree of folders")).clicked() {
            self.toggle_group_by_dir();
            ui.close();
        }
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Show")).color(theme::TEXT_DIM()).size(11.5));
        let on = self.settings.changes_show_ignored;
        let label = if on { "✓  Ignored Files" } else { "     Ignored Files" };
        let r = ui.add(egui::Button::new(RichText::new(label).color(theme::TEXT())).frame_when_inactive(on));
        if r.on_hover_text(tr("Also list the files that .gitignore hides")).clicked() {
            self.settings.changes_show_ignored = !on;
            crate::git::SHOW_IGNORED.store(!on, std::sync::atomic::Ordering::Relaxed);
            if let Some(repo) = self.repo.as_mut() {
                repo.ignored = if !on { crate::git::ignored(&repo.path) } else { vec![] };
            }
            self.save_settings_quietly();
            ui.close();
        }
    }

    fn toggle_group_by_dir(&mut self) {
        self.settings.changes_flat = !self.settings.changes_flat;
        self.save_settings_quietly();
    }

    fn save_settings_quietly(&self) {
        if self.screenshot.is_none() {
            self.settings.save();
        }
    }

    /// Collapse every folder (the groups stay open), or open everything again.
    fn collapse_changes(&mut self, collapse: bool) {
        if !collapse {
            self.changes_collapsed.clear();
            return;
        }
        for (_, title, items) in self.change_groups() {
            for e in items {
                let p = e.path.trim_end_matches('/');
                let mut acc = String::new();
                if let Some((dir, _)) = p.rsplit_once('/') {
                    for part in dir.split('/') {
                        if !acc.is_empty() {
                            acc.push('/');
                        }
                        acc.push_str(part);
                        self.changes_collapsed.insert(format!("{title}:{acc}"));
                    }
                }
            }
        }
    }

    fn changes_list(&mut self, ui: &mut Ui) {
        let groups = self.change_groups();
        let Some(repo) = self.repo.as_ref() else { return };
        if groups.iter().all(|g| g.2.is_empty()) {
            ui.painter().text(
                ui.max_rect().center(),
                egui::Align2::CENTER_CENTER,
                tr("No local changes"),
                FontId::proportional(13.0),
                theme::TEXT_MID(),
            );
            return;
        }
        // Rows to draw: groups, folders (when grouped by directory) and files.
        let flat = self.settings.changes_flat;
        let mut rows: Vec<Row> = vec![];
        for (g, title, items) in &groups {
            if items.is_empty() {
                continue;
            }
            let key = format!("g:{title}");
            let open = !self.changes_collapsed.contains(&key);
            rows.push(Row::Group { g: *g, title, key, paths: items.iter().map(|e| e.path.clone()).collect(), open });
            if !open {
                continue;
            }
            if flat {
                for e in items {
                    rows.push(Row::File { e: e.clone(), depth: 0, g: *g, show_dir: true });
                }
            } else {
                tree_rows(items, title, *g, &self.changes_collapsed, &mut rows);
            }
        }
        let unchecked = repo.unchecked.clone();
        let eol_only = repo.eol_only.clone();
        let submodules = repo.submodules.clone();
        let lfs_paths = repo.lfs_paths.clone();
        let host_file = self.hosting_of_changes();
        let root = repo.path.clone();
        let partial = repo.partial.clone();
        let selected = repo.status_selected.clone();
        // The selection: several files with Ctrl/Shift+click, otherwise the one selected file.
        let mut chosen: HashSet<String> = repo.status_multi.clone();
        if chosen.is_empty() {
            chosen.extend(selected.clone());
        }
        let entries: HashMap<String, (StatusEntry, Group)> = groups
            .iter()
            .flat_map(|(g, _, items)| items.iter().map(move |e| (e.path.clone(), (e.clone(), *g))))
            .collect();
        // Files in the order they are shown, for Shift+click ranges.
        let order: Vec<String> = rows.iter().filter_map(|r| if let Row::File { e, .. } = r { Some(e.path.clone()) } else { None }).collect();
        let mods = ui.input(|i| i.modifiers);
        let mut new_sel: Option<(HashSet<String>, String)> = None;
        let mut toggle: Vec<(String, bool)> = vec![];
        let mut fold: Option<String> = None;
        let mut select: Option<String> = None;
        let mut diff: Option<String> = None;
        let mut open_local: Option<std::path::PathBuf> = None;
        let mut history: Option<String> = None;
        let mut annotate: Option<String> = None;
        // A submodule's row: open it as a repository, or check out its recorded commit.
        let mut sub_open: Option<crate::submodule::Submodule> = None;
        let mut sub_update: Option<crate::submodule::Submodule> = None;
        // Track with LFS: the pattern chosen.
        let mut lfs_track: Option<String> = None;
        let mut follow: Option<String> = None;
        let mut merge: Option<(String, bool)> = None;
        let mut resolve_all = false;
        let mut job: Option<Job> = None;
        let mut rollback: Option<Vec<StatusEntry>> = None;
        let mut patch_of: Option<Vec<String>> = None;
        let mut ignore: Option<(bool, Vec<String>)> = None;
        let mut delete: Option<Vec<String>> = None;
        let checkable = |g: Group| g != Group::Conflicts && g != Group::Ignored;
        // A click in the list brings the arrow keys to it. Up and down go through the files,
        // Enter opens the diff, Space takes the file into the commit or out of it.
        if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(ui.max_rect()) {
            self.key_focus = crate::app::KeyFocus::Changes;
        }
        let mut moved_to: Option<String> = None;
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if self.key_focus == crate::app::KeyFocus::Changes && !typing && self.dialog.is_none() && !self.show_settings && !order.is_empty() {
            let (up, down, enter, space) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::Enter), i.key_pressed(Key::Space)));
            let at = selected.as_ref().and_then(|s| order.iter().position(|p| p == s));
            if up || down {
                let next = match at {
                    Some(k) if up => k.saturating_sub(1),
                    Some(k) => (k + 1).min(order.len() - 1),
                    None => 0,
                };
                moved_to = Some(order[next].clone());
                select = moved_to.clone();
            } else if let Some(p) = at.map(|k| order[k].clone()) {
                if enter {
                    diff = Some(p);
                } else if space && entries.get(&p).is_some_and(|(_, g)| checkable(*g)) {
                    toggle.push((p.clone(), unchecked.contains(&p)));
                }
            }
        }
        egui::ScrollArea::both().id_salt("changes_list").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let w = ui.available_width().max(240.0);
            for row in &rows {
                match row {
                    Row::Group { g, title, key, paths, open } => {
                        let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
                        let cy = r.center().y;
                        let chev = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
                        theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 12.0, cy), vec2(12.0, 12.0)), chev, theme::TEXT_MID());
                        let mut x = r.left() + 22.0;
                        if checkable(*g) {
                            let all_checked = paths.iter().all(|p| !unchecked.contains(p));
                            let cb = Rect::from_center_size(pos2(x + 8.0, cy), vec2(14.0, 14.0));
                            let cresp = ui.interact(cb, ui.scope_id().with(("grp", *title)), Sense::click());
                            paint_checkbox(ui, cb, all_checked);
                            if cresp.clicked() {
                                for p in paths {
                                    toggle.push((p.clone(), !all_checked));
                                }
                            }
                            x += 22.0;
                        }
                        let col = match g {
                            Group::Conflicts => theme::RED(),
                            Group::Ignored => theme::adapt(C_IGNORED),
                            _ => theme::TEXT(),
                        };
                        ui.painter().text(pos2(x, cy), egui::Align2::LEFT_CENTER, tr(title), FontId::proportional(13.0), col);
                        let tw = theme::text_width(ui, tr(title), &FontId::proportional(13.0));
                        let n = paths.len();
                        ui.painter().text(
                            pos2(x + 8.0 + tw, cy),
                            egui::Align2::LEFT_CENTER,
                            format!("{n} {}", crate::i18n::plural(n, "file", "files")),
                            FontId::proportional(12.0),
                            theme::TEXT_DIM(),
                        );
                        if resp.clicked() {
                            fold = Some(key.clone());
                        }
                        if *g == Group::Conflicts {
                            let font = FontId::proportional(12.5);
                            let tw = theme::text_width(ui, "Resolve…", &font);
                            let lr = Rect::from_min_size(pos2(r.right() - tw - 14.0, r.top()), vec2(tw + 8.0, r.height()));
                            let lresp = ui.interact(lr, ui.scope_id().with("resolve_link"), Sense::click());
                            let col = if lresp.hovered() { theme::TEXT() } else { theme::LINK() };
                            ui.painter().text(lr.left_center() + vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, tr("Resolve…"), font, col);
                            if lresp.on_hover_text(tr("Resolve the conflicts: by hand or with AI")).clicked() {
                                resolve_all = true;
                            }
                        }
                    }
                    Row::Folder { g, key, label, depth, paths, open } => {
                        let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
                        let cy = r.center().y;
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 0.0, theme::HOVER());
                        }
                        let base = r.left() + 20.0 + 18.0 * *depth as f32;
                        let chev = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
                        theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(base + 4.0, cy), vec2(12.0, 12.0)), chev, theme::TEXT_MID());
                        let mut x = base + 14.0;
                        if checkable(*g) {
                            let all_checked = paths.iter().all(|p| !unchecked.contains(p));
                            let cb = Rect::from_center_size(pos2(x + 8.0, cy), vec2(14.0, 14.0));
                            let cresp = ui.interact(cb, ui.scope_id().with(("dircb", key.as_str())), Sense::click());
                            paint_checkbox(ui, cb, all_checked);
                            if cresp.clicked() {
                                for p in paths {
                                    toggle.push((p.clone(), !all_checked));
                                }
                            }
                            x += 22.0;
                        }
                        theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(x + 8.0, cy), vec2(14.0, 14.0)), Icon::Folder, theme::TEXT_MID());
                        let font = FontId::proportional(13.0);
                        ui.painter().text(pos2(x + 20.0, cy), egui::Align2::LEFT_CENTER, label, font.clone(), theme::TEXT());
                        let lw = theme::text_width(ui, label, &font);
                        let n = paths.len();
                        ui.painter().text(
                            pos2(x + 28.0 + lw, cy),
                            egui::Align2::LEFT_CENTER,
                            format!("{n} {}", crate::i18n::plural(n, "file", "files")),
                            FontId::proportional(12.0),
                            theme::TEXT_DIM(),
                        );
                        if resp.clicked() {
                            fold = Some(key.clone());
                        }
                    }
                    Row::File { e, depth, g, show_dir } => {
                        let (g, e) = (*g, e);
                        let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
                        let cy = r.center().y;
                        if moved_to.as_ref() == Some(&e.path) {
                            ui.scroll_to_rect(r, None);
                        }
                        if chosen.contains(&e.path) || moved_to.as_ref() == Some(&e.path) {
                            ui.painter().rect_filled(r, 0.0, theme::SELECT());
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 0.0, theme::HOVER());
                        }
                        let mut x = r.left() + 20.0 + 18.0 * *depth as f32 + 14.0;
                        if checkable(g) {
                            let cb = Rect::from_center_size(pos2(x + 8.0, cy), vec2(14.0, 14.0));
                            let cresp = ui.interact(cb, ui.scope_id().with(("cb", &e.path)), Sense::click());
                            let on = !unchecked.contains(&e.path);
                            let part = partial.get(&e.path).filter(|s| on && !s.excluded.is_empty());
                            if part.is_some() {
                                paint_checkbox_partial(ui, cb);
                            } else {
                                paint_checkbox(ui, cb, on);
                            }
                            // A file committed in part: a click takes all of its changes again.
                            if cresp.on_hover_text(if part.is_some() { "Only some changes are included; click to include all" } else { "Include in commit" }).clicked() {
                                toggle.push((e.path.clone(), part.is_some() || !on));
                            }
                            x += 22.0;
                        }
                        let is_dir = e.path.ends_with('/');
                        theme::paint_icon(
                            ui.painter(),
                            Rect::from_center_size(pos2(x + 8.0, cy), vec2(14.0, 14.0)),
                            if is_dir { Icon::Folder } else { Icon::File },
                            theme::TEXT_MID(),
                        );
                        let name = file_name(e.path.trim_end_matches('/'));
                        let font = FontId::proportional(13.0);
                        let col = if g == Group::Ignored { theme::adapt(C_IGNORED) } else { status_color(e.kind()) };
                        ui.painter().text(pos2(x + 20.0, cy), egui::Align2::LEFT_CENTER, &name, font.clone(), col);
                        let nw = theme::text_width(ui, &name, &font);
                        let dir = if *show_dir { e.path.trim_end_matches('/').rsplit_once('/').map(|x| x.0).unwrap_or("") } else { "" };
                        let mut extra = match &e.old_path {
                            Some(o) => format!("{dir}   (renamed from {o})"),
                            None => dir.to_string(),
                        };
                        if let Some(s) = partial.get(&e.path).filter(|s| !unchecked.contains(&e.path) && !s.excluded.is_empty()) {
                            let total = s.total.max(s.excluded.len());
                            extra = format!("{extra}   {} of {total} changes", total - s.excluded.len());
                        }
                        if eol_only.contains(&e.path) {
                            extra = format!("{extra}   {}", tr("line endings only"));
                        }
                        if lfs_paths.contains(&e.path) {
                            extra = format!("{extra}   LFS");
                        }
                        let submodule = submodules.iter().find(|m| m.path == e.path);
                        if let Some(m) = submodule {
                            extra = format!("{extra}   {}  {}", tr("submodule"), crate::branches_view::submodule_note(m));
                        }
                        ui.painter().text(pos2(x + 28.0 + nw, cy), egui::Align2::LEFT_CENTER, extra.trim(), FontId::proportional(12.0), theme::TEXT_DIM());
                        if g == Group::Ignored {
                            let entry = e.clone();
                            resp.context_menu(|ui| {
                                if ui.button(tr("Copy Path")).clicked() {
                                    ui.ctx().copy_text(entry.path.clone());
                                }
                            });
                            continue;
                        }
                        if resp.clicked() {
                            let p = e.path.clone();
                            if mods.shift {
                                let anchor = selected.clone().unwrap_or_else(|| p.clone());
                                let a = order.iter().position(|x| *x == anchor).unwrap_or(0);
                                let b = order.iter().position(|x| *x == p).unwrap_or(0);
                                let mut set: HashSet<String> = if mods.command { chosen.clone() } else { HashSet::new() };
                                set.extend(order[a.min(b)..=a.max(b)].iter().cloned());
                                // Keep the anchor where it was so the range can grow or shrink.
                                new_sel = Some((set, anchor));
                            } else if mods.command {
                                let mut set = chosen.clone();
                                if !set.remove(&p) {
                                    set.insert(p.clone());
                                }
                                new_sel = Some((set, p));
                            } else {
                                select = Some(p);
                            }
                        }
                        if resp.secondary_clicked() && !chosen.contains(&e.path) {
                            // Right-click outside the selection selects just that file.
                            select = Some(e.path.clone());
                        }
                        if resp.double_clicked() {
                            if g == Group::Conflicts {
                                merge = Some((e.path.clone(), false));
                            } else {
                                diff = Some(e.path.clone());
                            }
                        }
                        let entry = e.clone();
                        let group = g;
                        // The menu acts on every selected file of the same group (or just this one).
                        let targets: Vec<StatusEntry> = if chosen.contains(&e.path) {
                            order
                                .iter()
                                .filter(|p| chosen.contains(*p))
                                .filter_map(|p| entries.get(p))
                                .filter(|(_, gg)| *gg == group)
                                .map(|(x, _)| x.clone())
                                .collect()
                        } else {
                            vec![entry.clone()]
                        };
                        let paths: Vec<String> = targets.iter().map(|t| t.path.clone()).collect();
                        let n = paths.len();
                        let many = |t: &str| if n > 1 { format!("{t} ({n} files)") } else { t.to_string() };
                        resp.context_menu(|ui| {
                            if ui.button(tr("Show Diff")).clicked() {
                                diff = Some(entry.path.clone());
                            }
                            if let Some(m) = submodule {
                                // A folder with a repository of its own, not a file: the file
                                // actions (history of lines, annotate, rollback) don't apply.
                                ui.separator();
                                if ui.button(tr("Open Submodule")).on_hover_text(tr("Open the submodule as a repository of its own")).clicked() {
                                    sub_open = Some(m.clone());
                                }
                                if ui.add_enabled(m.state != crate::submodule::State::Same, egui::Button::new(tr("Update Submodule"))).on_hover_text(tr("Check out the commit the repository has recorded for it (git submodule update)")).on_disabled_hover_text(tr("The recorded commit is already checked out")).clicked() {
                                    sub_update = Some(m.clone());
                                }
                                ui.separator();
                                if ui.button(tr("Copy Path")).clicked() {
                                    ui.ctx().copy_text(entry.path.clone());
                                    ui.close();
                                }
                                return;
                            }
                            let local = crate::chrome::repo_file(&root, &entry.path);
                            if ui.add_enabled(local.is_file(), egui::Button::new(tr("Open in Default Application"))).clicked() {
                                open_local = Some(local.clone());
                            }
                            if ui.add_enabled(!entry.is_untracked(), egui::Button::new(tr("Show History"))).on_hover_text(tr("The commits that changed this file, followed through its renames")).clicked() {
                                history = Some(entry.path.clone());
                            }
                            crate::dialogs::compare_file_items(ui, &entry.path, !entry.is_untracked());
                            if ui.add_enabled(!entry.is_untracked() && local.is_file(), egui::Button::new(tr("Annotate"))).on_hover_text(tr("Who last changed each line of the file, and in which commit")).clicked() {
                                annotate = Some(entry.path.clone());
                            }
                            if !entry.is_untracked() {
                                crate::hosting_view::file_item(ui, &host_file, &entry.path);
                            }
                            if group != Group::Conflicts && !lfs_paths.contains(&entry.path) {
                                ui.menu_button(tr("Track with LFS"), |ui| {
                                    for pat in crate::lfs::track_choices(&entry.path) {
                                        if ui.button(&pat).on_hover_text(tr("Keep these files in Git LFS from now on: the line is added to .gitattributes (git lfs track)")).clicked() {
                                            lfs_track = Some(pat);
                                            ui.close();
                                        }
                                    }
                                });
                            }
                            if group == Group::Conflicts {
                                ui.separator();
                                if ui.button(tr("Merge…")).clicked() {
                                    merge = Some((entry.path.clone(), false));
                                }
                                if ui.button(tr("Resolve with AI")).clicked() {
                                    merge = Some((entry.path.clone(), true));
                                }
                                let side = |flag: &str| -> Vec<Vec<String>> {
                                    let mut c = vec!["checkout".to_string(), flag.into(), "--".into()];
                                    c.extend(paths.iter().cloned());
                                    let mut a = vec!["add".to_string(), "--".into()];
                                    a.extend(paths.iter().cloned());
                                    vec![c, a]
                                };
                                if ui.button(many("Accept Yours")).clicked() {
                                    job = Some(Job::many("Accept yours", side("--ours")));
                                }
                                if ui.button(many("Accept Theirs")).clicked() {
                                    job = Some(Job::many("Accept theirs", side("--theirs")));
                                }
                                if ui.button(many("Mark as Resolved")).clicked() {
                                    let mut a = vec!["add".to_string(), "--".into()];
                                    a.extend(paths.iter().cloned());
                                    job = Some(Job::many("Mark resolved", vec![a]));
                                }
                            } else if group == Group::Unversioned {
                                if ui.add(egui::Button::new(many("Add to Git")).shortcut_text(crate::keymap::shown("Ctrl+Alt+A"))).clicked() {
                                    let mut a = vec!["add".to_string(), "--".into()];
                                    a.extend(paths.iter().cloned());
                                    job = Some(Job::many(if n > 1 { "Add files" } else { "Add file" }, vec![a]));
                                }
                                use crate::ignore::{What, pattern};
                                for (exclude, title, tip) in [
                                    (false, "Add to .gitignore", "Hide from the Changes list for everyone: the line is added to .gitignore, which is committed"),
                                    (true, "Add to .git/info/exclude", "Hide from the Changes list on this computer only; nothing to commit"),
                                ] {
                                    ui.menu_button(tr(title), |ui| {
                                        let all = |w: What| -> Vec<String> {
                                            let mut v: Vec<String> = paths.iter().filter_map(|p| pattern(p, w)).collect();
                                            v.dedup();
                                            v
                                        };
                                        let files = all(What::Path);
                                        let first = files.first().cloned().unwrap_or_default();
                                        let label = if files.len() > 1 { format!("{first}  (+{})", files.len() - 1) } else { first };
                                        if ui.button(label).on_hover_text(tr(tip)).clicked() {
                                            ignore = Some((exclude, files));
                                        }
                                        let mut dirs = all(What::Folder);
                                        dirs.sort();
                                        dirs.dedup();
                                        for d in dirs {
                                            if ui.button(&d).on_hover_text(tr("The whole folder")).clicked() {
                                                ignore = Some((exclude, vec![d.clone()]));
                                            }
                                        }
                                        let mut exts = all(What::Extension);
                                        exts.sort();
                                        exts.dedup();
                                        for x in exts {
                                            if ui.button(&x).on_hover_text(tr("Every file of this type, in any folder")).clicked() {
                                                ignore = Some((exclude, vec![x.clone()]));
                                            }
                                        }
                                    });
                                }
                                if ui.button(many("Delete…")).on_hover_text(tr("Delete the file from the disk")).clicked() {
                                    delete = Some(paths.clone());
                                }
                            } else {
                                if ui.button(many("Rollback…")).clicked() {
                                    rollback = Some(targets.clone());
                                }
                                if ui.button(many("Stop Tracking")).on_hover_text(tr("Remove the file from Git but keep it on the disk (git rm --cached); add it to .gitignore afterwards to keep it out")).clicked() {
                                    let mut a = vec!["rm".to_string(), "--cached".into(), "-r".into(), "-q".into(), "--".into()];
                                    a.extend(paths.iter().cloned());
                                    job = Some(Job::many("Stop tracking", vec![a]));
                                }
                            }
                            if group != Group::Conflicts {
                                ui.separator();
                                if ui.button(many("Include in Commit")).clicked() {
                                    toggle.extend(paths.iter().map(|p| (p.clone(), true)));
                                }
                                if ui.button(many("Exclude from Commit")).clicked() {
                                    toggle.extend(paths.iter().map(|p| (p.clone(), false)));
                                }
                            }
                            ui.separator();
                            if group != Group::Conflicts && group != Group::Ignored && ui.button(many("Create Patch…")).clicked() {
                                patch_of = Some(paths.clone());
                            }
                            if ui.button(if n > 1 { format!("Copy Paths ({n})") } else { "Copy Path".into() }).clicked() {
                                ui.ctx().copy_text(paths.join("\n"));
                            }
                        });
                    }
                }
            }
            ui.add_space(4.0);
        });
        if let Some(k) = fold {
            if !self.changes_collapsed.remove(&k) {
                self.changes_collapsed.insert(k);
            }
        }
        if let Some(repo) = self.repo.as_mut() {
            for (p, on) in toggle {
                repo.partial.remove(&p);
                if on {
                    repo.unchecked.remove(&p);
                } else {
                    repo.unchecked.insert(p);
                }
            }
            if let Some(s) = select {
                if !s.ends_with('/') && repo.status_selected.as_ref() != Some(&s) {
                    follow = Some(s.clone());
                }
                repo.status_selected = Some(s);
                repo.status_multi.clear();
            }
            if let Some((set, anchor)) = new_sel {
                repo.status_selected = Some(anchor);
                repo.status_multi = set;
            }
            // Forget files that are no longer listed.
            repo.status_multi.retain(|p| entries.contains_key(p));
        }
        if let Some(f) = open_local {
            self.open_file_at(&f, None);
        }
        if let Some(p) = history {
            self.open_file_history(&p, None);
        }
        if let Some(p) = lfs_track {
            self.run_lfs("LFS track", crate::lfs::track_args(&p));
        }
        if let Some(m) = sub_open {
            self.open_submodule(&m);
        }
        if let Some(m) = sub_update {
            self.confirm_submodule_update(Some(&m));
        }
        if let Some(p) = annotate {
            self.open_blame(&p, None, None);
        }
        if let Some(p) = diff {
            if let Some(r) = self.repo.as_mut() {
                r.status_selected = Some(p.clone());
                if !r.status_multi.contains(&p) {
                    r.status_multi.clear();
                }
            }
            self.open_worktree_diff_as(&p, crate::app::OpenDiff::DoubleClick);
        } else if let Some(p) = follow {
            // A click on another file shows it in the quick-view diff tab, if one is open.
            self.open_worktree_diff_as(&p, crate::app::OpenDiff::Follow);
        }
        if let Some(j) = job {
            self.run_job(j);
        }
        if let Some((p, ai)) = merge {
            if ai {
                self.resolve_with_ai(&p, true);
            } else if !self.try_external_merge(&p) {
                // No external merge tool for this type of file: the built-in merge window.
                self.open_merge(&p, true);
            }
        }
        if resolve_all {
            self.show_conflicts = true;
        }
        if let Some(entries) = rollback {
            self.rollback_dialog(entries);
        }
        if let Some(p) = patch_of {
            self.open_create_patch(p);
        }
        if let Some((exclude, lines)) = ignore {
            self.add_to_ignore(exclude, lines);
        }
        if let Some(paths) = delete {
            let n = paths.len();
            let what = if n == 1 { format!("'{}'", paths[0]) } else { format!("{n} {}", crate::i18n::plural(n, "file", "files")) };
            let mut a = vec!["clean".to_string(), "-f".into(), "-d".into(), "-q".into(), "--".into()];
            a.extend(paths);
            self.dialog = Some(Dialog::Confirm {
                title: tr("Delete").into(),
                text: crate::i18n::trf("Delete {} from the disk? The file is not in Git, so it cannot be brought back.", &[&what]),
                ok: tr("Delete").into(),
                job: Some(Job::many("Delete files", vec![a])),
            });
        }
    }

    fn confirm_rollback(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        let entries: Vec<StatusEntry> = repo
            .status
            .iter()
            .filter(|e| !e.is_untracked() && !e.is_conflict() && !repo.unchecked.contains(&e.path))
            .cloned()
            .collect();
        if entries.is_empty() {
            self.dialog = Some(Dialog::Message {
                title: tr("Rollback").into(),
                text: tr("Check the changed files you want to roll back first.").into(),
            });
            return;
        }
        self.rollback_dialog(entries);
    }

    fn rollback_dialog(&mut self, entries: Vec<StatusEntry>) {
        let mut cmds = vec![];
        let added: Vec<String> = entries.iter().filter(|e| e.x == 'A').map(|e| e.path.clone()).collect();
        let tracked: Vec<String> = entries
            .iter()
            .filter(|e| e.x != 'A')
            .flat_map(|e| std::iter::once(e.path.clone()).chain(e.old_path.clone()))
            .collect();
        if !added.is_empty() {
            let mut c = vec!["rm".to_string(), "--cached".into(), "-q".into(), "--".into()];
            c.extend(added);
            cmds.push(c);
        }
        if !tracked.is_empty() {
            let mut c = vec!["restore".to_string(), "--source=HEAD".into(), "--staged".into(), "--worktree".into(), "--".into()];
            c.extend(tracked);
            cmds.push(c);
        }
        // A submodule's folder is put back by checking out its recorded commit there; changes
        // of its own inside it are left alone.
        let subs: Vec<String> = self.repo.as_ref().map(|r| entries.iter().filter(|e| r.submodules.iter().any(|m| m.path == e.path)).map(|e| e.path.clone()).collect()).unwrap_or_default();
        if !subs.is_empty() {
            let mut c = vec!["submodule".to_string(), "update".into(), "--init".into(), "--".into()];
            c.extend(subs);
            cmds.push(c);
        }
        let n = entries.len();
        self.dialog = Some(Dialog::Confirm {
            title: tr("Rollback Changes").into(),
            text: format!(
                "Discard local changes in {n} file{}? This cannot be undone.",
                if n == 1 { "" } else { "s" }
            ),
            ok: "Rollback".into(),
            job: Some(Job::many("Rollback", cmds)),
        });
    }

    fn message_area(&mut self, ui: &mut Ui) {
        let focus = std::mem::take(&mut self.focus_message);
        let path = self.repo.as_ref().map(|r| r.path.clone());
        let Some(repo) = self.repo.as_mut() else { return };
        let checked: Vec<StatusEntry> = repo
            .status
            .iter()
            .filter(|e| !e.is_conflict() && !repo.unchecked.contains(&e.path))
            .cloned()
            .collect();
        let conflicts = repo.status.iter().any(|e| e.is_conflict());
        let op = repo.state.operation;
        let partial_sel = repo.partial.clone();
        let has_head = !repo.state.head_hash.is_empty();
        let ai_name = ai::harness(&self.settings.agent).map(|h| h.name);
        let ai_ready = self.agents.iter().any(|(id, _)| *id == self.settings.agent);
        let mut want_ai = false;
        let mut want_conflicts = false;
        ui.horizontal(|ui| {
            let was = repo.amend;
            ui.checkbox(&mut repo.amend, tr("Amend"));
            // Options of the commit: sign-off, signature, hooks, author, date.
            let on = self.settings.commit_signoff as usize + self.settings.commit_sign as usize + self.commit_opts.count();
            let label = if on > 0 { format!("{} ({on})", tr("Options")) } else { tr("Options").to_string() };
            let opts = ui.add(egui::Button::new(RichText::new(label).color(if on > 0 { theme::LINK() } else { theme::TEXT() })).frame(false)).on_hover_text(tr("Sign-off, signature, skipping hooks, another author or date"));
            egui::Popup::menu(&opts).width(330.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_width(320.0);
                let before = (self.settings.commit_signoff, self.settings.commit_sign);
                ui.checkbox(&mut self.settings.commit_signoff, tr("Sign-off")).on_hover_text(tr("Add a Signed-off-by line with your name (git commit --signoff)"));
                ui.checkbox(&mut self.settings.commit_sign, tr("Sign the commit")).on_hover_text(tr("Sign with your GPG or SSH key (git commit -S); the key is set up in Git Config: user.signingkey"));
                if before != (self.settings.commit_signoff, self.settings.commit_sign) && self.screenshot.is_none() {
                    self.settings.save();
                }
                ui.separator();
                ui.label(RichText::new(tr("For this commit only")).color(theme::TEXT_DIM()).size(11.5));
                ui.checkbox(&mut self.commit_opts.no_verify, tr("Skip the hooks")).on_hover_text(tr("Don't run the pre-commit and commit-msg hooks (git commit --no-verify)"));
                ui.add_space(2.0);
                ui.label(tr("Author:"));
                theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut self.commit_opts.author).hint_text("Name <email@example.com>").desired_width(f32::INFINITY)));
                ui.label(tr("Date:"));
                theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut self.commit_opts.date).hint_text("2026-11-20 14:30").desired_width(f32::INFINITY)));
            });
            if repo.amend && !was && repo.commit_msg.trim().is_empty() {
                if let Some(p) = &path {
                    repo.commit_msg = crate::git::run(p, &["log", "-1", "--format=%B"]).stdout.trim_end().to_string();
                }
            }
            // Commit message history
            let hist = theme::icon_button_sized(ui, Icon::Clock, tr("Commit Message History"), 22.0, false, true);
            if hist.clicked() {
                let mut list = self.settings.messages.clone();
                if let Some(p) = &path {
                    for l in crate::git::run(p, &["log", "-30", "--format=%s"]).stdout.lines() {
                        if !list.iter().any(|m| m.lines().next() == Some(l)) {
                            list.push(l.to_string());
                        }
                    }
                }
                self.msg_history = list;
            }
            egui::Popup::menu(&hist).width(360.0).show(|ui| {
                ui.label(RichText::new(tr("Recent commit messages")).color(theme::TEXT_DIM()).size(11.5));
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for m in &self.msg_history {
                        let first = m.lines().next().unwrap_or("");
                        if ui.button(first).clicked() {
                            repo.commit_msg = m.clone();
                        }
                    }
                });
            });
            // Generate with AI
            let running = self.ai_job.is_some();
            let tip = match (running, ai_name) {
                (true, _) => "Generating… click to stop".to_string(),
                (false, Some(n)) if ai_ready => format!("Generate Commit Message with {n}"),
                _ => "Generate Commit Message — choose an AI agent in Settings first".to_string(),
            };
            let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 5.0, theme::rgb(0x39, 0x3b, 0x40));
            }
            if running {
                egui::Spinner::new().size(14.0).paint_at(ui, r.shrink(4.0));
            } else {
                let col = if ai_ready { theme::ICON() } else { theme::TEXT_DIM() };
                theme::paint_icon(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), Icon::Spiral, col);
            }
            if resp.on_hover_text(tip).clicked() {
                want_ai = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let n = checked.len();
                ui.label(
                    RichText::new(crate::i18n::trf("{} selected", &[&format!("{n} {}", crate::i18n::plural(n, "file", "files"))]))
                        .color(theme::TEXT_DIM())
                        .size(12.0),
                );
            });
        });
        if let Some(e) = &self.ai_error {
            ui.label(RichText::new(e.lines().next().unwrap_or("")).color(theme::RED()).size(12.0));
        }
        let buttons_h = 34.0;
        let h = (ui.available_height() - buttons_h).max(40.0);
        let edit = egui::TextEdit::multiline(&mut repo.commit_msg)
            .id(egui::Id::unique("commit_msg"))
            .hint_text(tr("Commit Message"))
            .margin(egui::Margin::symmetric(8, 6))
            .desired_width(f32::INFINITY)
            .min_size(vec2(0.0, h))
            .font(FontId::proportional(13.0));
        let resp = egui::ScrollArea::vertical()
            .id_salt("msg_scroll")
            .max_height(h)
            .show(ui, |ui| ui.add_sized(vec2(ui.available_width(), h), edit))
            .inner;
        let resp = theme::edit_menu(resp);
        if focus {
            resp.request_focus();
        }
        let ctrl_enter = resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
        let msg = repo.commit_msg.trim().to_string();
        let can = !msg.is_empty() && !conflicts && (!checked.is_empty() || repo.amend || op.is_some());
        let amend = repo.amend;
        let mut go: Option<bool> = None;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let b = egui::Button::new(RichText::new(tr(if amend { "Amend Commit" } else { "Commit" })).color(Color32::WHITE))
                .fill(theme::ACCENT());
            if ui.add_enabled(can, b).clicked() || (can && ctrl_enter) {
                go = Some(false);
            }
            if ui
                .add_enabled(can, egui::Button::new(tr(if amend { "Amend Commit and Push…" } else { "Commit and Push…" })))
                .clicked()
            {
                go = Some(true);
            }
            if conflicts && ui.link(RichText::new(tr("Resolve Conflicts…")).color(theme::RED()).size(12.0)).clicked() {
                want_conflicts = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let gear = theme::icon_button_sized(ui, Icon::Gear, tr("Commit Options"), 22.0, false, true);
                egui::Popup::menu(&gear)
                    .width(380.0)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.set_width(370.0);
                        ui.label(RichText::new(tr("AI Assistant")).strong().color(theme::TEXT()));
                        ui.add_space(4.0);
                        let mut to_settings = false;
                        if crate::ai_view::prompt_editor(ui, &mut self.settings.ai_prompt, Some(&mut to_settings)) {
                            self.settings.save();
                        }
                        if to_settings {
                            self.open_settings(Some(crate::settings_view::Page::Commit));
                        }
                        ui.add_space(4.0);
                        let agent = ai_name.unwrap_or("none");
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Agent: {agent}")).color(theme::TEXT_DIM()).size(12.0));
                            if !ai_ready {
                                ui.label(RichText::new(tr("(not set up)")).color(theme::UNVERSIONED()).size(12.0));
                            }
                        });
                    });
            });
        });
        if let Some(push) = go {
            // Files with changes left out are committed through a separate index (see partial_commit).
            if let Some(dir) = path.as_ref().filter(|_| op.is_none() && checked.iter().any(|e| partial_sel.get(&e.path).is_some_and(|s| !s.excluded.is_empty()))) {
                if let Some(cmds) = partial_commit(dir, &checked, &partial_sel, has_head, &msg, amend) {
                    self.settings.add_message(&msg);
                    self.settings.save();
                    if let Some(r) = self.repo.as_mut() {
                        r.partial.clear();
                    }
                    let mut cmds = cmds;
                    add_commit_flags(&mut cmds, &commit_flags(self.settings.commit_signoff, self.settings.commit_sign, &self.commit_opts));
                    let job = Job::many(if amend { "Amend commit" } else { "Commit" }, cmds).then(if push { Then::OpenPush } else { Then::ClearMessage });
                    self.run_job(job);
                    return;
                }
            }
            let mut cmds = crate::ops::commit_commands(&checked, op, &msg, amend);
            self.settings.add_message(&msg);
            self.settings.save();
            add_commit_flags(&mut cmds, &commit_flags(self.settings.commit_signoff, self.settings.commit_sign, &self.commit_opts));
            let job = Job::many(if amend { "Amend commit" } else { "Commit" }, cmds)
                .then(if push { Then::OpenPush } else { Then::ClearMessage });
            self.run_job(job);
        }
        if want_ai {
            self.start_ai();
        }
        if want_conflicts {
            self.show_conflicts = true;
        }
    }

    fn stash_tab(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG()).inner_margin(10))
            .show(ui, |ui| {
                if ui.button(tr("Stash Changes…")).clicked() {
                    self.open_stash_dialog();
                }
                ui.add_space(8.0);
                let Some(repo) = self.repo.as_ref() else { return };
                if repo.stashes.is_empty() {
                    ui.label(RichText::new(tr("No stashes yet")).color(theme::TEXT_DIM()));
                    return;
                }
                let stashes = repo.stashes.clone();
                // The files are shown for a stash that still exists.
                if self.stash_open.as_ref().is_some_and(|(h, _)| !stashes.iter().any(|s| &s.hash == h)) {
                    self.stash_open = None;
                }
                let open = self.stash_open.clone();
                let mut job = None;
                let mut toggle: Option<String> = None;
                let mut show: Option<(crate::git::Stash, crate::git::StashFile)> = None;
                let mut take: Option<(crate::git::Stash, crate::git::StashFile)> = None;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for s in &stashes {
                        let files = open.as_ref().filter(|(h, _)| *h == s.hash).map(|(_, f)| f);
                        egui::Frame::new()
                            .stroke(Stroke::new(1.0, theme::BORDER()))
                            .corner_radius(6)
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(&s.message).color(theme::TEXT()));
                                ui.label(
                                    RichText::new(format!("{}  ·  {}", s.name, format_date(s.time)))
                                        .color(theme::TEXT_DIM())
                                        .size(12.0),
                                );
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button(tr("Pop")).on_hover_text(tr("Apply and remove the stash")).clicked() {
                                        job = Some(Job::one("Unstash", &["stash", "pop", &s.name]));
                                    }
                                    if ui.button(tr("Apply")).on_hover_text(tr("Apply and keep the stash")).clicked() {
                                        job = Some(Job::one("Apply stash", &["stash", "apply", &s.name]));
                                    }
                                    if ui.button(tr("Drop")).clicked() {
                                        self.dialog = Some(Dialog::Confirm {
                                            title: tr("Drop Stash").into(),
                                            text: format!("Delete '{}'? This cannot be undone.", s.message),
                                            ok: "Drop".into(),
                                            job: Some(Job::one("Drop stash", &["stash", "drop", &s.name])),
                                        });
                                    }
                                    if ui.button(tr("Branch…")).on_hover_text(tr("New branch from this stash: the stash is applied to it and removed")).clicked() {
                                        self.dialog = Some(Dialog::StashBranch { stash: s.hash.clone(), label: s.message.clone(), name: String::new() });
                                    }
                                    let label = if files.is_some() { tr("Hide Files") } else { tr("Show Files") };
                                    if ui.button(label).on_hover_text(tr("What this stash holds: open a file's changes or take a single file")).clicked() {
                                        toggle = Some(s.hash.clone());
                                    }
                                });
                                let Some(files) = files else { return };
                                ui.add_space(4.0);
                                if files.is_empty() {
                                    ui.label(RichText::new(tr("This stash holds no changes")).color(theme::TEXT_DIM()).size(12.0));
                                }
                                for f in files {
                                    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                                    if resp.hovered() {
                                        ui.painter().rect_filled(row, 3.0, theme::HOVER());
                                    }
                                    let letter = if f.untracked { '?' } else { f.file.status };
                                    let color = crate::app::status_color(letter);
                                    let font = FontId::proportional(13.0);
                                    ui.painter().text(pos2(row.left() + 10.0, row.center().y), egui::Align2::CENTER_CENTER, letter, FontId::monospace(12.0), color);
                                    let name = file_name(&f.file.path);
                                    theme::paint_clipped(ui.painter(), pos2(row.left() + 22.0, row.center().y), &name, font.clone(), color, row.width() - 30.0);
                                    let w = theme::text_width(ui, &name, &font);
                                    let dir = f.file.path.strip_suffix(name.as_str()).unwrap_or("").trim_end_matches('/');
                                    if !dir.is_empty() {
                                        theme::paint_clipped(ui.painter(), pos2(row.left() + 30.0 + w, row.center().y), dir, FontId::proportional(12.0), theme::TEXT_DIM(), (row.width() - 40.0 - w).max(0.0));
                                    }
                                    let can_take = f.file.status != 'D';
                                    if resp.clicked() {
                                        show = Some((s.clone(), f.clone()));
                                    }
                                    resp.on_hover_text(&f.file.path).context_menu(|ui| {
                                        if ui.button(tr("Show Diff")).clicked() {
                                            show = Some((s.clone(), f.clone()));
                                            ui.close();
                                        }
                                        if ui.add_enabled(can_take, egui::Button::new(tr("Take This File"))).on_hover_text(tr("Put the file into the working folder as it is in the stash; the stash stays")).clicked() {
                                            take = Some((s.clone(), f.clone()));
                                            ui.close();
                                        }
                                    });
                                }
                            });
                        ui.add_space(6.0);
                    }
                });
                if let Some(h) = toggle {
                    self.toggle_stash_files(&h);
                }
                if let Some((s, f)) = show {
                    self.open_stash_file(&s, &f);
                }
                if let Some((s, f)) = take {
                    self.take_stash_file(&s, &f);
                }
                if let Some(j) = job {
                    self.run_job(j);
                }
            });
    }

    // ------------------------------------------------------------ diff viewer

    pub fn diff_tab(&mut self, ui: &mut Ui, i: usize) {
        // The white-space mode chosen in the toolbar on the last frame.
        let ws: Option<u8> = ui.ctx().data_mut(|x| x.remove_temp(egui::Id::unique("set_diff_ws")));
        if let Some(mode) = ws {
            self.set_diff_ws(mode);
        }
        if i >= self.diffs.len() {
            self.tab = Tab::Log;
            return;
        }
        if self.diffs[i].diff.images.is_some() {
            crate::image_view::image_tab(ui, &mut self.diffs[i]);
            return;
        }
        // "Annotate with Git Blame": a column with the commit of each line of the new side.
        let can_annotate = self.diff_blame_tick(i);
        let ann_on = self.diff_annotate;
        let ann_w = if ann_on && can_annotate { crate::blame_view::DIFF_BLAME_W } else { 0.0 };
        let mut ann_toggle = false;
        let mut blame_click: Option<String> = None;
        let offset_id = egui::Id::unique(("diff_off", i));
        let cur = ui.ctx().data(|d| d.get_temp::<f32>(offset_id)).unwrap_or(0.0);
        // Local changes: each change can be left out of the commit or reverted with ».
        let worktree = self.diffs[i].worktree.clone();
        let (mut sel, file_on) = match (&worktree, self.repo.as_ref()) {
            (Some(p), Some(r)) => (r.partial.get(p).cloned().unwrap_or_default(), !r.unchecked.contains(p)),
            _ => (PartialSel::default(), true),
        };
        let blocks = crate::partial::blocks(&self.diffs[i].diff.rows);
        let block_at: HashMap<usize, usize> = blocks.iter().enumerate().map(|(k, b)| (b.first_row, k)).collect();
        let included = |b: &crate::partial::Block, sel: &PartialSel| file_on && !sel.excluded.contains(&b.hash);
        let n_included = blocks.iter().filter(|b| included(b, &sel)).count();
        let mut toggle: Option<usize> = None;
        let mut revert: Option<usize> = None;
        // "Open in an external diff tool" at the right end of the toolbar, when tools are on.
        let source = self.diffs[i].source.clone();
        let ext_tools: Vec<crate::settings::ExtTool> = if self.settings.ext_enabled && source.is_some() {
            self.settings.ext_tools.iter().filter(|t| t.kind == crate::settings::ExtKind::Diff).cloned().collect()
        } else {
            vec![]
        };
        let ext_default = source
            .as_ref()
            .and_then(|s| crate::external::tool_for(&self.settings, s.path(), crate::settings::ExtKind::Diff).cloned())
            .or_else(|| ext_tools.first().cloned());
        let mut ext_open: Option<crate::settings::ExtTool> = None;
        // The arrow keys move the text cursor here when the diff's text was clicked, or when
        // this tab is in front and no list beside it has taken them.
        let on_top = self.dialog.is_none() && !self.show_settings && self.merge_open.is_none();
        let keys_ok = on_top && (self.key_focus == crate::app::KeyFocus::Text || (self.key_focus == crate::app::KeyFocus::Log && self.tab == crate::app::Tab::Diff(i)));
        let mut focus_text = false;
        let scroll_id = egui::Id::unique(("diff_scroll_to", i));
        let d = &mut self.diffs[i];
        let side = d.side_by_side;
        // Search (Ctrl+F): the places to mark on each line, and the one to bring into view.
        let mut find_marks: HashMap<(u8, usize), Vec<(usize, usize, bool)>> = HashMap::new();
        let mut find_count = (0usize, 0usize);
        let mut find_go: Option<(usize, usize)> = None;
        if let Some(f) = d.find.as_mut() {
            let hits = f.hits(&d.diff, side).to_vec();
            if std::mem::take(&mut f.go) {
                if let Some(h) = hits.get(f.current) {
                    // A hit among folded lines: show all the lines.
                    if side && d.collapse && !fold_rows(&d.diff.rows, 3, &d.expanded).contains(&Disp::Row(h.line)) {
                        d.collapse = false;
                    }
                    find_go = Some((h.line, h.from));
                }
            }
            for (n, h) in hits.iter().enumerate() {
                find_marks.entry((h.side, h.line)).or_default().push((h.from, h.to, n == f.current));
            }
            find_count = (if hits.is_empty() { 0 } else { f.current + 1 }, hits.len());
        }
        let disp: Vec<Disp> = if !side {
            vec![]
        } else if d.collapse {
            fold_rows(&d.diff.rows, 3, &d.expanded)
        } else {
            (0..d.diff.rows.len()).map(Disp::Row).collect()
        };
        if let Some((line, _)) = find_go {
            d.jump = Some(if side { disp.iter().position(|x| *x == Disp::Row(line)).unwrap_or(0) } else { line });
        }
        // Where each change starts, in shown rows.
        let starts: Vec<usize> = blocks
            .iter()
            .map(|b| if side { disp.iter().position(|x| *x == Disp::Row(b.first_row)).unwrap_or(0) } else { b.first_row })
            .collect();
        egui::Panel::top("diff_bar")
            .exact_size(34.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG()).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let top_row = ((cur + 60.0) / DIFF_ROW) as usize;
                    if theme::icon_button_sized(ui, Icon::ChevronDown, tr("Next difference"), 24.0, false, true).clicked() {
                        d.jump = starts.iter().copied().find(|&s| s > top_row + 1).or(starts.first().copied());
                    }
                    if theme::icon_button_sized(ui, Icon::Expand, tr("Previous difference"), 24.0, false, true).clicked() {
                        d.jump = starts.iter().copied().rev().find(|&s| s + 1 < top_row).or(starts.last().copied());
                    }
                    ui.add_space(8.0);
                    ui.selectable_value(&mut d.side_by_side, true, tr("Side-by-side"));
                    ui.selectable_value(&mut d.side_by_side, false, tr("Unified"));
                    // White space: every change, or without the changes that only move it.
                    const WS: [(&str, &str); 3] = [
                        ("Show white-space changes", "Every change counts"),
                        ("Ignore changes in the amount of white space", "Lines that differ only in how much white space they have are shown as unchanged (git diff -b)"),
                        ("Ignore all white space", "Lines that differ only in white space are shown as unchanged (git diff -w)"),
                    ];
                    let ws = crate::git::diff_ws();
                    let label = if ws == 0 { tr("White space").to_string() } else { format!("{}: {}", tr("White space"), if ws == 1 { "-b" } else { "-w" }) };
                    let b = ui.add(egui::Button::new(RichText::new(label).color(if ws == 0 { theme::TEXT_MID() } else { theme::LINK() })).frame(false)).on_hover_text(tr("Leave changes of white space out of the diff — to see what a reformatted file really changes"));
                    egui::Popup::menu(&b).show(|ui| {
                        for (k, (name, tip)) in WS.iter().enumerate() {
                            if ui.radio(ws as usize == k, tr(name)).on_hover_text(tr(tip)).clicked() {
                                ui.ctx().data_mut(|x| x.insert_temp(egui::Id::unique("set_diff_ws"), k as u8));
                                ui.close();
                            }
                        }
                    });
                    ui.add_space(4.0);
                    let tip = if d.collapse { "Show All Unchanged Lines" } else { "Collapse Unchanged Fragments" };
                    if theme::icon_button_sized(ui, Icon::Collapse, tip, 24.0, d.collapse && side, side).clicked() {
                        d.collapse = !d.collapse;
                        d.expanded.clear();
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new(&d.subtitle).color(theme::TEXT_MID()));
                    let n = blocks.len();
                    let mut t = format!("{n} difference{}", if n == 1 { "" } else { "s" });
                    if worktree.is_some() {
                        t.push_str(&format!(", {n_included} included"));
                    }
                    ui.label(RichText::new(t).color(theme::TEXT_DIM()).size(12.0));
                    if let Some(def) = &ext_default {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            if ext_tools.len() > 1 {
                                let more = theme::icon_button_sized(ui, Icon::ChevronDown, tr("Choose the external tool"), 16.0, false, true);
                                egui::Popup::menu(&more).show(|ui| {
                                    ui.set_min_width(180.0);
                                    for t in &ext_tools {
                                        if ui.button(format!("Open in {}", t.name)).clicked() {
                                            ext_open = Some(t.clone());
                                        }
                                    }
                                });
                            }
                            if theme::icon_button_sized(ui, Icon::External, &format!("Open in External Tool ({})", def.name), 24.0, false, true).clicked() {
                                ext_open = Some(def.clone());
                            }
                        });
                    }
                });
            });
        // The search line under the toolbar.
        let mut find_close = false;
        if let Some(f) = d.find.as_mut() {
            egui::Panel::top("diff_find")
                .exact_size(34.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::BG()).inner_margin(egui::Margin::symmetric(10, 4)))
                .show(ui, |ui| {
                    let r = ui.max_rect().expand2(vec2(10.0, 4.0));
                    theme::hline(ui, r.top() + 0.5, r.left(), r.right());
                    ui.horizontal_centered(|ui| find_close = find_line(ui, f, "Search in the diff", find_count));
                });
        }
        if find_close {
            d.find = None;
        }
        if let (Some(t), Some(src)) = (ext_open, source.as_ref()) {
            self.open_in_external(src, &t);
        }
        // The file as it is now in the working folder, for "Open in Default Application".
        let open_file = match (self.repo.as_ref(), source.as_ref()) {
            (Some(r), Some(s)) => Some(crate::chrome::repo_file(&r.path, s.path())).filter(|p| p.is_file()),
            _ => None,
        };
        let mut open_req: Option<(std::path::PathBuf, Option<usize>)> = None;
        let history_path: Option<String> = source.as_ref().map(|s| s.path().to_string());
        let mut history_req = false;
        let mut history_lines: Option<(usize, usize)> = None;
        let syntax_on = !self.settings.syntax_off;
        let d = &mut self.diffs[i];
        // Syntax colours: worked out once for the diff, the first time it is drawn.
        if syntax_on && d.syntax.is_none() {
            let path = source.as_ref().map(|s| s.path().to_string()).unwrap_or_else(|| d.title.clone());
            d.syntax = Some(crate::syntax::for_diff(&path, &d.diff));
        }
        let syntax = d.syntax.take().filter(|_| syntax_on);
        let blame_st = d.blame.take();
        let mut open_fold: Option<usize> = None;
        let mut find_open = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG()))
            .show(ui, |ui| {
                let r = ui.max_rect();
                theme::hline(ui, r.top() + 0.5, r.left(), r.right());
                if d.diff.binary {
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, tr("Binary files differ"), FontId::proportional(13.0), theme::TEXT_MID());
                    return;
                }
                if d.diff.rows.is_empty() {
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, tr("No differences"), FontId::proportional(13.0), theme::TEXT_MID());
                    return;
                }
                let mut sa = egui::ScrollArea::vertical().id_salt(("diff", i)).auto_shrink([false, false]);
                if let Some(j) = d.jump.take() {
                    sa = sa.vertical_scroll_offset((j as f32 * DIFF_ROW - 80.0).max(0.0));
                }
                // The text cursor went out of sight on the last frame: just far enough to see it.
                if let Some(off) = ui.ctx().data_mut(|dd| dd.remove_temp::<f32>(scroll_id)) {
                    sa = sa.vertical_scroll_offset(off.max(0.0));
                }
                let mono = FontId::monospace(12.5);
                let rows = &d.diff.rows;
                let unified = &d.diff.unified;
                let n = if side { disp.len() } else { unified.len() };
                // Room on the right side for the "include in commit" boxes.
                let gutter = if worktree.is_some() { 20.0 } else { 0.0 };
                // How wide the text gets on each side, for the horizontal scrolling.
                let (cw_l, cw_r) = if side {
                    (
                        50.0 + Self::widest(ui, &mono, rows.iter().filter_map(|r| r.left.as_ref().map(|(_, t)| t.as_str()))) + 30.0,
                        gutter + ann_w + 50.0 + Self::widest(ui, &mono, rows.iter().filter_map(|r| r.right.as_ref().map(|(_, t)| t.as_str()))) + 30.0,
                    )
                } else {
                    let w = ann_w + 10.0 + theme::text_width(ui, "+ ", &mono) + Self::widest(ui, &mono, unified.iter().map(|(_, t)| t.as_str())) + 30.0;
                    (w, 0.0)
                };
                let split = d.split;
                let mut hscroll = d.hscroll;
                let full = Rect::from_min_max(pos2(r.left(), r.top() + 1.0), r.max);
                // Room for the vertical scrollbar on the right.
                let est_w = full.width() - 12.0;
                let overflow = if side { cw_l > est_w * split || cw_r > est_w * (1.0 - split) } else { cw_l > est_w };
                const HBAR: f32 = 12.0;
                let area = if overflow { Rect::from_min_max(full.min, pos2(full.right(), full.bottom() - HBAR)) } else { full };
                // Shift+wheel or a sideways swipe scrolls the text to the right / left.
                if ui.rect_contains_pointer(area) {
                    let dx = ui.input(|inp| inp.smooth_scroll_delta.x);
                    if dx != 0.0 {
                        hscroll -= dx;
                    }
                }
                let mut row_w = est_w;
                let mut au = ui.new_child(egui::UiBuilder::new().max_rect(area));
                au.set_clip_rect(area);
                au.spacing_mut().item_spacing.y = 0.0;
                // Selecting text: drag over it (Shift+click extends, double-click takes a word).
                // The area senses drags under the rows, so the » and include boxes stay on top.
                let sa = sa.scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() });
                let sel_resp = au.interact(area, egui::Id::unique(("diff_textsel", i)), Sense::click_and_drag());
                let cw = theme::text_width(ui, "MMMMMMMMMM", &mono) / 10.0;
                // Bring the found text into view sideways too.
                if let Some((_, col)) = find_go {
                    let x = col as f32 * cw;
                    let vis = if side { est_w * split.min(1.0 - split) } else { est_w } - 140.0;
                    if x < hscroll || x > hscroll + vis.max(60.0) {
                        hscroll = (x - 80.0).max(0.0);
                    }
                }
                let tsel = d.text_sel;
                // The text cursor is shown while the arrow keys are this diff's.
                let show_caret = keys_ok;
                let ptr = ui.input(|inp| inp.pointer.interact_pos());
                let (mut first_row, mut last_row, mut hit_row): (Option<(usize, Rect)>, Option<usize>, Option<usize>) = (None, None, None);
                // Where the text of each side starts (left, right, unified), and the divider.
                let mut origins = [0.0f32; 3];
                let mut mid_seen = area.center().x;
                let out = sa.show_rows(&mut au, DIFF_ROW, n, |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let w = ui.available_width();
                    row_w = w;
                    let (vis_l, vis_r) = if side { (w * split, w * (1.0 - split)) } else { (w, 0.0) };
                    let hs_l = hscroll.min((cw_l - vis_l).max(0.0));
                    let hs_r = hscroll.min((cw_r - vis_r).max(0.0));
                    for k in range {
                        let (rect, _) = ui.allocate_exact_size(vec2(w, DIFF_ROW), Sense::hover());
                        let p = ui.painter_at(rect);
                        let cy = rect.center().y;
                        if ptr.is_some_and(|pp| pp.y >= rect.top() && pp.y < rect.bottom()) {
                            hit_row = Some(k);
                        }
                        if first_row.is_none() {
                            first_row = Some((k, rect));
                        }
                        last_row = Some(k);
                        if side {
                            let half_l = (rect.width() * split).round();
                            origins[0] = rect.left() + 50.0 - hs_l;
                            origins[1] = rect.left() + half_l + gutter + ann_w + 50.0 - hs_r;
                            mid_seen = rect.left() + half_l;
                            let k = match disp[k] {
                                Disp::Row(k) => k,
                                Disp::Fold { from, to } => {
                                    // Hidden unchanged lines: a wavy line; click to show them.
                                    let resp = ui.interact(rect, ui.scope_id().with(("fold", from)), Sense::click());
                                    let col = if resp.hovered() { theme::TEXT_MID() } else { theme::TEXT_DIM() };
                                    let mut pts = vec![];
                                    let mut x = rect.left();
                                    let mut up = true;
                                    while x <= rect.right() {
                                        pts.push(pos2(x, cy + if up { -2.0 } else { 2.0 }));
                                        x += 6.0;
                                        up = !up;
                                    }
                                    p.add(egui::Shape::line(pts, Stroke::new(1.0, col.gamma_multiply(0.6))));
                                    let label = format!("  {} unchanged line{}  ", to - from, if to - from == 1 { "" } else { "s" });
                                    let g = p.layout_no_wrap(label, FontId::proportional(11.5), col);
                                    let lr = Rect::from_center_size(rect.center(), g.size() + vec2(4.0, 0.0));
                                    p.rect_filled(lr, 3.0, theme::BG());
                                    p.galley(lr.min + vec2(2.0, 0.0), g, col);
                                    if resp.on_hover_text(tr("Click to show these lines")).clicked() {
                                        open_fold = Some(from);
                                    }
                                    continue;
                                }
                            };
                            let row = &rows[k];
                            let (lbg, rbg) = match row.kind {
                                DiffSide::Context => (None, None),
                                DiffSide::Removed => (Some(theme::adapt(C_REMOVED)), None),
                                DiffSide::Added => (None, Some(theme::adapt(C_ADDED))),
                                DiffSide::Changed => (Some(theme::adapt(C_CHANGED)), Some(theme::adapt(C_CHANGED))),
                            };
                            if row.left.is_none() && row.right.is_none() {
                                p.rect_filled(rect, 0.0, theme::BG_BAR());
                                p.text(rect.center(), egui::Align2::CENTER_CENTER, "⋯", mono.clone(), theme::TEXT_DIM());
                                continue;
                            }
                            // Changed line: colour the words that differ on each side.
                            let words = match (&row.kind, &row.left, &row.right) {
                                (DiffSide::Changed, Some((_, l)), Some((_, r))) => crate::wordiff::changed_ranges(&l.replace('\t', "    "), &r.replace('\t', "    ")),
                                _ => (vec![], vec![]),
                            };
                            for (side_i, (cell, bg)) in [(&row.left, lbg), (&row.right, rbg)].into_iter().enumerate() {
                                let x0 = if side_i == 0 { rect.left() } else { rect.left() + half_l };
                                let x1 = if side_i == 0 { rect.left() + half_l } else { rect.right() };
                                let cell_rect = Rect::from_min_max(pos2(x0, rect.top()), pos2(x1, rect.bottom()));
                                let cp = ui.painter_at(cell_rect.shrink2(vec2(1.0, 0.0)));
                                if let Some(bg) = bg {
                                    if cell.is_some() {
                                        cp.rect_filled(cell_rect, 0.0, bg);
                                    } else {
                                        cp.rect_filled(cell_rect, 0.0, bg.gamma_multiply(0.35));
                                    }
                                }
                                if side_i == 1 && ann_w > 0.0 {
                                    let br = Rect::from_min_max(pos2(x0 + gutter, rect.top()), pos2(x0 + gutter + ann_w, rect.bottom()));
                                    let line = cell.as_ref().map(|(n, _)| *n);
                                    if let Some(h) = crate::blame_view::diff_blame_cell(ui, br, blame_st.as_ref().map(|(_, st)| st), line, ui.scope_id().with(("blame_cell", k))) {
                                        blame_click = Some(h);
                                    }
                                }
                                let g = if side_i == 1 { gutter + ann_w } else { 0.0 };
                                if let Some((ln, text)) = cell {
                                    cp.text(pos2(x0 + g + 40.0, cy), egui::Align2::RIGHT_CENTER, ln.to_string(), mono.clone(), theme::TEXT_DIM());
                                    // The text scrolls sideways; the line numbers stay. On the left the
                                    // » arrow keeps its own room.
                                    let right_end = if side_i == 0 && gutter > 0.0 { x1 - 22.0 } else { x1 - 1.0 };
                                    let tp = ui.painter_at(Rect::from_min_max(pos2(x0 + g + 46.0, rect.top()), pos2(right_end, rect.bottom())));
                                    let hs = if side_i == 0 { hs_l } else { hs_r };
                                    let shown = text.replace('\t', "    ");
                                    if let Some(s) = tsel.filter(|s| s.side as usize == side_i) {
                                        paint_text_sel(&tp, &s, k, shown.chars().count(), x0 + g + 50.0 - hs, cw, rect);
                                        if show_caret {
                                            paint_caret(&tp, &s, k, x0 + g + 50.0 - hs, cw, rect);
                                        }
                                    }
                                    if let Some(ms) = find_marks.get(&(side_i as u8, k)) {
                                        paint_find(&tp, ms, x0 + g + 50.0 - hs, cw, rect);
                                    }
                                    let marks = if side_i == 0 { &words.0 } else { &words.1 };
                                    let syn: &[crate::syntax::Span] = syntax.as_ref().and_then(|c| if side_i == 0 { c.left.get(k) } else { c.right.get(k) }).map_or(&[], |v| v.as_slice());
                                    if marks.is_empty() && syn.is_empty() {
                                        tp.text(pos2(x0 + g + 50.0 - hs, cy), egui::Align2::LEFT_CENTER, shown, mono.clone(), theme::TEXT());
                                    } else {
                                        let job = diff_line_job(&shown, marks, &mono, if side_i == 0 { theme::adapt(W_REMOVED) } else { theme::adapt(W_ADDED) }, syn, 0, theme::TEXT());
                                        let gal = ui.fonts_mut(|f| f.layout_job(job));
                                        tp.galley(pos2(x0 + g + 50.0 - hs, cy - gal.size().y / 2.0), gal, theme::TEXT());
                                    }
                                }
                            }
                            let mid = rect.left() + half_l;
                            p.line_segment([pos2(mid, rect.top()), pos2(mid, rect.bottom())], Stroke::new(1.0, theme::BORDER()));
                            // The first row of a change: » reverts it, the box includes it in the commit.
                            if let (Some(_), Some(&b)) = (&worktree, block_at.get(&k)) {
                                let ar = Rect::from_center_size(pos2(mid - 11.0, cy), vec2(18.0, DIFF_ROW - 2.0));
                                let aresp = ui.interact(ar, ui.scope_id().with(("revert", b)), Sense::click());
                                if aresp.hovered() {
                                    p.rect_filled(ar, 3.0, theme::HOVER());
                                }
                                p.text(ar.center(), egui::Align2::CENTER_CENTER, "»", FontId::proportional(15.0), if aresp.hovered() { theme::TEXT() } else { theme::TEXT_MID() });
                                if aresp.on_hover_text(tr("Revert this change in the file")).clicked() {
                                    revert = Some(b);
                                }
                                let cb = Rect::from_center_size(pos2(mid + 11.0, cy), vec2(13.0, 13.0));
                                let cresp = ui.interact(cb.expand(2.0), ui.scope_id().with(("include", b)), Sense::click());
                                p.rect_filled(cb.expand(2.0), 3.0, theme::BG());
                                paint_checkbox(ui, cb, included(&blocks[b], &sel));
                                if cresp.on_hover_text(tr("Include this change in the commit")).clicked() {
                                    toggle = Some(b);
                                }
                            }
                        } else {
                            let (tag, text) = &unified[k];
                            let (bg, col) = match tag {
                                '+' => (Some(theme::adapt(C_ADDED)), theme::TEXT()),
                                '-' => (Some(theme::adapt(C_REMOVED)), theme::TEXT()),
                                '@' => (Some(theme::BG_BAR()), theme::TEXT_DIM()),
                                _ => (None, theme::TEXT()),
                            };
                            if let Some(bg) = bg {
                                p.rect_filled(rect, 0.0, bg);
                            }
                            // The blame column stays put; the text scrolls sideways beside it.
                            let p = if ann_w > 0.0 {
                                let br = Rect::from_min_size(rect.min, vec2(ann_w, rect.height()));
                                let line = if matches!(tag, '-' | '@') { None } else { unified_new_line(unified, k) };
                                if let Some(h) = crate::blame_view::diff_blame_cell(ui, br, blame_st.as_ref().map(|(_, st)| st), line, ui.scope_id().with(("blame_cell", k))) {
                                    blame_click = Some(h);
                                }
                                p.with_clip_rect(Rect::from_min_max(pos2(rect.left() + ann_w, rect.top()), rect.max))
                            } else {
                                p
                            };
                            origins[2] = rect.left() + ann_w + 10.0 - hs_l + 2.0 * cw;
                            if let Some(s) = tsel.filter(|s| s.side == 2 && *tag != '@') {
                                paint_text_sel(&p, &s, k, text.replace('\t', "    ").chars().count(), origins[2], cw, rect);
                                if show_caret {
                                    paint_caret(&p, &s, k, origins[2], cw, rect);
                                }
                            }
                            if let Some(ms) = find_marks.get(&(2, k)) {
                                paint_find(&p, ms, origins[2], cw, rect);
                            }
                            let prefix = if *tag == '@' { String::new() } else { format!("{tag} ") };
                            let shown = format!("{prefix}{}", text.replace('\t', "    "));
                            // A removed line followed by the line it became: colour the words that differ.
                            let marks: Vec<std::ops::Range<usize>> = unified_partner(unified, k)
                                .map(|j| {
                                    let other = unified[j].1.replace('\t', "    ");
                                    let mine = text.replace('\t', "    ");
                                    let (old, new) = if *tag == '-' { crate::wordiff::changed_ranges(&mine, &other) } else { crate::wordiff::changed_ranges(&other, &mine) };
                                    let r = if *tag == '-' { old } else { new };
                                    r.into_iter().map(|x| x.start + prefix.len()..x.end + prefix.len()).collect()
                                })
                                .unwrap_or_default();
                            let syn: &[crate::syntax::Span] = syntax.as_ref().and_then(|c| c.unified.get(k)).map_or(&[], |v| v.as_slice());
                            if marks.is_empty() && syn.is_empty() {
                                p.text(pos2(rect.left() + ann_w + 10.0 - hs_l, cy), egui::Align2::LEFT_CENTER, shown, mono.clone(), col);
                            } else {
                                let job = diff_line_job(&shown, &marks, &mono, if *tag == '-' { theme::adapt(W_REMOVED) } else { theme::adapt(W_ADDED) }, syn, prefix.len(), col);
                                let gal = ui.fonts_mut(|f| f.layout_job(job));
                                p.galley(pos2(rect.left() + ann_w + 10.0 - hs_l, cy - gal.size().y / 2.0), gal, col);
                            }
                        }
                    }
                });
                {
                    // The selection counts lines of the whole diff (side by side: diff rows, so it
                    // stays put when unchanged lines are folded or unfolded; unified: its lines).
                    let total = if side { rows.len() } else { unified.len() };
                    // The text of line `k` on a side, as shown.
                    let line_text = |k: usize, s: u8| -> Option<String> {
                        if side {
                            rows.get(k).and_then(|r| if s == 0 { r.left.as_ref() } else { r.right.as_ref() }).map(|(_, t)| t.replace('\t', "    "))
                        } else {
                            unified.get(k).filter(|(t, _)| *t != '@').map(|(_, t)| t.replace('\t', "    "))
                        }
                    };
                    // Shown row → line: a folded block counts as its first (or last) hidden line.
                    let line_of = |shown: usize, first: bool| -> usize {
                        if !side {
                            return shown;
                        }
                        match disp.get(shown) {
                            Some(Disp::Row(r)) => *r,
                            Some(Disp::Fold { from, to }) => if first { *from } else { to.saturating_sub(1) },
                            None => rows.len().saturating_sub(1),
                        }
                    };
                    let hit = |pp: egui::Pos2, s: u8| -> (usize, usize) {
                        let len = |k: usize| line_text(k, s).map(|t| t.chars().count()).unwrap_or(0);
                        match (hit_row, first_row, last_row) {
                            (Some(k), _, _) if side && matches!(disp.get(k), Some(Disp::Fold { .. })) => (line_of(k, true), 0),
                            (Some(k), _, _) => {
                                let k = line_of(k, true);
                                (k, (((pp.x - origins[s as usize]) / cw).round().max(0.0) as usize).min(len(k)))
                            }
                            (None, Some((fk, fr)), _) if pp.y < fr.top() => (line_of(fk, true), 0),
                            (None, _, Some(lk)) => {
                                let k = line_of(lk, false);
                                (k, len(k))
                            }
                            _ => (0, 0),
                        }
                    };
                    // The line of the working file under a right-click, so the file opens there.
                    let ctx_id = egui::Id::unique(("diff_ctx_line", i));
                    if sel_resp.secondary_clicked() {
                        if let Some(pp) = ptr {
                            let s = if side { if pp.x < mid_seen { 0 } else { 1 } } else { 2 };
                            let (k, _) = hit(pp, s);
                            let line = if side {
                                // This row's line on the new side, or the next one below (a removed line).
                                let k = k.min(rows.len());
                                rows[k..].iter().find_map(|r| r.right.as_ref().map(|(n, _)| *n)).or_else(|| rows[..k].iter().rev().find_map(|r| r.right.as_ref().map(|(n, _)| *n + 1)))
                            } else {
                                unified_new_line(unified, k)
                            };
                            ui.ctx().data_mut(|dd| dd.insert_temp(ctx_id, line));
                        }
                    }
                    let ctx_line: Option<usize> = ui.ctx().data(|dd| dd.get_temp::<Option<usize>>(ctx_id)).flatten();
                    let mut new_sel = d.text_sel;
                    if sel_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                    }
                    let (pressed, shift) = ui.input(|inp| (inp.pointer.primary_pressed(), inp.modifiers.shift));
                    if let Some(pp) = ptr {
                        if pressed && sel_resp.hovered() {
                            let s = if side { if pp.x < mid_seen { 0 } else { 1 } } else { 2 };
                            let h = hit(pp, s);
                            new_sel = Some(match new_sel {
                                Some(old) if shift && old.side == s => TextSel { head: h, ..old },
                                _ => TextSel { side: s, anchor: h, head: h },
                            });
                        } else if sel_resp.dragged() {
                            if let Some(s) = new_sel.as_mut() {
                                s.head = hit(pp, s.side);
                            }
                        }
                        if sel_resp.double_clicked() {
                            if let Some(s) = new_sel.as_mut() {
                                if let Some(t) = line_text(s.head.0, s.side) {
                                    let (a, b) = word_at(&t, s.head.1);
                                    s.anchor = (s.head.0, a);
                                    s.head = (s.head.0, b);
                                }
                            }
                        }
                    }
                    if pressed && sel_resp.hovered() {
                        focus_text = true;
                    }
                    let no_focus = ui.memory(|m| m.focused().is_none());
                    // The keyboard: arrows, Home, End, Page Up / Down move the text cursor; with
                    // Shift the selection stretches from where it started; Ctrl goes by words
                    // and to the ends of the text.
                    if no_focus && keys_ok && total > 0 {
                        let page = ((area.height() / DIFF_ROW) as usize).saturating_sub(2).max(1);
                        let moves: Vec<(Move, bool)> = ui.input_mut(|inp| {
                            let mut found = vec![];
                            inp.events.retain(|e| match e {
                                egui::Event::Key { key, pressed: true, modifiers, .. } if !modifiers.alt => match key_move(*key, modifiers.command || modifiers.ctrl, page) {
                                    Some(m) => {
                                        found.push((m, modifiers.shift));
                                        false
                                    }
                                    None => true,
                                },
                                _ => true,
                            });
                            found
                        });
                        for (m, extend) in moves {
                            // The side the cursor is on; without one yet: the new version.
                            let s = match new_sel {
                                Some(sel) if side == (sel.side != 2) => sel.side,
                                _ if side => 1,
                                _ => 2,
                            };
                            let from = match new_sel {
                                Some(sel) if sel.side == s => sel.head,
                                _ => (first_row.map(|(fk, _)| line_of(fk, true)).unwrap_or(0), 0),
                            };
                            let head = move_caret(from, m, total, &|k| line_text(k, s));
                            let anchor = match new_sel {
                                Some(sel) if extend && sel.side == s => sel.anchor,
                                _ if extend => from,
                                _ => head,
                            };
                            new_sel = Some(TextSel { side: s, anchor, head });
                            // Bring the cursor into view: unfold the lines it went into, scroll
                            // no further than needed, and follow it sideways.
                            let shown = if side { disp.iter().position(|x| *x == Disp::Row(head.0)) } else { Some(head.0) };
                            let folded = if side { disp.iter().enumerate().find_map(|(at, x)| match x { Disp::Fold { from, to } if *from <= head.0 && head.0 < *to => Some((at, *from)), _ => None }) } else { None };
                            if let Some((at, first)) = folded {
                                open_fold = Some(first);
                                ui.ctx().data_mut(|dd| dd.insert_temp(scroll_id, ((at + head.0 - first) as f32 - 3.0) * DIFF_ROW));
                            } else if let (Some(dk), Some((top, _)), Some(bottom)) = (shown, first_row, last_row) {
                                if dk <= top {
                                    ui.ctx().data_mut(|dd| dd.insert_temp(scroll_id, (dk as f32 - 1.0) * DIFF_ROW));
                                } else if dk + 1 >= bottom {
                                    ui.ctx().data_mut(|dd| dd.insert_temp(scroll_id, (dk as f32 + 3.0) * DIFF_ROW - area.height()));
                                }
                            }
                            let x = head.1 as f32 * cw;
                            let vis = if side { est_w * split.min(1.0 - split) } else { est_w } - 140.0;
                            if x < hscroll || x > hscroll + vis.max(60.0) {
                                hscroll = (x - vis.max(60.0) / 2.0).max(0.0);
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                    let mut do_copy = no_focus && ui.input(|inp| inp.events.iter().any(|e| matches!(e, egui::Event::Copy)));
                    let mut select_all = no_focus && ui.input_mut(|inp| inp.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::A)));
                    let has_sel = new_sel.is_some_and(|s| !s.is_empty());
                    // The lines of the new version the selection covers ("Show History for Selection").
                    let sel_lines: Option<(usize, usize)> = new_sel.filter(|s| !s.is_empty()).and_then(|s| {
                        let (a, mut b) = s.ordered();
                        // A selection that ends at the start of a line does not take that line.
                        if b.1 == 0 && b.0 > a.0 {
                            b.0 -= 1;
                        }
                        let nums: Vec<usize> = if side {
                            rows.iter().skip(a.0).take(b.0 + 1 - a.0).filter_map(|r| r.right.as_ref().map(|(n, _)| *n)).collect()
                        } else {
                            (a.0..=b.0).filter(|&k| unified.get(k).is_some_and(|(t, _)| *t != '-' && *t != '@')).filter_map(|k| unified_new_line(unified, k)).collect()
                        };
                        Some((*nums.iter().min()?, *nums.iter().max()?))
                    });
                    sel_resp.context_menu(|ui| {
                        if ui.add_enabled(has_sel, egui::Button::new(tr("Copy")).shortcut_text(crate::keymap::shown("Ctrl+C"))).clicked() {
                            do_copy = true;
                            ui.close();
                        }
                        if ui.add(egui::Button::new(tr("Select All")).shortcut_text(crate::keymap::shown("Ctrl+A"))).clicked() {
                            select_all = true;
                            ui.close();
                        }
                        if ui.add(egui::Button::new(tr("Find…")).shortcut_text(crate::keymap::shown("Ctrl+F"))).clicked() {
                            find_open = true;
                            ui.close();
                        }
                        ui.separator();
                        let label = match ctx_line {
                            Some(l) => format!("Open in Editor at Line {l}"),
                            None => "Open in Default Application".to_string(),
                        };
                        if ui
                            .add_enabled(open_file.is_some(), egui::Button::new(label))
                            .on_disabled_hover_text(tr("The file is not in the working folder"))
                            .on_hover_text(tr("Opens the file with your default application, at this line when it is an editor RefBraid knows (VS Code, JetBrains IDEs, Sublime Text, gedit, Kate…). Settings → Tools → Open Files sets another program."))
                            .clicked()
                        {
                            if let Some(f) = &open_file {
                                open_req = Some((f.clone(), ctx_line));
                            }
                            ui.close();
                        }
                        if ui.add_enabled(history_path.is_some(), egui::Button::new(tr("Show History"))).on_hover_text(tr("The commits that changed this file, followed through its renames")).clicked() {
                            history_req = true;
                            ui.close();
                        }
                        if ui
                            .add_enabled(history_path.is_some() && sel_lines.is_some(), egui::Button::new(tr("Show History for Selection")))
                            .on_hover_text(tr("The commits that changed the selected lines (git log -L)"))
                            .on_disabled_hover_text(tr("Select some lines of the new version first"))
                            .clicked()
                        {
                            history_lines = sel_lines;
                            ui.close();
                        }
                        let label = if ann_on { tr("Hide Git Blame Annotations") } else { tr("Annotate with Git Blame") };
                        if ui
                            .add_enabled(can_annotate, egui::Button::new(label))
                            .on_hover_text(tr("Show beside each line of the new version the date and author of the commit that last changed it"))
                            .clicked()
                        {
                            ann_toggle = true;
                            ui.close();
                        }
                    });
                    if select_all && total > 0 {
                        let s = match new_sel {
                            Some(s) if side == (s.side != 2) => s.side,
                            _ if !side => 2,
                            _ => if ptr.is_some_and(|pp| pp.x < mid_seen) { 0 } else { 1 },
                        };
                        let end = (0..total).rev().find_map(|k| line_text(k, s).map(|t| (k, t.chars().count()))).unwrap_or((0, 0));
                        new_sel = Some(TextSel { side: s, anchor: (0, 0), head: end });
                    }
                    if do_copy {
                        if let Some(s) = new_sel.filter(|s| !s.is_empty()) {
                            let (a, b) = s.ordered();
                            let mut out: Vec<String> = vec![];
                            for k in a.0..=b.0 {
                                if let Some(t) = line_text(k, s.side) {
                                    if let Some((c0, c1)) = s.cols(k, t.chars().count()) {
                                        out.push(t.chars().skip(c0).take(c1 - c0).collect());
                                    }
                                }
                            }
                            ui.ctx().copy_text(out.join("\n"));
                        }
                    }
                    d.text_sel = new_sel;
                }
                // The divider between the sides: drag to give one side more room, double-click
                // to share the width evenly again.
                if side {
                    let mid_x = area.left() + (row_w * split).round();
                    let hr = Rect::from_min_max(pos2(mid_x - 2.5, area.top()), pos2(mid_x + 2.5, area.bottom()));
                    let sresp = ui.interact(hr, ui.scope_id().with(("diff_split", i)), Sense::click_and_drag());
                    if sresp.hovered() || sresp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        ui.painter().line_segment([pos2(mid_x, area.top()), pos2(mid_x, area.bottom())], Stroke::new(2.0, theme::ACCENT()));
                    }
                    if sresp.dragged() {
                        if let Some(pt) = sresp.interact_pointer_pos() {
                            d.split = ((pt.x - area.left()) / row_w.max(1.0)).clamp(0.15, 0.85);
                        }
                    }
                    if sresp.double_clicked() {
                        d.split = 0.5;
                    }
                }
                // Horizontal scrollbars under each side (one for the unified view).
                let mid_x = area.left() + (row_w * split).round();
                let parts: Vec<(f32, f32, f32)> = if side {
                    vec![(area.left(), mid_x, cw_l), (mid_x, area.left() + row_w, cw_r)]
                } else {
                    vec![(area.left(), area.left() + row_w, cw_l)]
                };
                let mut max_all: f32 = 0.0;
                if overflow {
                    let bar = Rect::from_min_max(pos2(full.left(), area.bottom()), full.max);
                    theme::hline(ui, bar.top() + 0.5, bar.left(), bar.right());
                    for (k, (x0, x1, content)) in parts.iter().copied().enumerate() {
                        let vis = x1 - x0;
                        let max = (content - vis).max(0.0);
                        max_all = max_all.max(max);
                        if max <= 0.0 {
                            continue;
                        }
                        let track = Rect::from_min_max(pos2(x0 + 4.0, bar.top() + 3.0), pos2(x1 - 4.0, bar.bottom() - 2.0));
                        let eff = hscroll.min(max);
                        let tw = (vis / content * track.width()).min(track.width()).max(24.0);
                        let room = (track.width() - tw).max(1.0);
                        let thumb = Rect::from_min_size(pos2(track.left() + eff / max * room, track.top()), vec2(tw, track.height()));
                        let tresp = ui.interact(track, ui.scope_id().with(("diff_hbar", i, k)), Sense::click_and_drag());
                        if tresp.dragged() {
                            hscroll = (eff + tresp.drag_delta().x * max / room).clamp(0.0, max);
                        } else if tresp.clicked() {
                            if let Some(pt) = tresp.interact_pointer_pos() {
                                hscroll = ((pt.x - track.left() - tw / 2.0) / room * max).clamp(0.0, max);
                            }
                        }
                        let col = if tresp.dragged() || tresp.hovered() { theme::TEXT_DIM() } else { theme::rgb(0x45, 0x48, 0x4d) };
                        ui.painter().rect_filled(thumb, 3.0, col);
                    }
                } else {
                    for (x0, x1, content) in parts {
                        max_all = max_all.max((content - (x1 - x0)).max(0.0));
                    }
                }
                d.hscroll = hscroll.clamp(0.0, max_all);
                ui.ctx().data_mut(|dd| dd.insert_temp(offset_id, out.state.clamped_offset().y));
            });
        d.blame = blame_st;
        d.syntax = syntax;
        if find_open {
            match d.find.as_mut() {
                Some(f) => f.focus = true,
                None => d.find = Some(crate::find::Find::opened()),
            }
        }
        if let Some(f) = open_fold {
            d.expanded.insert(f);
        }
        if ann_toggle {
            self.diff_annotate = !self.diff_annotate;
        }
        if focus_text {
            self.key_focus = crate::app::KeyFocus::Text;
        }
        if let Some(h) = blame_click {
            self.switch_log(0);
            self.navigate_to(&h);
            return;
        }
        if let Some((f, line)) = open_req {
            self.open_file_at(&f, line);
            return;
        }
        if let (true, Some(p)) = (history_req, &history_path) {
            self.open_file_history(p, None);
            return;
        }
        if let (Some((a, b)), Some(p)) = (history_lines, &history_path) {
            // The line numbers are those of the version the diff shows on its new side.
            let rev = match &source {
                Some(crate::app::CompareSource::Commit { hash, .. }) => Some(hash.clone()),
                _ => None,
            };
            self.open_lines_history(p, rev.as_deref(), a, b);
            return;
        }
        let Some(path) = worktree else { return };
        if let Some(b) = toggle {
            let h = blocks[b].hash;
            if !file_on {
                // Turning one change on in an unticked file: only that change goes in.
                sel.excluded = blocks.iter().map(|x| x.hash).filter(|x| *x != h).collect();
                if let Some(r) = self.repo.as_mut() {
                    r.unchecked.remove(&path);
                }
            } else if !sel.excluded.remove(&h) {
                sel.excluded.insert(h);
            }
            if blocks.iter().all(|x| sel.excluded.contains(&x.hash)) {
                // Nothing left: untick the file instead.
                sel.excluded.clear();
                if let Some(r) = self.repo.as_mut() {
                    r.unchecked.insert(path.clone());
                }
            }
        }
        sel.total = blocks.len();
        if let Some(r) = self.repo.as_mut() {
            sel.excluded.retain(|h| blocks.iter().any(|b| b.hash == *h));
            if sel.excluded.is_empty() {
                r.partial.remove(&path);
            } else {
                r.partial.insert(path.clone(), sel);
            }
        }
        if let Some(b) = revert {
            self.revert_change(&path, &blocks, b);
        }
    }

    /// Width of the widest of `texts` in `font` (only the longest few are measured).
fn widest<'a>(ui: &Ui, font: &FontId, texts: impl Iterator<Item = &'a str>) -> f32 {
    let mut top: Vec<(usize, &str)> = Vec::with_capacity(4);
    for t in texts {
        let n = t.chars().count() + t.matches('\t').count() * 3;
        if top.len() < 3 || n > top[top.len() - 1].0 {
            top.push((n, t));
            top.sort_by(|a, b| b.0.cmp(&a.0));
            top.truncate(3);
        }
    }
    top.iter().map(|(_, t)| theme::text_width(ui, &t.replace('\t', "    "), font)).fold(0.0, f32::max)
}

/// The » arrow: put one change of a local file back to how it is in HEAD.
    fn revert_change(&mut self, path: &str, blocks: &[crate::partial::Block], b: usize) {
        let Some(repo) = self.repo.as_ref() else { return };
        let dir = repo.path.clone();
        let tracked = repo.status.iter().any(|e| e.path == path && !e.is_untracked() && e.x != 'A');
        let old = if tracked { crate::git::blob(&dir, "HEAD", path).unwrap_or_default() } else { vec![] };
        let file = dir.join(path);
        let new = std::fs::read(&file).unwrap_or_default();
        let content = crate::partial::mix(&old, &new, blocks, |j| j != b);
        if let Err(e) = std::fs::write(&file, &content) {
            self.error = Some(format!("Could not revert the change in {path}: {e}"));
            return;
        }
        self.console.push(crate::app::ConsoleEntry {
            time: chrono::Local::now().timestamp(),
            cmd: format!("(revert one change in {path})"),
            output: String::new(),
            ok: true,
            running: false,
            job: None,
        });
        self.refresh();
        self.open_worktree_diff(path);
    }

    // ------------------------------------------------------------ console

    pub fn console_tab(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG()).inner_margin(10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Every git command this window runs, with its output.")).color(theme::TEXT_DIM()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr("Clear")).clicked() {
                            self.console.clear();
                        }
                    });
                });
                ui.add_space(6.0);
                if self.console.is_empty() {
                    ui.label(RichText::new(tr("Nothing has run yet.")).color(theme::TEXT_MID()));
                    return;
                }
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for e in &self.console {
                            ui.label(
                                RichText::new(format!("{}  {}{}", format_date(e.time), e.cmd, if e.running { "  …" } else { "" }))
                                    .monospace()
                                    .color(if e.running { theme::STAR() } else if e.ok { theme::GREEN() } else { theme::RED() }),
                            );
                            if !e.output.is_empty() {
                                ui.label(RichText::new(&e.output).monospace().color(if e.ok { theme::TEXT() } else { theme::RED() }));
                            }
                            ui.add_space(6.0);
                        }
                    });
            });
    }
}

/// A box with a dash: some of the file's changes are included.
fn paint_checkbox_partial(ui: &Ui, r: Rect) {
    let p = ui.painter();
    p.rect_filled(r, 3.0, theme::ACCENT());
    let c = r.center();
    p.line_segment([pos2(c.x - r.width() * 0.28, c.y), pos2(c.x + r.width() * 0.28, c.y)], Stroke::new(1.6, Color32::WHITE));
}

fn paint_checkbox(ui: &Ui, r: Rect, on: bool) {
    let p = ui.painter();
    if on {
        p.rect_filled(r, 3.0, theme::ACCENT());
        theme::paint_icon(p, r.shrink(1.0), Icon::Check, Color32::WHITE);
    } else {
        p.rect_stroke(r, 3.0, Stroke::new(1.0, theme::TEXT_DIM()), egui::StrokeKind::Inside);
    }
}

/// The commands for a commit that takes only some changes of some files. A separate index
/// (`.git/refbraid-index`) is filled from HEAD plus the chosen changes and committed with
/// `git commit` (hooks run as usual); then the real index of those files is set to the new HEAD.
/// The working folder is not touched. None when nothing would be committed.
fn partial_commit(
    dir: &std::path::Path,
    checked: &[StatusEntry],
    sel: &HashMap<String, PartialSel>,
    has_head: bool,
    msg: &str,
    amend: bool,
) -> Option<Vec<Vec<String>>> {
    use crate::git;
    let git_path = |name: &str| {
        let p = git::run(dir, &["rev-parse", "--git-path", name]).stdout.trim().to_string();
        let p = std::path::PathBuf::from(p);
        if p.is_absolute() { p } else { dir.join(p) }
    };
    let index = git_path("refbraid-index");
    let scratch = git_path("refbraid-partial");
    let env = format!("env:GIT_INDEX_FILE={}", index.display());
    let mut whole: Vec<&StatusEntry> = vec![];
    let mut parts: Vec<(String, String)> = vec![]; // path, "mode,blob,path"
    for e in checked {
        let Some(s) = sel.get(&e.path).filter(|s| !s.excluded.is_empty() && e.old_path.is_none()) else {
            whole.push(e);
            continue;
        };
        let d = git::worktree_diff_exact(dir, e, has_head);
        let bl = crate::partial::blocks(&d.rows);
        let take: Vec<bool> = bl.iter().map(|b| !s.excluded.contains(&b.hash)).collect();
        if take.iter().all(|t| *t) {
            whole.push(e);
            continue;
        }
        if !take.iter().any(|t| *t) {
            continue;
        }
        let old = if e.is_untracked() || !has_head || e.x == 'A' { vec![] } else { git::blob(dir, "HEAD", &e.path).unwrap_or_default() };
        let new = std::fs::read(dir.join(&e.path)).unwrap_or_default();
        let content = crate::partial::mix(&old, &new, &bl, |i| take[i]);
        std::fs::write(&scratch, &content).ok()?;
        let blob = git::run(dir, &["hash-object", "-w", &format!("--path={}", e.path), &scratch.display().to_string()]);
        if !blob.ok {
            return None;
        }
        let mode = git::run(dir, &["ls-tree", "HEAD", "--", &e.path]).stdout.split_whitespace().next().map(String::from);
        let mode = mode.unwrap_or_else(|| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let exec = std::fs::metadata(dir.join(&e.path)).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false);
                if exec { "100755".into() } else { "100644".into() }
            }
            #[cfg(not(unix))]
            {
                "100644".to_string()
            }
        });
        parts.push((e.path.clone(), format!("{mode},{},{}", blob.stdout.trim(), e.path)));
    }
    let _ = std::fs::remove_file(&scratch);
    if parts.is_empty() && whole.is_empty() {
        return None;
    }
    let s = |x: &str| x.to_string();
    let mut cmds: Vec<Vec<String>> = vec![];
    cmds.push(if has_head { vec![env.clone(), s("read-tree"), s("HEAD")] } else { vec![env.clone(), s("read-tree"), s("--empty")] });
    let mut paths: Vec<String> = vec![];
    if !whole.is_empty() {
        let mut c = vec![env.clone(), s("add"), s("-A"), s("--")];
        for e in &whole {
            c.push(e.path.clone());
            paths.push(e.path.clone());
            if let Some(o) = &e.old_path {
                c.push(o.clone());
                paths.push(o.clone());
            }
        }
        cmds.push(c);
    }
    for (p, info) in &parts {
        cmds.push(vec![env.clone(), s("update-index"), s("--add"), s("--cacheinfo"), info.clone()]);
        paths.push(p.clone());
    }
    let mut c = vec![env.clone(), s("commit"), s("-m"), s(msg)];
    if amend {
        c.push(s("--amend"));
    }
    cmds.push(c);
    let mut c = vec![s("reset"), s("-q"), s("--")];
    c.extend(paths);
    cmds.push(c);
    Some(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;

    #[test]
    fn the_text_cursor_moves_through_a_diff() {
        // The new side of a diff: line 2 is the gap beside a removed line.
        let text = ["fn total(items) {", "    let sum = 0;", "", "    sum + 1", "}"];
        let line = |k: usize| if k == 2 { None } else { text.get(k).map(|t| t.to_string()) };
        let go = |pos, m| move_caret(pos, m, 5, &line);
        // Along a line, and over its ends to the neighbours.
        assert_eq!(go((0, 3), Move::Right), (0, 4));
        assert_eq!(go((0, 17), Move::Right), (1, 0));
        assert_eq!(go((1, 0), Move::Left), (0, 17));
        assert_eq!(go((0, 0), Move::Left), (0, 0));
        assert_eq!(go((4, 1), Move::Right), (4, 1));
        // Up and down keep the column where the line is long enough, and step over the gap.
        assert_eq!(go((1, 12), Move::Down), (3, 11));
        assert_eq!(go((3, 6), Move::Up), (1, 6));
        assert_eq!(go((1, 16), Move::Up), (0, 16));
        assert_eq!(go((0, 5), Move::Up), (0, 0));
        assert_eq!(go((4, 0), Move::Down), (4, 1));
        // Ends of the line and of the text.
        assert_eq!((go((1, 7), Move::LineStart), go((1, 7), Move::LineEnd)), ((1, 0), (1, 16)));
        assert_eq!((go((3, 2), Move::DocStart), go((1, 2), Move::DocEnd)), ((0, 0), (4, 1)));
        // By words: to the start of the word, over spaces and signs; across the line's ends.
        assert_eq!(go((1, 4), Move::WordRight), (1, 8));
        assert_eq!(go((1, 8), Move::WordRight), (1, 14));
        assert_eq!(go((1, 16), Move::WordRight), (3, 0));
        assert_eq!(go((1, 11), Move::WordLeft), (1, 8));
        assert_eq!(go((1, 8), Move::WordLeft), (1, 4));
        assert_eq!(go((1, 0), Move::WordLeft), (0, 17));
        // A screenful.
        assert_eq!((go((0, 2), Move::PageDown(3)), go((4, 1), Move::PageUp(10))), ((4, 1), (0, 0)));
        // The keys.
        assert_eq!(key_move(Key::ArrowLeft, false, 20), Some(Move::Left));
        assert_eq!(key_move(Key::ArrowRight, true, 20), Some(Move::WordRight));
        assert_eq!((key_move(Key::Home, false, 20), key_move(Key::End, true, 20)), (Some(Move::LineStart), Some(Move::DocEnd)));
        assert_eq!((key_move(Key::PageDown, false, 20), key_move(Key::A, true, 20)), (Some(Move::PageDown(20)), None));
        // Shift keeps where the selection started: the selected text of two lines.
        let sel = TextSel { side: 1, anchor: (0, 3), head: go(go((0, 3), Move::Down), Move::LineEnd) };
        assert_eq!((sel.cols(0, 17), sel.cols(1, 16)), (Some((3, 17)), Some((0, 16))));
    }

    #[test]
    fn commit_options_reach_every_commit_command() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let o = CommitOpts { no_verify: true, author: " A B <a@b> ".into(), date: "2026-11-20 14:30".into() };
        assert_eq!(o.count(), 3);
        let flags = commit_flags(true, true, &o);
        assert_eq!(flags, s(&["--signoff", "-S", "--no-verify", "--author=A B <a@b>", "--date=2026-11-20 14:30"]));
        assert!(commit_flags(false, false, &CommitOpts::default()).is_empty());
        let mut cmds = vec![s(&["add", "--", "commit"]), s(&["commit", "-m", "msg", "--", "a.txt"]), s(&["env:GIT_INDEX_FILE=/tmp/i", "commit", "-m", "msg"]), s(&["reset", "-q"])];
        add_commit_flags(&mut cmds, &s(&["--signoff", "--no-verify"]));
        assert_eq!(cmds[0], s(&["add", "--", "commit"]));
        assert_eq!(cmds[1], s(&["commit", "--signoff", "--no-verify", "-m", "msg", "--", "a.txt"]));
        assert_eq!(cmds[2], s(&["env:GIT_INDEX_FILE=/tmp/i", "commit", "--signoff", "--no-verify", "-m", "msg"]));
        assert_eq!(cmds[3], s(&["reset", "-q"]));
        assert!(crate::app::is_commit_command("git commit -m x") && crate::app::is_commit_command("GIT_INDEX_FILE=/tmp/i git commit -m x") && !crate::app::is_commit_command("git add commit"));
    }

    #[test]
    fn commits_only_the_chosen_change() {
        let dir = std::env::temp_dir().join(format!("ge-partial-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let g = |args: &[&str]| assert!(git::run_env(&dir, &[("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")], args).ok, "{args:?}");
        g(&["init", "-q"]);
        let old: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("f.txt"), &old).unwrap();
        g(&["add", "f.txt"]);
        g(&["commit", "-qm", "base"]);
        let new = old.replace("line 2\n", "LINE TWO\n").replace("line 19\n", "LINE NINETEEN\n");
        std::fs::write(dir.join("f.txt"), &new).unwrap();
        let st = git::status(&dir);
        let e = st.iter().find(|e| e.path == "f.txt").unwrap().clone();
        let bl = crate::partial::blocks(&git::worktree_diff_exact(&dir, &e, true).rows);
        assert_eq!(bl.len(), 2);
        let mut sel = HashMap::new();
        sel.insert("f.txt".to_string(), PartialSel { excluded: [bl[1].hash].into_iter().collect(), total: 2 });
        let cmds = partial_commit(&dir, &[e], &sel, true, "part", false).unwrap();
        for c in &cmds {
            let env: Vec<(&str, &str)> = c.iter().map_while(|s| s.strip_prefix("env:").and_then(|x| x.split_once('='))).chain([("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")]).collect();
            let skip = c.iter().take_while(|s| s.starts_with("env:")).count();
            let args: Vec<&str> = c.iter().skip(skip).map(|s| s.as_str()).collect();
            let out = git::run_env(&dir, &env, &args);
            assert!(out.ok, "{args:?}: {}", out.text());
        }
        let committed = String::from_utf8(git::blob(&dir, "HEAD", "f.txt").unwrap()).unwrap();
        assert!(committed.contains("LINE TWO") && committed.contains("line 19\n"));
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), new);
        // What is left to commit is only the other change, and nothing is staged.
        let left = git::run(&dir, &["diff", "HEAD", "--stat"]).stdout;
        assert!(left.contains("1 insertion(+), 1 deletion(-)"), "{left}");
        assert!(git::run(&dir, &["diff", "--cached", "--name-only"]).stdout.trim().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
