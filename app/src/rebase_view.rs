//! The Interactive Rebase window: choose what happens to each commit, then let git run it.

use crate::app::{short, App, Dialog, Job};
use crate::i18n::tr;
use crate::form::{self, Part};
use crate::rebase::{self, Action, Plan, Preset};
use crate::theme::{self, Icon};
use eframe::egui::{self, pos2, vec2, FontId, Key, Rect, RichText, Sense, Stroke, Ui};

/// What the AI part of the window asks for.
#[derive(Default)]
struct AiAsk {
    start: bool,
    stop: bool,
    undo: bool,
}

const ROW_H: f32 = 24.0;
const WIDTH: f32 = 780.0;

impl App {
    /// Open the window for the commits from `from` up to the top of the current branch.
    pub fn open_rebase(&mut self, from: &str, preset: Preset) {
        let Some(repo) = self.repo.as_ref() else { return };
        let plan = match repo.state.operation {
            Some(op) => Err(format!("{} is in progress. Finish or abort it first.", op.title())),
            None => rebase::load(&repo.path, from),
        };
        self.dialog = Some(match plan {
            Ok(mut p) => {
                p.apply(preset);
                Dialog::Rebase(p)
            }
            Err(text) => Dialog::Message { title: tr("Interactive Rebase").into(), text },
        });
    }

    /// Menu items for commits of the log (indexes into the commit list; the newest is first there).
    pub fn open_rebase_for(&mut self, idx: &[usize], preset: fn(Vec<String>) -> Preset) {
        let Some(repo) = self.repo.as_ref() else { return };
        let hashes: Vec<String> = idx.iter().filter_map(|&i| repo.commits.get(i)).map(|c| c.hash.clone()).collect();
        let Some(oldest) = idx.iter().max().and_then(|&i| repo.commits.get(i)).map(|c| c.hash.clone()) else { return };
        self.open_rebase(&oldest, preset(hashes));
    }

    /// Screenshot helper (REFBRAID_VIEW=irebase): the window with a few actions chosen.
    pub fn demo_rebase(&mut self) {
        let Some(dir) = self.repo.as_ref().map(|r| r.path.clone()) else { return };
        // The longest run of plain commits under HEAD (the sample repository has merges).
        let Some(mut plan) = (1..=6).rev().find_map(|n| rebase::load(&dir, &format!("HEAD~{n}")).ok()).or_else(|| rebase::load(&dir, "HEAD").ok()) else { return };
        let n = plan.entries.len();
        if n >= 2 {
            plan.entries[1].action = Action::Squash;
        }
        if n >= 4 {
            plan.entries[2].action = Action::Drop;
            plan.entries[3].edited = Some(format!("{} (reworded)", plan.entries[3].message));
        }
        plan.selected = 0;
        self.dialog = Some(Dialog::Rebase(plan));
    }

    /// Ask the AI agent chosen in Settings for the best plan of these commits.
    fn start_rebase_ai(&mut self, plan: &mut Plan, branch: &str) {
        let agent = crate::ai::harness(&self.settings.agent).and_then(|h| self.agents.iter().find(|(id, _)| *id == h.id).map(|(_, p)| (h, p.clone())));
        let Some((h, program)) = agent else {
            self.open_settings(Some(crate::settings_view::Page::AiAgents));
            return;
        };
        let Some(repo) = self.repo.as_ref() else { return };
        let rules = if self.settings.ai_prompt.trim().is_empty() { crate::ai::DEFAULT_PROMPT.to_string() } else { self.settings.ai_prompt.replace("$GIT_BRANCH_NAME", branch) };
        let prompt = rebase::ai_prompt(&repo.path, branch, &plan.entries, &rules);
        plan.error = None;
        self.rebase_ai = Some((crate::ai::start_with(h, &program, &repo.path, prompt, |s| s.trim().to_string()), std::time::Instant::now()));
    }

