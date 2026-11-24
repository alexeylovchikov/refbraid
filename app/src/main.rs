#![cfg_attr(windows, windows_subsystem = "windows")]

mod ai;
mod ai_view;
mod app;
mod askpass;
mod bisect;
mod bisect_view;
mod blame;
mod blame_view;
mod branch_menu;
mod branches_view;
mod changelog;
mod chrome;
mod clone_view;
mod commit_view;
mod config_view;
mod credentials;
mod details_view;
mod dialogs;
mod form;
mod log_view;
mod merge;
mod ops;
mod merge_view;
mod partial;
mod patch;
mod i18n;
mod i18n_ru;
mod wordiff;
mod push_view;
mod rebase;
mod reflog;
mod reflog_view;
mod rebase_view;
mod favorites_view;
mod find;
mod external;
mod hosting;
mod hosting_view;
mod http;
mod jira;
mod keymap;
mod lfs;
mod lfs_view;
mod ext_tools_view;
mod split_view;
mod submodule;
mod syntax;
mod demo;
mod encoding;
mod git;
mod image_view;
mod graph;
mod ignore;
mod headless;
mod integrate;
mod settings;
mod settings_view;
mod skills;
#[cfg(test)]
mod testrepo;
mod theme;
mod undo;
mod update;
mod update_view;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // An interactive rebase starts this same program as git's "sequence editor": it hands git
    // the todo list prepared in the Interactive Rebase window.
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("--rebase-todo") {
        std::process::exit(rebase::sequence_editor(&argv[2..]));
    }
    // When git asks for a password it starts this same program with a prompt.
    if std::env::var_os("REFBRAID_ASKPASS").is_some() {
        let prompt = std::env::args().nth(1).unwrap_or_else(|| "Password:".into());
        std::process::exit(askpass::run(&prompt));
    }

    // Started from Finder or the Dock a program gets only the system folders in PATH, so git,
    // git-lfs and the agents installed with Homebrew or MacPorts would not be found.
    #[cfg(target_os = "macos")]
    {
        let path = std::env::var("PATH").unwrap_or_default();
        let mut dirs: Vec<&str> = path.split(':').filter(|d| !d.is_empty()).collect();
        let home = std::env::var("HOME").unwrap_or_default();
        let own = [format!("{home}/.local/bin"), format!("{home}/.cargo/bin")];
        let extra: Vec<&str> = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"].into_iter().chain(own.iter().map(String::as_str)).filter(|d| !dirs.contains(d)).collect();
        if !extra.is_empty() {
            // The folders the person's shell would put first come before the system ones.
            let mut all = extra;
            all.append(&mut dirs);
            // Nothing else runs yet: no other thread reads the environment.
            unsafe { std::env::set_var("PATH", all.join(":")) };
        }
    }

    // A crash would otherwise vanish without a trace (on Windows there is no console).
    std::panic::set_hook(Box::new(|info| {
        let main = std::thread::current().name() == Some("main");
        let bt = std::backtrace::Backtrace::force_capture();
        report(&format!("The program stopped because of an error:\n{info}"), &format!("{bt}"), main);
    }));

    let mut screenshot: Option<PathBuf> = None;
    let mut path: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            // A fresh sample repository stopped by merge conflicts.
            "--conflict-example" => path = demo::conflict_example(),
            // Used when building the installers: save the app icon and quit.
            "--write-icon" => {
                let out = args.next().map(PathBuf::from).unwrap_or_else(|| "refbraid.png".into());
                if let Err(e) = integrate::write_icon(&out) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--help" | "-h" => {
                println!("Usage: refbraid [FOLDER] [--screenshot FILE.png]");
                return Ok(());
            }
            _ => {
                let s = a.strip_prefix("file://").unwrap_or(&a).to_string();
                path = Some(PathBuf::from(percent_decode(&s)));
            }
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("RefBraid")
        .with_app_id("refbraid")
        .with_inner_size([1600.0, 820.0])
        .with_min_inner_size([800.0, 480.0])
        // Big enough for every place the system shows it; it is scaled down from this.
        .with_icon(std::sync::Arc::new(integrate::icon_data(256)));
    // The window opens maximised when it was closed that way.
    if screenshot.is_none() && settings::Settings::load().maximized {
        viewport = viewport.with_maximized(true);
    }
    if let Some(shot) = &screenshot {
        headless::watchdog(path.clone(), shot.clone());
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    // On Linux the desktop's own title bar (minimise / maximise / close) only appears for
    // X11 windows; Wayland windows get a plain one drawn by the toolkit. Use X11 when it is
    // available, unless REFBRAID_WAYLAND is set.
    #[cfg(target_os = "linux")]
    {
        let x11 = std::env::var("DISPLAY").map(|d| !d.is_empty()).unwrap_or(false);
        let builtin = settings::Settings::load().builtin_title_bar;
        if builtin {
            // The app draws the whole title bar itself (buttons, dragging, double-click).
            options.viewport = options.viewport.with_decorations(false);
        }
        if x11 && !builtin && std::env::var_os("REFBRAID_WAYLAND").is_none() {
            use winit::platform::x11::EventLoopBuilderExtX11;
            options.event_loop_builder = Some(Box::new(|b| {
                b.with_x11();
            }));
        }
    }
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        options.event_loop_builder = Some(Box::new(|b| {
            b.with_activate_ignoring_other_apps(false);
        }));
    }

    // On Windows draw with Direct3D 12 (works over Remote Desktop and in VMs);
    // if that cannot start, fall back to OpenGL.
    #[cfg(windows)]
    {
        options.renderer = eframe::Renderer::Wgpu;
        // Draw with Direct3D 12 only: some Vulkan drivers (old Intel igvk64.dll) crash the whole
        // process while starting, before any error can be returned. WGPU_BACKEND still overrides it.
        if std::env::var_os("WGPU_BACKEND").is_none() {
            if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
                setup.instance_descriptor.backends = eframe::wgpu::Backends::DX12;
            }
        }
        let first = run(options.clone(), path.clone(), screenshot.clone());
        let Err(wgpu_err) = first else { return first };
        log_line(&format!("Direct3D 12 did not start, trying OpenGL: {wgpu_err}"));
        options.renderer = eframe::Renderer::Glow;
        let second = run(options, path, screenshot);
        if let Err(e) = &second {
            report(
                "RefBraid could not open its window.",
                &format!("Direct3D 12: {wgpu_err}\nOpenGL: {e}"),
                true,
            );
        }
        return second;
    }
    #[cfg(not(windows))]
    {
        let r = run(options, path, screenshot);
        if let Err(e) = &r {
            report("RefBraid could not open its window.", &e.to_string(), true);
        }
        r
    }
}

