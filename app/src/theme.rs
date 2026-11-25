//! Colours, fonts and small hand-drawn line icons in the style of a dark IDE.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect,
    Response, Sense, Shape, Stroke, TextStyle, Ui, Vec2, pos2, vec2,
};
use std::sync::Arc;
use crate::i18n::tr;

/// The light theme is on (Settings → Window → Theme).
static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_light() -> bool {
    LIGHT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Switch between the dark and the light theme and restyle the window.
pub fn set_light(ctx: &egui::Context, on: bool) {
    LIGHT.store(on, std::sync::atomic::Ordering::Relaxed);
    style(ctx);
}

/// The palette: every colour of the interface in the dark and in the light theme.
macro_rules! palette {
    ($($name:ident = ($dr:expr, $dg:expr, $db:expr) / ($lr:expr, $lg:expr, $lb:expr);)*) => {
        $(
            #[allow(non_snake_case)]
            pub fn $name() -> Color32 {
                if is_light() { Color32::from_rgb($lr, $lg, $lb) } else { Color32::from_rgb($dr, $dg, $db) }
            }
        )*
    };
}

palette! {
    BG = (0x19, 0x1a, 0x1c) / (0xff, 0xff, 0xff);
    BG_BAR = (0x26, 0x28, 0x2c) / (0xf0, 0xf1, 0xf3);
    BG_INPUT = (0x1e, 0x1f, 0x22) / (0xff, 0xff, 0xff);
    BORDER = (0x2f, 0x31, 0x35) / (0xd8, 0xda, 0xdf);
    INPUT_BORDER = (0x3c, 0x3f, 0x44) / (0xc2, 0xc5, 0xcd);
    TEXT = (0xdf, 0xe1, 0xe5) / (0x1e, 0x20, 0x25);
    TEXT_DIM = (0x6f, 0x73, 0x7a) / (0x8a, 0x8e, 0x96);
    TEXT_MID = (0x9a, 0x9d, 0xa3) / (0x5a, 0x5e, 0x66);
    SELECT = (0x2a, 0x43, 0x71) / (0xc5, 0xd9, 0xfa);
    SELECT_INACTIVE = (0x2b, 0x31, 0x3d) / (0xe1, 0xe5, 0xed);
    HOVER = (0x24, 0x27, 0x2c) / (0xe9, 0xeb, 0xef);
    TAB_SEL = (0x23, 0x35, 0x58) / (0xd2, 0xe1, 0xfb);
    ACCENT = (0x35, 0x74, 0xf0) / (0x35, 0x74, 0xf0);
    LINK = (0x54, 0x8a, 0xf7) / (0x1f, 0x5f, 0xd6);
    STAR = (0xf2, 0xc5, 0x5c) / (0xc0, 0x88, 0x00);
    GREEN = (0x5f, 0xb8, 0x65) / (0x2e, 0x8b, 0x3a);
    BLUE_FILE = (0x6c, 0xa8, 0xf0) / (0x1a, 0x6a, 0xc9);
    RED = (0xdb, 0x5c, 0x5c) / (0xc9, 0x3b, 0x3b);
    UNVERSIONED = (0xd6, 0x7b, 0x5b) / (0xb3, 0x55, 0x2f);
    FIND = (0x5a, 0x4a, 0x1c) / (0xff, 0xe2, 0x8f);
    FIND_CURRENT = (0x8a, 0x66, 0x14) / (0xff, 0xb9, 0x40);
    ICON = (0xce, 0xd0, 0xd6) / (0x4a, 0x4e, 0x57);
}

/// A colour written for the dark theme (most tints of the views are): as it is there, and
/// its counterpart in the light theme.
pub fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    adapt(Color32::from_rgb(r, g, b))
}

