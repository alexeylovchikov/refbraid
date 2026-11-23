//! Small modal dialogs: new branch, rename, tag, delete, reset, push, pull, stash, checkout.

use crate::app::{App, Dialog, Job, ResetMode, short};
use crate::i18n::{tr, trf};
use crate::form::{self, Part};
use crate::theme;
use eframe::egui::{self, Key, RichText, Ui};

fn line(ui: &mut Ui, text: &mut String, focus: bool) {
    let r = theme::edit_menu(ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY)));
    if focus && !r.has_focus() && ui.memory(|m| m.focused().is_none()) {
        r.request_focus();
    }
}

/// The program's version, from Cargo.toml (raised with `make bump-patch` and friends).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn about_body(ui: &mut Ui, git: &str) {
    let id = egui::Id::unique("about_icon");
    let tex = ui.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
        let icon = crate::integrate::icon_data(128);
        let img = egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
        let t = ui.ctx().load_texture("about_icon", img, egui::TextureOptions::LINEAR);
        ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
        t
    });
    ui.horizontal_top(|ui| {
        ui.add(egui::Image::new((tex.id(), egui::vec2(64.0, 64.0))));
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(tr("RefBraid")).size(20.0).strong().color(theme::TEXT()));
            ui.label(RichText::new(format!("Version {VERSION}")).size(14.0).color(theme::TEXT()));
            ui.label(RichText::new(if git.is_empty() { "git was not found" } else { git }).color(theme::TEXT_MID()));
            ui.add_space(8.0);
            ui.label(
                RichText::new(tr("A standalone Git window: history graph, branches, commit, diff, push and pull.\nEvery action runs your installed git; the Console tab shows the exact commands."))
                    .color(theme::TEXT_DIM()),
            );
        });
    });
}

/// The `git merge` command of the Merge window. `mode`: 0 = git's default, 1 = --no-ff,
/// 2 = --ff-only, 3 = --squash.
pub fn merge_args(rev: &str, mode: u8, no_commit: bool, message: &str) -> Vec<String> {
    let mut a = vec!["merge".to_string()];
    match mode {
        1 => a.push("--no-ff".into()),
        2 => a.push("--ff-only".into()),
        3 => a.push("--squash".into()),
        _ => {}
    }
    let msg = message.trim();
    if mode == 2 {
        // Nothing to commit and nothing to describe.
    } else if mode == 3 {
        // A squash never commits; the message goes to the Commit panel.
    } else {
        if no_commit {
            a.push("--no-commit".into());
        }
        if msg.is_empty() {
            a.push("--no-edit".into());
        } else {
            a.extend(["-m".to_string(), msg.to_string()]);
        }
    }
    a.push(rev.to_string());
    a
}

/// The `git cherry-pick` / `git revert` command of the options window. `mainline` is the
/// parent (1-based) a merge commit's changes are taken against.
pub fn pick_args(revert: bool, hashes: &[String], record: bool, no_commit: bool, mainline: Option<usize>) -> Vec<String> {
    let mut a = vec![if revert { "revert" } else { "cherry-pick" }.to_string()];
    if revert && !no_commit {
        a.push("--no-edit".into());
    }
    if !revert && record {
        a.push("-x".into());
    }
    if no_commit {
        a.push("--no-commit".into());
    }
    if let Some(m) = mainline {
        a.extend(["-m".to_string(), m.max(1).to_string()]);
    }
    a.extend(hashes.iter().cloned());
    a
}

fn merge_job(name: &str, rev: &str, mode: u8, no_commit: bool, message: &str) -> Job {
    let job = Job::many(&format!("Merge {name}"), vec![merge_args(rev, mode, no_commit, message)]);
    // Squash and --no-commit leave the result as local changes: the Commit panel finishes it.
    if mode == 3 || (no_commit && mode != 2) { job.then(crate::app::Then::ShowCommit) } else { job }
}

impl App {
    /// The Merge window, opened with the options chosen last time.
    pub fn open_merge_options(&mut self, rev: &str, name: &str) {
        self.dialog = Some(Dialog::Merge { rev: rev.to_string(), name: name.to_string(), mode: self.settings.merge_mode.min(3), no_commit: self.settings.merge_no_commit, message: String::new() });
    }

    /// Cherry-pick (or revert) the commits at `rows` of the log with options: oldest first when
    /// picking, newest first when reverting.
    pub fn open_pick(&mut self, rows: &[usize], revert: bool) {
        let Some(repo) = self.repo.as_ref() else { return };
        let mut rows: Vec<usize> = rows.to_vec();
        rows.sort_unstable();
        if !revert {
            rows.reverse();
        }
        let list: Vec<&crate::git::Commit> = rows.iter().filter_map(|&i| repo.commits.get(i)).collect();
        let commits: Vec<(String, String)> = list.iter().map(|c| (c.hash.clone(), c.subject.clone())).collect();
        // One merge commit: its parents to choose from. (Several commits share one -m, so the
        // choice is offered only when every merge among them has the same number of parents.)
        let parents: Vec<(String, String)> = list
            .iter()
            .find(|c| c.parents.len() > 1)
            .map(|c| c.parents.iter().map(|p| (p.clone(), crate::git::run(&repo.path, &["log", "-1", "--format=%s", p]).stdout.trim().to_string())).collect())
            .unwrap_or_default();
        self.dialog = Some(Dialog::Pick { revert, commits, parents, record: false, no_commit: false, mainline: 1 });
    }