/// Where startup errors and crashes are written.
fn log_file() -> Option<PathBuf> {
    Some(settings::config_dir()?.join("error.log"))
}

fn log_line(text: &str) {
    let Some(f) = log_file() else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(out, "[{now}] refbraid {}\n{text}\n", env!("CARGO_PKG_VERSION"));
    }
}

/// Writes the error to the log and stderr; with `show`, also tells the person in a message box.
fn report(title: &str, details: &str, show: bool) {
    log_line(&format!("{title}\n{details}"));
    eprintln!("{title}\n{details}");
    if show {
        let mut text = title.to_string();
        // Short details go into the box; long ones (a backtrace) only into the log.
        if !details.trim().is_empty() && details.lines().count() <= 8 {
            text.push_str("\n\n");
            text.push_str(details.trim_end());
        }
        if let Some(f) = log_file() {
            text.push_str(&format!("\n\nDetails were saved to:\n{}", f.display()));
        }
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("RefBraid")
            .set_description(text)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}

fn run(options: eframe::NativeOptions, path: Option<PathBuf>, screenshot: Option<PathBuf>) -> eframe::Result<()> {
    eframe::run_native(
        "RefBraid",
        options,
        Box::new(move |cc| {
            theme::setup(&cc.egui_ctx);
            Ok(Box::new(app::App::new(&cc.egui_ctx, path, screenshot)))
        }),
    )
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