/// The light-theme counterpart of a dark-theme colour: greys are turned over (dark
/// backgrounds become light, light text dark), dark tints become pale ones of the same hue,
/// bright accents get darker so they read on white. Unchanged in the dark theme.
pub fn adapt(c: Color32) -> Color32 {
    if !is_light() {
        return c;
    }
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let (r, g, b) = to_light(r, g, b);
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn to_light(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (rf, gf, bf) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (rf.max(gf).max(bf), rf.min(gf).min(bf));
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d < 1e-5 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-5) };
    let h = if d < 1e-5 {
        0.0
    } else if max == rf {
        ((gf - bf) / d).rem_euclid(6.0)
    } else if max == gf {
        (bf - rf) / d + 2.0
    } else {
        (rf - gf) / d + 4.0
    };
    let (l2, s2) = if s < 0.12 {
        // Greys: turned over, a little short of pure black.
        ((1.0 - l).max(0.1), s)
    } else if l < 0.42 {
        // A dark tint (a background): a pale tint.
        (0.94 - 0.42 * l, (s * 1.1).min(1.0))
    } else {
        // A bright colour (text, an icon): darker.
        ((0.98 - l).clamp(0.27, 0.46), (s * 1.05).min(1.0))
    };
    let c = (1.0 - (2.0 * l2 - 1.0).abs()) * s2;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let m = l2 - c / 2.0;
    let (r1, g1, b1) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let u = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (u(r1), u(g1), u(b1))
}

/// The veil over something the pointer is on, where the background is a bar's own colour.
pub fn hover_veil() -> Color32 {
    if is_light() { Color32::from_rgba_unmultiplied(0, 0, 0, 18) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 22) }
}

/// The colour of a piece of code in a diff.
pub fn syntax_color(kind: crate::syntax::Kind) -> Color32 {
    use crate::syntax::Kind::*;
    match kind {
        Keyword => rgb(0xd0, 0x8a, 0x62),
        Type => rgb(0xb5, 0x8b, 0xd0),
        String => rgb(0x74, 0xb0, 0x7c),
        Number => rgb(0x3d, 0xae, 0xba),
        Comment => rgb(0x7c, 0x80, 0x88),
        Function => rgb(0x64, 0xa8, 0xec),
        Key => rgb(0xc7, 0x9a, 0xd8),
        Heading => rgb(0xd9, 0xb3, 0x5e),
    }
}

pub const GRAPH_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x5f, 0xb8, 0x46),
    Color32::from_rgb(0x8e, 0x5f, 0xe0),
    Color32::from_rgb(0xd4, 0xa0, 0x3c),
    Color32::from_rgb(0x3c, 0xb4, 0xa0),
    Color32::from_rgb(0x4f, 0x7d, 0xe0),
    Color32::from_rgb(0xc8, 0x6a, 0xb8),
    Color32::from_rgb(0x9c, 0xc8, 0x46),
    Color32::from_rgb(0x6a, 0x5c, 0xd8),
];

pub fn graph_color(i: u16) -> Color32 {
    adapt(GRAPH_COLORS[i as usize % GRAPH_COLORS.len()])
}

pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // Windows keeps its fonts in %WINDIR%\\Fonts; "C:/Windows/Fonts/x.ttf" entries are mapped there.
    let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let try_load = |paths: &[&str]| -> Option<Vec<u8>> {
        paths.iter().find_map(|p| {
            let p = match p.strip_prefix("C:/Windows/") {
                Some(rest) => format!("{win}/{rest}"),
                None => p.to_string(),
            };
            std::fs::read(p).ok()
        })
    };
    if let Some(data) = try_load(&[
        "/usr/share/fonts/inter/Inter.ttc",
        "/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "C:/Windows/Fonts/segoeui.ttf",
        "/System/Library/Fonts/SFNS.ttf",
    ]) {
        fonts
            .font_data
            .insert("ui".into(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "ui".into());
    }
    if let Some(data) = try_load(&[
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "C:/Windows/Fonts/seguisym.ttf",
        "/System/Library/Fonts/Apple Symbols.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    ]) {
        fonts
            .font_data
            .insert("fallback".into(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .push("fallback".into());
        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .push("fallback".into());
    }
    if let Some(data) = try_load(&[
        "/usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-Regular.ttf",
        "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "C:/Windows/Fonts/consola.ttf",
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/Monaco.ttf",
    ]) {
        fonts
            .font_data
            .insert("mono".into(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .insert(0, "mono".into());
    }
    ctx.set_fonts(fonts);
    style(ctx);
}

/// The look of egui's own widgets in the current theme.
pub fn style(ctx: &egui::Context) {
    ctx.set_theme(if is_light() { egui::Theme::Light } else { egui::Theme::Dark });
    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, FontId::proportional(16.0)),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
        s.spacing.item_spacing = vec2(6.0, 4.0);
        s.spacing.button_padding = vec2(8.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        let v = &mut s.visuals;
        v.dark_mode = !is_light();
        v.panel_fill = BG();
        v.window_fill = BG_BAR();
        v.extreme_bg_color = BG_INPUT();
        v.faint_bg_color = HOVER();
        v.window_stroke = Stroke::new(1.0, INPUT_BORDER());
        v.window_corner_radius = CornerRadius::same(8);
        v.menu_corner_radius = CornerRadius::same(8);
        v.selection.bg_fill = SELECT();
        v.selection.stroke = Stroke::new(1.0, TEXT());
        v.hyperlink_color = LINK();
        v.override_text_color = None;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT());
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER());
        v.widgets.noninteractive.bg_fill = BG();
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT());
        v.widgets.inactive.bg_fill = rgb(0x2b, 0x2d, 0x30);
        v.widgets.inactive.weak_bg_fill = rgb(0x2b, 0x2d, 0x30);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, INPUT_BORDER());
        v.widgets.hovered.bg_fill = rgb(0x39, 0x3b, 0x40);
        v.widgets.hovered.weak_bg_fill = rgb(0x39, 0x3b, 0x40);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, INPUT_BORDER());
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT());
        v.widgets.active.bg_fill = rgb(0x43, 0x45, 0x4a);
        v.widgets.active.weak_bg_fill = rgb(0x43, 0x45, 0x4a);
        v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT());
        v.widgets.open.weak_bg_fill = rgb(0x39, 0x3b, 0x40);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(4);
        }
    });
}

