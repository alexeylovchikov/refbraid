//! The Files panel — the tree of the files git tracks in the checked-out branch — and the tab
//! a file is opened in for reading (a double-click in the tree) and, with Edit, for editing.

use crate::app::{App, Tab, file_name};
use crate::files::{self, Content, Entry, Kind, Row};
use crate::i18n::{tr, trf};
use crate::theme::{self, Icon};
use eframe::egui::{self, FontId, Rect, RichText, Sense, Ui, pos2, vec2};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

const ROW_H: f32 = 22.0;
/// The column of line numbers of the editor.
const EDIT_NUM_W: f32 = 54.0;

/// What the panel knows: the files of one repository and which folders are open.
#[derive(Default)]
pub struct FilesState {
    /// The repository the list was read for.
    repo: Option<PathBuf>,
    list: Vec<Entry>,
    root: files::Dir,
    open: HashSet<String>,
    /// The lines of the tree, worked out again when a folder is toggled or the list is read.
    rows: Option<Vec<Row>>,
    wait: Option<Receiver<(PathBuf, Vec<Entry>)>>,
    loaded: bool,
    /// The repository changed (a commit, a checkout, a pull…): read the list again.
    pub stale: bool,
    filter: String,
    /// The files found by the filter, and the filter they were found for.
    found: Option<(String, Vec<usize>)>,
    selected: Option<String>,
}

impl FilesState {
    /// Open a folder and select a file in it (screenshots).
    pub fn open_for_demo(&mut self, dir: &str) {
        self.open.insert(dir.to_string());
        self.rows = None;
        self.selected = self.list.iter().find(|e| e.path.starts_with(&format!("{dir}/"))).map(|e| e.path.clone());
    }
}

/// A file open for reading.
pub struct FileTab {
    pub path: String,
    kind: Kind,
    /// None while it is read.
    content: Option<Content>,
    wait: Option<Receiver<Content>>,
    colors: Option<Vec<Vec<crate::syntax::Span>>>,
    /// The repository changed: read the file again when the tab is shown.
    pub stale: bool,
}

enum Act {
    Toggle(String),
    Select(String),
    Open(usize),
    Annotate(String),
    History(String),
}

/// What a file being edited asks before its changes are lost, and what follows the answer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ask {
    /// The tab is closed.
    Close,
    /// The program is closed.
    Quit,
}

/// A file of the working folder being edited in its tab.
pub struct EditTab {
    pub e: files::Editing,
    /// Asking about the changes not saved yet.
    pub ask: Option<Ask>,
    /// The file changed on disk after it was opened, while it has changes here: Save asks
    /// whether to overwrite it.
    pub outside: bool,
    /// Why the last save failed.
    pub note: Option<String>,
    /// Put the text cursor into the editor on the next frame.
    focus: bool,
}

/// The file being edited in a tab, if any, and whether it has changes not saved.
pub fn unsaved(diffs: &[crate::app::DiffTab]) -> Option<usize> {
    diffs.iter().position(|d| d.edit.as_ref().is_some_and(|e| e.e.modified()))
}

/// What the right-click menu of the editor asked for.
enum Menu {
    Open(Option<usize>),
    History,
    Lines(usize, usize),
    Annotate(Option<usize>),
}

/// Select characters `a..b` of line `l` in the editor `id`.
fn select_in(ctx: &egui::Context, id: egui::Id, text: &str, l: usize, a: usize, b: usize) {
    let start: usize = text.split('\n').take(l).map(|x| x.chars().count() + 1).sum();
    let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(start + a), egui::text::CCursor::new(start + b))));
    st.store(ctx, id);
}

/// What the tab of a file open for reading says under its name (tabs are told apart by it).
pub fn reading_subtitle(path: &str) -> String {
    format!("{path}  ·  {}", tr("working folder"))
}

/// The lines of a file as the diff tab takes them: every line unchanged, with its number. The
/// tab then gives the reading what a diff has — selecting text, the search, the text cursor.
pub fn reading_diff(lines: &[String]) -> crate::git::DiffText {
    crate::git::DiffText {
        rows: lines.iter().enumerate().map(|(n, l)| crate::git::DiffRow { left: Some((n + 1, l.clone())), right: Some((n + 1, l.clone())), kind: crate::git::DiffSide::Context }).collect(),
        unified: lines.iter().map(|l| (' ', l.clone())).collect(),
        ..Default::default()
    }
}

impl App {
    pub fn toggle_files(&mut self) {
        self.show_files = !self.show_files;
    }

