//! Help → Check for Updates: the look at GitHub once a day when the program starts, the notice
//! about a newer version and the window with what is new in it and the file to download — or,
//! on RHEL and the like, the button that downloads the package and installs it with dnf.

use crate::app::{App, Toast};
use crate::i18n::{tr, trf};
use crate::theme;
use crate::update::{self, Pack, Release, Step};
use eframe::egui::{self, RichText};
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// The running version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub struct State {
    /// The answer on its way, and whether the user asked (errors are then shown).
    wait: Option<(Receiver<Result<Release, String>>, bool)>,
    /// The latest release known, whatever its version.
    pub latest: Option<Release>,
    pub error: Option<String>,
    /// The window is open.
    pub open: bool,
    /// The program looked by itself since it was started.
    looked: bool,
    /// Installing the update: the steps from the thread, and the last one.
    install: Option<(Receiver<Step>, Step)>,
    /// How the program was installed, when a screenshot shows another way than the real one.
    pub pack: Option<Pack>,
}

fn now() -> i64 {
    chrono::Local::now().timestamp()
}

fn open(ui: &egui::Ui, url: &str) {
    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
}

impl State {
    /// Show a step of the installation without doing it (screenshots).
    pub fn show_step(&mut self, step: Step) {
        let (_, rx) = std::sync::mpsc::channel();
        self.install = Some((rx, step));
    }
}

impl App {
    /// A newer version the user has not been told to forget.
    pub fn update_found(&self) -> Option<&Release> {
        self.update.latest.as_ref().filter(|r| update::newer(&r.version, VERSION))
    }

