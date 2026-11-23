//! Bisect in the window: starting it, the banner with Good / Bad / Skip while it goes on, and
//! the marks in a commit's menu.

use crate::app::{App, Dialog, Job, Tab, short};
use crate::i18n::{tr, trf};
use crate::theme;
use eframe::egui::{self, RichText, Ui};

impl App {
    /// Git → Bisect…: `bad` and `good` are filled in from the commits selected in the Log.
    pub fn open_bisect_dialog(&mut self, bad: Option<&str>, good: Option<&str>) {
        if let Some(op) = self.repo.as_ref().and_then(|r| r.state.operation) {
            self.dialog = Some(Dialog::Message { title: tr("Bisect").into(), text: trf("A {} is in progress. Finish or abort it first.", &[&op.title().to_lowercase()]) });
            return;
        }
        self.dialog = Some(Dialog::Bisect { bad: bad.unwrap_or("HEAD").to_string(), good: good.unwrap_or("").to_string() });
    }

    /// Bisect between the two commits selected in the Log: the newer one is the bad one.
    pub fn bisect_between(&mut self, a: usize, b: usize) {
        let Some(repo) = self.repo.as_ref() else { return };
        let (new, old) = (a.min(b), a.max(b));
        let (Some(new), Some(old)) = (repo.commits.get(new).map(|c| c.hash.clone()), repo.commits.get(old).map(|c| c.hash.clone())) else { return };
        self.open_bisect_dialog(Some(short(&new)), Some(short(&old)));
    }

    /// The bisect in progress: which commit is being tested and the answers to give.
    pub fn bisect_banner(&mut self, ui: &mut Ui) {
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(b) = repo.state.bisect.clone() else { return };
        let subject = |hash: &str| repo.index.get(hash).and_then(|&i| repo.commits.get(i)).map(|c| c.subject.clone()).unwrap_or_default();
        let head = repo.state.head_hash.clone();
        let text = match &b.found {
            Some(h) => trf("Bisect: the first bad commit is {} {}", &[&short(h), &subject(h)]),
            None if b.testing() => {
                let left = trf("About {} more to test ({} commits left).", &[&b.steps(), &b.left.saturating_sub(1)]);
                format!("{} {left}", trf("Bisect: is {} {} good or bad?", &[&short(&head), &subject(&head)]))
            }
            None => tr("Bisect: mark one good and one bad commit in the Log (right-click a commit).").to_string(),
        };
        let mut job: Option<Job> = None;
        let mut show: Option<String> = None;
        egui::Panel::top("bisect_banner")
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::rgb(0x23, 0x30, 0x3d)).inner_margin(egui::Margin::symmetric(12, 6)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(if b.found.is_some() { tr("Finish") } else { tr("Stop") }).on_hover_text(tr("End the bisect and go back to where you were (git bisect reset)")).clicked() {
                            job = Some(Job::one("Bisect reset", &["bisect", "reset"]));
                        }
                        if let Some(h) = &b.found {
                            if ui.button(tr("Show Commit")).clicked() {
                                show = Some(h.clone());
                            }
                        } else if b.testing() {
                            if ui.button(tr("Skip")).on_hover_text(tr("This commit cannot be tested (it does not build, for example)")).clicked() {
                                job = Some(Job::one("Bisect skip", &["bisect", "skip"]));
                            }
                            if ui.button(RichText::new(tr("Bad")).color(theme::RED())).on_hover_text(tr("The problem is there in this commit")).clicked() {
                                job = Some(Job::one("Bisect bad", &["bisect", "bad"]));
                            }
                            if ui.button(RichText::new(tr("Good")).color(theme::GREEN())).on_hover_text(tr("This commit works as it should")).clicked() {
                                job = Some(Job::one("Bisect good", &["bisect", "good"]));
                            }
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(text).color(theme::TEXT())).truncate());
                        });
                    });
                });
            });
        if let Some(j) = job {
            self.run_job(j);
        }
        if let Some(h) = show {
            self.tab = Tab::Log;
            self.navigate_to(&h);
        }
    }

    /// The bisect items of a commit's menu in the Log.
    pub fn bisect_items(&mut self, ui: &mut Ui, picked: &[usize], hash: &str) {
        let active = self.repo.as_ref().is_some_and(|r| r.state.bisect.is_some());
        if active {
            let found = self.repo.as_ref().is_some_and(|r| r.state.bisect.as_ref().is_some_and(|b| b.found.is_some()));
            for (label, word) in [("Bisect: Mark as Good", "good"), ("Bisect: Mark as Bad", "bad")] {
                if ui.add_enabled(!found, egui::Button::new(tr(label))).clicked() {
                    self.run_job(Job::one(&format!("Bisect {word}"), &["bisect", word, hash]));
                }
            }
        } else if picked.len() == 2 {
            if ui.button(tr("Bisect Between These Commits…")).on_hover_text(tr("Find the commit that broke something: the newer one is taken as bad, the older one as good")).clicked() {
                self.bisect_between(picked[0], picked[1]);
            }
        } else if ui.button(tr("Bisect: This Commit Is Good…")).on_hover_text(tr("Find the commit that broke something between this commit and the current one")).clicked() {
            self.open_bisect_dialog(None, Some(short(hash)));
        }
    }
}