    /// Every frame: read the list of files when the panel is shown and has none for the
    /// repository in front, or the repository changed. git is asked on a thread.
    pub fn files_tick(&mut self) {
        let Some(path) = self.repo.as_ref().map(|r| r.path.clone()) else { return };
        if self.files.repo.as_ref() != Some(&path) {
            // Another repository came to the front: its own tree, nothing open.
            self.files = FilesState { repo: Some(path.clone()), ..Default::default() };
        }
        if let Some(rx) = &self.files.wait {
            if let Ok((p, list)) = rx.try_recv() {
                self.files.wait = None;
                if p == path {
                    self.set_files(list);
                }
            }
        }
        if !self.show_files || self.files.wait.is_some() || (self.files.loaded && !self.files.stale) {
            return;
        }
        self.files.stale = false;
        // A screenshot and a test wait for the list, like for the log.
        if self.screenshot.is_some() || cfg!(test) {
            let list = files::list(&path);
            self.set_files(list);
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let list = files::list(&path);
            let _ = tx.send((path, list));
            ctx.request_repaint();
        });
        self.files.wait = Some(rx);
    }

    fn set_files(&mut self, list: Vec<Entry>) {
        self.files.root = files::tree(&list);
        self.files.list = list;
        self.files.rows = None;
        self.files.found = None;
        self.files.loaded = true;
    }

    /// Open (or bring to the front) the tab that shows a tracked file for reading.
    pub fn open_file_tab(&mut self, path: &str) {
        self.files.selected = Some(path.to_string());
        // Already open for reading: that tab comes to the front, with the file read again.
        let subtitle = reading_subtitle(path);
        if let Some(i) = self.diffs.iter().position(|d| d.reading && d.subtitle == subtitle) {
            self.tab = Tab::Diff(i);
            self.reload_reading_tabs();
            return;
        }
        if let Some(i) = self.file_tabs.iter().position(|t| t.path == path) {
            self.file_tabs[i].stale = true;
            self.tab = Tab::File(i);
            return;
        }
        let kind = self.files.list.iter().find(|e| e.path == path).map_or(Kind::File, |e| e.kind);
        self.file_tabs.push(FileTab { path: path.to_string(), kind, content: None, wait: None, colors: None, stale: true });
        let i = self.file_tabs.len() - 1;
        self.tab = Tab::File(i);
        // A screenshot shows the file at once: read here, not on a thread.
        if self.screenshot.is_some() && !cfg!(test) {
            if let Some(root) = self.repo.as_ref().map(|r| r.path.clone()) {
                self.file_tabs[i].content = Some(files::read(&root, &Entry { path: path.to_string(), kind }));
                self.file_tabs[i].stale = false;
                self.promote_file_tab(i);
            }
        }
    }

    /// A file that turned out to be a text or a picture moves into a tab of the diff kind,
    /// which knows how to show both: the text with selection, search and a text cursor, the
    /// picture in the image viewer. Returns whether it did.
    pub fn promote_file_tab(&mut self, i: usize) -> bool {
        let diff = match self.file_tabs.get(i).and_then(|t| t.content.as_ref()) {
            Some(Content::Text(lines)) => reading_diff(lines),
            Some(Content::Image(bytes)) => crate::git::DiffText { images: Some(crate::git::ImagePair { path: self.file_tabs[i].path.clone(), before: None, after: Some(bytes.clone()) }), ..Default::default() },
            _ => return false,
        };
        let t = self.file_tabs.remove(i);
        let Some(k) = self.push_diff_as(file_name(&t.path), reading_subtitle(&t.path), diff, crate::app::OpenDiff::Normal) else { return false };
        let d = &mut self.diffs[k];
        d.reading = true;
        d.side_by_side = false;
        d.collapse = false;
        d.preview = false;
        d.jump = None;
        d.worktree = None;
        d.from_commit = false;
        // The file it is, for Show History, Annotate with Git Blame and Open in Default Application.
        d.source = Some(crate::app::CompareSource::Worktree { path: t.path, old_path: None, tracked: true });
        self.tab = Tab::Diff(k);
        if self.diffs[k].diff.images.is_none() {
            if let Err(why) = self.start_editing(k) {
                self.diffs[k].read_only = Some(why);
            }
        }
        true
    }

    /// The repository changed: the files open for reading are read again, and the ones whose
    /// text is different now are shown anew.
    pub fn reload_reading_tabs(&mut self) {
        let Some(root) = self.repo.as_ref().map(|r| r.path.clone()) else { return };
        for d in self.diffs.iter_mut().filter(|d| d.reading && d.diff.images.is_none()) {
            if d.edit.is_some() {
                Self::reload_edited(&root, d);
                continue;
            }
            let Some(path) = d.source.as_ref().map(|s| s.path().to_string()) else { continue };
            // A file that is gone or is no text any more keeps what was read last.
            let Content::Text(lines) = files::read(&root, &Entry { path, kind: Kind::File }) else { continue };
            if lines.len() == d.diff.unified.len() && lines.iter().zip(&d.diff.unified).all(|(a, b)| *a == b.1) {
                continue;
            }
            d.diff = reading_diff(&lines);
            d.syntax = None;
            d.text_sel = None;
            d.blame = None;
            if let Some(f) = d.find.as_mut() {
                f.cache = None;
            }
        }
    }

    /// A text file opened from the Files panel goes straight into the editor: read again as
    /// it is on disk. A file that can't be written back unchanged (not UTF-8, mixed line ends,
    /// too big) stays in the reading view, which says why.
    pub fn start_editing(&mut self, i: usize) -> Result<(), String> {
        let Some(root) = self.repo.as_ref().map(|r| r.path.clone()) else { return Err(String::new()) };
        let Some(path) = self.diffs.get(i).and_then(|d| d.source.as_ref()).map(|s| s.path().to_string()) else { return Err(String::new()) };
        let file = crate::chrome::repo_file(&root, &path);
        let read = match std::fs::symlink_metadata(&file) {
            Ok(m) if m.is_file() => std::fs::read(&file).map_err(|e| e.to_string()),
            Ok(_) => Err(tr("The file is not an ordinary file").to_string()),
            Err(e) => Err(e.to_string()),
        };
        let e = read.and_then(|bytes| files::Editing::from_bytes(bytes).map_err(|why| tr(why).to_string()))?;
        let d = &mut self.diffs[i];
        d.edit = Some(Box::new(EditTab { e, ask: None, outside: false, note: None, focus: true }));
        d.read_only = None;
        Ok(())
    }

    /// Write the edited text to the file. A file changed on disk since it was read is not
    /// overwritten unless `force`: the tab asks first. Returns whether the file was written.
    pub fn save_edit(&mut self, i: usize, force: bool) -> bool {
        let Some(root) = self.repo.as_ref().map(|r| r.path.clone()) else { return false };
        let Some(d) = self.diffs.get_mut(i) else { return false };
        let Some(path) = d.source.as_ref().map(|s| s.path().to_string()) else { return false };
        let Some(t) = d.edit.as_mut() else { return false };
        let file = crate::chrome::repo_file(&root, &path);
        if !force && std::fs::read(&file).ok().as_deref() != Some(&t.e.disk[..]) {
            t.outside = true;
            return false;
        }
        let bytes = t.e.bytes();
        if let Err(err) = std::fs::write(&file, &bytes) {
            t.note = Some(trf("The file could not be saved: {}", &[&err]));
            return false;
        }
        t.e.saved = t.e.text.clone();
        t.e.disk = bytes.clone();
        t.outside = false;
        t.note = None;
        // What reading shows, and the changes of the Commit panel, follow.
        d.diff = reading_diff(&files::lines_of(&bytes));
        d.syntax = None;
        d.text_sel = None;
        d.blame = None;
        self.refresh_soon();
        true
    }

    /// The answer to Ask: the changes saved (or let go of), then what was asked for.
    fn answer(&mut self, i: usize, save: bool, ctx: &egui::Context) {
        let Some(ask) = self.diffs.get(i).and_then(|d| d.edit.as_ref()).and_then(|t| t.ask) else { return };
        if save && !self.save_edit(i, false) {
            // Not written (changed outside, or an error): the tab says so and keeps the text.
            if let Some(t) = self.diffs[i].edit.as_mut() {
                t.ask = None;
            }
            return;
        }
        self.diffs[i].edit = None;
        match ask {
            Ask::Close => self.close_diff(i),
            Ask::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// The window is being closed with an edited file not saved: it stays open and the file's
    /// tab asks what to do (one file at a time, in the repository in front first).
    pub fn ask_before_quit(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if unsaved(&self.diffs).is_none() {
            let Some(root) = self.parked.iter().find(|w| unsaved(&w.diffs).is_some()).and_then(|w| w.repo.as_ref()).map(|r| r.path.clone()) else { return };
            if !self.can_switch() {
                // Its tab can't come to the front now: the window stays, and says why.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.notify(crate::app::Toast { title: tr("Save the edited files first").into(), text: trf("A file edited in {} has changes that are not saved.", &[&root.display()]), ok: false, at: std::time::Instant::now() });
                return;
            }
            self.open(&root);
        }
        let Some(i) = unsaved(&self.diffs) else { return };
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if let Some(t) = self.diffs[i].edit.as_mut() {
            t.ask = Some(Ask::Quit);
        }
        self.tab = Tab::Diff(i);
    }

    /// Whether a file edited in any repository tab has changes not saved.
    pub fn unsaved_anywhere(&self) -> bool {
        unsaved(&self.diffs).is_some() || self.parked.iter().any(|w| unsaved(&w.diffs).is_some())
    }

    /// The repository changed: a file being edited that changed on disk is read again when
    /// it has no changes here, and otherwise only marked, so that Save asks before overwriting.
    fn reload_edited(root: &std::path::Path, d: &mut crate::app::DiffTab) {
        let (Some(t), Some(path)) = (d.edit.as_mut(), d.source.as_ref().map(|s| s.path().to_string())) else { return };
        let Ok(bytes) = std::fs::read(crate::chrome::repo_file(root, &path)) else { return };
        if bytes == t.e.disk {
            return;
        }
        if t.e.modified() {
            t.outside = true;
        } else if let Ok(e) = files::Editing::from_bytes(bytes) {
            d.diff = reading_diff(&files::lines_of(&e.disk));
            t.e = e;
        }
    }

    /// The tab of a file being edited: its toolbar, the questions, and the editor.
    pub fn edit_tab(&mut self, ui: &mut Ui, i: usize) {
        let ctx = ui.ctx().clone();
        let syntax_on = !self.settings.syntax_off;
        let mut act: Option<&str> = None;
        let front = self.dialog.is_none() && !self.show_settings && self.merge_open.is_none();
        let crate::app::DiffTab { source, edit, find, .. } = &mut self.diffs[i];
        let path = source.as_ref().map(|s| s.path().to_string()).unwrap_or_default();
        let Some(t) = edit.as_mut() else { return };
        if front && ctx.input_mut(|k| k.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            act = Some("save");
        }
        let modified = t.e.modified();
        egui::Panel::top("edit_bar")
            .exact_size(34.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG()).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(format!("{path}  ·  {}", tr("working folder"))).color(theme::TEXT_MID()));
                    let n = t.e.text.lines().count();
                    ui.label(RichText::new(format!("{n} {}", crate::i18n::plural(n, "line", "lines"))).color(theme::TEXT_DIM()).size(12.0));
                    if modified {
                        ui.label(RichText::new(tr("modified")).color(theme::LINK()).size(12.0));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::icon_button_sized(ui, Icon::Search, &format!("{}  ({})", tr("Find in the file"), crate::keymap::shown("Ctrl+F")), 24.0, find.is_some(), true).clicked() {
                            *find = if find.is_some() { None } else { Some(crate::find::Find::opened()) };
                        }
                        let save = format!("{}  ({})", tr("Save"), crate::keymap::shown("Ctrl+S"));
                        if ui.add_enabled(modified, egui::Button::new(tr("Save"))).on_hover_text(save).clicked() {
                            act = Some("save");
                        }
                    });
                });
            });
        // A question or a warning under the toolbar.
        let name = file_name(&path).to_string();
        let bar = if let Some(ask) = t.ask {
            let q = match ask {
                Ask::Quit => trf("{} has changes that are not saved. Save them before closing the program?", &[&name]),
                _ => trf("{} has changes that are not saved. Save them?", &[&name]),
            };
            Some((q, vec![("ask_save", tr("Save")), ("ask_drop", tr("Don't Save")), ("ask_cancel", tr("Cancel"))]))
        } else if t.outside {
            Some((trf("{} was changed on disk after it was opened here.", &[&name]), vec![("overwrite", tr("Overwrite It")), ("reload", tr("Load It and Lose the Changes Here")), ("keep", tr("Keep Editing"))]))
        } else {
            t.note.clone().map(|n| (n, vec![("note_ok", tr("OK"))]))
        };
        if let Some((text, buttons)) = bar {
            egui::Panel::top("edit_ask")
                .exact_size(36.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::rgb(0x3d, 0x35, 0x22)).inner_margin(egui::Margin::symmetric(10, 4)))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.label(RichText::new(text).color(theme::TEXT()));
                        ui.add_space(8.0);
                        for (id, label) in buttons {
                            if ui.button(label).clicked() {
                                act = Some(id);
                            }
                        }
                    });
                });
        }
        let id = egui::Id::unique(("file_edit", i, &path));
        let mut menu: Option<Menu> = None;
        let mut focus = std::mem::take(&mut t.focus);
        // The search (Ctrl+F): its hits are marked in the text, the current one brought into view.
        let lh = crate::merge_view::LH;
        let mut marks: Vec<crate::merge_view::Mark> = vec![];
        let mut go: Option<(usize, usize, usize)> = None;
        let mut find_close = false;
        if let Some(f) = find.as_mut() {
            let hits = crate::find::Matcher::new(&f.query, f.regex, f.case).map(|m| m.in_text(&t.e.text)).unwrap_or_default();
            if f.current >= hits.len() {
                f.current = 0;
            }
            if std::mem::take(&mut f.go) {
                go = hits.get(f.current).copied();
            }
            marks = hits.iter().enumerate().map(|(k, &(l, a, b))| (l, a, b, k == f.current)).collect();
            let count = (if hits.is_empty() { 0 } else { f.current + 1 }, hits.len());
            egui::Panel::top("edit_find")
                .exact_size(34.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::BG()).inner_margin(egui::Margin::symmetric(10, 4)))
                .show(ui, |ui| {
                    let r = ui.max_rect().expand2(vec2(10.0, 4.0));
                    theme::hline(ui, r.top() + 0.5, r.left(), r.right());
                    ui.horizontal_centered(|ui| find_close = crate::commit_view::find_line(ui, f, "Search in the file", count));
                });
            // Closed with Esc: the text cursor goes to the found text.
            if find_close {
                if let Some(&(l, a, b)) = hits.get(f.current) {
                    select_in(&ctx, id, &t.e.text, l, a, b);
                }
                focus = true;
            }
        }
        if find_close {
            *find = None;
        }
        // Tab: spaces (or a tab) the way the file is indented; several lines selected move right.
        if ctx.memory(|m| m.has_focus(id)) && ctx.input_mut(|k| k.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
            if let Some(range) = egui::TextEdit::load_state(&ctx, id).and_then(|st| st.cursor.char_range()) {
                let unit = files::indent_unit(&path, &t.e.text);
                let (text, a, b) = files::indent(&t.e.text, range.primary.index.0, range.secondary.index.0, unit);
                t.e.text = text;
                if let Some(mut st) = egui::TextEdit::load_state(&ctx, id) {
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
                    st.store(&ctx, id);
                }
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG())).show(ui, |ui| {
            let r = ui.max_rect();
            theme::hline(ui, r.top() + 0.5, r.left(), r.right());
            let look_path = if syntax_on { path.as_str() } else { "" };
            let mut sa = egui::ScrollArea::both().id_salt(("file_edit_scroll", i)).auto_shrink([false, false]);
            if let Some((l, a, _)) = go {
                sa = sa.vertical_scroll_offset((l as f32 * lh - 80.0).max(0.0));
                let cw = ui.fonts_mut(|f| f.glyph_width(&FontId::monospace(13.0), '0'));
                let x = a as f32 * cw;
                let seen = r.width() - EDIT_NUM_W - 40.0;
                sa = sa.horizontal_scroll_offset(if x > seen { x - seen / 2.0 } else { 0.0 });
            }
            sa.show(ui, |ui| {
                let n = t.e.text.split('\n').count();
                let avail = ui.available_size();
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (nums, _) = ui.allocate_exact_size(vec2(EDIT_NUM_W, (n as f32 * lh).max(avail.y)), Sense::hover());
                    let clip = nums.intersect(ui.clip_rect());
                    let p = ui.painter().with_clip_rect(clip);
                    let first = ((clip.top() - nums.top()) / lh).floor().max(0.0) as usize;
                    let last = (((clip.bottom() - nums.top()) / lh).ceil() as usize + 1).min(n);
                    for k in first..last {
                        p.text(pos2(nums.right() - 12.0, nums.top() + k as f32 * lh + lh / 2.0), egui::Align2::RIGHT_CENTER, (k + 1).to_string(), FontId::monospace(11.5), theme::TEXT_DIM());
                    }
                    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _w: f32| {
                        let look = crate::merge_view::Look { path: look_path, st: Default::default(), marks: &marks };
                        let job = crate::merge_view::cached_job(ui.ctx(), buf.as_str(), theme::TEXT(), look);
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                    let w = (avail.x - EDIT_NUM_W).max(100.0);
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut t.e.text)
                            .id(id)
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .lock_focus(true)
                            .desired_rows(1)
                            .desired_width(w)
                            .min_size(vec2(w, avail.y))
                            .layouter(&mut layouter),
                    );
                    let cursor = egui::TextEdit::load_state(ui.ctx(), id).and_then(|st| st.cursor.char_range());
                    r.context_menu(|ui| {
                        theme::edit_menu_items(ui, &r);
                        ui.separator();
                        // The lines of the cursor or of the selection (1-based).
                        let line_of = |c: usize| t.e.text.chars().take(c).filter(|&ch| ch == '\n').count() + 1;
                        let lines = cursor.map(|c| {
                            let (a, b) = (c.primary.index.0.min(c.secondary.index.0), c.primary.index.0.max(c.secondary.index.0));
                            (line_of(a), line_of(b))
                        });
                        let at = lines.map(|l| l.0);
                        let label = match at {
                            Some(l) => trf("Open in Editor at Line {}", &[&l]),
                            None => tr("Open in Default Application").to_string(),
                        };
                        if ui.button(label).on_hover_text(tr("Opens the file with your default application, at this line when it is an editor RefBraid knows (VS Code, JetBrains IDEs, Sublime Text, gedit, Kate…). Settings → Tools → Open Files sets another program.")).clicked() {
                            menu = Some(Menu::Open(at));
                            ui.close();
                        }
                        if ui.button(tr("Show History")).on_hover_text(tr("The commits that changed this file, followed through its renames")).clicked() {
                            menu = Some(Menu::History);
                            ui.close();
                        }
                        let sel = lines.filter(|(a, b)| a != b || cursor.is_some_and(|c| !c.is_empty()));
                        if ui.add_enabled(sel.is_some(), egui::Button::new(tr("Show History for Selection"))).on_hover_text(tr("The commits that changed the selected lines (git log -L)")).clicked() {
                            menu = sel.map(|(a, b)| Menu::Lines(a, b));
                            ui.close();
                        }
                        if ui.button(tr("Annotate with Git Blame")).on_hover_text(tr("The date and author of the commit that last changed each line, in a tab of its own")).clicked() {
                            menu = Some(Menu::Annotate(at));
                            ui.close();
                        }
                    });
                });
            });
        });
        if focus {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if let Some(m) = menu {
            let root = self.repo.as_ref().map(|r| r.path.clone());
            match m {
                Menu::Open(line) => {
                    if let Some(root) = root {
                        self.open_file_at(&crate::chrome::repo_file(&root, &path), line);
                    }
                }
                Menu::History => self.open_file_history(&path, None),
                Menu::Lines(a, b) => self.open_lines_history(&path, None, a, b),
                Menu::Annotate(line) => self.open_blame(&path, None, line),
            }
            return;
        }
        match act {
            Some("save") => {
                self.save_edit(i, false);
            }
            Some("ask_save") => self.answer(i, true, &ctx),
            Some("ask_drop") => self.answer(i, false, &ctx),
            Some("ask_cancel") => self.diffs[i].edit.as_mut().unwrap().ask = None,
            Some("overwrite") => {
                self.save_edit(i, true);
            }
            Some("reload") => {
                let mut ok = false;
                if let (Some(root), Some(path)) = (self.repo.as_ref().map(|r| r.path.clone()), self.diffs[i].source.as_ref().map(|s| s.path().to_string())) {
                    if let Some(e) = std::fs::read(crate::chrome::repo_file(&root, &path)).ok().and_then(|b| files::Editing::from_bytes(b).ok()) {
                        let t = self.diffs[i].edit.as_mut().unwrap();
                        t.e = e;
                        t.outside = false;
                        ok = true;
                    }
                }
                if !ok {
                    // Gone, or no longer editable: back to reading what there is.
                    self.diffs[i].edit = None;
                    self.reload_reading_tabs();
                }
            }
            Some("keep") => self.diffs[i].edit.as_mut().unwrap().outside = false,
            Some("note_ok") => self.diffs[i].edit.as_mut().unwrap().note = None,
            _ => {}
        }
    }

    pub fn close_file_tab(&mut self, i: usize) {
        if i < self.file_tabs.len() {
            self.file_tabs.remove(i);
        }
        self.tab = match self.tab {
            Tab::File(j) if j == i => {
                if self.file_tabs.is_empty() { Tab::Log } else { Tab::File(j.min(self.file_tabs.len() - 1)) }
            }
            Tab::File(j) if j > i => Tab::File(j - 1),
            t => t,
        };
    }

    pub fn file_tab_title(&self, i: usize) -> String {
        file_name(&self.file_tabs[i].path).to_string()
    }

    /// The Files panel: the header, the filter and the tree.
    pub fn files_panel(&mut self, ui: &mut Ui) {
        egui::Panel::top("files_header").exact_size(34.0).resizable(false).frame(egui::Frame::new().fill(theme::BG())).show(ui, |ui| {
            let r = ui.max_rect();
            theme::hline(ui, r.bottom() - 0.5, r.left(), r.right());
            ui.horizontal_centered(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new(tr("Files")).strong().color(theme::TEXT()));
                if self.files.loaded {
                    ui.label(RichText::new(self.files.list.len().to_string()).color(theme::TEXT_DIM()).size(12.0));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    if theme::icon_button_sized(ui, Icon::Minus, tr("Hide"), 22.0, false, true).clicked() {
                        self.show_files = false;
                    }
                    if theme::icon_button_sized(ui, Icon::Refresh, tr("Read the list of files again"), 22.0, false, self.files.wait.is_none()).clicked() {
                        self.files.stale = true;
                    }
                    if theme::icon_button_sized(ui, Icon::Collapse, tr("Close all folders"), 22.0, false, !self.files.open.is_empty()).clicked() {
                        self.files.open.clear();
                        self.files.rows = None;
                    }
                });
            });
        });
        if self.repo.is_none() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(tr("No repository is open")).color(theme::TEXT_DIM())));
            return;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let w = ui.available_width() - 8.0;
            crate::details_view::search_box(ui, &mut self.files.filter, tr("File name"), w, None);
        });
        ui.add_space(4.0);
        if !self.files.loaded {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(tr("Reading the files…")).color(theme::TEXT_DIM())));
            return;
        }
        if self.files.list.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| ui.add(egui::Label::new(RichText::new(tr("Git tracks no files here yet")).color(theme::TEXT_DIM())).wrap()));
            return;
        }
        let filter = self.files.filter.trim().to_string();
        let mut act: Option<Act> = None;
        if filter.is_empty() {
            if self.files.rows.is_none() {
                self.files.rows = Some(files::rows(&self.files.list, &self.files.root, &self.files.open));
            }
            let st = &self.files;
            let rows = st.rows.as_deref().unwrap_or(&[]);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().id_salt("files_tree").auto_shrink([false, false]).show_rows(ui, ROW_H, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for n in range {
                    match &rows[n] {
                        Row::Dir { depth, path, open } => {
                            let resp = tree_row(ui, *depth, Some(*open), files::file_name(path), "", false, None);
                            if resp.clicked() {
                                act = Some(Act::Toggle(path.clone()));
                            }
                            resp.on_hover_text(path);
                        }
                        Row::File { depth, index } => {
                            let e = &st.list[*index];
                            file_row(ui, *depth, *index, e, "", st.selected.as_deref() == Some(e.path.as_str()), &mut act);
                        }
                    }
                }
            });
        } else {
            if self.files.found.as_ref().is_none_or(|f| f.0 != filter) {
                self.files.found = Some((filter.clone(), files::find(&self.files.list, &filter, 500)));
            }
            let st = &self.files;
            let found = st.found.as_ref().map(|f| f.1.as_slice()).unwrap_or(&[]);
            if found.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(tr("No file is named so")).color(theme::TEXT_DIM())));
            }
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().id_salt("files_found").auto_shrink([false, false]).show_rows(ui, ROW_H, found.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for n in range {
                    let e = &st.list[found[n]];
                    // The folder after the name, so files of one name can be told apart.
                    let dir = e.path.rsplit_once('/').map_or("", |x| x.0);
                    file_row(ui, 0, found[n], e, dir, st.selected.as_deref() == Some(e.path.as_str()), &mut act);
                }
            });
        }
        match act {
            Some(Act::Toggle(path)) => {
                if !self.files.open.remove(&path) {
                    self.files.open.insert(path);
                }
                self.files.rows = None;
            }
            Some(Act::Select(path)) => self.files.selected = Some(path),
            Some(Act::Open(i)) => {
                if let Some(path) = self.files.list.get(i).map(|e| e.path.clone()) {
                    self.files.selected = Some(path.clone());
                    self.open_file_tab(&path);
                }
            }
            Some(Act::Annotate(path)) => self.open_blame(&path, None, None),
            Some(Act::History(path)) => self.open_file_history(&path, None),
            None => {}
        }
    }

    /// The tab with a file for reading.
    pub fn file_tab(&mut self, ui: &mut Ui, i: usize) {
        let Some(root) = self.repo.as_ref().map(|r| r.path.clone()) else { return };
        let wait_here = self.screenshot.is_some() || cfg!(test);
        let ctx = self.ctx.clone();
        let Some(tab) = self.file_tabs.get_mut(i) else { return };
        if let Some(c) = tab.wait.as_ref().and_then(|rx| rx.try_recv().ok()) {
            tab.wait = None;
            // The same text again keeps its colours: nothing to work out anew.
            if tab.content.as_ref() != Some(&c) {
                tab.colors = None;
                tab.content = Some(c);
            }
        }
        if tab.stale && tab.wait.is_none() {
            tab.stale = false;
            let entry = Entry { path: tab.path.clone(), kind: tab.kind };
            if wait_here {
                tab.content = Some(files::read(&root, &entry));
                tab.colors = None;
            } else {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(files::read(&root, &entry));
                    ctx.request_repaint();
                });
                tab.wait = Some(rx);
            }
        }
        if self.promote_file_tab(i) {
            // A text or a picture: shown by the tab it moved into, from the next frame on.
            self.ctx.request_repaint();
            return;
        }
        let Some(tab) = self.file_tabs.get_mut(i) else { return };
        let mut annotate = false;
        let mut history = false;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG())).show(ui, |ui| {
            // Header: the file and where it is read from.
            let bar = Rect::from_min_size(ui.max_rect().min, vec2(ui.max_rect().width(), 30.0));
            theme::hline(ui, bar.bottom() - 0.5, bar.left(), bar.right());
            let w = theme::paint_clipped(ui.painter(), pos2(bar.left() + 12.0, bar.center().y), &tab.path, FontId::proportional(13.0), theme::TEXT(), bar.width() - 330.0);
            ui.painter().text(pos2(bar.left() + 12.0 + w + 10.0, bar.center().y), egui::Align2::LEFT_CENTER, format!("·  {}  ·  {}", tr("working folder"), tr("read only")), FontId::proportional(12.0), theme::TEXT_MID());
            let mut tools = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(8.0, 4.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
            if tab.kind == Kind::File {
                if tools.link(tr("Show History")).clicked() {
                    history = true;
                }
                tools.add_space(8.0);
                if tools.link(tr("Annotate")).clicked() {
                    annotate = true;
                }
            }
            ui.advance_cursor_after_rect(bar);
            let area = ui.available_rect_before_wrap();
            let say = |ui: &Ui, text: String| {
                ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(13.0), theme::TEXT_DIM());
            };
            // What can be read went into a tab of the diff kind; here is why this one can't.
            match &tab.content {
                None => say(ui, tr("Reading the file…").to_string()),
                Some(Content::Text(_) | Content::Image(_)) => {}
                Some(Content::Binary(n)) => say(ui, trf("Not a text file ({}): there is nothing to read here.", &[&files::size_text(*n)])),
                Some(Content::TooBig(n)) => say(ui, trf("The file is too big to show here ({}).", &[&files::size_text(*n)])),
                Some(Content::Missing) => say(ui, tr("The file is not in the working folder: it was deleted and the deletion is not committed yet.").to_string()),
                Some(Content::Link(to)) => say(ui, trf("A symbolic link to {}", &[to])),
                Some(Content::Submodule) => say(ui, tr("A submodule: a repository of its own. Open it from the Submodules section of the branch tree.").to_string()),
                Some(Content::Failed(e)) => say(ui, trf("The file could not be read: {}", &[e])),
            }
        });
        let Some(path) = self.file_tabs.get(i).map(|t| t.path.clone()) else { return };
        if annotate {
            self.open_blame(&path, None, None);
        }
        if history {
            self.open_file_history(&path, None);
        }
    }
}

