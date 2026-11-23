//! Git → Git Config…: the repository's remotes and the most used `git config` values,
//! for this repository and for all repositories (global), in one window.

use crate::app::{App, Job};
use crate::i18n::tr;
use crate::form::{self, Part};
use crate::git;
use crate::theme;
use eframe::egui::{self, RichText, Ui, vec2};
use std::path::Path;

pub struct RemoteRow {
    /// The name it had when the window opened; None for a remote added here.
    orig: Option<String>,
    orig_url: String,
    orig_push: String,
    name: String,
    url: String,
    push_url: String,
    removed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    /// A choice; "" means "not set".
    Choice(&'static [&'static str]),
}

struct Setting {
    key: &'static str,
    label: &'static str,
    hint: &'static str,
    kind: Kind,
}

const SETTINGS: &[Setting] = &[
    Setting { key: "user.name", label: "User name", hint: "Author name of new commits", kind: Kind::Text },
    Setting { key: "user.email", label: "User email", hint: "Author email of new commits", kind: Kind::Text },
    Setting { key: "pull.rebase", label: "Pull with rebase", hint: "pull.rebase: rebase local commits instead of making a merge commit", kind: Kind::Choice(&["true", "false", "merges"]) },
    Setting { key: "push.autoSetupRemote", label: "Push sets upstream", hint: "push.autoSetupRemote: the first push of a new branch creates it on the server", kind: Kind::Choice(&["true", "false"]) },
    Setting { key: "fetch.prune", label: "Prune on fetch", hint: "fetch.prune: forget remote branches that were deleted on the server", kind: Kind::Choice(&["true", "false"]) },
    Setting { key: "core.autocrlf", label: "Line endings", hint: "core.autocrlf: convert line endings on checkout and commit", kind: Kind::Choice(&["true", "input", "false"]) },
    Setting { key: "init.defaultBranch", label: "Default branch", hint: "init.defaultBranch: name of the first branch in new repositories", kind: Kind::Text },
];

pub struct ConfigForm {
    remotes: Vec<RemoteRow>,
    /// Per setting: (original local, local, original global, global).
    values: Vec<(String, String, String, String)>,
}

fn get(dir: &Path, scope: &str, key: &str) -> String {
    git::run(dir, &["config", scope, "--get", key]).stdout.trim().to_string()
}

impl ConfigForm {
    pub fn load(dir: &Path) -> Self {
        let names = git::run(dir, &["remote"]).stdout;
        let remotes = names
            .lines()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(|n| {
                let url = get(dir, "--local", &format!("remote.{n}.url"));
                let push = get(dir, "--local", &format!("remote.{n}.pushurl"));
                RemoteRow { orig: Some(n.to_string()), orig_url: url.clone(), orig_push: push.clone(), name: n.to_string(), url, push_url: push, removed: false }
            })
            .collect();
        let values = SETTINGS
            .iter()
            .map(|s| {
                let l = get(dir, "--local", s.key);
                let g = get(dir, "--global", s.key);
                (l.clone(), l, g.clone(), g)
            })
            .collect();
        ConfigForm { remotes, values }
    }

