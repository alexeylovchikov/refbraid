//! Adds the app to the file manager: Files (Nautilus) on Linux, Explorer's folder menu on Windows.

use eframe::egui::IconData;
#[cfg(not(windows))]
use std::path::PathBuf;

/// The app icon, drawn in code: a rounded square with a small branch graph.
///
/// It is drawn several times bigger and scaled down, so the edges come out smooth; in the
/// small sizes (title bar, lists) the lines and dots are made thicker to stay readable.
pub fn icon_data(size: u32) -> IconData {
    static DRAWN: std::sync::Mutex<Vec<(u32, Vec<u8>)>> = std::sync::Mutex::new(Vec::new());
    let mut drawn = DRAWN.lock().unwrap();
    let rgba = match drawn.iter().find(|(s, _)| *s == size) {
        Some((_, d)) => d.clone(),
        None => {
            let d = smooth_icon(size);
            drawn.push((size, d.clone()));
            d
        }
    };
    IconData { rgba, width: size, height: size }
}

fn smooth_icon(size: u32) -> Vec<u8> {
    let k = if size >= 128 { 4 } else { 8 };
    let bold = if size <= 24 { 1.45 } else if size <= 48 { 1.2 } else { 1.0 };
    let big = raw_icon(size * k, bold);
    let mut out = vec![0u8; (size * size * 4) as usize];
    let n = (k * k) as f32;
    for y in 0..size {
        for x in 0..size {
            // Colours are averaged by how much of the pixel they cover (the corners are see-through).
            let mut sum = [0f32; 4];
            for dy in 0..k {
                for dx in 0..k {
                    let i = (((y * k + dy) * size * k + x * k + dx) * 4) as usize;
                    let a = big[i + 3] as f32;
                    for c in 0..3 {
                        sum[c] += big[i + c] as f32 * a;
                    }
                    sum[3] += a;
                }
            }
            let o = ((y * size + x) * 4) as usize;
            if sum[3] > 0.0 {
                for c in 0..3 {
                    out[o + c] = (sum[c] / sum[3]).round() as u8;
                }
                out[o + 3] = (sum[3] / n).round() as u8;
            }
        }
    }
    out
}

