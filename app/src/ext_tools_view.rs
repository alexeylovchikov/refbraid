//! Settings → Tools → Diff & Merge Tools: external compare programs (Beyond Compare, Meld,
//! KDiff3…), the rules that pick one by file type, and the window that adds or edits a tool.

use crate::app::App;
use crate::i18n::tr;
use crate::external;
use crate::form::{self, Part};
use crate::settings::{ExtAssoc, ExtKind, ExtTool, DEFAULT_ASSOC};
use crate::theme::{self, Icon};
use eframe::egui::{self, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::sync::{Arc, Mutex};

/// The tool being added or edited.
pub struct ToolEdit {
    /// Index in the tool list; None = a new tool.
    pub index: Option<usize>,
    pub tool: ExtTool,
    /// What the test runs printed.
    pub log: Arc<Mutex<Vec<String>>>,
}

impl ToolEdit {
    pub fn new(kind: ExtKind) -> Self {
        let args = if kind == ExtKind::Diff { "%1 %2 %3" } else { "%1 %2 %3 %4" };
        ToolEdit { index: None, tool: ExtTool { kind, name: String::new(), path: String::new(), args: args.into() }, log: Arc::default() }
    }

    pub fn existing(index: usize, tool: &ExtTool) -> Self {
        ToolEdit { index: Some(index), tool: tool.clone(), log: Arc::default() }
    }
}

fn kind_title(k: ExtKind) -> &'static str {
    match k {
        ExtKind::Diff => "Diff tool",
        ExtKind::Merge => "Merge tool",
    }
}

/// File-type rules that named a removed tool go back to the built-in viewer.
fn forget_tool(assoc: &mut [ExtAssoc], t: &ExtTool) {
    for a in assoc {
        let slot = if t.kind == ExtKind::Diff { &mut a.diff } else { &mut a.merge };
        if *slot == t.name {
            slot.clear();
        }
    }
}

/// "Built-in" or one of the tools of a kind.
fn tool_choice(ui: &mut Ui, id: (usize, usize), value: &mut String, names: &[String], width: f32) {
    let shown = if value.is_empty() { "Built-in".to_string() } else { value.clone() };
    egui::ComboBox::from_id_salt(("ext_assoc_tool", id.0, id.1)).width(width).selected_text(shown).show_ui(ui, |ui| {
        ui.selectable_value(value, String::new(), "Built-in");
        for n in names {
            ui.selectable_value(value, n.clone(), n);
        }
    });
}