/// What a file is, by its name: the shape of its icon in the tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// Source code: angle brackets.
    Code,
    /// A text to read: a page with lines.
    Doc,
    /// Settings and data in text: braces.
    Config,
    Image,
    /// A script for the shell: a prompt.
    Shell,
    /// An archive or a package: a box.
    Archive,
    /// Tables and databases: a cylinder.
    Data,
    /// A file of git itself: a branch.
    Git,
    /// Anything else: a plain page.
    Plain,
}

/// The shape and the colour (written for the dark theme) of the icon of a file.
pub fn file_look(path: &str) -> (Shape, [u8; 3]) {
    const GREY: [u8; 3] = [0x9a, 0xa4, 0xb2];
    let name = files::file_name(path);
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |x| x.1);
    // Names that say more than their ending (or have none).
    match lower.as_str() {
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" | ".mailmap" => return (Shape::Git, [0xe0, 0x7a, 0x5a]),
        "makefile" | "dockerfile" | "containerfile" | "justfile" | "cmakelists.txt" | "rakefile" | "gemfile" | "procfile" => return (Shape::Config, [0xc0, 0x9a, 0x60]),
        "license" | "licence" | "copying" | "notice" | "authors" | "changelog" | "readme" => return (Shape::Doc, [0x7a, 0xb0, 0xe0]),
        _ => {}
    }
    match ext {
        "rs" => (Shape::Code, [0xde, 0x8a, 0x5a]),
        "py" | "pyi" => (Shape::Code, [0x5a, 0x9f, 0xd8]),
        "js" | "mjs" | "cjs" | "jsx" => (Shape::Code, [0xe0, 0xc0, 0x50]),
        "ts" | "tsx" => (Shape::Code, [0x4f, 0x8f, 0xd8]),
        "go" => (Shape::Code, [0x55, 0xc0, 0xd8]),
        "java" | "scala" | "groovy" | "gradle" => (Shape::Code, [0xd8, 0x7a, 0x50]),
        "kt" | "kts" => (Shape::Code, [0xa8, 0x7b, 0xe8]),
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "m" | "mm" => (Shape::Code, [0x6a, 0x9a, 0xd8]),
        "cs" | "fs" | "vb" => (Shape::Code, [0x8f, 0x6f, 0xd0]),
        "rb" | "erb" => (Shape::Code, [0xd8, 0x5a, 0x5a]),
        "php" => (Shape::Code, [0x8a, 0x8f, 0xd0]),
        "swift" => (Shape::Code, [0xe8, 0x7a, 0x4a]),
        "dart" | "lua" | "pl" | "pm" | "r" | "jl" | "ex" | "exs" | "erl" | "hs" | "clj" | "zig" | "nim" | "vue" | "svelte" => (Shape::Code, [0x60, 0xb8, 0xa8]),
        "html" | "htm" | "xhtml" | "jsp" | "twig" | "j2" | "jinja" => (Shape::Code, [0xe0, 0x7a, 0x4a]),
        "css" | "scss" | "sass" | "less" => (Shape::Code, [0x6a, 0x8f, 0xe0]),
        "sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" | "ps1" => (Shape::Shell, [0x7a, 0xc0, 0x7a]),
        "md" | "markdown" | "txt" | "rst" | "adoc" | "tex" | "pdf" | "doc" | "docx" | "odt" | "rtf" => (Shape::Doc, [0x7a, 0xb0, 0xe0]),
        "json" | "jsonc" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "xml" | "env" | "properties" | "lock" | "editorconfig" | "plist" | "spec" | "rc" | "wxs" => (Shape::Config, GREY),
        "sql" | "csv" | "tsv" | "db" | "sqlite" | "parquet" | "xls" | "xlsx" | "ods" => (Shape::Data, [0x60, 0xb8, 0xa8]),
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "jar" | "war" | "rpm" | "deb" | "dmg" | "msi" | "appimage" | "whl" => (Shape::Archive, [0xc0, 0x9a, 0x60]),
        e if crate::git::IMAGE_EXTENSIONS.contains(&e) => (Shape::Image, [0xc8, 0x7a, 0xc8]),
        _ => (Shape::Plain, GREY),
    }
}

