//! Git LFS in the interface: the commands of Git → LFS and the window with the patterns of
//! files kept in LFS.

use crate::app::{App, Dialog, Job};
use crate::i18n::tr;
use crate::theme;
use eframe::egui::{self, RichText, Ui};

/// The Tracked Patterns window.
pub struct LfsForm {
    pub patterns: Vec<String>,
    /// The pattern being typed in.
    pub new: String,
}

impl App {
    /// Run a command of git-lfs, or say how to get the program when it is not installed.
    pub fn run_lfs(&mut self, title: &str, cmds: Vec<Vec<String>>) {
        if !crate::lfs::installed() {
            self.dialog = Some(Dialog::Message { title: tr("Git LFS").into(), text: crate::lfs::how_to_install() });
            return;
        }
        self.run_job(Job::many(title, cmds));
    }

    /// The items of Git → LFS.
    pub fn lfs_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(240.0);
        if ui.button(tr("Pull")).on_hover_text(tr("Download the LFS files of the current commit and put them into the working folder (git lfs pull)")).clicked() {
            self.run_lfs("LFS pull", crate::lfs::pull_args());
            ui.close();
        }
        if ui.button(tr("Fetch")).on_hover_text(tr("Download the LFS files of the current commit without touching the working folder (git lfs fetch)")).clicked() {
            self.run_lfs("LFS fetch", crate::lfs::fetch_args(false));
            ui.close();
        }
        if ui.button(tr("Fetch All")).on_hover_text(tr("Download the LFS files of every branch and commit: may be a lot (git lfs fetch --all)")).clicked() {
            self.run_lfs("LFS fetch", crate::lfs::fetch_args(true));
            ui.close();
        }
        ui.separator();
        if ui.button(tr("Tracked Patterns…")).on_hover_text(tr("Which files of the repository are kept in LFS")).clicked() {
            self.open_lfs_patterns();
            ui.close();
        }
    }

    pub fn open_lfs_patterns(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        self.lfs_form = Some(LfsForm { patterns: crate::lfs::patterns(&repo.path), new: String::new() });
    }

    pub fn lfs_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.lfs_form.take() else { return };
        // After a track or untrack ran, .gitattributes says something else.
        if let Some(repo) = self.repo.as_ref().filter(|_| self.running.is_empty()) {
            f.patterns = crate::lfs::patterns(&repo.path);
        }
        let mut keep = true;
        let mut track: Option<String> = None;
        let mut untrack: Option<String> = None;
        let installed = crate::lfs::installed();
        let closed = crate::form::show(ctx, "lfspatterns", "LFS: Tracked Patterns", 520.0, |ui, part| {
            if part == crate::form::Part::Footer {
                if crate::form::button(ui, tr("Close")) {
                    keep = false;
                }
                return;
            }
            ui.label(RichText::new(tr("Files matching these patterns are kept in Git LFS: the repository holds a small pointer, the content is stored apart. The list is the .gitattributes file in the root of the repository; commit it after a change.")).color(theme::TEXT_DIM()).size(12.0));
            ui.add_space(8.0);
            if !installed {
                ui.label(RichText::new(tr("Git LFS is not installed on this computer: the list can be read but not changed.")).color(theme::STAR()).size(12.0));
                ui.add_space(8.0);
            }
            egui::ScrollArea::vertical().max_height(220.0 + crate::form::grow(ui)).auto_shrink([false, true]).show(ui, |ui| {
                if f.patterns.is_empty() {
                    ui.label(RichText::new(tr("No patterns yet.")).color(theme::TEXT_DIM()));
                }
                for p in &f.patterns {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(p).monospace());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(installed, egui::Button::new(tr("Remove"))).on_hover_text(tr("Stop keeping these files in LFS from now on (git lfs untrack); files already committed stay as they are")).clicked() {
                                untrack = Some(p.clone());
                            }
                        });
                    });
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let can = installed && !f.new.trim().is_empty();
                let add = ui.add_enabled(can, egui::Button::new(tr("Add"))).clicked();
                let r = theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.new).hint_text("*.psd").desired_width(f32::INFINITY)));
                if add || (can && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    track = Some(f.new.trim().to_string());
                    f.new.clear();
                }
            });
        });
        if let Some(p) = track {
            self.run_lfs("LFS track", crate::lfs::track_args(&p));
        }
        if let Some(p) = untrack {
            self.run_lfs("LFS untrack", crate::lfs::untrack_args(&p));
        }
        if keep && !closed {
            self.lfs_form = Some(f);
        }
    }
}