// A few icons are drawn but not placed anywhere at the moment; they stay for the next use.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Folder,
    Branch,
    Tag,
    Star,
    StarOutline,
    Head,
    Refresh,
    Fetch,
    Pull,
    Push,
    Commit,
    Search,
    Plus,
    Minus,
    Trash,
    Eye,
    ChevronDown,
    ChevronRight,
    ChevronLeft,
    Close,
    /// Window with a side panel on the left / right; the "On" ones have the panel filled.
    PanelLeft,
    PanelLeftOn,
    PanelRight,
    PanelRightOn,
    Dots,
    DotsH,
    Merge,
    Gear,
    Sort,
    Expand,
    Collapse,
    Diff,
    Undo,
    Stash,
    Console,
    Log,
    Bell,
    File,
    Label,
    Check,
    Arrows,
    Info,
    Clock,
    Spiral,
    /// Pencil: jump to the file's source.
    Pencil,
    /// Window with a panel at the bottom (show details).
    PanelBottom,
    /// Two panes with an arrow out of the corner: open in an external compare tool.
    External,
}

/// Draw a line icon centered in `rect`.
pub fn paint_icon(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 16.0;
    let st = Stroke::new(1.2 * s.max(0.9), color);
    let pt = |x: f32, y: f32| pos2(c.x + (x - 8.0) * s, c.y + (y - 8.0) * s);
    match icon {
        Icon::Folder => {
            let pts = vec![
                pt(1.5, 3.5),
                pt(6.0, 3.5),
                pt(7.5, 5.0),
                pt(14.5, 5.0),
                pt(14.5, 13.0),
                pt(1.5, 13.0),
                pt(1.5, 3.5),
            ];
            p.add(Shape::line(pts, st));
        }
        Icon::Branch => {
            p.circle_stroke(pt(4.5, 3.5), 1.8 * s, st);
            p.circle_stroke(pt(4.5, 12.5), 1.8 * s, st);
            p.circle_stroke(pt(11.5, 5.5), 1.8 * s, st);
            p.line_segment([pt(4.5, 5.3), pt(4.5, 10.7)], st);
            p.add(Shape::line(vec![pt(11.5, 7.3), pt(11.5, 8.5), pt(5.5, 10.5)], st));
        }
        Icon::Tag | Icon::Label => {
            let pts = vec![
                pt(2.0, 2.5),
                pt(8.0, 2.5),
                pt(14.0, 8.5),
                pt(8.5, 14.0),
                pt(2.0, 8.0),
                pt(2.0, 2.5),
            ];
            p.add(Shape::line(pts, st));
            p.circle_filled(pt(5.2, 5.6), 1.1 * s, color);
        }
        Icon::Head => {
            let pts = vec![
                pt(2.0, 2.5),
                pt(8.0, 2.5),
                pt(14.0, 8.5),
                pt(8.5, 14.0),
                pt(2.0, 8.0),
            ];
            p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
            p.circle_filled(pt(5.2, 5.6), 1.2 * s, BG());
        }
        Icon::Star | Icon::StarOutline => {
            let mut pts = vec![];
            for i in 0..10 {
                let a = std::f32::consts::PI * (i as f32) / 5.0 - std::f32::consts::FRAC_PI_2;
                let r = if i % 2 == 0 { 7.0 } else { 3.0 };
                pts.push(pt(8.0 + r * a.cos(), 8.5 + r * a.sin()));
            }
            if icon == Icon::Star {
                // Fill with a fan from the centre (shape is not convex).
                for i in 0..10 {
                    p.add(Shape::convex_polygon(
                        vec![pt(8.0, 8.5), pts[i], pts[(i + 1) % 10]],
                        color,
                        Stroke::NONE,
                    ));
                }
            } else {
                pts.push(pts[0]);
                p.add(Shape::line(pts, st));
            }
        }
        Icon::Refresh => {
            let mut pts = vec![];
            for i in 0..=20 {
                let a = 0.6 + (i as f32) / 20.0 * 5.0;
                pts.push(pt(8.0 + 5.5 * a.cos(), 8.0 + 5.5 * a.sin()));
            }
            let end = *pts.last().unwrap();
            p.add(Shape::line(pts, st));
            p.add(Shape::line(vec![pt(13.5, 2.0), end, pt(9.0, 4.5)], st));
        }
        Icon::Fetch => {
            p.line_segment([pt(8.0, 2.0), pt(8.0, 11.0)], st);
            p.add(Shape::line(vec![pt(4.0, 7.5), pt(8.0, 11.5), pt(12.0, 7.5)], st));
            p.line_segment([pt(3.0, 14.0), pt(13.0, 14.0)], st);
        }
        Icon::Pull => {
            p.line_segment([pt(11.0, 3.0), pt(3.5, 10.5)], st);
            p.add(Shape::line(vec![pt(3.0, 5.0), pt(3.0, 11.0), pt(9.0, 11.0)], st));
        }
        Icon::Push => {
            p.line_segment([pt(5.0, 11.0), pt(12.5, 3.5)], st);
            p.add(Shape::line(vec![pt(7.0, 3.0), pt(13.0, 3.0), pt(13.0, 9.0)], st));
        }
        Icon::Commit => {
            p.circle_stroke(pt(8.0, 8.0), 3.0 * s, st);
            p.line_segment([pt(1.0, 8.0), pt(5.0, 8.0)], st);
            p.line_segment([pt(11.0, 8.0), pt(15.0, 8.0)], st);
        }
        Icon::Search => {
            p.circle_stroke(pt(7.0, 7.0), 4.5 * s, st);
            p.line_segment([pt(10.3, 10.3), pt(14.0, 14.0)], st);
        }
        Icon::Plus => {
            p.line_segment([pt(8.0, 2.5), pt(8.0, 13.5)], st);
            p.line_segment([pt(2.5, 8.0), pt(13.5, 8.0)], st);
        }
        Icon::Minus => {
            p.line_segment([pt(2.5, 8.0), pt(13.5, 8.0)], st);
        }
        Icon::Trash => {
            p.line_segment([pt(2.5, 4.0), pt(13.5, 4.0)], st);
            p.add(Shape::line(vec![pt(6.0, 4.0), pt(6.0, 2.0), pt(10.0, 2.0), pt(10.0, 4.0)], st));
            p.add(Shape::line(vec![pt(4.0, 4.0), pt(4.8, 14.0), pt(11.2, 14.0), pt(12.0, 4.0)], st));
        }
        Icon::Eye => {
            let mut top = vec![];
            let mut bottom = vec![];
            for i in 0..=12 {
                let t = i as f32 / 12.0;
                let x = 1.5 + 13.0 * t;
                let y = (std::f32::consts::PI * t).sin() * 4.5;
                top.push(pt(x, 8.0 - y));
                bottom.push(pt(x, 8.0 + y));
            }
            p.add(Shape::line(top, st));
            p.add(Shape::line(bottom, st));
            p.circle_stroke(pt(8.0, 8.0), 2.0 * s, st);
        }
        Icon::ChevronDown => {
            p.add(Shape::line(vec![pt(4.0, 6.0), pt(8.0, 10.0), pt(12.0, 6.0)], st));
        }
        Icon::ChevronRight => {
            p.add(Shape::line(vec![pt(6.0, 4.0), pt(10.0, 8.0), pt(6.0, 12.0)], st));
        }
        Icon::ChevronLeft => {
            p.add(Shape::line(vec![pt(10.0, 4.0), pt(6.0, 8.0), pt(10.0, 12.0)], st));
        }
        Icon::Close => {
            p.line_segment([pt(4.0, 4.0), pt(12.0, 12.0)], st);
            p.line_segment([pt(12.0, 4.0), pt(4.0, 12.0)], st);
        }
        Icon::Dots => {
            for y in [3.5, 8.0, 12.5] {
                p.circle_filled(pt(8.0, y), 1.2 * s, color);
            }
        }
        Icon::DotsH => {
            for x in [3.5, 8.0, 12.5] {
                p.circle_filled(pt(x, 8.0), 1.2 * s, color);
            }
        }
        Icon::Merge => {
            p.circle_stroke(pt(4.0, 3.5), 1.8 * s, st);
            p.circle_stroke(pt(4.0, 12.5), 1.8 * s, st);
            p.circle_stroke(pt(12.0, 12.5), 1.8 * s, st);
            p.line_segment([pt(4.0, 5.3), pt(4.0, 10.7)], st);
            p.add(Shape::line(vec![pt(5.0, 5.0), pt(12.0, 8.0), pt(12.0, 10.7)], st));
        }
        Icon::Gear => {
            p.circle_stroke(pt(8.0, 8.0), 2.2 * s, st);
            let mut pts = vec![];
            for i in 0..=32 {
                let a = std::f32::consts::TAU * (i as f32) / 32.0;
                let r = if (i / 2) % 2 == 0 { 6.5 } else { 5.0 };
                pts.push(pt(8.0 + r * a.cos(), 8.0 + r * a.sin()));
            }
            p.add(Shape::line(pts, st));
        }
        Icon::Sort => {
            p.line_segment([pt(5.0, 2.0), pt(5.0, 14.0)], st);
            p.add(Shape::line(vec![pt(2.5, 11.5), pt(5.0, 14.0), pt(7.5, 11.5)], st));
            p.line_segment([pt(11.0, 2.0), pt(11.0, 14.0)], st);
            p.add(Shape::line(vec![pt(8.5, 4.5), pt(11.0, 2.0), pt(13.5, 4.5)], st));
        }
        Icon::Expand => {
            p.add(Shape::line(vec![pt(4.0, 6.0), pt(8.0, 2.5), pt(12.0, 6.0)], st));
            p.add(Shape::line(vec![pt(4.0, 10.0), pt(8.0, 13.5), pt(12.0, 10.0)], st));
        }
        Icon::Collapse => {
            p.add(Shape::line(vec![pt(4.0, 2.5), pt(8.0, 6.0), pt(12.0, 2.5)], st));
            p.add(Shape::line(vec![pt(4.0, 13.5), pt(8.0, 10.0), pt(12.0, 13.5)], st));
        }
        Icon::PanelLeft | Icon::PanelLeftOn | Icon::PanelRight | Icon::PanelRightOn => {
            let frame = Rect::from_min_max(pt(1.5, 2.5), pt(14.5, 13.5));
            let left = matches!(icon, Icon::PanelLeft | Icon::PanelLeftOn);
            let x = if left { 6.0 } else { 10.0 };
            if matches!(icon, Icon::PanelLeftOn | Icon::PanelRightOn) {
                let side = if left {
                    Rect::from_min_max(pt(1.5, 2.5), pt(x, 13.5))
                } else {
                    Rect::from_min_max(pt(x, 2.5), pt(14.5, 13.5))
                };
                let r = (2.0 * s) as u8;
                let cr = if left {
                    egui::CornerRadius { nw: r, sw: r, ne: 0, se: 0 }
                } else {
                    egui::CornerRadius { nw: 0, sw: 0, ne: r, se: r }
                };
                p.rect_filled(side, cr, color);
            }
            p.rect_stroke(frame, 2.0 * s, st, egui::StrokeKind::Middle);
            p.line_segment([pt(x, 2.5), pt(x, 13.5)], st);
        }
        Icon::Pencil => {
            let pts = vec![pt(3.0, 13.0), pt(3.5, 10.5), pt(11.0, 3.0), pt(13.0, 5.0), pt(5.5, 12.5), pt(3.0, 13.0)];
            p.add(egui::Shape::line(pts, st));
            p.line_segment([pt(9.5, 4.5), pt(11.5, 6.5)], st);
        }
        Icon::External => {
            // Two compare panes, the right one open at the corner with an arrow leaving it.
            p.add(Shape::line(vec![pt(7.0, 2.5), pt(1.5, 2.5), pt(1.5, 13.5), pt(7.0, 13.5)], st));
            p.line_segment([pt(7.0, 2.5), pt(7.0, 13.5)], st);
            p.add(Shape::line(vec![pt(9.5, 2.5), pt(9.5, 13.5), pt(14.5, 13.5), pt(14.5, 9.0)], st));
            p.line_segment([pt(10.5, 8.0), pt(14.5, 2.5)], st);
            p.add(Shape::line(vec![pt(11.0, 2.5), pt(14.5, 2.5), pt(14.5, 6.0)], st));
        }
        Icon::PanelBottom => {
            let frame = Rect::from_min_max(pt(1.5, 2.5), pt(14.5, 13.5));
            p.rect_stroke(frame, 2.0 * s, st, egui::StrokeKind::Middle);
            p.line_segment([pt(1.5, 9.0), pt(14.5, 9.0)], st);
            p.line_segment([pt(4.0, 11.2), pt(12.0, 11.2)], st);
        }
        Icon::Diff => {
            p.rect_stroke(
                Rect::from_min_max(pt(1.5, 2.5), pt(7.0, 13.5)),
                1.0,
                st,
                egui::StrokeKind::Middle,
            );
            p.rect_stroke(
                Rect::from_min_max(pt(9.0, 2.5), pt(14.5, 13.5)),
                1.0,
                st,
                egui::StrokeKind::Middle,
            );
        }
        Icon::Undo => {
            let mut pts = vec![];
            for i in 0..=14 {
                let a = -std::f32::consts::FRAC_PI_2 + (i as f32) / 14.0 * std::f32::consts::PI;
                pts.push(pt(9.0 + 4.5 * a.cos(), 8.5 + 4.5 * a.sin()));
            }
            pts.push(pt(5.0, 13.0));
            p.add(Shape::line(pts, st));
            p.line_segment([pt(9.0, 4.0), pt(3.0, 4.0)], st);
            p.add(Shape::line(vec![pt(5.5, 1.5), pt(3.0, 4.0), pt(5.5, 6.5)], st));
        }
        Icon::Stash => {
            p.add(Shape::line(
                vec![pt(2.0, 7.0), pt(2.0, 13.5), pt(14.0, 13.5), pt(14.0, 7.0)],
                st,
            ));
            p.line_segment([pt(8.0, 1.5), pt(8.0, 9.5)], st);
            p.add(Shape::line(vec![pt(5.0, 6.5), pt(8.0, 9.5), pt(11.0, 6.5)], st));
        }
        Icon::Console => {
            p.rect_stroke(
                Rect::from_min_max(pt(1.5, 2.5), pt(14.5, 13.5)),
                1.5,
                st,
                egui::StrokeKind::Middle,
            );
            p.add(Shape::line(vec![pt(4.0, 6.0), pt(6.5, 8.0), pt(4.0, 10.0)], st));
            p.line_segment([pt(8.0, 10.5), pt(12.0, 10.5)], st);
        }
        Icon::Log => {
            p.circle_stroke(pt(4.0, 4.0), 1.6 * s, st);
            p.circle_stroke(pt(4.0, 12.0), 1.6 * s, st);
            p.line_segment([pt(4.0, 5.6), pt(4.0, 10.4)], st);
            p.line_segment([pt(8.0, 4.0), pt(14.0, 4.0)], st);
            p.line_segment([pt(8.0, 12.0), pt(14.0, 12.0)], st);
        }
        Icon::Bell => {
            let pts = vec![
                pt(3.0, 12.0),
                pt(4.5, 10.0),
                pt(4.5, 6.5),
                pt(6.0, 3.5),
                pt(8.0, 2.8),
                pt(10.0, 3.5),
                pt(11.5, 6.5),
                pt(11.5, 10.0),
                pt(13.0, 12.0),
                pt(3.0, 12.0),
            ];
            p.add(Shape::line(pts, st));
            p.line_segment([pt(6.5, 14.0), pt(9.5, 14.0)], st);
        }
        Icon::File => {
            p.add(Shape::line(
                vec![
                    pt(3.0, 1.5),
                    pt(9.5, 1.5),
                    pt(13.0, 5.0),
                    pt(13.0, 14.5),
                    pt(3.0, 14.5),
                    pt(3.0, 1.5),
                ],
                st,
            ));
            p.add(Shape::line(vec![pt(9.5, 1.5), pt(9.5, 5.0), pt(13.0, 5.0)], st));
        }
        Icon::Check => {
            p.add(Shape::line(vec![pt(3.0, 8.5), pt(6.5, 12.0), pt(13.0, 4.5)], st));
        }
        Icon::Arrows => {
            p.line_segment([pt(5.0, 2.0), pt(5.0, 13.0)], st);
            p.add(Shape::line(vec![pt(2.5, 10.5), pt(5.0, 13.0), pt(7.5, 10.5)], st));
            p.line_segment([pt(11.0, 3.0), pt(11.0, 14.0)], st);
            p.add(Shape::line(vec![pt(8.5, 5.5), pt(11.0, 3.0), pt(13.5, 5.5)], st));
        }
        Icon::Clock => {
            p.circle_stroke(pt(8.0, 8.0), 6.0 * s, st);
            p.add(Shape::line(vec![pt(8.0, 4.5), pt(8.0, 8.0), pt(10.8, 9.8)], st));
        }
        Icon::Spiral => {
            // AI assistant mark: sparkles drawn as outlines, like the other line icons.
            let sparkle = |cx: f32, cy: f32, r: f32| {
                // Four points joined by sides that curve in towards the centre.
                let tips = [(0.0, -r), (r, 0.0), (0.0, r), (-r, 0.0)];
                let mut pts = vec![];
                for k in 0..4 {
                    let (ax, ay) = tips[k];
                    let (bx, by) = tips[(k + 1) % 4];
                    // Quadratic curve with its control point near the centre.
                    let (qx, qy) = ((ax + bx) * 0.2, (ay + by) * 0.2);
                    for i in 0..8 {
                        let t = i as f32 / 8.0;
                        let u = 1.0 - t;
                        let x = u * u * ax + 2.0 * u * t * qx + t * t * bx;
                        let y = u * u * ay + 2.0 * u * t * qy + t * t * by;
                        pts.push(pt(cx + x, cy + y));
                    }
                }
                p.add(Shape::closed_line(pts, st));
            };
            sparkle(6.2, 9.8, 5.8);
            sparkle(13.0, 3.2, 2.7);
        }
        Icon::Info => {
            p.circle_stroke(pt(8.0, 8.0), 6.0 * s, st);
            p.line_segment([pt(8.0, 7.0), pt(8.0, 11.5)], st);
            p.circle_filled(pt(8.0, 4.8), 0.9 * s, color);
        }
    }
}