impl App {
    pub(crate) fn page_ext_tools(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.checkbox(&mut d.s.ext_enabled, tr("Enable external tools"));
        ui.add_space(2.0);
        ui.label(
            RichText::new(tr("Double-clicking a file opens it in the tool chosen for its type below; a single click keeps the quick view. The diff toolbar gets a button to open the file in a tool."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        ui.add_space(12.0);
        let mut edit: Option<ToolEdit> = None;
        let enabled = d.s.ext_enabled;
        ui.add_enabled_ui(enabled, |ui| {
            // The tools
            ui.label(RichText::new(tr("Configure external tools:")).color(theme::TEXT()));
            ui.add_space(6.0);
            let frame_w = ui.available_width();
            egui::Frame::new().stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(4).show(ui, |ui| {
                ui.set_width(frame_w - 2.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let add = theme::icon_button_sized(ui, Icon::Plus, tr("Add a tool"), 24.0, false, enabled);
                    egui::Popup::menu(&add).show(|ui| {
                        ui.set_min_width(150.0);
                        if ui.button(tr("Diff Tool…")).clicked() {
                            edit = Some(ToolEdit::new(ExtKind::Diff));
                        }
                        if ui.button(tr("Merge Tool…")).clicked() {
                            edit = Some(ToolEdit::new(ExtKind::Merge));
                        }
                    });
                    let sel = d.tool_sel.filter(|&i| i < d.s.ext_tools.len());
                    if theme::icon_button_sized(ui, Icon::Minus, tr("Remove the selected tool"), 24.0, false, enabled && sel.is_some()).clicked() {
                        if let Some(i) = sel {
                            let t = d.s.ext_tools.remove(i);
                            forget_tool(&mut d.s.ext_assoc, &t);
                            d.tool_sel = None;
                        }
                    }
                    if theme::icon_button_sized(ui, Icon::Pencil, tr("Edit the selected tool"), 24.0, false, enabled && sel.is_some()).clicked() {
                        if let Some(i) = sel {
                            edit = Some(ToolEdit::existing(i, &d.s.ext_tools[i]));
                        }
                    }
                });
                let w = ui.available_width();
                let top = ui.cursor().top();
                theme::hline(ui, top + 0.5, ui.max_rect().left(), ui.max_rect().left() + w);
                ui.add_space(4.0);
                let mut any = false;
                for kind in [ExtKind::Diff, ExtKind::Merge] {
                    let idx: Vec<usize> = d.s.ext_tools.iter().enumerate().filter(|(_, t)| t.kind == kind).map(|(i, _)| i).collect();
                    if idx.is_empty() {
                        continue;
                    }
                    any = true;
                    let (r, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
                    theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(11.0, 11.0)), Icon::ChevronDown, theme::TEXT_MID());
                    ui.painter().text(pos2(r.left() + 26.0, r.center().y), egui::Align2::LEFT_CENTER, kind_title(kind), FontId::proportional(13.0), theme::TEXT());
                    for i in idx {
                        let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
                        if d.tool_sel == Some(i) {
                            ui.painter().rect_filled(r, 0.0, theme::SELECT());
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 0.0, theme::HOVER());
                        }
                        let t = &d.s.ext_tools[i];
                        theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 46.0, r.center().y), vec2(14.0, 14.0)), Icon::Diff, theme::TEXT_MID());
                        ui.painter().text(pos2(r.left() + 60.0, r.center().y), egui::Align2::LEFT_CENTER, &t.name, FontId::proportional(13.0), theme::TEXT());
                        let resp = resp.on_hover_text(format!("{} {}", t.path, t.args));
                        if resp.clicked() {
                            d.tool_sel = Some(i);
                        }
                        if resp.double_clicked() {
                            edit = Some(ToolEdit::existing(i, &d.s.ext_tools[i]));
                        }
                    }
                }
                if !any {
                    let (r, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, tr("No tools yet: add Beyond Compare, Meld, KDiff3… with +"), FontId::proportional(13.0), theme::TEXT_DIM());
                }
                let used = ui.cursor().top() - top;
                if used < 130.0 {
                    ui.add_space(130.0 - used);
                }
            });