/// The icon with hard edges, `size` pixels wide; `bold` makes the lines and dots thicker.
fn raw_icon(size: u32, bold: f32) -> Vec<u8> {
    let s = size as f32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let r = s * 0.2;
    let node = |cx: f32, cy: f32, x: f32, y: f32, rad: f32| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= rad;
    let seg = |x0: f32, y0: f32, x1: f32, y1: f32, x: f32, y: f32, w: f32| {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let t = (((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (px, py) = (x0 + t * dx, y0 + t * dy);
        ((x - px).powi(2) + (y - py).powi(2)).sqrt() <= w
    };
    for yi in 0..size {
        for xi in 0..size {
            let (x, y) = (xi as f32 + 0.5, yi as f32 + 0.5);
            // rounded square mask
            let qx = (x - s / 2.0).abs() - (s / 2.0 - r);
            let qy = (y - s / 2.0).abs() - (s / 2.0 - r);
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() - r;
            if d > 0.0 {
                continue;
            }
            let t = (x + y) / (2.0 * s);
            let mut c = [
                (0x2b as f32 * (1.0 - t) + 0x1c as f32 * t) as u8,
                (0x5a as f32 * (1.0 - t) + 0x2a as f32 * t) as u8,
                (0xd8 as f32 * (1.0 - t) + 0x6a as f32 * t) as u8,
                255u8,
            ];
            let w = s * 0.045 * bold;
            let green = [0x7c, 0xe0, 0x6a, 255];
            let violet = [0xc8, 0xa8, 0xff, 255];
            if seg(s * 0.35, s * 0.2, s * 0.35, s * 0.8, x, y, w) {
                c = green;
            }
            if seg(s * 0.35, s * 0.62, s * 0.66, s * 0.42, x, y, w)
                || seg(s * 0.66, s * 0.42, s * 0.66, s * 0.28, x, y, w)
            {
                c = violet;
            }
            for (cx, cy, col) in [
                (s * 0.35, s * 0.25, green),
                (s * 0.35, s * 0.75, green),
                (s * 0.66, s * 0.3, violet),
            ] {
                if node(cx, cy, x, y, s * 0.095 * bold) {
                    c = [255, 255, 255, 255];
                }
                if node(cx, cy, x, y, s * 0.06 * bold) {
                    c = col;
                }
            }
            let i = ((yi * size + xi) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&c);
        }
    }
    rgba
}

/// One picture of an .ico file as a 32-bit bitmap: the header, the pixels bottom row first
/// (blue, green, red, alpha) and the one-bit mask (all zero: the alpha says what is see-through).
fn ico_bitmap(icon: &IconData) -> Vec<u8> {
    let (w, h) = (icon.width, icon.height);
    let mask_row = w.div_ceil(32) * 4;
    let mut out = Vec::with_capacity((40 + w * h * 4 + mask_row * h) as usize);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    // The height counts the pixels and the mask.
    out.extend_from_slice(&((h * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(w * h * 4 + mask_row * h).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    for y in (0..h).rev() {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            out.extend_from_slice(&[icon.rgba[i + 2], icon.rgba[i + 1], icon.rgba[i], icon.rgba[i + 3]]);
        }
    }
    out.resize(out.len() + (mask_row * h) as usize, 0);
    out
}

/// The icon as macOS shows applications: the picture takes four fifths of the square, with
/// clear edges around it, like the icons next to it in the Dock.
fn padded_icon(size: u32) -> Vec<u8> {
    let inner = (size * 4 / 5).max(1);
    let pad = (size - inner) / 2;
    let small = icon_data(inner);
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..inner {
        let from = (y * inner * 4) as usize;
        let to = (((y + pad) * size + pad) * 4) as usize;
        out[to..to + (inner * 4) as usize].copy_from_slice(&small.rgba[from..from + (inner * 4) as usize]);
    }
    out
}

/// The file of a macOS application icon: a list of PNG pictures, each under the name of its
/// size (`ic11`…`ic14` are the double-density ones).
fn icns(picture: impl Fn(u32) -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    let kinds: [(&[u8; 4], u32); 10] = [(b"icp4", 16), (b"icp5", 32), (b"ic07", 128), (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024), (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512)];
    let mut body = Vec::new();
    for (kind, size) in kinds {
        let png = picture(size)?;
        body.extend_from_slice(kind);
        body.extend_from_slice(&(png.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(&png);
    }
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Save the app icon as .png (256 px), .ico (16-256 px) or .icns (16-1024 px, for macOS) — used
/// when building the installers.
pub fn write_icon(path: &std::path::Path) -> Result<(), String> {
    let png = |size: u32| -> Result<Vec<u8>, String> {
        let icon = icon_data(size);
        let mut out = Vec::new();
        image::write_buffer_with_format(
            &mut std::io::Cursor::new(&mut out),
            &icon.rgba,
            size,
            size,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
        Ok(out)
    };
    let is = |ext: &str| path.extension().map(|e| e.eq_ignore_ascii_case(ext)).unwrap_or(false);
    let data = if is("icns") {
        icns(|size| {
            let mut out = Vec::new();
            image::write_buffer_with_format(&mut std::io::Cursor::new(&mut out), &padded_icon(size), size, size, image::ExtendedColorType::Rgba8, image::ImageFormat::Png).map_err(|e| e.to_string())?;
            Ok(out)
        })?
    } else if is("ico") {
        // Every size Windows asks for at 100-250 % scaling, so it never has to stretch one.
        let sizes = [16u32, 20, 24, 32, 40, 48, 64, 72, 96, 128, 256];
        // The big one is a PNG; the others are plain bitmaps, which every part of Windows reads.
        let images: Vec<Vec<u8>> = sizes.iter().map(|&s| if s >= 256 { png(s) } else { Ok(ico_bitmap(&icon_data(s))) }).collect::<Result<_, _>>()?;
        let mut out = vec![0u8, 0, 1, 0, sizes.len() as u8, 0];
        let mut offset = 6 + 16 * sizes.len() as u32;
        for (s, img) in sizes.iter().zip(&images) {
            let b = if *s >= 256 { 0 } else { *s as u8 };
            out.extend_from_slice(&[b, b, 0, 0, 1, 0, 32, 0]);
            out.extend_from_slice(&(img.len() as u32).to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            offset += img.len() as u32;
        }
        for img in images {
            out.extend_from_slice(&img);
        }
        out
    } else {
        png(256)?
    };
    std::fs::write(path, data).map_err(|e| e.to_string())
}

/// Whether this system has a file manager the program can add itself to (not macOS yet).
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));

#[cfg(not(windows))]
fn home() -> Option<PathBuf> {
    crate::git::home_dir()
}

/// Windows: add "Open in RefBraid" to the right-click menu of folders (for this user only).
#[cfg(windows)]
pub fn install() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = crate::git::plain_path(exe).display().to_string();
    let reg = |args: &[&str]| -> Result<(), String> {
        let ok = crate::git::no_console(&mut std::process::Command::new("reg"))
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok { Ok(()) } else { Err("Could not write to the registry.".into()) }
    };
    for (key, arg) in [
        (r"HKCU\Software\Classes\Directory\shell\RefBraid", "%1"),
        (r"HKCU\Software\Classes\Directory\Background\shell\RefBraid", "%V"),
    ] {
        reg(&["add", key, "/ve", "/d", "Open in RefBraid", "/f"])?;
        reg(&["add", key, "/v", "Icon", "/d", &exe, "/f"])?;
        let cmd = format!("\"{exe}\" \"{arg}\"");
        reg(&["add", &format!(r"{key}\command"), "/ve", "/d", &cmd, "/f"])?;
    }
    Ok("Done. In Explorer, right-click a folder (or an empty spot inside it) → \"Open in RefBraid\". On Windows 11 it is under \"Show more options\".".into())
}

#[cfg(windows)]
pub fn installed() -> bool {
    crate::git::no_console(&mut std::process::Command::new("reg"))
        .args(["query", r"HKCU\Software\Classes\Directory\shell\RefBraid"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Install: copy the program to ~/.local/bin, add a desktop entry for folders and a Nautilus script.
/// When it was installed system-wide (RPM package, /usr/bin) the program is used where it is.
#[cfg(not(windows))]
pub fn install() -> Result<String, String> {
    let home = home().ok_or("No home folder")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let system_wide = exe.starts_with("/usr/") || exe.starts_with("/opt/");
    if system_wide {
        // The package already adds RefBraid to Files for everyone; entries in the home folder
        // would only be left behind when the package is removed.
        tidy_user_files(&home, false);
        let ext = [
            "/usr/lib64/nautilus/extensions-4/libnautilus-python.so",
            "/usr/lib/nautilus/extensions-4/libnautilus-python.so",
            "/usr/lib64/nautilus/extensions-3.0/libnautilus-python.so",
        ]
        .iter()
        .any(|p| std::path::Path::new(p).exists());
        return Ok(if ext {
            "RefBraid is installed as a package, so it is already in Files: right-click a folder → \"Open in RefBraid\" or \"Open With…\" → RefBraid. (Restart Files with \"nautilus -q\" if it isn't there yet.)".into()
        } else {
            "RefBraid is installed as a package, so it is already in \"Open With…\" for folders in Files. For \"Open in RefBraid\" in the right-click menu install nautilus-python (dnf install nautilus-python) and restart Files.".into()
        });
    }
    let bin_dir = home.join(".local/bin");
    let target = if system_wide { exe.clone() } else { bin_dir.join("refbraid") };
    if exe != target {
        std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
        let tmp = bin_dir.join(".refbraid.new");
        std::fs::copy(&exe, &tmp).map_err(|e| format!("Copy failed: {e}"))?;
        std::fs::rename(&tmp, &target).map_err(|e| format!("Copy failed: {e}"))?;
    }
    // Icon
    let icon_dir = home.join(".local/share/icons/hicolor/256x256/apps");
    let _ = std::fs::create_dir_all(&icon_dir);
    let icon = icon_data(256);
    let _ = image::save_buffer(
        icon_dir.join("refbraid.png"),
        &icon.rgba,
        256,
        256,
        image::ExtendedColorType::Rgba8,
    );

    // Desktop entry: shows up in "Open With…" for folders.
    let apps = home.join(".local/share/applications");
    std::fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=RefBraid\nComment=Browse history, branches and commit changes of a Git repository\nExec=\"{}\" %f\nIcon=refbraid\nTerminal=false\nCategories=Development;RevisionControl;\nMimeType=inode/directory;\nStartupWMClass=refbraid\n",
        target.display()
    );
    std::fs::write(apps.join("refbraid.desktop"), desktop).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps)
        .status();

    // Nautilus script: right click → Scripts → Open in RefBraid.
    let scripts = home.join(".local/share/nautilus/scripts");
    std::fs::create_dir_all(&scripts).map_err(|e| e.to_string())?;
    let script_path = scripts.join("Open in RefBraid");
    let script = format!(
        "#!/bin/sh\n# Opens the selected folder (or the current one) in RefBraid.\nsel=$(printf '%s\\n' \"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\" | head -n 1)\n[ -z \"$sel\" ] && sel=$(pwd)\nexec \"{}\" \"$sel\"\n",
        target.display()
    );
    std::fs::write(&script_path, script).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
    }
    Ok(if system_wide {
        "Done. In the Files app, right-click a folder → \"Open With…\" → RefBraid, or right-click → Scripts → \"Open in RefBraid\".".to_string()
    } else {
        format!(
            "Done. In the Files app, right-click a folder → \"Open With…\" → RefBraid, or right-click → Scripts → \"Open in RefBraid\". The program was copied to {}.",
            target.display()
        )
    })
}

#[cfg(not(windows))]
pub fn installed() -> bool {
    home()
        .map(|h| h.join(".local/share/applications/refbraid.desktop").exists())
        .unwrap_or(false)
}

/// The program a desktop entry or script of ours starts (its `Exec=` / `exec` line).
#[cfg(not(windows))]
fn program_in(text: &str) -> Option<PathBuf> {
    let line = text.lines().find(|l| l.starts_with("Exec=") || l.trim_start().starts_with("exec "))?;
    let rest = line.trim_start().trim_start_matches("Exec=").trim_start_matches("exec ").trim();
    let prog = match rest.strip_prefix('"') {
        Some(q) => q.split('"').next()?,
        None => rest.split_whitespace().next()?,
    };
    Some(PathBuf::from(prog))
}

/// Take out what "Add to Files" put in the home folder. With `only_stale`, only the entries
/// whose program is gone (left over after the package or the copy was removed) or that
/// duplicate the package's own entry. Returns what was taken out.
#[cfg(not(windows))]
fn tidy_user_files(home: &std::path::Path, only_stale: bool) -> Vec<PathBuf> {
    let apps = home.join(".local/share/applications");
    let desktop = apps.join("refbraid.desktop");
    let script = home.join(".local/share/nautilus/scripts/Open in RefBraid");
    let icon = home.join(".local/share/icons/hicolor/256x256/apps/refbraid.png");
    let ours = |p: &std::path::Path| -> Option<PathBuf> {
        let text = std::fs::read_to_string(p).ok()?;
        if text.contains("RefBraid") { program_in(&text) } else { None }
    };
    let wanted = |prog: &PathBuf| !only_stale || !prog.exists() || prog.starts_with("/usr/") || prog.starts_with("/opt/");
    let mut gone = vec![];
    let mut desktop_gone = false;
    if let Some(prog) = ours(&desktop) {
        if wanted(&prog) && std::fs::remove_file(&desktop).is_ok() {
            gone.push(desktop.clone());
            desktop_gone = true;
        }
    }
    if let Some(prog) = ours(&script) {
        if wanted(&prog) && std::fs::remove_file(&script).is_ok() {
            gone.push(script.clone());
        }
    }
    if (desktop_gone || !only_stale) && icon.exists() && std::fs::remove_file(&icon).is_ok() {
        gone.push(icon);
    }
    if !only_stale {
        // The copy of the program made by "Add to Files" (never the package's one).
        let copy = home.join(".local/bin/refbraid");
        if copy.exists() && std::fs::remove_file(&copy).is_ok() {
            gone.push(copy);
        }
    }
    if desktop_gone || !only_stale {
        let _ = std::process::Command::new("update-desktop-database").arg(&apps).status();
    }
    gone
}

/// At start-up: take out menu entries an earlier version left in the home folder that point to
/// a program that no longer exists, or that duplicate the package's entry.
#[cfg(not(windows))]
pub fn cleanup_stale() {
    if let Some(home) = home() {
        let _ = tidy_user_files(&home, true);
    }
}

#[cfg(windows)]
pub fn cleanup_stale() {}

/// "Remove from Files": take RefBraid out of the file manager for this user.
#[cfg(not(windows))]
pub fn uninstall() -> Result<String, String> {
    let home = home().ok_or("No home folder")?;
    let gone = tidy_user_files(&home, false);
    Ok(if gone.is_empty() {
        "There was nothing to take out of your home folder.".into()
    } else {
        format!("Taken out:\n{}", gone.iter().map(|p| format!("  {}", p.display())).collect::<Vec<_>>().join("\n"))
    })
}

/// Windows: take "Open in RefBraid" out of the folder menu.
#[cfg(windows)]
pub fn uninstall() -> Result<String, String> {
    for key in [
        r"HKCU\Software\Classes\Directory\shell\RefBraid",
        r"HKCU\Software\Classes\Directory\Background\shell\RefBraid",
    ] {
        let _ = crate::git::no_console(&mut std::process::Command::new("reg"))
            .args(["delete", key, "/f"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok("\"Open in RefBraid\" was taken out of the folder menu.".into())
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn stale_entries_are_taken_out() {
        let home = std::env::temp_dir().join(format!("ge-home-{}", std::process::id()));
        let apps = home.join(".local/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        let desktop = apps.join("refbraid.desktop");
        // Points to the package's program: a duplicate of the package's own entry.
        std::fs::write(&desktop, "[Desktop Entry]\nName=RefBraid\nExec=\"/usr/bin/refbraid\" %f\n").unwrap();
        assert_eq!(program_in(&std::fs::read_to_string(&desktop).unwrap()), Some(PathBuf::from("/usr/bin/refbraid")));
        tidy_user_files(&home, true);
        assert!(!desktop.exists());
        // Points to a copy that still exists: kept.
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("refbraid"), "x").unwrap();
        std::fs::write(&desktop, format!("[Desktop Entry]\nName=RefBraid\nExec=\"{}\" %f\n", bin.join("refbraid").display())).unwrap();
        tidy_user_files(&home, true);
        assert!(desktop.exists());
        // "Remove from Files" takes everything out.
        tidy_user_files(&home, false);
        assert!(!desktop.exists() && !bin.join("refbraid").exists());
    }
}