    /// Git → Clean…: list what `git clean` would delete and ask.
    pub fn open_clean(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        self.dialog = Some(Dialog::Clean(crate::ignore::CleanForm { ignored: false, files: crate::ignore::clean_preview(&repo.path, false) }));
    }

    /// Add ignore lines to `.gitignore` (or to `.git/info/exclude`) and reload the changes.
    pub fn add_to_ignore(&mut self, exclude: bool, lines: Vec<String>) {
        let Some(repo) = self.repo.as_ref() else { return };
        let res = crate::ignore::add(&repo.path, exclude, &lines);
        let file = if exclude { ".git/info/exclude" } else { ".gitignore" };
        self.notify(match res {
            Ok(true) => crate::app::Toast { title: trf("Added to {}", &[&file]), text: lines.join("\n"), ok: true, at: std::time::Instant::now() },
            Ok(false) => crate::app::Toast { title: trf("Already in {}", &[&file]), text: lines.join("\n"), ok: true, at: std::time::Instant::now() },
            Err(e) => crate::app::Toast { title: trf("Could not write {}", &[&file]), text: e, ok: false, at: std::time::Instant::now() },
        });
        self.refresh();
    }

    pub fn open_about(&mut self) {
        let git = crate::git::run(std::path::Path::new("."), &["--version"]).stdout.trim().to_string();
        self.dialog = Some(Dialog::About { git });
    }

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialog.take() else { return };
        if matches!(d, Dialog::CreatePatch(_) | Dialog::ApplyPatch(_)) {
            self.patch_dialog(ctx, d);
            return;
        }
        if matches!(d, Dialog::Rebase(_)) {
            self.rebase_dialog(ctx, d);
            return;
        }
        if matches!(d, Dialog::Clone(_)) {
            self.clone_dialog(ctx, d);
            return;
        }
        let mut keep = true;
        let mut job: Option<Job> = None;
        let mut group_done: Option<(Option<String>, String)> = None;
        let mut next: Option<Dialog> = None;
        let mut always_smart = false;
        let mut merge_choice: Option<(u8, bool, String, String)> = None;
        let mut pick_msg: Option<String> = None;
        let mut reload_clean = false;
        let mut compare_with: Option<(String, crate::app::CompareItem)> = None;
        // What was picked in the palette: an action, or a branch to check out.
        let mut picked_action: Option<&'static str> = None;
        let mut picked_branch: Option<(String, crate::git::RefKind)> = None;
        // The palette's lines: (name, the note at the right).
        let palette_items: Vec<(String, String)> = match &d {
            Dialog::Palette { branches: false, .. } => crate::keymap::ACTIONS.iter().map(|a| (tr(a.name).trim_end_matches('…').to_string(), self.key_of(a.id))).collect(),
            Dialog::Palette { branches: true, .. } => self.palette_branches().into_iter().map(|(name, kind, current)| (name, if current { tr("current").to_string() } else if kind == crate::git::RefKind::Remote { tr("remote").to_string() } else { String::new() })).collect(),
            _ => vec![],
        };
        let title = match &d {
            Dialog::NewBranch { .. } => "Create New Branch".to_string(),
            Dialog::RenameBranch { .. } => "Rename Branch".into(),
            Dialog::NewTag { .. } => "Create Tag".into(),
            Dialog::DeleteBranch { .. } => "Delete Branch".into(),
            Dialog::Reset { .. } => "Git Reset".into(),
            Dialog::Pull { .. } => "Pull to Current Branch".into(),
            Dialog::Stash { .. } => "Stash Changes".into(),
            Dialog::StashBranch { .. } => "New Branch from Stash".into(),
            Dialog::Bisect { .. } => "Bisect".into(),
            Dialog::Palette { branches: false, .. } => "Find Action".into(),
            Dialog::Palette { branches: true, .. } => "Checkout Branch".into(),
            Dialog::CompareFile { branches: true, .. } => "Compare with Branch".into(),
            Dialog::CompareFile { branches: false, .. } => "Compare with Revision".into(),
            Dialog::Checkout { .. } => "Checkout Tag or Revision".into(),
            Dialog::Confirm { title, .. } => title.clone(),
            Dialog::Message { title, .. } | Dialog::Output { title, .. } => title.clone(),
            Dialog::CheckoutProblem { args, .. } if args.first().is_some_and(|a| a == "checkout") => "Git Checkout Problem".into(),
            Dialog::CheckoutProblem { .. } => "Git Update Problem".into(),
            Dialog::DivergentPull { .. } => "Branches Have Diverged".into(),
            Dialog::About { .. } => "About RefBraid".into(),
            Dialog::Changelog => "Changelog".into(),
            Dialog::FavGroup { old: None, .. } => "New Group".into(),
            Dialog::FavGroup { old: Some(_), .. } => "Rename Group".into(),
            Dialog::EditMessage { .. } => "Edit Commit Message".into(),
            Dialog::Merge { .. } => "Merge".into(),
            Dialog::Clean(_) => "Clean Working Folder".into(),
            Dialog::Pick { revert: false, .. } => "Cherry-Pick".into(),
            Dialog::Pick { revert: true, .. } => "Revert Commit".into(),
            Dialog::CreatePatch(_) | Dialog::ApplyPatch(_) | Dialog::Rebase(_) | Dialog::Clone(_) => String::new(),
        };
        let head = self
            .repo
            .as_ref()
            .and_then(|r| r.state.head_branch.clone())
            .unwrap_or_else(|| "HEAD".into());
        let width = match &d {
            Dialog::Changelog | Dialog::Output { .. } => 680.0,
            Dialog::EditMessage { .. } | Dialog::Reset { .. } | Dialog::Merge { .. } | Dialog::CheckoutProblem { .. } | Dialog::DivergentPull { .. } => 560.0,
            _ => 480.0,
        };
        let enter = ctx.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
        let prefixes = self.settings.branch_prefixes();
        let closed = form::show(ctx, "dialog", &title, width, |ui, part| {
            let body = part == Part::Body;
            match &mut d {
                Dialog::NewBranch { from, prefix, name, checkout } => {
                    // The prefix is added in front unless the name already starts with it.
                    let full = if name.trim().starts_with(prefix.as_str()) { name.trim().to_string() } else { format!("{prefix}{}", name.trim()) };
                    let valid = !name.trim().is_empty() && !full.contains(char::is_whitespace);
                    if body {
                        ui.label(RichText::new(format!("From: {from}")).color(theme::TEXT_MID()));
                        ui.add_space(6.0);
                        ui.label("New branch name:");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("branch_prefix")
                                .width(110.0)
                                .selected_text(if prefix.is_empty() { RichText::new(tr("No prefix")).color(theme::TEXT_DIM()) } else { RichText::new(prefix.as_str()) })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(prefix, String::new(), "No prefix");
                                    for p in &prefixes {
                                        let r = ui.selectable_value(prefix, p.name.clone(), &p.name);
                                        if !p.description.is_empty() {
                                            r.on_hover_text(&p.description);
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(tr("Branch prefix (edit the list in Settings → Version Control → Branch Prefixes)"));
                            line(ui, name, true);
                        });
                        let desc = prefixes.iter().find(|p| !prefix.is_empty() && p.name == *prefix).map(|p| p.description.trim()).unwrap_or("");
                        if !desc.is_empty() {
                            ui.add_space(2.0);
                            ui.label(RichText::new(desc).color(theme::TEXT_DIM()).size(12.0));
                        }
                        if !name.trim().is_empty() && !prefix.is_empty() {
                            ui.add_space(2.0);
                            ui.label(RichText::new(format!("Branch: {full}")).color(theme::TEXT_MID()).size(12.0));
                        }
                        ui.add_space(6.0);
                        ui.checkbox(checkout, tr("Checkout branch"));
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Create"), valid, enter);
                        if ok {
                            let n = full.clone();
                            job = Some(if *checkout {
                                Job::one(&format!("Create branch {n}"), &["checkout", "-b", &n, from])
                            } else {
                                Job::one(&format!("Create branch {n}"), &["branch", &n, from])
                            });
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::RenameBranch { old, new } => {
                    if body {
                        ui.label(format!("Rename '{old}' to:"));
                        line(ui, new, true);
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Rename"), !new.trim().is_empty() && new.trim() != old, enter);
                        if ok {
                            job = Some(Job::one("Rename branch", &["branch", "-m", old, new.trim()]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::NewTag { rev, name, message } => {
                    if body {
                        ui.label(RichText::new(format!("On: {}", short(rev))).color(theme::TEXT_MID()));
                        ui.add_space(6.0);
                        ui.label("Tag name:");
                        line(ui, name, true);
                        ui.add_space(6.0);
                        ui.label("Message (optional, makes an annotated tag):");
                        line(ui, message, false);
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Create Tag"), !name.trim().is_empty(), enter);
                        if ok {
                            let n = name.trim().to_string();
                            job = Some(if message.trim().is_empty() {
                                Job::one(&format!("Create tag {n}"), &["tag", &n, rev])
                            } else {
                                Job::one(&format!("Create tag {n}"), &["tag", "-a", &n, "-m", message.trim(), rev])
                            });
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::DeleteBranch { name, remote, force } => {
                    if body {
                        if *remote {
                            ui.label(format!("Delete remote branch '{name}' from the server?"));
                        } else {
                            ui.label(format!("Delete local branch '{name}'?"));
                            ui.add_space(4.0);
                            ui.checkbox(force, tr("Force delete (even if it is not merged)"));
                        }
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Delete"), true, enter);
                        if ok {
                            job = Some(Job::many(&format!("Delete {name}"), vec![crate::ops::delete_branch_args(name, *remote, *force)]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::Reset { hash, subject, mode } => {
                    if body {
                        ui.label(format!("Reset '{head}' to {} \"{subject}\"", short(hash)));
                        ui.add_space(6.0);
                        for (m, t, help) in [
                            (ResetMode::Soft, "Soft", "Files are kept; the changes of the dropped commits stay staged."),
                            (ResetMode::Mixed, "Mixed", "Files are kept; the changes become unstaged."),
                            (ResetMode::Hard, "Hard", "Throws away local changes and the dropped commits' changes."),
                            (ResetMode::Keep, "Keep", "Like Hard, but stops if local changes would be lost."),
                        ] {
                            ui.radio_value(mode, m, t);
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                ui.label(RichText::new(help).color(theme::TEXT_DIM()).size(11.5));
                            });
                        }
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Reset"), true, enter);
                        if ok {
                            job = Some(Job::many("Reset", vec![crate::ops::reset_args(mode, hash)]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::Pull { rebase, source } => {
                    if body {
                        ui.label(format!("Pull into '{head}' from:"));
                        line(ui, source, false);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.radio_value(rebase, false, tr("Merge"));
                            ui.radio_value(rebase, true, tr("Rebase"));
                        });
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Pull"), !source.trim().is_empty(), enter);
                        if ok {
                            let (remote, rb) = source
                                .trim()
                                .split_once('/')
                                .map(|(a, b)| (a.to_string(), b.to_string()))
                                .unwrap_or(("origin".into(), source.trim().to_string()));
                            let mode = if *rebase { "--rebase" } else { "--no-rebase" };
                            job = Some(Job::one("Pull", &["pull", "--no-edit", mode, &remote, &rb]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::Stash { message, untracked, keep_index, paths, only } => {
                    if body {
                        ui.label("Message:");
                        line(ui, message, true);
                        ui.add_space(4.0);
                        ui.checkbox(untracked, tr("Include unversioned files"));
                        ui.checkbox(keep_index, tr("Keep the staged changes in place")).on_hover_text(tr("What is already added to the index stays in the working folder (git stash --keep-index)"));
                        if !paths.is_empty() {
                            let label = match paths.as_slice() {
                                [one] => crate::i18n::trf("Only the selected file: {}", &[&crate::app::file_name(one)]),
                                _ => crate::i18n::trf("Only the {} selected files", &[&paths.len()]),
                            };
                            ui.checkbox(only, label).on_hover_text(paths.join("\n"));
                        }
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Create Stash"), true, enter);
                        if ok {
                            let chosen: &[String] = if *only { paths } else { &[] };
                            // A selected file that is not under Git yet is only taken with the unversioned ones.
                            let new_file = self.repo.as_ref().is_some_and(|r| r.status.iter().any(|e| e.is_untracked() && chosen.contains(&e.path)));
                            job = Some(Job::many("Stash", vec![crate::git::stash_push_args(message, *untracked || new_file, *keep_index, chosen)]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::CompareFile { path, branches, filter, items } => {
                    if body {
                        ui.label(RichText::new(crate::i18n::trf("{} in the working folder is compared with:", &[&*path])).color(theme::TEXT_MID()));
                        ui.add_space(4.0);
                        line(ui, filter, true);
                        ui.add_space(4.0);
                        let needle = filter.trim().to_lowercase();
                        let shown: Vec<&crate::app::CompareItem> = items.iter().filter(|i| needle.is_empty() || i.label.to_lowercase().contains(&needle) || i.note.to_lowercase().contains(&needle)).collect();
                        egui::Frame::new().fill(theme::BG()).stroke(egui::Stroke::new(1.0, theme::BORDER())).inner_margin(6).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(320.0 + form::grow(ui)).auto_shrink([false, false]).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if shown.is_empty() {
                                    let none = if items.is_empty() && !*branches { tr("No commit has changed this file yet") } else { tr("Nothing found") };
                                    ui.label(RichText::new(none).color(theme::TEXT_DIM()));
                                }
                                for (k, it) in shown.iter().enumerate() {
                                    let (row, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), if *branches { 24.0 } else { 38.0 }), egui::Sense::click());
                                    if resp.hovered() {
                                        ui.painter().rect_filled(row, 3.0, theme::HOVER());
                                    }
                                    let x = row.left() + 8.0;
                                    if *branches {
                                        theme::paint_clipped(ui.painter(), egui::pos2(x, row.center().y), &it.label, egui::FontId::proportional(13.0), theme::TEXT(), row.width() - 110.0);
                                        ui.painter().text(egui::pos2(row.right() - 8.0, row.center().y), egui::Align2::RIGHT_CENTER, &it.note, egui::FontId::proportional(12.0), theme::TEXT_DIM());
                                    } else {
                                        theme::paint_clipped(ui.painter(), egui::pos2(x, row.top() + 12.0), &it.label, egui::FontId::proportional(13.0), theme::TEXT(), row.width() - 16.0);
                                        let note = match &it.old_path {
                                            Some(o) => format!("{}  ·  {o}", it.note),
                                            None => it.note.clone(),
                                        };
                                        theme::paint_clipped(ui.painter(), egui::pos2(x, row.top() + 28.0), &note, egui::FontId::proportional(11.5), theme::TEXT_DIM(), row.width() - 16.0);
                                    }
                                    // Enter takes the only (or the first) one found.
                                    if resp.clicked() || (k == 0 && enter && !needle.is_empty()) {
                                        compare_with = Some((path.clone(), (*it).clone()));
                                    }
                                }
                            });
                        });
                    } else if form::button(ui, tr("Close")) {
                        keep = false;
                    }
                }
                Dialog::Palette { query, at, branches } => {
                    if body {
                        let shown = crate::keymap::search(query, palette_items.iter().map(|(n, _)| n.as_str()).enumerate());
                        // The arrows move through the list while the cursor stays in the field.
                        let (up, down) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)));
                        let before = query.clone();
                        line(ui, query, true);
                        if *query != before {
                            *at = 0;
                        }
                        if !shown.is_empty() {
                            if down {
                                *at = (*at + 1) % shown.len();
                            }
                            if up {
                                *at = (*at + shown.len() - 1) % shown.len();
                            }
                            *at = (*at).min(shown.len() - 1);
                        }
                        ui.add_space(6.0);
                        let mut go: Option<usize> = None;
                        egui::ScrollArea::vertical().max_height(330.0 + form::grow(ui)).auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if shown.is_empty() {
                                ui.label(RichText::new(tr("Nothing found")).color(theme::TEXT_DIM()));
                            }
                            for (k, &i) in shown.iter().enumerate() {
                                let (name, note) = &palette_items[i];
                                let (row, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
                                if k == *at {
                                    ui.painter().rect_filled(row, 4.0, theme::SELECT());
                                    if up || down {
                                        ui.scroll_to_rect(row, None);
                                    }
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(row, 4.0, theme::HOVER());
                                }
                                theme::paint_clipped(ui.painter(), egui::pos2(row.left() + 10.0, row.center().y), name, egui::FontId::proportional(13.0), theme::TEXT(), row.width() - 150.0);
                                ui.painter().text(egui::pos2(row.right() - 10.0, row.center().y), egui::Align2::RIGHT_CENTER, note, egui::FontId::proportional(12.0), theme::TEXT_DIM());
                                if resp.clicked() {
                                    go = Some(i);
                                }
                            }
                        });
                        if enter {
                            go = go.or(shown.get(*at).copied());
                        }
                        if let Some(i) = go {
                            if *branches {
                                picked_branch = self.palette_branches().into_iter().nth(i).map(|(name, kind, _)| (name, kind));
                            } else {
                                picked_action = crate::keymap::ACTIONS.get(i).map(|a| a.id);
                            }
                            keep = false;
                        }
                    } else {
                        if form::button(ui, tr("Close")) {
                            keep = false;
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(RichText::new(tr("↑ ↓ to choose, Enter to run, Esc to close")).color(theme::TEXT_DIM()).size(12.0));
                        });
                    }
                }
                Dialog::Bisect { bad, good } => {
                    if body {
                        ui.add(egui::Label::new(RichText::new(tr("Finds the commit that broke something. Git checks out commits between the two below one by one; you try each and answer Good or Bad in the bar that appears above the Log.")).color(theme::TEXT_MID())).wrap());
                        ui.add_space(8.0);
                        ui.label(tr("Bad commit (the problem is there):"));
                        line(ui, bad, false);
                        ui.add_space(4.0);
                        ui.label(tr("Good commit (it still worked):"));
                        line(ui, good, true);
                        ui.add_space(2.0);
                        ui.label(RichText::new(tr("A hash, a tag or a branch.")).color(theme::TEXT_DIM()).size(12.0));
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Start"), !bad.trim().is_empty() && !good.trim().is_empty() && bad.trim() != good.trim(), enter);
                        if ok {
                            job = Some(Job::one("Bisect start", &["bisect", "start", bad.trim(), good.trim()]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::StashBranch { stash, label, name } => {
                    if body {
                        ui.label(RichText::new(crate::i18n::trf("From the stash: {}", &[&*label])).color(theme::TEXT_MID()));
                        ui.add_space(6.0);
                        ui.label(tr("Branch name:"));
                        line(ui, name, true);
                        ui.add_space(4.0);
                        ui.label(RichText::new(tr("The branch starts at the commit the stash was made on; the stash is applied to it and removed.")).color(theme::TEXT_DIM()).size(12.0));
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Create Branch"), !name.trim().is_empty(), enter);
                        if ok {
                            job = Some(Job::one("New branch from stash", &["stash", "branch", name.trim(), stash]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::Checkout { rev } => {
                    if body {
                        ui.label("Tag, branch or commit hash:");
                        line(ui, rev, true);
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Checkout"), !rev.trim().is_empty(), enter);
                        if ok {
                            job = Some(Job::one(&format!("Checkout {}", rev.trim()), &["checkout", rev.trim()]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::Confirm { text, ok, job: j, .. } => {
                    if body {
                        ui.label(text.as_str());
                    } else {
                        let (go, cancel) = form::ok_cancel(ui, ok, true, enter);
                        if go {
                            job = j.take();
                        }
                        keep &= !(go || cancel);
                    }
                }
                Dialog::CheckoutProblem { args, files, always } => {
                    let target = args.iter().skip(1).rev().find(|a| !a.starts_with('-')).cloned().unwrap_or_default();
                    // A checkout, or a pull / merge (an update).
                    let switch = args.first().is_some_and(|a| a == "checkout");
                    if body {
                        ui.label(if switch {
                            format!("Your local changes to these files would be overwritten by checkout of {target}:")
                        } else {
                            "Your local changes to these files would be overwritten by the incoming changes:".to_string()
                        });
                        ui.add_space(6.0);
                        egui::Frame::new()
                            .fill(theme::BG_INPUT())
                            .stroke(egui::Stroke::new(1.0, theme::INPUT_BORDER()))
                            .corner_radius(4)
                            .inner_margin(6)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                egui::ScrollArea::vertical().max_height(150.0 + form::grow(ui)).show(ui, |ui| {
                                    for f in files.iter() {
                                        ui.horizontal(|ui| {
                                            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                            theme::paint_icon(ui.painter(), r, theme::Icon::File, theme::TEXT_MID());
                                            ui.label(RichText::new(f).color(theme::BLUE_FILE()));
                                        });
                                    }
                                    if files.is_empty() {
                                        ui.label(RichText::new(tr("(git did not list the files)")).color(theme::TEXT_DIM()));
                                    }
                                });
                            });
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(if switch {
                                "Smart Checkout puts your changes aside (stash), switches to the branch and applies them again. If they clash with the branch, you resolve the conflicts as usual.\nForce Checkout switches without keeping your changes to these files."
                            } else {
                                "Smart Update puts your changes aside (stash), updates the branch and applies them again. If they clash with the incoming changes, you resolve the conflicts as usual."
                            })
                            .color(theme::TEXT_MID()),
                        );
                        ui.add_space(8.0);
                        ui.checkbox(always, tr("Always do this, don't ask again"));
                    } else {
                        if form::button(ui, if switch { "Don't Checkout" } else { "Don't Update" }) {
                            keep = false;
                        }
                        if switch && form::button(ui, tr("Force Checkout")) {
                            let mut forced = args.clone();
                            forced.insert(1, "-f".into());
                            let n = files.len();
                            next = Some(Dialog::Confirm {
                                title: tr("Force Checkout").into(),
                                text: format!("Switch to {target} without keeping your local changes to {n} file{}? They cannot be brought back.", if n == 1 { "" } else { "s" }),
                                ok: "Force Checkout".into(),
                                job: Some(Job::many(&format!("Force checkout {target}"), vec![forced])),
                            });
                            keep = false;
                        }
                        if form::primary(ui, if switch { "Smart Checkout" } else { "Smart Update" }, true, enter) {
                            job = Some(Job::smart_checkout(args));
                            always_smart = *always;
                            keep = false;
                        }
                    }
                }
                Dialog::DivergentPull { args, target, counts, rebase, remember } => {
                    if body {
                        let what = match counts {
                            Some((mine, theirs)) => format!(
                                "Your branch and {target} have gone different ways: {mine} commit{} only here and {theirs} commit{} only there.",
                                if *mine == 1 { "" } else { "s" },
                                if *theirs == 1 { "" } else { "s" }
                            ),
                            None => format!("Your branch and {target} have gone different ways: each has commits the other doesn't."),
                        };
                        ui.label(what);
                        ui.label(RichText::new(tr("Choose how to bring the incoming commits in:")).color(theme::TEXT_MID()));
                        ui.add_space(10.0);
                        for (value, title, text) in [
                            (false, "Merge", "Join both lines with a merge commit. History stays exactly as it happened."),
                            (true, "Rebase", "Put your local commits on top of the incoming ones, for a straight history. Use it only for commits you haven't pushed yet."),
                        ] {
                            ui.radio_value(rebase, value, RichText::new(title).strong());
                            ui.horizontal(|ui| {
                                ui.add_space(26.0);
                                ui.add(egui::Label::new(RichText::new(text).color(theme::TEXT_DIM()).size(12.5)).wrap());
                            });
                            ui.add_space(6.0);
                        }
                        ui.add_space(4.0);
                        ui.checkbox(remember, tr("Remember for this repository (don't ask again)"))
                            .on_hover_text(if *rebase { "Runs: git config pull.rebase true" } else { "Runs: git config pull.rebase false" });
                    } else {
                        let cancel = form::button(ui, tr("Cancel"));
                        let go = form::primary(ui, if *rebase { "Pull with Rebase" } else { "Pull with Merge" }, true, enter);
                        if go {
                            let mut cmds: Vec<Vec<String>> = vec![];
                            if *remember {
                                cmds.push(vec!["config".into(), "pull.rebase".into(), if *rebase { "true" } else { "false" }.into()]);
                            }
                            cmds.push(crate::app::pull_with(args, *rebase));
                            job = Some(Job::many(if *rebase { "Pull (rebase)" } else { "Pull (merge)" }, cmds));
                        }
                        keep &= !(go || cancel);
                    }
                }
                Dialog::Output { note, text, .. } => {
                    if body {
                        ui.add(egui::Label::new(RichText::new(note.as_str()).color(theme::TEXT_MID())).wrap());
                        ui.add_space(6.0);
                        egui::Frame::new().fill(theme::BG()).stroke(egui::Stroke::new(1.0, theme::BORDER())).inner_margin(8).show(ui, |ui| {
                            egui::ScrollArea::both().max_height(300.0 + form::grow(ui)).auto_shrink([false, true]).show(ui, |ui| {
                                ui.add(egui::Label::new(RichText::new(text.as_str()).monospace().size(12.5).color(theme::TEXT())).selectable(true).extend());
                            });
                        });
                    } else {
                        if form::primary(ui, tr("Close"), true, enter) {
                            keep = false;
                        }
                        if form::button(ui, tr("Copy")) {
                            ui.ctx().copy_text(text.clone());
                        }
                    }
                }
                Dialog::Message { text, .. } => {
                    if body {
                        ui.label(text.as_str());
                    } else if form::primary(ui, tr("OK"), true, enter) {
                        keep = false;
                    }
                }
                Dialog::FavGroup { old, name } => {
                    let taken = self.settings.fav_groups.iter().any(|g| g.name == name.trim() && Some(&g.name) != old.as_ref());
                    let valid = !name.trim().is_empty() && !taken;
                    if body {
                        ui.label(RichText::new(tr("Group name")).color(theme::TEXT_MID()));
                        ui.add_space(4.0);
                        line(ui, name, true);
                        if taken {
                            ui.add_space(4.0);
                            ui.label(RichText::new(tr("There is already a group with this name.")).color(theme::RED()));
                        }
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, if old.is_some() { "Rename" } else { "Create" }, valid, enter);
                        if ok {
                            group_done = Some((old.clone(), name.clone()));
                            keep = false;
                        }
                        if cancel {
                            keep = false;
                        }
                    }
                }
                Dialog::About { git } => {
                    if body {
                        about_body(ui, git);
                    } else {
                        if form::primary(ui, tr("OK"), true, enter) {
                            keep = false;
                        }
                        if form::button(ui, tr("Copy Version")) {
                            ui.ctx().copy_text(format!("RefBraid {VERSION}\n{git}"));
                        }
                    }
                }
                Dialog::Changelog => {
                    if body {
                        crate::changelog::body(ui);
                    } else if form::primary(ui, tr("Close"), true, enter) {
                        keep = false;
                    }
                }
                Dialog::EditMessage { hash, message } => {
                    if body {
                        ui.label(RichText::new(short(hash)).color(theme::TEXT_MID()));
                        ui.add_space(4.0);
                        theme::edit_menu(ui.add(egui::TextEdit::multiline(message).desired_rows(6 + (form::grow(ui) / 17.0) as usize).desired_width(f32::INFINITY)));
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Save"), !message.trim().is_empty(), false);
                        if ok {
                            job = Some(Job::one("Edit commit message", &["commit", "--amend", "--only", "-m", message.trim()]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::Merge { rev, name, mode, no_commit, message } => {
                    if body {
                        ui.label(trf("Merge '{}' into '{}'", &[name, &head]));
                        ui.add_space(8.0);
                        for (m, t, help) in [
                            (0u8, "Default", "Fast-forward when possible, otherwise a merge commit."),
                            (1, "Always create a merge commit (--no-ff)", "The branch stays visible in the history even when a fast-forward is possible."),
                            (2, "Fast-forward only (--ff-only)", "Only move the branch forward; stop if the branches have gone different ways."),
                            (3, "Squash (--squash)", "Bring all the changes in as local changes, to commit them as one commit. No merge commit is made."),
                        ] {
                            ui.radio_value(mode, m, tr(t));
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                ui.add(egui::Label::new(RichText::new(tr(help)).color(theme::TEXT_DIM()).size(11.5)).wrap());
                            });
                        }
                        ui.add_space(6.0);
                        ui.add_enabled(*mode != 3 && *mode != 2, egui::Checkbox::new(no_commit, tr("Don't commit: stop before the merge commit (--no-commit)")));
                        ui.add_space(6.0);
                        ui.label(RichText::new(if *mode == 3 { tr("Commit message (put into the Commit panel):") } else { tr("Commit message (optional):") }).color(theme::TEXT_MID()));
                        ui.add_enabled_ui(*mode != 2, |ui| {
                            theme::edit_menu(ui.add(egui::TextEdit::multiline(message).desired_rows(3 + (form::grow(ui) / 17.0) as usize).desired_width(f32::INFINITY)));
                        });
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, tr("Merge"), true, false);
                        if ok {
                            job = Some(merge_job(name, rev, *mode, *no_commit, message));
                            merge_choice = Some((*mode, *no_commit, message.trim().to_string(), name.clone()));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::Clean(f) => {
                    if body {
                        ui.label(tr("These files and folders are not in Git and will be deleted from the disk. This cannot be undone."));
                        ui.add_space(6.0);
                        if ui.checkbox(&mut f.ignored, tr("Also delete ignored files (build output, caches, local settings)")).changed() {
                            reload_clean = true;
                        }
                        ui.add_space(6.0);
                        egui::Frame::new().fill(theme::BG_INPUT()).stroke(egui::Stroke::new(1.0, theme::INPUT_BORDER())).corner_radius(4).inner_margin(6).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(220.0 + form::grow(ui)).show(ui, |ui| {
                                for p in f.files.iter() {
                                    ui.horizontal(|ui| {
                                        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                        theme::paint_icon(ui.painter(), r, if p.ends_with('/') { theme::Icon::Folder } else { theme::Icon::File }, theme::TEXT_MID());
                                        ui.label(RichText::new(p).color(theme::RED()));
                                    });
                                }
                                if f.files.is_empty() {
                                    ui.label(RichText::new(tr("Nothing to delete: the working folder is clean.")).color(theme::TEXT_DIM()));
                                }
                            });
                        });
                    } else {
                        let n = f.files.len();
                        let label = if n == 0 { tr("Delete").to_string() } else { format!("{} ({n})", tr("Delete")) };
                        let (ok, cancel) = form::ok_cancel(ui, &label, n > 0, false);
                        if ok {
                            job = Some(Job::many("Clean", vec![crate::ignore::clean_args(false, f.ignored)]));
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::Pick { revert, commits, parents, record, no_commit, mainline } => {
                    if body {
                        ui.label(if *revert {
                            tr("Create a commit that undoes the changes of:")
                        } else {
                            tr("Apply the changes of these commits to the current branch:")
                        });
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(110.0 + form::grow(ui)).show(ui, |ui| {
                            for (h, subj) in commits.iter() {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(short(h)).monospace().color(theme::TEXT_MID()));
                                    ui.add(egui::Label::new(subj.as_str()).truncate());
                                });
                            }
                        });
                        if !parents.is_empty() {
                            ui.add_space(8.0);
                            ui.label(RichText::new(if *revert {
                                tr("This is a merge commit. Undo its changes relative to which parent?")
                            } else {
                                tr("This is a merge commit. Take its changes relative to which parent?")
                            }).color(theme::TEXT_MID()));
                            for (i, (h, subj)) in parents.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.radio_value(mainline, i + 1, trf("Parent {}", &[&(i + 1)]));
                                    ui.label(RichText::new(short(h)).monospace().color(theme::TEXT_MID()));
                                    ui.add(egui::Label::new(RichText::new(subj.as_str()).color(theme::TEXT_DIM())).truncate());
                                });
                            }
                        }
                        ui.add_space(8.0);
                        if !*revert {
                            ui.checkbox(record, tr("Add \"(cherry picked from commit …)\" to the message (-x)"));
                        }
                        ui.checkbox(no_commit, tr("Don't commit: leave the result as local changes (--no-commit)"));
                    } else {
                        let (ok, cancel) = form::ok_cancel(ui, if *revert { tr("Revert") } else { tr("Cherry-Pick") }, !commits.is_empty(), enter);
                        if ok {
                            let hashes: Vec<String> = commits.iter().map(|c| c.0.clone()).collect();
                            let m = if parents.is_empty() { None } else { Some(*mainline) };
                            let what = if commits.len() == 1 { short(&commits[0].0).to_string() } else { format!("{} commits", commits.len()) };
                            let mut j = Job::many(&format!("{} {what}", if *revert { "Revert" } else { "Cherry-pick" }), vec![pick_args(*revert, &hashes, *record, *no_commit, m)]);
                            if *no_commit {
                                j = j.then(crate::app::Then::ShowCommit);
                                pick_msg = Some(commits.iter().map(|(h, subj)| if *revert { format!("Revert \"{subj}\"\n\nThis reverts commit {h}.") } else { subj.clone() }).collect::<Vec<_>>().join("\n\n"));
                            }
                            job = Some(j);
                        }
                        keep &= !(ok || cancel);
                    }
                }
                Dialog::CreatePatch(_) | Dialog::ApplyPatch(_) | Dialog::Rebase(_) | Dialog::Clone(_) => {}
            }
        });
        if closed {
            keep = false;
        }
        if let Some((path, it)) = compare_with {
            keep = false;
            self.open_compare_file(&it.rev, None, &git_file(&path, it.old_path.clone()));
        }
        if reload_clean {
            if let (Dialog::Clean(f), Some(r)) = (&mut d, self.repo.as_ref()) {
                f.files = crate::ignore::clean_preview(&r.path, f.ignored);
            }
        }
        if keep {
            self.dialog = Some(d);
        } else if next.is_some() {
            self.dialog = next;
        }
        if let Some(id) = picked_action {
            self.run_action(id);
        }
        if let Some((name, kind)) = picked_branch {
            self.checkout_branch(&name, kind);
        }
        if always_smart {
            self.settings.smart_checkout = true;
            if self.screenshot.is_none() {
                self.settings.save();
            }
        }
        if let Some((mode, no_commit, mut message, name)) = merge_choice {
            if message.is_empty() {
                message = if mode == 3 { format!("Merge branch '{name}' (squashed)") } else { format!("Merge branch '{name}'") };
            }
            self.settings.merge_mode = mode;
            self.settings.merge_no_commit = no_commit;
            if self.screenshot.is_none() {
                self.settings.save();
            }
            if mode == 3 || (no_commit && mode != 2) {
                self.pending_commit_msg = Some(message);
            }
        }
        if pick_msg.is_some() {
            self.pending_commit_msg = pick_msg;
        }
        if let Some(j) = job {
            self.run_job(j);
        }
        if let Some((old, name)) = group_done {
            self.save_fav_group(old.as_deref(), &name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_merge_commands() {
        let j = |v: Vec<String>| v.join(" ");
        assert_eq!(j(merge_args("feature", 0, false, "")), "merge --no-edit feature");
        assert_eq!(j(merge_args("feature", 1, true, " Join ")), "merge --no-ff --no-commit -m Join feature");
        assert_eq!(j(merge_args("feature", 2, true, "ignored")), "merge --ff-only feature");
        assert_eq!(j(merge_args("tags/v1", 3, true, "ignored")), "merge --squash tags/v1");
    }

    #[test]
    fn builds_pick_commands() {
        let j = |v: Vec<String>| v.join(" ");
        let h = vec!["a1".to_string(), "b2".to_string()];
        assert_eq!(j(pick_args(false, &h, false, false, None)), "cherry-pick a1 b2");
        assert_eq!(j(pick_args(false, &h[..1], true, true, Some(2))), "cherry-pick -x --no-commit -m 2 a1");
        assert_eq!(j(pick_args(true, &h[..1], true, false, Some(1))), "revert --no-edit -m 1 a1");
        assert_eq!(j(pick_args(true, &h[..1], false, true, None)), "revert --no-commit a1");
    }
}

/// A file as the comparison wants it: its name now and, when it was renamed, its name before.
fn git_file(path: &str, old_path: Option<String>) -> crate::git::ChangedFile {
    crate::git::ChangedFile { status: if old_path.is_some() { 'R' } else { 'M' }, path: path.to_string(), old_path }
}

/// The items "Compare with Branch…" and "Compare with Revision…" of a file's menu. The window
/// is opened by `App::take_compare_request` after the menu is drawn.
pub fn compare_file_items(ui: &mut Ui, path: &str, enabled: bool) {
    for (label, tip, branches) in [
        ("Compare with Branch…", "The file in the working folder against the file in another branch", true),
        ("Compare with Revision…", "The file in the working folder against the file as one of its commits left it", false),
    ] {
        if ui.add_enabled(enabled, egui::Button::new(tr(label))).on_hover_text(tr(tip)).clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("compare_file_request"), (path.to_string(), branches)));
            ui.close();
        }
    }
}

impl App {
    /// The branches the palette offers to check out: the current one, the other local ones,
    /// then the remote ones (name, kind, is the current one).
    pub fn palette_branches(&self) -> Vec<(String, crate::git::RefKind, bool)> {
        use crate::git::RefKind;
        let Some(repo) = self.repo.as_ref() else { return vec![] };
        let mut v: Vec<(String, RefKind, bool)> = repo.state.branches.iter().filter(|b| b.kind != RefKind::Tag && !b.name.ends_with("/HEAD")).map(|b| (b.name.clone(), b.kind, b.kind == RefKind::Local && b.is_head)).collect();
        v.sort_by_key(|(name, kind, current)| (!*current, *kind != RefKind::Local, name.clone()));
        v
    }
}
