//! The look shared by all modal windows: a title bar with a close cross, padded content
//! and a footer with the buttons on the right, like the Settings window.

use crate::theme::{self, Icon};
use crate::i18n::tr;
use eframe::egui::{self, pos2, vec2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui};

/// Which part of the window the callback is asked to fill.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The content, laid out top to bottom.
    Body,
    /// The footer, laid out right to left: add the rightmost button first.
    Footer,
}

const TITLE_H: f32 = 36.0;
const FOOT_H: f32 = 50.0;

/// Show a modal window. `add` is called once for the body and once for the footer.
/// Returns true when the person asked to close it (cross, Escape or a click outside).
pub fn show(ctx: &egui::Context, id: &str, title: &str, width: f32, add: impl FnMut(&mut Ui, Part)) -> bool {
    show_with(ctx, id, title, width, true, add)
}

/// Like [`show`]; with `padded` false the body reaches the window's edges (for split panes).
///
/// The window can be moved by its title bar and made bigger with the grip in its bottom-right
/// corner (or its right and bottom edges); a double-click on the title puts it back. Each
/// window (by id and title) remembers its place and size while the program runs. Lists in the
/// body can take the added height with [`grow`].
pub fn show_with(ctx: &egui::Context, id: &str, title: &str, width: f32, padded: bool, add: impl FnMut(&mut Ui, Part)) -> bool {
    let shown = crate::i18n::tr(title).to_string();
    show_titled(ctx, id, title, &shown, width, padded, add)
}

/// Like [`show_with`], with a title bar text (`shown`, already translated) that may change —
/// such as a file name — while `title` keeps the remembered place and size.
pub fn show_titled(ctx: &egui::Context, id: &str, title: &str, shown: &str, width: f32, padded: bool, mut add: impl FnMut(&mut Ui, Part)) -> bool {
    let mut close = false;
    let wkey = format!("{id}\u{1}{title}");
    let delta = delta(ctx, &wkey);
    let base = width;
    let width = (width + delta.x).max(width * 0.7).min(ctx.content_rect().width() - 16.0).max(240.0);
    let extra = delta.y.max(0.0);
    let natural_key = key(&wkey, "natural");
    let m = egui::Modal::new(egui::Id::unique(id))
        .area(area(ctx, id, &wkey))
        .frame(egui::Frame::new().fill(theme::BG()).stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(8))
        .show(ctx, |ui| {
            let spacing = ui.spacing().item_spacing;
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            // Title bar
            let (title_r, _) = ui.allocate_exact_size(vec2(width, TITLE_H), Sense::hover());
            title_drag(ui, title_r.with_max_x(title_r.right() - 36.0), &wkey);
            ui.painter().rect_filled(title_r, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, theme::BG_BAR());
            theme::hline(ui, title_r.bottom() - 0.5, title_r.left(), title_r.right());
            theme::paint_clipped(ui.painter(), pos2((title_r.center().x - theme::text_width(ui, shown, &FontId::proportional(14.0)) / 2.0).max(title_r.left() + 12.0), title_r.center().y), shown, FontId::proportional(14.0), theme::TEXT(), title_r.width() - 60.0);
            let close_r = Rect::from_center_size(pos2(title_r.right() - 20.0, title_r.center().y), vec2(24.0, 24.0));
            let cr = ui.interact(close_r, egui::Id::unique((id, "x")), Sense::click()).on_hover_text(tr("Close"));
            if cr.hovered() {
                ui.painter().rect_filled(close_r, 4.0, theme::HOVER());
            }
            theme::paint_icon(ui.painter(), close_r.shrink(6.0), Icon::Close, theme::TEXT_MID());
            if cr.clicked() {
                close = true;
            }
            // Body
            let margin = if padded { egui::Margin { left: 20, right: 20, top: 16, bottom: 18 } } else { egui::Margin::ZERO };
            let natural: Option<f32> = ui.ctx().data(|d| d.get_temp(natural_key));
            egui::Frame::new()
                .inner_margin(margin)
                .show(ui, |ui| {
                    ui.set_width(width - margin.sum().x);
                    ui.spacing_mut().item_spacing = spacing;
                    ui.ctx().data_mut(|d| d.insert_temp(grow_key(), extra));
                    add(ui, Part::Body);
                    ui.ctx().data_mut(|d| d.insert_temp(grow_key(), 0.0f32));
                    let h = ui.min_rect().height();
                    match natural {
                        // Made bigger: the body keeps the added height even when nothing in it
                        // takes it, so the footer stays at the bottom.
                        // (add_space, not set_min_height: that one counts from the cursor.)
                        Some(n) if extra > 0.0 => ui.add_space((n + extra - h).max(0.0)),
                        _ => ui.ctx().data_mut(|d| {
                            d.insert_temp(natural_key, h);
                        }),
                    }
                });
            // Footer
            let (foot, _) = ui.allocate_exact_size(vec2(width, FOOT_H), Sense::hover());
            theme::hline(ui, foot.top() + 0.5, foot.left(), foot.right());
            let mut fu = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(foot.shrink2(vec2(16.0, 10.0)))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            fu.spacing_mut().item_spacing = vec2(8.0, spacing.y);
            add(&mut fu, Part::Footer);
            let whole = ui.min_rect();
            resize_grip(ui, whole, &wkey, vec2(-0.3 * base, 0.0));
        });
    keep_on_screen(ctx, &wkey, m.response.rect);
    close || m.should_close()
}