            // Which tool each type of file opens with
            ui.add_space(18.0);
            ui.label(RichText::new(tr("Configure external diff/merge tools associated with a file type:")).color(theme::TEXT()));
            ui.add_space(6.0);
            let names = |k: ExtKind| d.s.ext_tools.iter().filter(|t| t.kind == k).map(|t| t.name.clone()).collect::<Vec<_>>();
            let (diff_names, merge_names) = (names(ExtKind::Diff), names(ExtKind::Merge));
            let rows_n = d.s.ext_assoc_rows().len();
            egui::Frame::new().stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(4).show(ui, |ui| {
                ui.set_width(frame_w - 2.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    if theme::icon_button_sized(ui, Icon::Plus, tr("Add a file type"), 24.0, false, enabled).clicked() {
                        d.s.ext_assoc_rows().push(ExtAssoc::default());
                        d.assoc_sel = Some(rows_n);
                    }
                    // The Default row stays.
                    let sel = d.assoc_sel.filter(|&i| i > 0 && i < rows_n);
                    if theme::icon_button_sized(ui, Icon::Minus, tr("Remove the selected file type"), 24.0, false, enabled && sel.is_some()).clicked() {
                        if let Some(i) = sel {
                            d.s.ext_assoc_rows().remove(i);
                            d.assoc_sel = None;
                        }
                    }
                });
                let w = ui.available_width();
                let (c1, c2) = ((w * 0.38).max(150.0), (w * 0.69).max(300.0));
                let (hr, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
                ui.painter().rect_filled(hr, 0.0, theme::BG_BAR());
                theme::hline(ui, hr.top() + 0.5, hr.left(), hr.right());
                theme::hline(ui, hr.bottom() - 0.5, hr.left(), hr.right());
                for (x, t) in [(0.0, "File Type"), (c1, "Diff Tool"), (c2, "Merge Tool")] {
                    ui.painter().text(pos2(hr.left() + x + 8.0, hr.center().y), egui::Align2::LEFT_CENTER, t, FontId::proportional(13.0), theme::TEXT());
                }
                theme::vline(ui, hr.left() + c1, hr.top(), hr.bottom());
                theme::vline(ui, hr.left() + c2, hr.top(), hr.bottom());
                let assoc_sel = &mut d.assoc_sel;
                let rows = d.s.ext_assoc_rows();
                for (i, a) in rows.iter_mut().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
                    if *assoc_sel == Some(i) {
                        ui.painter().rect_filled(r, 0.0, theme::SELECT());
                    }
                    if resp.clicked() {
                        *assoc_sel = Some(i);
                    }
                    let cell = |x0: f32, x1: f32| Rect::from_min_max(pos2(r.left() + x0 + 4.0, r.top() + 3.0), pos2(r.left() + x1 - 4.0, r.bottom() - 3.0));
                    let (pr, dr, mr) = (cell(0.0, c1), cell(c1, c2), cell(c2, w));
                    if a.pattern == DEFAULT_ASSOC {
                        ui.painter().text(pos2(pr.left() + 4.0, r.center().y), egui::Align2::LEFT_CENTER, DEFAULT_ASSOC, FontId::proportional(13.0), theme::TEXT());
                    } else {
                        let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(pr));
                        let te = egui::TextEdit::singleline(&mut a.pattern)
                            .id(egui::Id::unique(("ext_assoc_pattern", i)))
                            .hint_text(RichText::new(tr("*.json")).color(theme::TEXT_DIM()))
                            .font(FontId::monospace(12.5))
                            .desired_width(pr.width());
                        let er = theme::edit_menu(cu.add(te));
                        if er.has_focus() || er.clicked() {
                            *assoc_sel = Some(i);
                        }
                    }
                    let mut du = ui.new_child(egui::UiBuilder::new().max_rect(dr));
                    tool_choice(&mut du, (i, 0), &mut a.diff, &diff_names, dr.width() - 10.0);
                    let mut mu = ui.new_child(egui::UiBuilder::new().max_rect(mr));
                    tool_choice(&mut mu, (i, 1), &mut a.merge, &merge_names, mr.width() - 10.0);
                    theme::vline(ui, r.left() + c1, r.top(), r.bottom());
                    theme::vline(ui, r.left() + c2, r.top(), r.bottom());
                }
                ui.add_space(6.0);
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new(tr("File Type is a file name pattern such as *.json, *.tpl.* or Makefile. The first matching row is used and Default covers all other files. Built-in keeps RefBraid's own diff or merge window."))
                    .color(theme::TEXT_DIM())
                    .size(12.0),
            );
        });
        if let Some(e) = edit {
            d.tool_edit = Some(e);
        }
    }

    /// The "Edit An External Tool" window, over Settings.
    pub(crate) fn ext_tool_window(&mut self, ctx: &egui::Context) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        let Some(mut e) = d.tool_edit.take() else { return };
        let title = match (e.index, e.tool.kind) {
            (Some(_), _) => "Edit An External Tool",
            (None, ExtKind::Diff) => "Add Diff Tool",
            (None, ExtKind::Merge) => "Add Merge Tool",
        };
        let taken: Vec<String> = d
            .s
            .ext_tools
            .iter()
            .enumerate()
            .filter(|(i, t)| Some(*i) != e.index && t.kind == e.tool.kind)
            .map(|(_, t)| t.name.trim().to_string())
            .collect();
        let problem = if e.tool.path.trim().is_empty() {
            Some("Choose the program.")
        } else if e.tool.name.trim().is_empty() {
            Some("Give the tool a name.")
        } else if taken.iter().any(|n| *n == e.tool.name.trim()) {
            Some("Another tool already has this name.")
        } else {
            None
        };
        let (mut ok, mut cancel) = (false, false);
        let merge = e.tool.kind == ExtKind::Merge;
        let closed = form::show(ctx, "ext_tool_edit", title, 600.0, |ui, part| match part {
            Part::Body => {
                egui::Grid::new("ext_tool_grid").num_columns(2).spacing(vec2(12.0, 10.0)).show(ui, |ui| {
                    ui.label("Program path:");
                    ui.horizontal(|ui| {
                        let w = 600.0 - 40.0 - 130.0 - 36.0;
                        let r = ui.add(egui::TextEdit::singleline(&mut e.tool.path).hint_text(tr("/usr/bin/bcompare")).desired_width(w));
                        theme::edit_menu(r);
                        if theme::icon_button(ui, Icon::Folder, tr("Choose the program")).clicked() {
                            if let Some(p) = rfd::FileDialog::new().set_title("External Tool").pick_file() {
                                if e.tool.name.trim().is_empty() {
                                    let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                    let mut c = n.chars();
                                    e.tool.name = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
                                }
                                e.tool.path = p.display().to_string();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Tool name:");
                    let r = ui.add(egui::TextEdit::singleline(&mut e.tool.name).hint_text(tr("Beyond Compare")).desired_width(f32::INFINITY));
                    theme::edit_menu(r);
                    ui.end_row();
                    ui.label("Argument pattern:");
                    let r = ui.add(egui::TextEdit::singleline(&mut e.tool.args).font(FontId::monospace(13.0)).desired_width(f32::INFINITY));
                    theme::edit_menu(r);
                    ui.end_row();
                });
                ui.add_space(10.0);
                let help = if merge {
                    "Different tools take different parameters; put the files where your tool expects them. Use quotes around an argument with spaces.\n%1 - left (local changes, ours)\n%2 - right (incoming changes, theirs)\n%3 - base (the common version both sides started from)\n%4 - result (where the merged file is saved)"
                } else {
                    "Different tools take different parameters; put the files where your tool expects them. Use quotes around an argument with spaces.\n%1 - left (the version before the change)\n%2 - right (the version after it, e.g. your local changes)\n%3 - base (the common version, for a three-side diff; left out when there is none)"
                };
                ui.label(RichText::new(help).color(theme::TEXT_MID()).size(12.5));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let can = !e.tool.path.trim().is_empty();
                    for (three, label) in [(false, "Test Diff"), (true, "Test Three-Side Diff")] {
                        if ui.add_enabled(can, egui::Button::new(label).min_size(vec2(0.0, 26.0))).clicked() {
                            let msg = match external::sample_files(three) {
                                Ok(f) => {
                                    let result = if merge { f[3].as_deref() } else { None };
                                    match external::launch(&e.tool, [f[0].as_deref(), f[1].as_deref(), f[2].as_deref(), result], Some(e.log.clone())) {
                                        Ok(cmd) => format!("Started: {cmd}"),
                                        Err(err) => err,
                                    }
                                }
                                Err(err) => format!("Could not write the sample files: {err}"),
                            };
                            e.log.lock().unwrap_or_else(|p| p.into_inner()).push(msg);
                        }
                    }
                });
                ui.add_space(8.0);
                let lines = e.log.lock().unwrap_or_else(|p| p.into_inner()).clone();
                egui::Frame::new()
                    .fill(theme::BG_INPUT())
                    .stroke(Stroke::new(1.0, theme::INPUT_BORDER()))
                    .corner_radius(4)
                    .inner_margin(6)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical().id_salt("ext_tool_log").min_scrolled_height(110.0).max_height(110.0).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                            if lines.is_empty() {
                                ui.label(RichText::new(tr("The output of a test run appears here.")).color(theme::TEXT_DIM()).size(12.5));
                            }
                            for l in &lines {
                                ui.label(RichText::new(l).monospace().size(12.0).color(theme::TEXT()));
                            }
                        });
                    });
            }
            Part::Footer => {
                let (o, c) = form::ok_cancel(ui, tr("OK"), problem.is_none(), false);
                ok = o;
                cancel = c;
                if let Some(p) = problem {
                    ui.label(RichText::new(p).color(theme::TEXT_DIM()).size(12.5));
                }
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
        if ok {
            let mut t = e.tool.clone();
            t.name = t.name.trim().to_string();
            t.path = t.path.trim().to_string();
            match e.index.filter(|&i| i < d.s.ext_tools.len()) {
                Some(i) => {
                    let old = d.s.ext_tools[i].name.clone();
                    if old != t.name {
                        for a in d.s.ext_assoc.iter_mut() {
                            let slot = if t.kind == ExtKind::Diff { &mut a.diff } else { &mut a.merge };
                            if *slot == old {
                                *slot = t.name.clone();
                            }
                        }
                    }
                    d.s.ext_tools[i] = t;
                }
                None => {
                    d.s.ext_tools.push(t);
                    d.tool_sel = Some(d.s.ext_tools.len() - 1);
                }
            }
        } else if !cancel && !closed {
            d.tool_edit = Some(e);
        }
    }
}
