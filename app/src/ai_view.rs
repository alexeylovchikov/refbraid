//! AI agent screens: generating a commit message, the Settings window and the first-run question.

use crate::ai::{self, HARNESSES};
use crate::i18n::tr;
use crate::app::{App, Toast};
use crate::git::StatusEntry;
use crate::theme;
use eframe::egui::{self, RichText, Ui};
use std::time::Instant;

/// The "Prompt for generation" editor, shared by the commit options popup and Settings.
/// Returns true when the text changed.
pub fn prompt_editor(ui: &mut Ui, prompt: &mut String, show_settings: Option<&mut bool>) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Prompt for generation:")).color(theme::TEXT()));
        ui.label(RichText::new("ⓘ").color(theme::TEXT_DIM())).on_hover_text(
            tr("Instructions the AI agent follows when it writes the commit message. The changes you checked are sent along with it."),
        );
        if let Some(flag) = show_settings {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.link(tr("Open in Settings…")).clicked() {
                    *flag = true;
                    ui.close();
                }
            });
        }
    });
    // Long prompts scroll inside the field instead of stretching the window.
    let max_h = (ui.ctx().content_rect().height() * 0.25).clamp(110.0, 260.0);
    let r = egui::ScrollArea::vertical()
        .id_salt("ai_prompt_scroll")
        .max_height(max_h)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(prompt)
                    .margin(egui::Margin::symmetric(8, 6))
                    .font(egui::FontId::proportional(13.0))
                    .hint_text(ai::DEFAULT_PROMPT)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            )
        })
        .inner;
    let r = theme::edit_menu(r);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(RichText::new(tr("Use the ")).color(theme::TEXT_DIM()).size(12.0));
        ui.label(RichText::new(tr("$GIT_BRANCH_NAME")).color(theme::LINK()).size(12.0));
        ui.label(RichText::new(tr(" variable to reference the branch name")).color(theme::TEXT_DIM()).size(12.0));
    });
    r.changed()
}

/// The checkbox of the automatic commit message, shared by the commit options popup and
/// Settings. Returns true when it was changed.
pub fn auto_message_checkbox(ui: &mut Ui, on: &mut bool) -> bool {
    ui.checkbox(on, tr("Write the message when the Commit panel opens"))
        .on_hover_text(tr("When you open the Commit panel and the message is empty, the AI agent is asked for one at once, as if you pressed the spiral button. The checked changes are sent to the agent each time."))
        .changed()
}

/// Whether opening the Commit panel should ask the agent for a message right away: the
/// setting is on, there is nothing typed, no commit is being amended and something is checked.
pub fn auto_message_due(on: bool, message: &str, amend: bool, checked_files: usize, agent_ready: bool, running: bool) -> bool {
    on && agent_ready && !running && !amend && checked_files > 0 && message.trim().is_empty()
}