fn key(wkey: &str, what: &str) -> egui::Id {
    egui::Id::unique(("form_window", wkey, what))
}

fn grow_key() -> egui::Id {
    egui::Id::unique("form_grow_now")
}

/// Height the person added to the window being drawn: lists and text boxes in its body can
/// add it to their own height to use the room.
pub fn grow(ui: &Ui) -> f32 {
    ui.ctx().data(|d| d.get_temp(grow_key())).unwrap_or(0.0)
}

/// How far the person moved the window from the middle of the screen.
pub fn offset(ctx: &egui::Context, wkey: &str) -> egui::Vec2 {
    ctx.data(|d| d.get_temp(key(wkey, "offset"))).unwrap_or_default()
}

/// How much bigger (or narrower) the person made the window than its own size.
pub fn delta(ctx: &egui::Context, wkey: &str) -> egui::Vec2 {
    ctx.data(|d| d.get_temp(key(wkey, "delta"))).unwrap_or_default()
}

/// The area of a modal window: centred, shifted by where it was moved to.
pub fn area(ctx: &egui::Context, id: &str, wkey: &str) -> egui::Area {
    egui::Modal::default_area(egui::Id::unique(id)).anchor(egui::Align2::CENTER_CENTER, offset(ctx, wkey))
}

/// Dragging the title bar moves the window; a double-click puts it back in the middle at its
/// own size.
pub fn title_drag(ui: &Ui, rect: Rect, wkey: &str) {
    let r = ui.interact(rect, key(wkey, "title"), Sense::click_and_drag());
    if r.dragged() {
        let d = r.drag_delta();
        ui.ctx().data_mut(|m| {
            let o: &mut egui::Vec2 = m.get_temp_mut_or_default(key(wkey, "offset"));
            *o += d;
        });
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if r.double_clicked() {
        ui.ctx().data_mut(|m| {
            m.remove::<egui::Vec2>(key(wkey, "offset"));
            m.remove::<egui::Vec2>(key(wkey, "delta"));
        });
    }
}

/// The grip in the bottom-right corner and the right and bottom edges: dragging them makes
/// the window bigger or smaller (not below `min`, the smallest change of its own size).
pub fn resize_grip(ui: &Ui, rect: Rect, wkey: &str, min: egui::Vec2) {
    let corner = Rect::from_min_max(rect.max - vec2(16.0, 16.0), rect.max);
    let right = Rect::from_min_max(pos2(rect.right() - 5.0, rect.top() + TITLE_H), pos2(rect.right() + 2.0, corner.top()));
    let bottom = Rect::from_min_max(pos2(rect.left() + 8.0, rect.bottom() - 5.0), pos2(corner.left(), rect.bottom() + 2.0));
    // Three short diagonal lines, as most programs draw a resize grip.
    let p = ui.painter();
    for i in 1..=3 {
        let o = i as f32 * 4.0;
        p.line_segment([pos2(rect.right() - 3.0 - o, rect.bottom() - 3.0), pos2(rect.right() - 3.0, rect.bottom() - 3.0 - o)], Stroke::new(1.0, theme::TEXT_DIM()));
    }
    let max = ui.ctx().content_rect().size();
    for (r, which, axes, cursor) in [
        (corner, "grip", vec2(1.0, 1.0), egui::CursorIcon::ResizeNwSe),
        (right, "grip_r", vec2(1.0, 0.0), egui::CursorIcon::ResizeHorizontal),
        (bottom, "grip_b", vec2(0.0, 1.0), egui::CursorIcon::ResizeVertical),
    ] {
        let resp = ui.interact(r, key(wkey, which), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if resp.dragged() {
            let d = resp.drag_delta() * axes;
            ui.ctx().data_mut(|m| {
                let cur: egui::Vec2 = m.get_temp(key(wkey, "delta")).unwrap_or_default();
                let new = (cur + d).max(min).min(max);
                let moved = new - cur;
                m.insert_temp(key(wkey, "delta"), new);
                // The window is anchored at its middle: shift it by half the change so the
                // top-left corner stays where it is.
                let o: &mut egui::Vec2 = m.get_temp_mut_or_default(key(wkey, "offset"));
                *o += moved / 2.0;
            });
        }
    }
}

/// A vertical divider between two panes of a window that can be dragged: returns the width of
/// the left pane (starting at `default`, kept at least `min` on either side). Remembered while
/// the program runs; a double-click puts it back.
///
/// Lay the panes out with [`split_width`] first and call this after them, so the divider gets
/// the pointer before the panes' own rows.
pub fn splitter(ui: &Ui, rect: Rect, name: &str, default: f32, min: f32) -> f32 {
    let id = egui::Id::unique(("form_splitter", name));
    let lo = min.min(rect.width() / 2.0);
    let w = split_width(ui.ctx(), rect, name, default, min);
    let x = rect.left() + w;
    let hit = Rect::from_min_max(pos2(x - 3.0, rect.top()), pos2(x + 4.0, rect.bottom()));
    let resp = ui.interact(hit, id, Sense::click_and_drag());
    let active = resp.hovered() || resp.dragged();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let col = if active { theme::ACCENT() } else { theme::BORDER() };
    ui.painter().line_segment([pos2(x + 0.5, rect.top()), pos2(x + 0.5, rect.bottom())], Stroke::new(if active { 2.0 } else { 1.0 }, col));
    if resp.dragged() {
        let nw = (w + resp.drag_delta().x).max(lo).min(rect.width() - lo);
        ui.ctx().data_mut(|d| d.insert_temp(id, nw));
    }
    if resp.double_clicked() {
        ui.ctx().data_mut(|d| d.remove::<f32>(id));
    }
    w
}

/// The left pane's width for [`splitter`].
pub fn split_width(ctx: &egui::Context, rect: Rect, name: &str, default: f32, min: f32) -> f32 {
    let stored: f32 = ctx.data(|d| d.get_temp(egui::Id::unique(("form_splitter", name)))).unwrap_or(default);
    let lo = min.min(rect.width() / 2.0);
    stored.max(lo).min(rect.width() - lo)
}

/// Don't let a moved window go off the screen (and keep the moving smooth when it would).
pub fn keep_on_screen(ctx: &egui::Context, wkey: &str, rect: Rect) {
    let screen = ctx.content_rect();
    let room = ((screen.size() - rect.size()) / 2.0).max(vec2(0.0, 0.0));
    ctx.data_mut(|m| {
        let o: &mut egui::Vec2 = m.get_temp_mut_or_default(key(wkey, "offset"));
        *o = o.max(-room).min(room);
    });
}

/// The main (blue) footer button. Enter presses it too when it is enabled.
pub fn primary(ui: &mut Ui, text: &str, enabled: bool, enter: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(theme::ACCENT()).min_size(vec2(84.0, 28.0)),
    )
    .clicked()
        || (enabled && enter)
}

/// A plain footer button.
pub fn button(ui: &mut Ui, text: &str) -> bool {
    ui.add(egui::Button::new(text).min_size(vec2(84.0, 28.0))).clicked()
}

/// The usual pair: "Cancel" on the right, the main button to its left. Returns (ok, cancel).
pub fn ok_cancel(ui: &mut Ui, ok: &str, enabled: bool, enter: bool) -> (bool, bool) {
    let cancel = button(ui, crate::i18n::tr("Cancel"));
    let ok = primary(ui, ok, enabled, enter);
    (ok, cancel)
}