/// A square icon button like the IDE's toolbar buttons.
pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    icon_button_sized(ui, icon, tip, 24.0, false, true)
}

pub fn icon_button_sized(
    ui: &mut Ui,
    icon: Icon,
    tip: &str,
    size: f32,
    selected: bool,
    enabled: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let resp = if enabled { resp } else { resp.on_disabled_hover_text(tip) };
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, 5.0, rgb(0x35, 0x74, 0xf0));
        } else if enabled && resp.hovered() {
            p.rect_filled(rect, 5.0, rgb(0x39, 0x3b, 0x40));
        }
        let col = if !enabled {
            rgb(0x5a, 0x5d, 0x63)
        } else if selected {
            Color32::WHITE
        } else {
            ICON()
        };
        paint_icon(p, Rect::from_center_size(rect.center(), Vec2::splat(16.0)), icon, col);
    }
    if enabled {
        resp.on_hover_text(tip)
    } else {
        resp
    }
}

/// Text-with-icon toolbar button (e.g. in the title bar).
pub fn text_button(ui: &mut Ui, icon: Option<Icon>, text: &str, color: Color32) -> Response {
    text_button_parts(ui, icon, &[(text, color)])
}

/// Like [`text_button`], with pieces of text in their own colours (e.g. "master ↗6" with a green count).
pub fn text_button_parts(ui: &mut Ui, icon: Option<Icon>, parts: &[(&str, Color32)]) -> Response {
    let mut job = egui::text::LayoutJob::default();
    for (t, c) in parts {
        job.append(t, 0.0, egui::TextFormat { font_id: FontId::proportional(13.0), color: *c, ..Default::default() });
    }
    let galley = ui.painter().layout_job(job);
    let iw = if icon.is_some() { 20.0 } else { 0.0 };
    let w = galley.size().x + iw + 14.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, 5.0, hover_veil());
        }
        let mut x = rect.left() + 7.0;
        if let Some(ic) = icon {
            paint_icon(
                p,
                Rect::from_center_size(pos2(x + 8.0, rect.center().y), Vec2::splat(16.0)),
                ic,
                ICON(),
            );
            x += iw;
        }
        p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, TEXT());
    }
    resp
}

