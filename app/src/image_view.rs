//! Picture files in a diff tab: the version before and after side by side (or one of them),
//! with zoom (buttons, mouse wheel, keys), dragging and rotation by quarter turns.

use crate::app::DiffTab;
use crate::i18n::tr;
use crate::theme::{self, Icon};
use eframe::egui::{self, Color32, FontId, Key, Mesh, Pos2, Rect, RichText, Sense, Shape, TextureHandle, Ui, Vec2, pos2, vec2};
use std::sync::{Arc, OnceLock};

/// One version of the picture, ready to draw.
pub enum Side {
    /// The file does not exist on this side (added or deleted).
    Missing,
    Ok { tex: TextureHandle, size: [usize; 2], bytes: usize },
    Err(String),
}

pub struct ImageState {
    pub before: Side,
    pub after: Side,
    /// None = fit the window; otherwise screen pixels per picture pixel.
    pub zoom: Option<f32>,
    /// Quarter turns clockwise.
    pub rot: u8,
    /// Moved by dragging, from the middle of the view.
    pub offset: Vec2,
    format: String,
    /// Before and after next to each other, or one picture. A picture without a "before" (a new
    /// file) or without an "after" (a deleted one) starts with the one it has.
    pub side_by_side: bool,
}

const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 64.0;

fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