/// Draw the icon of a file: a few strokes in a 14-point square, drawn here like every icon.
fn paint_file_icon(p: &egui::Painter, r: Rect, path: &str) {
    let (shape, [red, green, blue]) = file_look(path);
    let col = theme::rgb(red, green, blue);
    let s = egui::Stroke::new(1.3, col);
    let c = r.center();
    let at = |x: f32, y: f32| pos2(c.x + x, c.y + y);
    let line = |a: egui::Pos2, b: egui::Pos2| p.line_segment([a, b], s);
    let page = || {
        // A sheet with a folded corner.
        p.add(egui::Shape::line(vec![at(1.5, -5.5), at(-4.0, -5.5), at(-4.0, 5.5), at(4.0, 5.5), at(4.0, -3.0), at(1.5, -5.5), at(1.5, -3.0), at(4.0, -3.0)], s));
    };
    match shape {
        Shape::Code => {
            p.add(egui::Shape::line(vec![at(-2.5, -3.5), at(-6.0, 0.0), at(-2.5, 3.5)], s));
            p.add(egui::Shape::line(vec![at(2.5, -3.5), at(6.0, 0.0), at(2.5, 3.5)], s));
            line(at(1.2, -4.5), at(-1.2, 4.5));
        }
        Shape::Doc => {
            page();
            line(at(-2.0, -0.5), at(2.0, -0.5));
            line(at(-2.0, 2.5), at(2.0, 2.5));
        }
        Shape::Config => {
            p.add(egui::Shape::line(vec![at(-2.0, -5.5), at(-3.5, -5.5), at(-3.5, -1.2), at(-5.5, 0.0), at(-3.5, 1.2), at(-3.5, 5.5), at(-2.0, 5.5)], s));
            p.add(egui::Shape::line(vec![at(2.0, -5.5), at(3.5, -5.5), at(3.5, -1.2), at(5.5, 0.0), at(3.5, 1.2), at(3.5, 5.5), at(2.0, 5.5)], s));
        }
        Shape::Image => {
            p.rect_stroke(Rect::from_center_size(c, vec2(12.0, 10.0)), 1.5, s, egui::StrokeKind::Middle);
            p.add(egui::Shape::line(vec![at(-5.0, 3.5), at(-1.5, -0.5), at(1.0, 2.0), at(2.5, 0.5), at(5.0, 3.5)], s));
            p.circle_filled(at(2.8, -2.2), 1.2, col);
        }
        Shape::Shell => {
            p.rect_stroke(Rect::from_center_size(c, vec2(12.0, 10.0)), 1.5, s, egui::StrokeKind::Middle);
            p.add(egui::Shape::line(vec![at(-3.5, -2.0), at(-1.2, 0.0), at(-3.5, 2.0)], s));
            line(at(0.5, 2.2), at(3.5, 2.2));
        }
        Shape::Archive => {
            p.rect_stroke(Rect::from_min_max(at(-5.5, -2.5), at(5.5, 5.0)), 1.0, s, egui::StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(at(-6.0, -5.0), at(6.0, -2.5)), 1.0, s, egui::StrokeKind::Middle);
            line(at(-1.5, 0.3), at(1.5, 0.3));
        }
        Shape::Data => {
            // A cylinder: the lid, the sides and two bands.
            let arc = |y: f32, top: bool| {
                let pts: Vec<egui::Pos2> = (0..=8).map(|i| {
                    let t = std::f32::consts::PI * i as f32 / 8.0;
                    at(-5.0 * t.cos(), y + if top { -1.8 } else { 1.8 } * t.sin())
                }).collect();
                p.add(egui::Shape::line(pts, s));
            };
            arc(-3.5, true);
            arc(-3.5, false);
            arc(0.3, false);
            arc(4.0, false);
            line(at(-5.0, -3.5), at(-5.0, 4.0));
            line(at(5.0, -3.5), at(5.0, 4.0));
        }
        Shape::Git => theme::paint_icon(p, r, Icon::Branch, col),
        Shape::Plain => page(),
    }
}