impl App {
    /// Every frame: when the Commit panel has just been opened, write the message with the
    /// agent if the setting says so. The changed files may still be on their way from git, so
    /// the chance is kept for a couple of seconds.
    pub fn auto_message_tick(&mut self) {
        let shown = self.show_commit && self.commit_tab == crate::app::CommitTab::Changes && self.repo.is_some();
        if shown && !self.commit_seen {
            self.auto_message_until = Some(Instant::now() + std::time::Duration::from_secs(3));
        }
        self.commit_seen = shown;
        let Some(until) = self.auto_message_until else { return };
        if !shown || Instant::now() > until || !self.settings.ai_auto_message || self.screenshot.is_some() || cfg!(test) {
            self.auto_message_until = None;
            return;
        }
        let Some(repo) = self.repo.as_ref() else { return };
        // Something typed (or a message put there by a merge) is never written over.
        if !repo.commit_msg.trim().is_empty() || repo.amend {
            self.auto_message_until = None;
            return;
        }
        let checked = repo.status.iter().filter(|e| !e.is_conflict() && !repo.unchecked.contains(&e.path)).count();
        if auto_message_due(true, &repo.commit_msg, repo.amend, checked, self.agent_ready(), self.ai_job.is_some()) {
            self.auto_message_until = None;
            self.start_ai();
        } else {
            self.ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    pub fn agent_name(&self) -> Option<&'static str> {
        ai::harness(&self.settings.agent).map(|h| h.name)
    }

    pub fn agent_ready(&self) -> bool {
        self.agents.iter().any(|(id, _)| *id == self.settings.agent)
    }

    /// Ask the chosen agent for a commit message for the checked changes.
    pub fn start_ai(&mut self) {
        if let Some(job) = self.ai_job.take() {
            ai::cancel(&job);
            return;
        }
        let Some(h) = ai::harness(&self.settings.agent) else {
            self.open_settings(Some(crate::settings_view::Page::AiAgents));
            return;
        };
        let Some((_, program)) = self.agents.iter().find(|(id, _)| *id == h.id).cloned() else {
            self.open_settings(Some(crate::settings_view::Page::AiAgents));
            return;
        };
        let Some(repo) = self.repo.as_ref() else { return };
        let files: Vec<StatusEntry> = repo
            .status
            .iter()
            .filter(|e| !e.is_conflict() && !repo.unchecked.contains(&e.path))
            .cloned()
            .collect();
        if files.is_empty() && !repo.amend {
            self.ai_error = Some("Check at least one changed file first.".into());
            return;
        }
        let branch = repo.state.head_branch.clone().unwrap_or_else(|| "HEAD".into());
        let prompt = ai::build_prompt(&repo.path, &branch, &files, &self.settings.ai_prompt, repo.amend);
        self.ai_error = None;
        self.ai_job = Some(ai::start(h, &program, &repo.path, prompt));
    }

    pub fn poll_ai(&mut self) {
        let Some(job) = self.ai_job.as_ref() else { return };
        self.ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let Ok(res) = job.rx.try_recv() else { return };
        let agent = job.agent.clone();
        self.ai_job = None;
        match res {
            Ok(msg) => {
                if let Some(r) = self.repo.as_mut() {
                    r.commit_msg = msg;
                }
            }
            Err(e) => {
                self.ai_error = Some(e.clone());
                self.notify(Toast {
                    title: format!("{agent} could not write a message"),
                    text: e,
                    ok: false,
                    at: Instant::now(),
                });
            }
        }
    }

    fn agent_list(&mut self, ui: &mut Ui, only_found: bool) {
        if agent_radio_list(ui, &self.agents, &mut self.settings.agent, only_found) {
            self.settings.save();
        }
    }

    pub fn ai_windows(&mut self, ctx: &egui::Context) {
        if self.show_agent_setup {
            let mut close = false;
            let closed = crate::form::show(ctx, "agent_setup", "Connect an AI Agent", 520.0, |ui, part| {
                if part == crate::form::Part::Footer {
                    if crate::form::button(ui, tr("Skip")) {
                        self.settings.agent.clear();
                        close = true;
                    }
                    if crate::form::primary(ui, tr("Continue"), true, false) {
                        close = true;
                    }
                    return;
                }
                ui.label(
                    RichText::new(tr("RefBraid can write commit messages for you with an AI agent that is already installed on this computer. These were found:"))
                        .color(theme::TEXT_MID()),
                );
                ui.add_space(8.0);
                self.agent_list(ui, true);
                ui.add_space(6.0);
                ui.label(
                    RichText::new(tr("You can change this any time in File → Settings."))
                        .color(theme::TEXT_DIM())
                        .size(12.0),
                );
            });
            if close || closed {
                self.show_agent_setup = false;
                self.settings.ai_setup_done = true;
                self.settings.save();
            }
        }
    }
}

/// Radio buttons for choosing the AI agent. Returns true when the choice changed.
pub fn agent_radio_list(ui: &mut Ui, agents: &[(&'static str, std::path::PathBuf)], agent: &mut String, only_found: bool) -> bool {
    let mut changed = false;
    for h in HARNESSES {
        let found = agents.iter().find(|(id, _)| *id == h.id).map(|(_, p)| p.clone());
        if only_found && found.is_none() {
            continue;
        }
        ui.horizontal(|ui| {
            let resp = ui.add_enabled(found.is_some(), egui::RadioButton::new(*agent == h.id, h.name));
            if resp.clicked() {
                *agent = h.id.to_string();
                changed = true;
            }
            match &found {
                Some(p) => ui.label(RichText::new(p.display().to_string()).color(theme::TEXT_DIM()).size(11.5)),
                None => ui.label(RichText::new(tr("not installed")).color(theme::TEXT_DIM()).size(11.5)),
            };
        });
    }
    if ui.radio(agent.is_empty(), "Don't use an AI agent").clicked() {
        agent.clear();
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::auto_message_due;

    #[test]
    fn the_message_is_written_by_itself_only_into_an_empty_field() {
        // (setting, message, amend, checked files, agent ready, already running)
        assert!(auto_message_due(true, "", false, 2, true, false));
        assert!(auto_message_due(true, "  \n", false, 1, true, false));
        // Off by the setting; something typed; an amend; nothing checked; no agent; one at work.
        assert!(!auto_message_due(false, "", false, 2, true, false));
        assert!(!auto_message_due(true, "Fix the form", false, 2, true, false));
        assert!(!auto_message_due(true, "", true, 2, true, false));
        assert!(!auto_message_due(true, "", false, 0, true, false));
        assert!(!auto_message_due(true, "", false, 2, false, false));
        assert!(!auto_message_due(true, "", false, 2, true, true));
    }
}