/// Decode a picture into pixels no larger than `max_side`; also gives its real size.
pub fn decode(bytes: &[u8], path: &str, max_side: usize) -> Result<(egui::ColorImage, [usize; 2]), String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".svg") || lower.ends_with(".svgz") {
        let opt = resvg::usvg::Options { fontdb: fonts(), ..Default::default() };
        let tree = resvg::usvg::Tree::from_data(bytes, &opt).map_err(|e| e.to_string())?;
        let s = tree.size();
        let (w, h) = (s.width().max(1.0), s.height().max(1.0));
        // Vector pictures are drawn sharp enough to zoom in a few times.
        let target = (max_side.min(4096)) as f32;
        let k = (target / w.max(h)).min(8.0);
        let (pw, ph) = ((w * k).round().max(1.0) as u32, (h * k).round().max(1.0) as u32);
        let mut pix = resvg::tiny_skia::Pixmap::new(pw, ph).ok_or("The picture is too large")?;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
        let img = egui::ColorImage::from_rgba_premultiplied([pw as usize, ph as usize], pix.data());
        return Ok((img, [w.round() as usize, h.round() as usize]));
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let size = [img.width() as usize, img.height() as usize];
    let img = if img.width().max(img.height()) as usize > max_side {
        img.resize(max_side as u32, max_side as u32, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok((egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()), size))
}

fn load(ctx: &egui::Context, bytes: Option<&Arc<Vec<u8>>>, path: &str, name: &str) -> Side {
    let Some(b) = bytes else { return Side::Missing };
    let max = ctx.input(|i| i.max_texture_side).clamp(512, 8192);
    match decode(b, path, max) {
        Ok((img, size)) => {
            // Small raster pictures (icons) stay crisp when zoomed in; SVGs are drawn large and smooth.
            let opts = if img.size[0].max(img.size[1]) <= 256 { egui::TextureOptions::NEAREST } else { egui::TextureOptions::LINEAR };
            Side::Ok { tex: ctx.load_texture(name, img, opts), size, bytes: b.len() }
        }
        Err(e) => Side::Err(e),
    }
}

impl ImageState {
    pub fn new(ctx: &egui::Context, pair: &crate::git::ImagePair) -> Self {
        let format = pair.path.rsplit_once('.').map(|(_, e)| e.to_ascii_uppercase()).unwrap_or_default();
        ImageState {
            before: load(ctx, pair.before.as_ref(), &pair.path, &format!("img-before:{}", pair.path)),
            after: load(ctx, pair.after.as_ref(), &pair.path, &format!("img-after:{}", pair.path)),
            zoom: None,
            rot: 0,
            offset: Vec2::ZERO,
            format,
            side_by_side: pair.before.is_some() && pair.after.is_some(),
        }
    }

    fn main_size(&self) -> Option<[usize; 2]> {
        match (&self.after, &self.before) {
            (Side::Ok { size, .. }, _) | (_, Side::Ok { size, .. }) => Some(*size),
            _ => None,
        }
    }
}

fn human_bytes(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// A grey checkerboard behind the picture so transparent parts are visible.
fn checker(ctx: &egui::Context) -> TextureHandle {
    static TEX: OnceLock<TextureHandle> = OnceLock::new();
    TEX.get_or_init(|| {
        let (a, b) = (Color32::from_gray(0x3a), Color32::from_gray(0x2c));
        let img = egui::ColorImage::new([2, 2], vec![a, b, b, a]);
        let opts = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Nearest, wrap_mode: egui::TextureWrapMode::Repeat, ..Default::default() };
        ctx.load_texture("img-checker", img, opts)
    })
    .clone()
}

/// The picture rotated by `rot` quarter turns, filling `rect` (already in turned proportions).
fn rotated_mesh(tex: egui::TextureId, rect: Rect, rot: u8) -> Mesh {
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    let mut m = Mesh::with_texture(tex);
    for (i, c) in corners.iter().enumerate() {
        m.vertices.push(egui::epaint::Vertex { pos: *c, uv: uvs[(i + 4 - rot as usize % 4) % 4], color: Color32::WHITE });
    }
    m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    m
}

fn turned(size: [usize; 2], rot: u8) -> Vec2 {
    if rot % 2 == 1 { vec2(size[1] as f32, size[0] as f32) } else { vec2(size[0] as f32, size[1] as f32) }
}

fn fit_scale(pane: Rect, size: Vec2) -> f32 {
    ((pane.width() - 24.0) / size.x).min((pane.height() - 24.0) / size.y).clamp(MIN_ZOOM, 8.0)
}

/// Draw one side; returns the scale used and the response (for wheel and drag).
fn pane(ui: &mut Ui, st: &mut ImageState, which: u8, rect: Rect, title: &str) {
    let side = if which == 0 { &st.before } else { &st.after };
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), 24.0));
    let body = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
    let info = match side {
        Side::Ok { size, bytes, .. } => format!("{}×{} · {} · {}", size[0], size[1], st.format, human_bytes(*bytes)),
        Side::Missing => "no file".into(),
        Side::Err(_) => "cannot be shown".into(),
    };
    let p = ui.painter_at(rect);
    let t = p.text(pos2(head.left() + 10.0, head.center().y), egui::Align2::LEFT_CENTER, title, FontId::proportional(12.5), theme::TEXT());
    p.text(pos2(t.right() + 8.0, head.center().y), egui::Align2::LEFT_CENTER, info, FontId::proportional(12.0), theme::TEXT_DIM());
    let resp = ui.interact(body, ui.scope_id().with(("img_pane", which)), Sense::click_and_drag());
    let p = ui.painter_at(body);
    let (tex, size) = match side {
        Side::Ok { tex, size, .. } => (tex.id(), *size),
        Side::Missing => {
            let msg = if which == 0 { "The file is new in this change" } else { "The file was deleted" };
            p.text(body.center(), egui::Align2::CENTER_CENTER, msg, FontId::proportional(13.0), theme::TEXT_DIM());
            return;
        }
        Side::Err(e) => {
            p.text(body.center(), egui::Align2::CENTER_CENTER, format!("Cannot show this picture: {e}"), FontId::proportional(13.0), theme::RED());
            return;
        }
    };
    let shown = turned(size, st.rot);
    let fit = fit_scale(body, shown);
    let scale = st.zoom.unwrap_or(fit);
    let center = body.center() + st.offset;
    let r = Rect::from_center_size(center, shown * scale);
    // Checkerboard under the picture, in 10-pixel squares.
    let ck = checker(ui.ctx());
    let mut m = Mesh::with_texture(ck.id());
    let uv = |q: Pos2| pos2((q.x - r.left()) / 20.0, (q.y - r.top()) / 20.0);
    m.add_rect_with_uv(r, Rect::from_min_max(uv(r.min), uv(r.max)), Color32::WHITE);
    p.add(Shape::mesh(m));
    p.add(Shape::mesh(rotated_mesh(tex, r, st.rot)));
    // Wheel zooms around the pointer, dragging moves the picture, double-click fits / 1:1.
    if resp.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 300.0);
        if wheel.abs() > 0.1 {
            if let Some(ptr) = ui.input(|i| i.pointer.hover_pos()) {
                let new = (scale * (wheel * 0.0025).exp()).clamp(MIN_ZOOM, MAX_ZOOM);
                let new_center = ptr - (ptr - center) * (new / scale);
                st.offset = new_center - body.center();
                st.zoom = Some(new);
            }
        }
    }
    if resp.dragged() {
        st.offset += resp.drag_delta();
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if resp.double_clicked() {
        st.zoom = if st.zoom.is_none() { Some(1.0) } else { None };
        st.offset = Vec2::ZERO;
    }
}