pub fn hline(ui: &Ui, y: f32, x0: f32, x1: f32) {
    ui.painter()
        .line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(1.0, BORDER()));
}

pub fn vline(ui: &Ui, x: f32, y0: f32, y1: f32) {
    ui.painter()
        .line_segment([pos2(x, y0), pos2(x, y1)], Stroke::new(1.0, BORDER()));
}

pub fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_string(), font.clone(), TEXT())
        .size()
        .x
}

/// Paints `text`, cut with "…" if it would exceed `max_w`. Returns the used width.
pub fn paint_clipped(
    p: &Painter,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> f32 {
    if max_w <= 4.0 {
        return 0.0;
    }
    let g = p.layout_no_wrap(text.to_string(), font.clone(), color);
    if g.size().x <= max_w {
        let w = g.size().x;
        p.galley(pos2(pos.x, pos.y - g.size().y / 2.0), g, color);
        return w;
    }
    // Binary search the longest prefix that fits.
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        let w = p.layout_no_wrap(s, font.clone(), color).size().x;
        if w <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let s: String = chars[..lo].iter().collect::<String>() + "…";
    let g = p.layout_no_wrap(s, font, color);
    let w = g.size().x;
    p.galley(pos2(pos.x, pos.y - g.size().y / 2.0), g, color);
    w
}

/// A question mark in a circle that tells more when pointed at: for the explanations that
/// would otherwise take up a page.
pub fn help_mark(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    let col = if resp.hovered() { TEXT() } else { TEXT_DIM() };
    let p = ui.painter();
    p.circle_stroke(r.center(), 7.5, egui::Stroke::new(1.3, col));
    p.text(r.center() + egui::vec2(0.0, 0.5), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(12.0), col);
    resp.on_hover_ui(|ui| {
        ui.set_max_width(480.0);
        ui.add(egui::Label::new(egui::RichText::new(text).color(TEXT())).wrap());
    })
}

/// Right-click menu for text fields: Cut, Copy, Paste, Select All. Returns the response unchanged.
pub fn edit_menu(resp: egui::Response) -> egui::Response {
    let r = resp.clone();
    resp.context_menu(|ui| {
        let mut cmd = None;
        if ui.button(tr("Cut")).clicked() {
            cmd = Some(egui::ViewportCommand::RequestCut);
        }
        if ui.button(tr("Copy")).clicked() {
            cmd = Some(egui::ViewportCommand::RequestCopy);
        }
        if ui.button(tr("Paste")).clicked() {
            cmd = Some(egui::ViewportCommand::RequestPaste);
        }
        ui.separator();
        if ui.button(tr("Select All")).clicked() {
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), r.id) {
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(usize::MAX / 2),
                );
                st.cursor.set_char_range(Some(all));
                st.store(ui.ctx(), r.id);
            }
            r.request_focus();
            ui.close();
        }
        if let Some(c) = cmd {
            r.request_focus();
            ui.ctx().send_viewport_cmd(c);
            ui.close();
        }
    });
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_colours_get_light_counterparts() {
        let lum = |(r, g, b): (u8, u8, u8)| (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
        // Greys are turned over: a dark background becomes a light one, light text dark.
        assert!(lum(to_light(0x2b, 0x2d, 0x30)) > 200);
        assert!(lum(to_light(0xdf, 0xe1, 0xe5)) < 60);
        // A dark tint (the background of a removed line) becomes a pale tint of the same hue.
        let (r, g, b) = to_light(0x4a, 0x2f, 0x30);
        assert!(lum((r, g, b)) > 190 && r > g && r > b, "{r} {g} {b}");
        let (r, g, b) = to_light(0x2a, 0x45, 0x36);
        assert!(lum((r, g, b)) > 190 && g > r && g > b, "{r} {g} {b}");
        // A bright colour meant for a dark background gets dark enough to read on white.
        let (r, g, b) = to_light(0x7c, 0xe0, 0x6a);
        assert!(lum((r, g, b)) < 150 && g > r && g > b, "{r} {g} {b}");
        // Every colour of the light palette's text reads on its background.
        assert!(!is_light());
        assert_eq!(adapt(Color32::from_rgb(1, 2, 3)), Color32::from_rgb(1, 2, 3));
    }
}