/// A line of the tree: a folder (`open` says whether it shows what is in it) or a file.
fn tree_row(ui: &mut Ui, depth: usize, open: Option<bool>, name: &str, note: &str, selected: bool, file: Option<&str>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let p = ui.painter_at(r);
    if selected {
        p.rect_filled(r, 0.0, theme::SELECT());
    } else if resp.hovered() {
        p.rect_filled(r, 0.0, theme::HOVER());
    }
    let cy = r.center().y;
    let mut x = r.left() + 8.0 + depth as f32 * 14.0;
    if let Some(open) = open {
        theme::paint_icon(&p, Rect::from_center_size(pos2(x + 6.0, cy), vec2(12.0, 12.0)), if open { Icon::ChevronDown } else { Icon::ChevronRight }, theme::TEXT_MID());
    }
    x += 16.0;
    let ir = Rect::from_center_size(pos2(x + 7.0, cy), vec2(14.0, 14.0));
    match file {
        Some(path) => paint_file_icon(&p, ir, path),
        None => theme::paint_icon(&p, ir, Icon::Folder, theme::TEXT_MID()),
    }
    x += 20.0;
    let font = FontId::proportional(13.0);
    let w = theme::paint_clipped(&p, pos2(x, cy), name, font, theme::TEXT(), r.right() - x - 6.0);
    if !note.is_empty() {
        let nx = x + w + 8.0;
        theme::paint_clipped(&p, pos2(nx, cy), note, FontId::proportional(12.0), theme::TEXT_DIM(), (r.right() - nx - 6.0).max(0.0));
    }
    resp
}