/// The diff tab of a picture file: its own toolbar and the picture(s).
pub fn image_tab(ui: &mut Ui, d: &mut DiffTab) {
    let Some(pair) = d.diff.images.clone() else { return };
    if d.image.is_none() {
        d.image = Some(ImageState::new(ui.ctx(), &pair));
    }
    let st = d.image.as_mut().unwrap();
    let both = st.side_by_side;
    let zoom_by = |st: &mut ImageState, k: f32, fit: f32| {
        let cur = st.zoom.unwrap_or(fit);
        st.zoom = Some((cur * k).clamp(MIN_ZOOM, MAX_ZOOM));
        st.offset *= k;
    };
    // The scale "fit" would use in the right-hand (or only) view, for the zoom label and buttons.
    let avail = ui.available_rect_before_wrap();
    let pane_w = if both { avail.width() / 2.0 } else { avail.width() };
    let fit_now = st.main_size().map(|s| fit_scale(Rect::from_min_size(Pos2::ZERO, vec2(pane_w, avail.height() - 58.0)), turned(s, st.rot))).unwrap_or(1.0);
    let (k_in, k_out, k_fit, k_one) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::COMMAND, Key::Plus) || i.consume_key(egui::Modifiers::COMMAND, Key::Equals) || i.consume_key(egui::Modifiers::NONE, Key::Plus),
            i.consume_key(egui::Modifiers::COMMAND, Key::Minus) || i.consume_key(egui::Modifiers::NONE, Key::Minus),
            i.consume_key(egui::Modifiers::COMMAND, Key::Num0),
            i.consume_key(egui::Modifiers::COMMAND, Key::Num1),
        )
    });
    egui::Panel::top("image_bar")
        .exact_size(34.0)
        .resizable(false)
        .frame(egui::Frame::new().fill(theme::BG()).inner_margin(egui::Margin::symmetric(10, 4)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.selectable_value(&mut st.side_by_side, true, tr("Side-by-side")).on_hover_text(tr("Before and after next to each other"));
                ui.selectable_value(&mut st.side_by_side, false, tr("After only")).on_hover_text(tr("Only the new version (or the old one if the file was deleted)"));
                ui.add_space(10.0);
                if theme::icon_button(ui, Icon::Minus, tr("Zoom Out  Ctrl+−")).clicked() || k_out {
                    zoom_by(st, 1.0 / 1.25, fit_now);
                }
                let pct = (st.zoom.unwrap_or(fit_now) * 100.0).round();
                ui.label(RichText::new(format!("{pct}%")).color(theme::TEXT_MID()).monospace()).on_hover_text(tr("Mouse wheel zooms, drag to move, double-click switches Fit / 100%"));
                if theme::icon_button(ui, Icon::Plus, tr("Zoom In  Ctrl++")).clicked() || k_in {
                    zoom_by(st, 1.25, fit_now);
                }
                if ui.selectable_label(st.zoom.is_none(), "Fit").on_hover_text(tr("Fit the window  Ctrl+0")).clicked() || k_fit {
                    st.zoom = None;
                    st.offset = Vec2::ZERO;
                }
                if ui.selectable_label(st.zoom == Some(1.0), "1:1").on_hover_text(tr("Actual size  Ctrl+1")).clicked() || k_one {
                    st.zoom = Some(1.0);
                    st.offset = Vec2::ZERO;
                }
                ui.add_space(10.0);
                if ui.button("⟲").on_hover_text(tr("Rotate Left")).clicked() {
                    st.rot = (st.rot + 3) % 4;
                    st.offset = Vec2::ZERO;
                }
                if ui.button("⟳").on_hover_text(tr("Rotate Right")).clicked() {
                    st.rot = (st.rot + 1) % 4;
                    st.offset = Vec2::ZERO;
                }
                ui.add_space(12.0);
                ui.label(RichText::new(&d.subtitle).color(theme::TEXT_MID()));
            });
        });
    let st = d.image.as_mut().unwrap();
    egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG())).show(ui, |ui| {
        let r = ui.max_rect();
        theme::hline(ui, r.top() + 0.5, r.left(), r.right());
        let r = Rect::from_min_max(pos2(r.left(), r.top() + 1.0), r.max);
        if st.side_by_side {
            let mid = r.center().x;
            pane(ui, st, 0, Rect::from_min_max(r.min, pos2(mid - 0.5, r.bottom())), "Before");
            theme::vline(ui, mid, r.top(), r.bottom());
            pane(ui, st, 1, Rect::from_min_max(pos2(mid + 0.5, r.top()), r.max), "After");
        } else {
            let which = if matches!(st.after, Side::Missing) { 0 } else { 1 };
            pane(ui, st, which, r, if which == 0 { "Before" } else { "After" });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turn_uvs() {
        let m = rotated_mesh(egui::TextureId::default(), Rect::from_min_size(Pos2::ZERO, vec2(2.0, 1.0)), 1);
        // Turned right: the top-left corner shows the picture's bottom-left.
        assert_eq!(m.vertices[0].uv, pos2(0.0, 1.0));
        assert_eq!(m.vertices[1].uv, pos2(0.0, 0.0));
    }

    #[test]
    fn decodes_svg_and_png() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#;
        let (img, size) = decode(svg, "a.svg", 1024).unwrap();
        assert_eq!(size, [20, 10]);
        assert_eq!(img.size[0], img.size[1] * 2);
        let mut png = vec![];
        image::RgbaImage::new(3, 2).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (_, size) = decode(&png, "b.png", 1024).unwrap();
        assert_eq!(size, [3, 2]);
        assert!(decode(b"nope", "c.png", 1024).is_err());
    }
}