    /// The git commands that make the changes; empty when nothing changed.
    fn commands(&self) -> Vec<Vec<String>> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        let mut cmds = vec![];
        for r in &self.remotes {
            let name = r.name.trim();
            let url = r.url.trim();
            let push = r.push_url.trim();
            match &r.orig {
                Some(orig) if r.removed => cmds.push(v(&["remote", "remove", orig])),
                Some(orig) => {
                    if name != orig && !name.is_empty() {
                        cmds.push(v(&["remote", "rename", orig, name]));
                    }
                    if url != r.orig_url && !url.is_empty() {
                        cmds.push(v(&["remote", "set-url", name, url]));
                    }
                    if push != r.orig_push {
                        if push.is_empty() {
                            cmds.push(v(&["config", "--local", "--unset", &format!("remote.{name}.pushurl")]));
                        } else {
                            cmds.push(v(&["remote", "set-url", "--push", name, push]));
                        }
                    }
                }
                None if !r.removed && !name.is_empty() && !url.is_empty() => {
                    cmds.push(v(&["remote", "add", name, url]));
                    if !push.is_empty() {
                        cmds.push(v(&["remote", "set-url", "--push", name, push]));
                    }
                }
                None => {}
            }
        }
        for (s, (ol, l, og, g)) in SETTINGS.iter().zip(&self.values) {
            for (scope, orig, new) in [("--local", ol, l), ("--global", og, g)] {
                let new = new.trim();
                if new == orig.trim() {
                    continue;
                }
                if new.is_empty() {
                    cmds.push(v(&["config", scope, "--unset", s.key]));
                } else {
                    cmds.push(v(&["config", scope, s.key, new]));
                }
            }
        }
        cmds
    }

    fn valid(&self) -> bool {
        let live: Vec<&RemoteRow> = self.remotes.iter().filter(|r| !r.removed).collect();
        let names_ok = live.iter().all(|r| {
            let n = r.name.trim();
            (r.orig.is_none() && n.is_empty() && r.url.trim().is_empty()) || (!n.is_empty() && !n.contains(char::is_whitespace) && !r.url.trim().is_empty())
        });
        let unique = live.iter().enumerate().all(|(i, a)| live.iter().skip(i + 1).all(|b| a.name.trim() != b.name.trim() || a.name.trim().is_empty()));
        names_ok && unique
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).strong().color(theme::TEXT()));
    ui.add_space(4.0);
}

fn edit(ui: &mut Ui, text: &mut String, width: f32, hint: &str) -> egui::Response {
    let te = egui::TextEdit::singleline(text).hint_text(RichText::new(hint).color(theme::TEXT_DIM()).italics());
    theme::edit_menu(ui.add_sized(vec2(width, 24.0), te))
}

fn choice(ui: &mut Ui, id: (&str, &str), value: &mut String, options: &[&str], inherited: &str, width: f32) {
    let shown = if value.is_empty() {
        if inherited.is_empty() { "not set".to_string() } else { format!("not set ({inherited} global)") }
    } else {
        value.clone()
    };
    egui::ComboBox::from_id_salt(id).width(width).selected_text(RichText::new(shown).color(if value.is_empty() { theme::TEXT_DIM() } else { theme::TEXT() })).show_ui(ui, |ui| {
        ui.selectable_value(value, String::new(), "not set");
        for o in options {
            ui.selectable_value(value, o.to_string(), *o);
        }
    });
}

impl App {
    pub fn open_git_config(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        self.git_config = Some(ConfigForm::load(&repo.path));
    }