/// A file of the tree or of the found list: a click selects it, a double-click opens it.
fn file_row(ui: &mut Ui, depth: usize, index: usize, e: &Entry, dir: &str, selected: bool, act: &mut Option<Act>) {
    let note = match e.kind {
        Kind::Link => tr("link").to_string(),
        Kind::Submodule => tr("submodule").to_string(),
        Kind::File => dir.to_string(),
    };
    let resp = tree_row(ui, depth, None, files::file_name(&e.path), &note, selected, Some(&e.path));
    if resp.double_clicked() {
        *act = Some(Act::Open(index));
    } else if resp.clicked() {
        *act = Some(Act::Select(e.path.clone()));
    }
    let path = e.path.clone();
    let plain = e.kind == Kind::File;
    resp.on_hover_text(&e.path).context_menu(|ui| {
        if ui.button(tr("Open")).on_hover_text(tr("Show the file in a tab, for reading")).clicked() {
            *act = Some(Act::Open(index));
            ui.close();
        }
        if ui.add_enabled(plain, egui::Button::new(tr("Annotate"))).clicked() {
            *act = Some(Act::Annotate(path.clone()));
            ui.close();
        }
        if ui.add_enabled(plain, egui::Button::new(tr("Show History"))).clicked() {
            *act = Some(Act::History(path.clone()));
            ui.close();
        }
        if ui.button(tr("Copy Path")).clicked() {
            ui.ctx().copy_text(path.clone());
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testrepo::TestRepo;

    #[test]
    fn knows_a_file_by_its_name() {
        let shape = |p: &str| file_look(p).0;
        assert_eq!(shape("src/main.rs"), Shape::Code);
        assert_eq!(shape("web/Index.HTML"), Shape::Code);
        assert_eq!(shape("docs/notes.md"), Shape::Doc);
        assert_eq!(shape("LICENSE"), Shape::Doc);
        assert_eq!(shape("Cargo.toml"), Shape::Config);
        assert_eq!(shape("app/Makefile"), Shape::Config);
        assert_eq!(shape("docs/log.PNG"), Shape::Image);
        assert_eq!(shape("packaging/build-rpm.sh"), Shape::Shell);
        assert_eq!(shape("dist/app.tar.gz"), Shape::Archive);
        assert_eq!(shape("data/people.csv"), Shape::Data);
        assert_eq!(shape(".gitignore"), Shape::Git);
        assert_eq!(shape("bin/tool"), Shape::Plain);
        assert_eq!(shape("a.unknown"), Shape::Plain);
        // The same shape in different colours tells the languages apart.
        assert_ne!(file_look("a.rs").1, file_look("a.py").1);
    }

    #[test]
    fn the_panel_lists_the_files_and_opens_them_for_reading() {
        let r = TestRepo::new("files-view");
        r.write("src/main.rs", "fn main() {}\n");
        r.write("README.md", "# Title\n");
        r.commit("Add files");
        let mut app = r.app();
        // Nothing is read while the panel is hidden.
        app.files_tick();
        assert!(!app.files.loaded);
        app.show_files = true;
        app.files_tick();
        assert_eq!(app.files.list.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), vec!["README.md", "src/main.rs"]);
        // A new file committed: the list is read again after the refresh that follows a job.
        r.write("docs/a.md", "a\n");
        r.commit("Add a doc");
        app.refresh();
        app.files_tick();
        assert_eq!(app.files.list.len(), 3);
        // Two files open as two tabs; the same file again brings its tab to the front.
        app.open_file_tab("README.md");
        app.open_file_tab("src/main.rs");
        app.open_file_tab("README.md");
        assert_eq!((app.file_tabs.len(), app.tab), (2, Tab::File(0)));
        assert_eq!(app.file_tab_title(1), "main.rs");
        // Closing the first one keeps the other in front, under its new number.
        app.tab = Tab::File(1);
        app.close_file_tab(0);
        assert_eq!((app.file_tabs.len(), app.tab, app.file_tabs[0].path.as_str()), (1, Tab::File(0), "src/main.rs"));
        app.close_file_tab(0);
        assert_eq!(app.tab, Tab::Log);
        // Once read, a text moves into a tab of the diff kind, marked as a file being read:
        // every line unchanged and numbered, so it is selected and searched like a diff.
        app.open_file_tab("src/main.rs");
        assert!(!app.promote_file_tab(0), "nothing is read yet");
        let entry = Entry { path: "src/main.rs".into(), kind: Kind::File };
        app.file_tabs[0].content = Some(files::read(&r.dir, &entry));
        assert!(app.promote_file_tab(0));
        assert!(app.file_tabs.is_empty());
        let d = &app.diffs[0];
        assert_eq!((app.tab, d.reading, d.side_by_side, d.tab_title().as_str()), (Tab::Diff(0), true, false, "main.rs"));
        // A UTF-8 text opens straight in the editor.
        assert_eq!(d.edit.as_ref().map(|t| t.e.text.as_str()), Some("fn main() {}\n"));
        assert_eq!(d.diff.unified, vec![(' ', "fn main() {}".to_string())]);
        assert_eq!(d.diff.rows[0].right, Some((1, "fn main() {}".to_string())));
        // Opened again: the same tab, not a second one.
        app.tab = Tab::Log;
        app.open_file_tab("src/main.rs");
        assert_eq!((app.tab, app.diffs.len(), app.file_tabs.len()), (Tab::Diff(0), 1, 0));
        // The file changed on disk: after a refresh the tab shows the new text.
        r.write("src/main.rs", "fn main() {\n    println!(\"hi\");\n}\n");
        app.refresh();
        assert_eq!(app.diffs[0].diff.unified.len(), 3);
        // A binary file stays in a tab that says why it can't be read.
        r.write("data.bin", "\u{0}\u{1}");
        r.commit("Add a binary");
        app.open_file_tab("data.bin");
        let i = app.file_tabs.len() - 1;
        app.file_tabs[i].content = Some(files::read(&r.dir, &Entry { path: "data.bin".into(), kind: Kind::File }));
        assert!(!app.promote_file_tab(i));
        assert_eq!(app.file_tabs.len(), 1);
    }
    #[test]
    fn a_file_open_for_reading_is_edited_and_saved() {
        let r = TestRepo::new("files-edit");
        r.write("app.yml", "name: one\r\nport: 1\r\n");
        r.commit("Add the config");
        let mut app = r.app();
        app.show_files = true;
        app.files_tick();
        app.open_file_tab("app.yml");
        app.file_tabs[0].content = Some(files::read(&r.dir, &Entry { path: "app.yml".into(), kind: Kind::File }));
        assert!(app.promote_file_tab(0));
        let t = app.diffs[0].edit.as_mut().expect("editing");
        assert_eq!(t.e.text, "name: one\nport: 1\n");
        t.e.text = "name: two\nport: 1\n".into();
        assert_eq!(app.diffs[0].tab_title(), "app.yml *");
        // Closing the tab with changes not saved asks first, and keeps it.
        app.close_diff(0);
        assert_eq!((app.diffs.len(), app.diffs[0].edit.as_ref().unwrap().ask), (1, Some(Ask::Close)));
        app.diffs[0].edit.as_mut().unwrap().ask = None;
        // Saved with the line ends the file had; the change shows in git.
        assert!(app.save_edit(0, false));
        assert_eq!(r.read("app.yml"), "name: two\r\nport: 1\r\n");
        assert_eq!(r.status(), vec![" M app.yml".to_string()]);
        assert_eq!((app.diffs[0].tab_title().as_str(), app.diffs[0].diff.unified[0].1.as_str()), ("app.yml", "name: two"));
        // Changed on disk meanwhile: not overwritten unless asked to.
        app.diffs[0].edit.as_mut().unwrap().e.text = "name: three\nport: 1\n".into();
        r.write("app.yml", "name: other\r\n");
        assert!(!app.save_edit(0, false));
        assert!(app.diffs[0].edit.as_ref().unwrap().outside);
        assert_eq!(r.read("app.yml"), "name: other\r\n");
        assert!(app.save_edit(0, true));
        assert_eq!(r.read("app.yml"), "name: three\r\nport: 1\r\n");
        // No changes here: a change on disk is simply taken in after a refresh.
        r.write("app.yml", "name: four\r\n");
        app.refresh();
        assert_eq!(app.diffs[0].edit.as_ref().unwrap().e.text, "name: four\n");
        // A file in another encoding is only read.
        r.write("latin.txt", "caf\u{e9}");
        std::fs::write(r.dir.join("latin.txt"), b"caf\xe9\n").unwrap();
        r.commit("Add a Latin-1 file");
        app.open_file_tab("latin.txt");
        let k = app.file_tabs.len() - 1;
        app.file_tabs[k].content = Some(files::read(&r.dir, &Entry { path: "latin.txt".into(), kind: Kind::File }));
        assert!(app.promote_file_tab(k));
        let j = app.diffs.len() - 1;
        assert!(app.diffs[j].edit.is_none());
        assert!(app.diffs[j].read_only.as_deref().is_some_and(|w| w.contains("UTF-8")));
        // With no changes the tab closes at once.
        app.close_diff(0);
        assert_eq!(app.diffs.len(), 1);
    }
}