    /// Screenshot helper (REFBRAID_VIEW=irebaseai): the window after a made-up AI answer.
    pub fn demo_rebase_ai(&mut self) {
        let Some(Dialog::Rebase(plan)) = self.dialog.as_mut() else { return };
        for e in plan.entries.iter_mut() {
            e.action = Action::Pick;
            e.edited = None;
        }
        let mut answer = String::new();
        for (i, e) in plan.entries.iter().enumerate() {
            let action = if i == 1 { "fixup" } else if i == 2 { "squash" } else { "pick" };
            answer += &format!("COMMIT: {}\nACTION: {action}\n", &e.hash[..9]);
            if i == 0 {
                answer += &format!("MESSAGE: {} (with its fixes)\n", e.message.lines().next().unwrap_or(""));
            }
        }
        answer += "NOTE: The second commit only fixes the first one, so it is folded into it; the third continues the same task and is squashed with its message kept.";
        if let Ok((entries, note)) = rebase::parse_ai(&answer, &plan.entries) {
            plan.ai_before = Some(std::mem::replace(&mut plan.entries, entries));
            plan.ai_note = Some(note);
            plan.selected = 0;
        }
    }

    pub fn rebase_dialog(&mut self, ctx: &egui::Context, mut d: Dialog) {
        let mut keep = true;
        let mut job: Option<Job> = None;
        let branch = self.repo.as_ref().and_then(|r| r.state.head_branch.clone()).unwrap_or_else(|| "HEAD".into());
        let dir = self.repo.as_ref().map(|r| r.path.clone());
        // The AI's answer: its plan replaces the one in the window (Undo brings that back).
        if let (Some((run, _)), Dialog::Rebase(plan)) = (&self.rebase_ai, &mut d) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            if let Ok(res) = run.rx.try_recv() {
                let agent = run.agent.clone();
                match res.and_then(|text| rebase::parse_ai(&text, &plan.entries).map_err(|e| format!("{e}\n\n{}", text.chars().take(400).collect::<String>()))) {
                    Ok((entries, note)) => {
                        plan.ai_before = Some(std::mem::replace(&mut plan.entries, entries));
                        plan.ai_note = Some(if note.is_empty() { format!("{agent} suggested this plan.") } else { note });
                        plan.selected = 0;
                        plan.error = None;
                    }
                    Err(e) => plan.error = Some(format!("{agent}: {e}")),
                }
                self.rebase_ai = None;
            }
        }
        let ai_state = self.rebase_ai.as_ref().map(|(r, t)| (r.agent.clone(), t.elapsed().as_secs()));
        let mut ask = AiAsk::default();
        let closed = form::show(ctx, "rebase_dialog", "Interactive Rebase", WIDTH, |ui, part| {
            let Dialog::Rebase(plan) = &mut d else { return };
            if part == Part::Body {
                rebase_body(ui, plan, &branch, ai_state.as_ref(), &mut ask);
                return;
            }
            let problem = plan.problem();
            let (ok, cancel) = form::ok_cancel(ui, tr("Start Rebasing"), problem.is_none() && plan.changed(), false);
            if cancel {
                keep = false;
            }
            if ok {
                let Some(dir) = &dir else { return };
                let cmd = rebase::prepare(dir, plan).and_then(|(file, args)| {
                    let mut cmd = vec![format!("env:GIT_SEQUENCE_EDITOR={}", rebase::editor_command(&file)?)];
                    cmd.extend(args);
                    Ok(cmd)
                });
                match cmd {
                    Ok(cmd) => {
                        job = Some(Job::many(&format!("Rebase {branch}"), vec![cmd]));
                        keep = false;
                    }
                    Err(e) => plan.error = Some(e),
                }
            }
        });
        if let Dialog::Rebase(plan) = &mut d {
            if ask.start && self.rebase_ai.is_none() {
                self.start_rebase_ai(plan, &branch);
            }
            if ask.undo {
                if let Some(before) = plan.ai_before.take() {
                    plan.entries = before;
                    plan.ai_note = None;
                    plan.selected = plan.selected.min(plan.entries.len() - 1);
                }
            }
        }
        if ask.stop || !keep || closed {
            if let Some((run, _)) = self.rebase_ai.take() {
                crate::ai::cancel(&run);
            }
        }
        if keep && !closed && self.dialog.is_none() {
            self.dialog = Some(d);
        }
        if let Some(j) = job {
            self.run_job(j);
        }
    }
}

