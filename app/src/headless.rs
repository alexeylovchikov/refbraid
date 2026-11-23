//! Screenshot fallback: when the desktop doesn't let the window draw (for example a remote
//! session with nobody connected), draw the same picture in memory and save it.

use crate::app::App;
use eframe::egui::{self, Color32, ImageData, Pos2, Rect, TextureId, epaint};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const WAIT: Duration = Duration::from_secs(20);
const SIZE: [usize; 2] = [1600, 820];

/// If no picture was saved after a while, render one off-screen and quit.
pub fn watchdog(path: Option<PathBuf>, shot: PathBuf) {
    let start = SystemTime::now();
    std::thread::spawn(move || {
        std::thread::sleep(WAIT);
        let saved = std::fs::metadata(&shot)
            .and_then(|m| m.modified())
            .map(|t| t >= start)
            .unwrap_or(false);
        if saved {
            return;
        }
        eprintln!("The window is not drawing; saving the picture from an off-screen render.");
        match render(path, shot.clone()) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("Off-screen render failed: {e}");
                std::process::exit(1);
            }
        }
    });
}

struct Texture {
    w: usize,
    h: usize,
    px: Vec<Color32>,
}

fn render(path: Option<PathBuf>, shot: PathBuf) -> Result<(), String> {
    let ctx = egui::Context::default();
    crate::theme::setup(&ctx);
    let mut app = App::new(&ctx, path, Some(shot.clone()));
    app.headless = true;
    let mut textures: HashMap<TextureId, Texture> = HashMap::new();
    let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(SIZE[0] as f32, SIZE[1] as f32));
    let mut last = None;
    for i in 0..40 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(i as f64 / 60.0),
            focused: false,
            events: crate::app::screenshot_events(),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| app.draw(ui));
        let mut delta = std::mem::take(&mut out.textures_delta);
        for (id, deltas) in &delta.set {
            for d in deltas {
                apply(&mut textures, *id, d);
            }
        }
        for id in &delta.free {
            textures.retain(|k, _| k != id);
        }
        delta.clear();
        last = Some(out);
        // Let background loading (commit details, etc.) finish between frames.
        std::thread::sleep(Duration::from_millis(30));
    }
    let out = last.ok_or("nothing drawn")?;
    let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
    let mut fb = vec![crate::theme::BG(); SIZE[0] * SIZE[1]];
    for p in &prims {
        if let epaint::Primitive::Mesh(mesh) = &p.primitive {
            if let Some(tex) = textures.get(&mesh.texture_id) {
                draw_mesh(&mut fb, tex, mesh, p.clip_rect);
            }
        }
    }
    let mut buf = Vec::with_capacity(fb.len() * 4);
    for c in &fb {
        buf.extend_from_slice(&[c.r(), c.g(), c.b(), 255]);
    }
    if let Some(dir) = shot.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    image::save_buffer(&shot, &buf, SIZE[0] as u32, SIZE[1] as u32, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())
}

fn apply(textures: &mut HashMap<TextureId, Texture>, id: TextureId, d: &epaint::ImageDelta) {
    let ImageData::Color(img) = &d.image;
    let [w, h] = img.size;
    match d.pos {
        None => {
            textures.insert(id, Texture { w, h, px: img.pixels.clone() });
        }
        Some([x0, y0]) => {
            if let Some(t) = textures.get_mut(&id) {
                for y in 0..h {
                    for x in 0..w {
                        if x0 + x < t.w && y0 + y < t.h {
                            t.px[(y0 + y) * t.w + x0 + x] = img.pixels[y * w + x];
                        }
                    }
                }
            }
        }
    }
}

fn sample(t: &Texture, u: f32, v: f32) -> [f32; 4] {
    let x = (u * t.w as f32 - 0.5).clamp(0.0, t.w as f32 - 1.0);
    let y = (v * t.h as f32 - 0.5).clamp(0.0, t.h as f32 - 1.0);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(t.w - 1), (y0 + 1).min(t.h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |x: usize, y: usize| t.px[y * t.w + x].to_array();
    let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let mut out = [0.0; 4];
    for i in 0..4 {
        let top = a[i] as f32 * (1.0 - fx) + b[i] as f32 * fx;
        let bot = c[i] as f32 * (1.0 - fx) + d[i] as f32 * fx;
        out[i] = (top * (1.0 - fy) + bot * fy) / 255.0;
    }
    out
}

fn draw_mesh(fb: &mut [Color32], tex: &Texture, mesh: &epaint::Mesh, clip: Rect) {
    let (w, h) = (SIZE[0] as f32, SIZE[1] as f32);
    let cx0 = clip.min.x.max(0.0);
    let cy0 = clip.min.y.max(0.0);
    let cx1 = clip.max.x.min(w);
    let cy1 = clip.max.y.min(h);
    if cx0 >= cx1 || cy0 >= cy1 {
        return;
    }
    for tri in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [
            &mesh.vertices[tri[0] as usize],
            &mesh.vertices[tri[1] as usize],
            &mesh.vertices[tri[2] as usize],
        ];
        let area = (b.pos.x - a.pos.x) * (c.pos.y - a.pos.y) - (b.pos.y - a.pos.y) * (c.pos.x - a.pos.x);
        if area.abs() < 1e-6 {
            continue;
        }
        let minx = a.pos.x.min(b.pos.x).min(c.pos.x).max(cx0).floor() as i32;
        let maxx = a.pos.x.max(b.pos.x).max(c.pos.x).min(cx1).ceil() as i32;
        let miny = a.pos.y.min(b.pos.y).min(c.pos.y).max(cy0).floor() as i32;
        let maxy = a.pos.y.max(b.pos.y).max(c.pos.y).min(cy1).ceil() as i32;
        let (ca, cb, cc) = (a.color.to_array(), b.color.to_array(), c.color.to_array());
        for py in miny.max(0)..maxy.min(SIZE[1] as i32) {
            let y = py as f32 + 0.5;
            if y < cy0 || y > cy1 {
                continue;
            }
            for px in minx.max(0)..maxx.min(SIZE[0] as i32) {
                let x = px as f32 + 0.5;
                if x < cx0 || x > cx1 {
                    continue;
                }
                let w0 = ((b.pos.x - x) * (c.pos.y - y) - (b.pos.y - y) * (c.pos.x - x)) / area;
                let w1 = ((c.pos.x - x) * (a.pos.y - y) - (c.pos.y - y) * (a.pos.x - x)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                    continue;
                }
                let u = w0 * a.uv.x + w1 * b.uv.x + w2 * c.uv.x;
                let v = w0 * a.uv.y + w1 * b.uv.y + w2 * c.uv.y;
                let t = sample(tex, u, v);
                let mut src = [0.0f32; 4];
                for i in 0..4 {
                    let vc = (w0 * ca[i] as f32 + w1 * cb[i] as f32 + w2 * cc[i] as f32) / 255.0;
                    src[i] = vc * t[i];
                }
                let dst = &mut fb[py as usize * SIZE[0] + px as usize];
                let d = dst.to_array();
                let inv = 1.0 - src[3];
                let mix = |s: f32, d: u8| ((s + d as f32 / 255.0 * inv) * 255.0).round().clamp(0.0, 255.0) as u8;
                *dst = Color32::from_rgba_premultiplied(mix(src[0], d[0]), mix(src[1], d[1]), mix(src[2], d[2]), 255);
            }
        }
    }
}