    fn start_update_check(&mut self, ctx: &egui::Context, manual: bool) {
        if self.update.wait.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::fetch(update::API));
            ctx.request_repaint();
        });
        self.update.wait = Some((rx, manual));
        self.update.error = None;
    }

    /// Help → Check for Updates…
    pub fn check_for_updates(&mut self, ctx: &egui::Context) {
        self.update.open = true;
        self.start_update_check(ctx, true);
    }

    /// Every frame: look once a day by itself, and take the answer when it comes.
    pub fn update_tick(&mut self, ctx: &egui::Context) {
        // Never from a screenshot run or a test: nothing is asked and nothing is written.
        let quiet = self.screenshot.is_some() || cfg!(test);
        if !quiet && !self.update.looked && !self.settings.no_update_check && now() - self.settings.update_checked >= update::EVERY_SECS {
            self.update.looked = true;
            self.start_update_check(ctx, false);
        }
        let Some((rx, manual)) = &self.update.wait else { return };
        let manual = *manual;
        let Ok(answer) = rx.try_recv() else { return };
        self.update.wait = None;
        if !quiet {
            // A failure counts too: a network that GitHub refuses is not asked again at once.
            self.settings.update_checked = now();
            self.settings.save();
        }
        match answer {
            Ok(r) => {
                let news = update::newer(&r.version, VERSION) && self.settings.update_skip != r.version;
                if news && !manual && !self.update.open {
                    self.notify(Toast { title: trf("RefBraid {} is available", &[&r.version]), text: tr("Help → Check for Updates shows what is new and where to get it.").to_string(), ok: true, at: Instant::now() });
                }
                self.update.latest = Some(r);
            }
            // Said only to the one who asked: the look at start fails silently.
            Err(e) if manual => self.update.error = Some(e),
            Err(_) => {}
        }
    }

    /// The line of the Help menu.
    pub fn update_menu_item(&mut self, ui: &mut egui::Ui) {
        let label = match self.update_found() {
            Some(r) => trf("Update to {}…", &[&r.version]),
            None => tr("Check for Updates…").to_string(),
        };
        if ui.button(label).on_hover_text(tr("Look on GitHub for a newer version of RefBraid")).clicked() {
            if self.update_found().is_some() {
                self.update.open = true;
            } else {
                self.check_for_updates(ui.ctx());
            }
            ui.close();
        }
    }

    /// Download the package, check it and install it with dnf, on a thread.
    fn start_install(&mut self, ctx: &egui::Context, r: Release, pack: Pack) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            update::install(&r, pack, &|step| {
                let _ = tx.send(step);
                ctx.request_repaint();
            });
        });
        self.update.install = Some((rx, Step::Downloading));
    }

    pub fn update_window(&mut self, ctx: &egui::Context) {
        if !self.update.open {
            return;
        }
        if let Some((rx, step)) = self.update.install.as_mut() {
            while let Ok(s) = rx.try_recv() {
                *step = s;
            }
        }
        let step = self.update.install.as_ref().map(|(_, s)| s.clone());
        let busy = matches!(step, Some(Step::Downloading | Step::Checking | Step::Installing));
        let pack = self.update.pack.unwrap_or_else(update::pack);
        let can_install = update::can_install(pack) || self.update.pack.is_some();
        let mut install = false;
        let mut restart = false;
        let checking = self.update.wait.is_some();
        let found = self.update_found().cloned();
        let latest = self.update.latest.as_ref().map(|r| r.version.clone());
        let error = self.update.error.clone();
        let asset = found.as_ref().and_then(|r| r.asset_for(pack).cloned());
        let installable = can_install && asset.is_some() && found.as_ref().is_some_and(|r| r.assets.iter().any(|(n, _)| n == update::SUMS));
        let mut keep = true;
        let mut skip = false;
        let mut again = false;
        let closed = crate::form::show(ctx, "updatewin", tr("Check for Updates"), 620.0, |ui, part| {
            if part == crate::form::Part::Footer {
                if crate::form::button(ui, tr("Close")) {
                    keep = false;
                }
                if step == Some(Step::Done) {
                    if crate::form::primary(ui, tr("Restart RefBraid"), true, false) {
                        restart = true;
                    }
                    return;
                }
                let Some(r) = &found else {
                    if !checking && ui.button(tr("Check Again")).clicked() {
                        again = true;
                    }
                    return;
                };
                match &asset {
                    Some((_, url)) if installable => {
                        if crate::form::primary(ui, &trf("Install {}", &[&r.version]), !busy, false) {
                            install = true;
                        }
                        if crate::form::button(ui, tr("Download in the Browser")) {
                            open(ui, url);
                        }
                    }
                    Some((name, url)) => {
                        if crate::form::primary(ui, &trf("Download {}", &[name]), true, false) {
                            open(ui, url);
                        }
                        if crate::form::button(ui, tr("Release Page")) {
                            open(ui, &r.url);
                        }
                    }
                    None => {
                        if crate::form::primary(ui, tr("Open the Release Page"), true, false) {
                            open(ui, &r.url);
                        }
                    }
                }
                if ui.button(tr("Skip This Version")).on_hover_text(tr("Don't tell about this version at start again")).clicked() {
                    skip = true;
                }
                return;
            }
            if checking {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tr("Looking for a newer version on GitHub…")).color(theme::TEXT_MID()));
                });
                return;
            }
            if let Some(e) = &error {
                ui.add(egui::Label::new(RichText::new(format!("{}: {e}", tr("Could not check"))).color(theme::RED())).wrap());
                ui.add_space(6.0);
                ui.hyperlink_to(tr("The releases on GitHub"), format!("https://github.com/{}/releases", update::REPO));
                return;
            }
            let Some(r) = &found else {
                ui.label(RichText::new(format!("✓ {}", trf("RefBraid {} is the latest version.", &[&VERSION]))).color(theme::GREEN()).size(14.0));
                if let Some(v) = latest.as_ref().filter(|v| *v != VERSION) {
                    ui.add_space(4.0);
                    ui.label(RichText::new(trf("The latest release is {}: this build is newer.", &[&v])).color(theme::TEXT_MID()));
                }
                return;
            };
            ui.label(RichText::new(trf("RefBraid {} is available", &[&r.version])).size(17.0).strong().color(theme::TEXT()));
            ui.label(RichText::new(trf("You have {}.", &[&VERSION])).color(theme::TEXT_MID()));
            ui.add_space(4.0);
            let how = match (&asset, pack) {
                (Some(_), _) if installable => tr("Install downloads the package, checks it against the release's checksums and installs it with dnf. The system asks for your password."),
                (Some(_), Pack::AppImage) => tr("The file is downloaded by your browser. Put it in place of the old one and make it executable."),
                (Some(_), _) => tr("The file is downloaded by your browser. Close RefBraid, then install it the way you installed this version."),
                (None, _) => tr("The release page lists the files for every system."),
            };
            ui.add(egui::Label::new(RichText::new(how).color(theme::TEXT_DIM()).size(12.0)).wrap());
            // How far the installation got.
            if let Some(s) = &step {
                ui.add_space(8.0);
                let (text, color) = match s {
                    Step::Downloading => (tr("Downloading the package…").to_string(), theme::TEXT_MID()),
                    Step::Checking => (tr("Checking the package…").to_string(), theme::TEXT_MID()),
                    Step::Installing => (tr("Installing — enter your password in the window of the system…").to_string(), theme::TEXT_MID()),
                    Step::Done => (trf("RefBraid {} is installed. Restart the program to use it.", &[&r.version]), theme::GREEN()),
                    Step::Failed(e) => (format!("{}: {}", tr("Could not install"), tr(e)), theme::RED()),
                };
                ui.horizontal_wrapped(|ui| {
                    if busy {
                        ui.spinner();
                    }
                    ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
                });
            }
            ui.add_space(8.0);
            theme::hline(ui, ui.cursor().top(), ui.max_rect().left(), ui.max_rect().right());
            ui.add_space(8.0);
            crate::changelog::notes(ui, r.notes());
        });
        if again {
            self.start_update_check(ctx, true);
        }
        if install {
            if let Some(r) = found.clone() {
                if self.update.pack.is_some() {
                    // A screenshot: nothing is downloaded, the step is only shown.
                    self.update.show_step(Step::Installing);
                } else {
                    self.start_install(ctx, r, pack);
                }
            }
        }
        if restart {
            self.restart(ctx);
        }
        if skip {
            if let Some(r) = &found {
                self.settings.update_skip = r.version.clone();
                self.settings.save();
            }
            keep = false;
        }
        if !keep || closed {
            self.update.open = false;
            self.update.error = None;
            // A finished or failed installation is forgotten; one in progress goes on.
            if !busy {
                self.update.install = None;
            }
        }
    }
}