    pub fn git_config_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.git_config.take() else { return };
        let mut keep = true;
        let mut apply = false;
        let title = match &self.repo {
            Some(r) => format!("{} – {}", crate::i18n::tr("Git Config"), r.name),
            None => "Git Config".into(),
        };
        let closed = form::show(ctx, "git_config", &title, 760.0, |ui, part| {
            if part == Part::Footer {
                let (ok, cancel) = form::ok_cancel(ui, tr("OK"), f.valid(), false);
                apply = ok;
                keep &= !(ok || cancel);
                return;
            }
            let full = ui.available_width();
            egui::ScrollArea::vertical().max_height((ui.ctx().content_rect().height() - 220.0).max(240.0) + form::grow(ui)).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(full - 8.0);
                // Remotes
                section(ui, "Remotes");
                let name_w = 110.0;
                let url_w = (full - name_w - 70.0) / 2.0;
                egui::Grid::new("remotes_grid").num_columns(4).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                    for t in ["Name", "URL", "Push URL (optional)"] {
                        ui.label(RichText::new(t).color(theme::TEXT_DIM()).size(12.0));
                    }
                    ui.end_row();
                    for r in f.remotes.iter_mut().filter(|r| !r.removed) {
                        edit(ui, &mut r.name, name_w, "origin");
                        edit(ui, &mut r.url, url_w, "https://… or git@…");
                        edit(ui, &mut r.push_url, url_w, "same as URL");
                        if theme::icon_button(ui, theme::Icon::Close, tr("Remove this remote")).clicked() {
                            r.removed = true;
                        }
                        ui.end_row();
                    }
                });
                if f.remotes.iter().all(|r| r.removed) {
                    ui.label(RichText::new(tr("This repository has no remotes.")).color(theme::TEXT_DIM()));
                }
                ui.add_space(2.0);
                if ui.link(tr("+ Add Remote")).clicked() {
                    let name = if f.remotes.iter().any(|r| !r.removed && r.name == "origin") { String::new() } else { "origin".into() };
                    f.remotes.push(RemoteRow { orig: None, orig_url: String::new(), orig_push: String::new(), name, url: String::new(), push_url: String::new(), removed: false });
                }
                ui.add_space(12.0);
                ui.separator();
                // Settings, this repository and global
                section(ui, "Settings");
                ui.label(RichText::new(tr("Empty means not set: a value for this repository wins over the global one.")).color(theme::TEXT_DIM()).size(12.0));
                ui.add_space(6.0);
                let label_w = 150.0;
                let col_w = (full - label_w - 40.0) / 2.0;
                egui::Grid::new("config_grid").num_columns(3).spacing(vec2(10.0, 6.0)).min_col_width(label_w).show(ui, |ui| {
                    ui.label("");
                    ui.label(RichText::new(tr("This repository (--local)")).color(theme::TEXT_DIM()).size(12.0));
                    ui.label(RichText::new(tr("All repositories (--global)")).color(theme::TEXT_DIM()).size(12.0));
                    ui.end_row();
                    for (s, (_, l, _, g)) in SETTINGS.iter().zip(f.values.iter_mut()) {
                        ui.label(RichText::new(s.label).color(theme::TEXT())).on_hover_text(s.hint);
                        match s.kind {
                            Kind::Text => {
                                let inherited = if g.is_empty() { String::new() } else { format!("{g}  (global)") };
                                edit(ui, l, col_w, &inherited).on_hover_text(s.hint);
                                edit(ui, g, col_w, "").on_hover_text(s.hint);
                            }
                            Kind::Choice(opts) => {
                                let inherited = g.clone();
                                choice(ui, (s.key, "local"), l, opts, &inherited, col_w - 8.0);
                                choice(ui, (s.key, "global"), g, opts, "", col_w - 8.0);
                            }
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
            });
        });
        if closed {
            keep = false;
        }
        if apply {
            let cmds = f.commands();
            if !cmds.is_empty() {
                self.run_job(Job::many("Git config", cmds));
            }
        }
        if keep {
            self.git_config = Some(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_for_changes() {
        let row = |o: Option<&str>, n: &str, u: &str| RemoteRow {
            orig: o.map(String::from),
            orig_url: if o.is_some() { "old-url".into() } else { String::new() },
            orig_push: String::new(),
            name: n.into(),
            url: u.into(),
            push_url: String::new(),
            removed: false,
        };
        let mut values: Vec<(String, String, String, String)> = SETTINGS.iter().map(|_| Default::default()).collect();
        values[0] = ("Me".into(), String::new(), String::new(), "Global Me".into());
        let f = ConfigForm { remotes: vec![row(Some("origin"), "upstream", "new-url"), row(None, "fork", "fork-url")], values };
        let c: Vec<String> = f.commands().iter().map(|c| c.join(" ")).collect();
        assert_eq!(
            c,
            vec![
                "remote rename origin upstream",
                "remote set-url upstream new-url",
                "remote add fork fork-url",
                "config --local --unset user.name",
                "config --global user.name Global Me",
            ]
        );
        assert!(f.valid());
    }
}