fn action_label(plan: &Plan, i: usize) -> (&'static str, egui::Color32) {
    let e = &plan.entries[i];
    match e.action {
        Action::Pick if e.edited.is_some() && rebase::final_message(&plan.entries, i).is_some() => ("reword", theme::LINK()),
        Action::Pick => ("pick", theme::TEXT_MID()),
        Action::Squash => ("squash", theme::STAR()),
        Action::Fixup => ("fixup", theme::STAR()),
        Action::Drop => ("drop", theme::RED()),
    }
}

fn rebase_body(ui: &mut Ui, plan: &mut Plan, branch: &str, ai: Option<&(String, u64)>, ask: &mut AiAsk) {
    let n = plan.entries.len();
    plan.selected = plan.selected.min(n - 1);
    ui.label(
        RichText::new(format!("{n} commit{} of '{branch}', oldest first — the order they are applied in.", if n == 1 { "" } else { "s" })).color(theme::TEXT_MID()),
    );
    ui.add_space(4.0);

    // Toolbar: what to do with the selected commit.
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    let (alt_up, alt_down, up, down) = ui.input(|i| {
        (
            i.modifiers.alt && i.key_pressed(Key::ArrowUp),
            i.modifiers.alt && i.key_pressed(Key::ArrowDown),
            !i.modifiers.alt && i.key_pressed(Key::ArrowUp),
            !i.modifiers.alt && i.key_pressed(Key::ArrowDown),
        )
    });
    let mut scroll = false;
    ui.horizontal(|ui| {
        let sel = plan.selected;
        if ui.add_enabled(sel > 0, egui::Button::new(tr("Move Up"))).on_hover_text(tr("Apply this commit earlier (Alt+Up)")).clicked() || (alt_up && sel > 0) {
            plan.shift(false);
            scroll = true;
        }
        if ui.add_enabled(sel + 1 < n, egui::Button::new(tr("Move Down"))).on_hover_text(tr("Apply this commit later (Alt+Down)")).clicked() || (alt_down && sel + 1 < n) {
            plan.shift(true);
            scroll = true;
        }
        ui.separator();
        let sel = plan.selected;
        let cur = plan.entries[sel].action;
        if ui.selectable_label(cur == Action::Pick && plan.entries[sel].edited.is_none(), "Pick").on_hover_text(tr("Keep the commit as it is")).clicked() {
            plan.entries[sel].action = Action::Pick;
            plan.entries[sel].edited = None;
        }
        if ui.selectable_label(action_label(plan, sel).0 == "reword", "Reword").on_hover_text(tr("Keep the commit and change its message below")).clicked() {
            plan.entries[sel].action = Action::Pick;
            plan.focus_message = true;
        }
        if ui.selectable_label(cur == Action::Squash, tr("Squash")).on_hover_text(tr("Join into the commit above; its message is added to that commit's message")).clicked() {
            plan.entries[sel].action = Action::Squash;
        }
        if ui.selectable_label(cur == Action::Fixup, tr("Fixup")).on_hover_text(tr("Join into the commit above; its message is thrown away")).clicked() {
            plan.entries[sel].action = Action::Fixup;
        }
        if ui.selectable_label(cur == Action::Drop, tr("Drop")).on_hover_text(tr("Remove the commit and its changes")).clicked() {
            plan.entries[sel].action = Action::Drop;
        }
        // The AI suggests the whole plan.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match ai {
            Some(_) => {
                if ui.button(tr("Stop")).clicked() {
                    ask.stop = true;
                }
                ui.add(egui::Spinner::new().size(14.0));
            }
            None => {
                let (r, resp) = ui.allocate_exact_size(vec2(theme::text_width(ui, tr("Suggest with AI"), &FontId::proportional(13.0)) + 38.0, 24.0), Sense::click());
                let fill = if resp.hovered() { theme::HOVER() } else { theme::BG_BAR() };
                ui.painter().rect(r, 4.0, fill, Stroke::new(1.0, theme::BORDER()), egui::StrokeKind::Inside);
                theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(14.0, 14.0)), Icon::Spiral, theme::LINK());
                ui.painter().text(pos2(r.left() + 26.0, r.center().y), egui::Align2::LEFT_CENTER, tr("Suggest with AI"), FontId::proportional(13.0), theme::TEXT());
                if resp
                    .on_hover_text(tr("The AI agent chosen in Settings looks at the commits and their changes and suggests what to squash, reword, drop or move. You can change its plan before rebasing."))
                    .clicked()
                {
                    ask.start = true;
                }
            }
        });
    });
    // What the AI is doing, or what it suggested.
    if let Some((agent, secs)) = ai {
        ui.add_space(4.0);
        ui.label(RichText::new(crate::i18n::trf("{} is working out the best plan for these commits… {} s", &[agent, secs])).color(theme::TEXT_MID()));
    } else if let Some(note) = plan.ai_note.clone() {
        ui.add_space(4.0);
        egui::Frame::new().fill(theme::BG_BAR()).stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                theme::paint_icon(ui.painter(), r, Icon::Spiral, theme::LINK());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Undo")).on_hover_text(tr("Bring back the plan as it was before the AI's suggestion")).clicked() {
                        ask.undo = true;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(note).color(theme::TEXT())).wrap());
                    });
                });
            });
        });
    }
    if !typing {
        if up && plan.selected > 0 {
            plan.selected -= 1;
            scroll = true;
        }
        if down && plan.selected + 1 < n {
            plan.selected += 1;
            scroll = true;
        }
    }
    ui.add_space(4.0);

    // The commits. Height added to the window goes to the list as far as it has rows to show,
    // the rest to the message.
    let grow = form::grow(ui);
    let list_grow = ((n as f32 - 11.0) * ROW_H).clamp(0.0, grow * 0.6);
    let message_rows = 6 + ((grow - list_grow) / 17.0) as usize;
    let font = FontId::proportional(13.0);
    let mono = FontId::monospace(12.0);
    egui::Frame::new().fill(theme::BG_INPUT()).stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(4).inner_margin(1).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("rebase_rows").max_height(ROW_H * 11.0 + list_grow).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in 0..n {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                if resp.clicked() {
                    plan.selected = i;
                }
                if i == plan.selected && scroll {
                    ui.scroll_to_rect(r, None);
                }
                if !ui.is_rect_visible(r) {
                    continue;
                }
                let p = ui.painter_at(r);
                if i == plan.selected {
                    p.rect_filled(r, 0.0, theme::SELECT());
                } else if resp.hovered() {
                    p.rect_filled(r, 0.0, theme::HOVER());
                }
                let e = &plan.entries[i];
                let cy = r.center().y;
                let (label, color) = action_label(plan, i);
                p.text(pos2(r.left() + 10.0, cy), egui::Align2::LEFT_CENTER, label, font.clone(), color);
                p.text(pos2(r.left() + 70.0, cy), egui::Align2::LEFT_CENTER, short(&e.hash), mono.clone(), theme::LINK());
                // Right side: "pushed" mark and the author.
                let mut right = r.right() - 10.0;
                if e.pushed {
                    let g = p.layout_no_wrap("pushed".into(), FontId::proportional(11.0), theme::STAR());
                    right -= g.size().x;
                    p.galley(pos2(right, cy - g.size().y / 2.0), g, theme::STAR());
                    right -= 10.0;
                }
                let g = p.layout_no_wrap(e.author.clone(), FontId::proportional(12.0), theme::TEXT_DIM());
                let aw = g.size().x.min(150.0);
                right -= aw;
                p.with_clip_rect(Rect::from_min_max(pos2(right, r.top()), pos2(right + aw, r.bottom()))).galley(pos2(right, cy - g.size().y / 2.0), g, theme::TEXT_DIM());
                // The subject: joined commits are indented under the commit they go into.
                let joined = matches!(e.action, Action::Squash | Action::Fixup);
                let x = r.left() + 140.0 + if joined { 16.0 } else { 0.0 };
                if joined {
                    p.text(pos2(x - 14.0, cy), egui::Align2::LEFT_CENTER, "↳", font.clone(), theme::TEXT_DIM());
                }
                let subject = if e.action == Action::Pick { rebase::text(&plan.entries, i) } else { e.message.clone() };
                let subject = subject.lines().next().unwrap_or("").to_string();
                let color = match e.action {
                    Action::Pick => theme::TEXT(),
                    Action::Squash => theme::TEXT_MID(),
                    Action::Fixup | Action::Drop => theme::TEXT_DIM(),
                };
                let w = theme::paint_clipped(&p, pos2(x, cy), &subject, font.clone(), color, right - 12.0 - x);
                if e.action == Action::Drop {
                    p.line_segment([pos2(x, cy + 1.0), pos2(x + w, cy + 1.0)], Stroke::new(1.0, theme::TEXT_DIM()));
                }
            }
        });
    });
    ui.add_space(8.0);

    // The message of the selected commit.
    let sel = plan.selected;
    let action = plan.entries[sel].action;
    if action == Action::Pick {
        let default = rebase::default_message(&plan.entries, sel);
        let joined = default != plan.entries[sel].message;
        ui.label(RichText::new(if joined { "Commit message (with the messages of the squashed commits):" } else { "Commit message:" }).color(theme::TEXT_MID()));
        let mut text = rebase::text(&plan.entries, sel);
        let r = theme::edit_menu(ui.add(egui::TextEdit::multiline(&mut text).id_salt("rebase_message").desired_rows(message_rows).desired_width(f32::INFINITY)));
        if r.changed() {
            plan.entries[sel].edited = (text != default).then_some(text);
        }
        if plan.focus_message {
            r.request_focus();
            plan.focus_message = false;
        }
    } else {
        let (title, note) = match action {
            Action::Squash => ("Squash:", "The changes go into the commit above, and this message is added to that commit's message. Select the commit above to edit the joined message."),
            Action::Fixup => ("Fixup:", "The changes go into the commit above. This message is thrown away."),
            _ => ("Drop:", "The commit and its changes are removed from the branch."),
        };
        ui.label(RichText::new(title).color(theme::TEXT_MID()));
        let mut text = format!("{note}\n\n{}", plan.entries[sel].message);
        ui.add_enabled(false, egui::TextEdit::multiline(&mut text).id_salt("rebase_note").desired_rows(message_rows).desired_width(f32::INFINITY));
        plan.focus_message = false;
    }

    // Warnings and errors.
    // Everything from the first changed commit on gets a new hash.
    let first = (0..n).find(|&i| plan.entries[i].hash != plan.original[i] || plan.entries[i].action != Action::Pick || rebase::final_message(&plan.entries, i).is_some());
    let touched = first.is_some_and(|f| plan.entries[f..].iter().any(|e| e.pushed));
    let line = if let Some(e) = plan.error.clone().or_else(|| plan.problem()) {
        Some((e, theme::RED()))
    } else if touched {
        Some(("Commits that are already pushed will be rewritten: the branch will need a force push afterwards.".to_string(), theme::STAR()))
    } else {
        None
    };
    ui.add_space(6.0);
    let (text, color) = line.unwrap_or((" ".into(), theme::TEXT_DIM()));
    ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
}
