//! File → Settings: a window with a list of pages on the left, like the IDE's Settings dialog.
//! Changes are made on a copy and take effect with OK or Apply.

use crate::ai;
use crate::i18n::tr;
use crate::app::App;
use crate::details_view::links_with;
use crate::settings::{ALL_REPOS, IssueRule, Settings, default_issue_rules};
use crate::theme::{self, Icon};
use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Window,
    Keymap,
    Credentials,
    Commit,
    MergeConflicts,
    IssueNavigation,
    Jira,
    Hosting,
    Log,
    Git,
    BranchPrefixes,
    AiAgents,
    ExternalTools,
    OpenFiles,
    FileManager,
}

impl Page {
    fn title(self) -> &'static str {
        tr(self.title_en())
    }
    fn title_en(self) -> &'static str {
        match self {
            Page::Window => "Window",
            Page::Keymap => "Keymap",
            Page::Credentials => "Credentials",
            Page::Commit => "Commit Messages",
            Page::MergeConflicts => "Merge Conflicts",
            Page::IssueNavigation => "Issue Navigation",
            Page::Jira => "JIRA",
            Page::Hosting => "Hosting",
            Page::Log => "Log",
            Page::Git => "Git",
            Page::BranchPrefixes => "Branch Prefixes",
            Page::AiAgents => "Agents",
            Page::ExternalTools => "Diff & Merge Tools",
            Page::OpenFiles => "Open Files",
            Page::FileManager => "File Manager",
        }
    }
    fn group(self) -> &'static str {
        tr(self.group_en())
    }
    fn group_en(self) -> &'static str {
        match self {
            Page::Window | Page::Keymap => "Appearance",
            Page::IssueNavigation | Page::Jira | Page::Hosting | Page::Log | Page::Git | Page::BranchPrefixes | Page::Credentials => "Version Control",
            // Everything the AI agent does: which agent, commit messages, resolving conflicts.
            Page::AiAgents | Page::Commit | Page::MergeConflicts => "AI",
            Page::ExternalTools | Page::OpenFiles | Page::FileManager => "Tools",
        }
    }
    /// Words the search box matches besides the title.
    fn keywords(self) -> &'static str {
        match self {
            Page::Window => "title bar minimize maximize close buttons system built-in decorations window colour color tint palette accent theme language english russian interface язык repository tabs strip вкладки update updates new version обновление",
            Page::Keymap => "keyboard shortcut shortcuts keys hotkey hotkeys keymap клавиши сочетания",
            Page::Credentials => "credentials password token ssh key keys public private helper sign in login authentication пароль ключ вход",
            Page::Commit => "ai assistant prompt commit message generation",
            Page::MergeConflicts => "conflict merge resolve ai prompt rules skill skills claude codex SKILL.md",
            Page::IssueNavigation => "jira issue tracker link pattern regex regular expression highlight",
            Page::Jira => "jira atlassian issue status token personal access api server cloud tracker",
            Page::Hosting => "hosting github gitlab bitbucket pull request merge request token ci pipeline checks host",
            Page::Log => "sort topological date first parent merges commits limit history",
            Page::Git => "executable path program refresh automatic",
            Page::BranchPrefixes => "branch prefix feature release hotfix bugfix support new branch name",
            Page::AiAgents => "ai agents claude codex gemini agent harness",
            Page::ExternalTools => "external diff merge tool compare beyond bcompare meld kdiff3 winmerge program",
            Page::OpenFiles => "editor open file line default application program command vscode code pycharm sublime gedit kate notepad",
            Page::FileManager => "nautilus files explorer open with folder menu integration",
        }
    }
}

const PAGES: [Page; 15] = [
    Page::Window,
    Page::Keymap,
    Page::IssueNavigation,
    Page::Jira,
    Page::Hosting,
    Page::BranchPrefixes,
    Page::Log,
    Page::Git,
    Page::Credentials,
    Page::AiAgents,
    Page::Commit,
    Page::MergeConflicts,
    Page::ExternalTools,
    Page::OpenFiles,
    Page::FileManager,
];

/// What the Credentials page shows (read when the page is first opened).
pub struct CredState {
    pub helper: String,
    pub keys: Vec<crate::credentials::SshKey>,
    pub new_name: String,
    pub new_comment: String,
    /// The last result: (text, it is an error).
    pub note: Option<(String, bool)>,
}

/// The copy being edited while the window is open.
pub struct SettingsDraft {
    pub s: Settings,
    /// Issue patterns of the open project.
    pub issues: Vec<IssueRule>,
    /// The open repository's own patterns (None: it uses the shared ones), and whether its tab
    /// is shown.
    pub own_issues: Option<Vec<IssueRule>>,
    pub issues_for_repo: bool,
    pub page: Page,
    pub search: String,
    pub sel_rule: Option<usize>,
    pub sample: String,
    pub git_check: Option<Result<String, String>>,
    /// JIRA: the answer of "Test Connection", and the check still running.
    pub jira_check: Option<Result<String, String>>,
    pub jira_wait: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    /// Hosting: the same for the host in that row.
    /// A new JIRA token typed and not applied yet.
    pub jira_entered: String,
    pub host_check: Option<(usize, Result<String, String>)>,
    pub host_wait: Option<(usize, std::sync::mpsc::Receiver<Result<String, String>>)>,
    /// Folder typed into "Additional folders".
    pub new_skill_dir: String,
    /// Skills found with the settings being edited, and the settings they were found with.
    pub skills: Vec<crate::skills::Skill>,
    pub skills_key: Option<(bool, Vec<String>)>,
    /// Diff & Merge Tools: the selected tool and file-type row, the tool being edited.
    pub tool_sel: Option<usize>,
    pub assoc_sel: Option<usize>,
    pub tool_edit: Option<crate::ext_tools_view::ToolEdit>,
    /// Open Files: "a command" chosen (kept while its text is being typed).
    pub editor_custom: bool,
}

impl App {
    /// Start a new copy of the program on the same repository and close this one.
    pub fn restart(&mut self, ctx: &egui::Context) {
        // Inside an AppImage start the AppImage itself (its mount goes away with this process).
        let Some(exe) = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok()) else { return };
        let mut c = std::process::Command::new(exe);
        if let Some(r) = &self.repo {
            c.arg(&r.path);
        }
        if c.spawn().is_ok() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn repo_key(&self) -> Option<String> {
        self.repo.as_ref().map(|r| r.path.display().to_string())
    }

    pub fn open_settings(&mut self, page: Option<Page>) {
        // The Credentials page reads the keys and the helper anew each time.
        self.cred = None;
        let key = self.repo_key();
        let issues = self.settings.shared_issue_rules();
        let own_issues = key.as_deref().and_then(|k| self.settings.issues.get(k).cloned());
        let sample = self
            .repo
            .as_ref()
            .and_then(|r| {
                r.commits
                    .iter()
                    .take(500)
                    .find(|c| !crate::details_view::issue_links(&c.subject).is_empty())
                    .or(r.commits.first())
                    .map(|c| c.subject.clone())
            })
            .unwrap_or_else(|| "APP-1308 Fix the waybill number".into());
        let last = self.last_settings_page;
        self.settings_draft = Some(SettingsDraft {
            s: self.settings.clone(),
            issues,
            issues_for_repo: own_issues.is_some(),
            own_issues,
            page: page.or(last).unwrap_or(Page::IssueNavigation),
            search: String::new(),
            sel_rule: None,
            sample,
            git_check: None,
            jira_check: None,
            jira_wait: None,
            jira_entered: String::new(),
            host_check: None,
            host_wait: None,
            new_skill_dir: String::new(),
            skills: vec![],
            skills_key: None,
            tool_sel: None,
            assoc_sel: None,
            tool_edit: None,
            editor_custom: !self.settings.editor_cmd.trim().is_empty(),
        });
        self.show_settings = true;
    }

    fn draft_dirty(&self) -> bool {
        let Some(d) = &self.settings_draft else { return false };
        let own_now = self.repo_key().and_then(|k| self.settings.issues.get(&k).cloned());
        d.s != self.settings || d.issues != self.settings.shared_issue_rules() || (self.repo.is_some() && d.own_issues != own_now)
    }

    /// Make the edited settings the real ones and use them right away.
    fn apply_settings(&mut self) {
        let Some(d) = &self.settings_draft else { return };
        let mut s = d.s.clone();
        if s.issues.contains_key(ALL_REPOS) || d.issues != default_issue_rules() {
            s.issues.insert(ALL_REPOS.to_string(), d.issues.clone());
        }
        if let Some(k) = self.repo_key() {
            match &d.own_issues {
                Some(own) => {
                    s.issues.insert(k, own.clone());
                }
                None => {
                    s.issues.remove(&k);
                }
            }
        }
        let log_changed = s.log_topo != self.settings.log_topo
            || s.log_first_parent != self.settings.log_first_parent
            || s.log_no_merges != self.settings.log_no_merges
            || s.log_limit() != self.settings.log_limit()
            || s.graph_fixed != self.settings.graph_fixed;
        crate::graph::set_fixed_lanes(s.graph_fixed);
        crate::i18n::set(crate::i18n::Lang::from_code(&s.language));
        crate::encoding::set_fallback(s.fallback_enc);
        let jira_entered = std::mem::take(&mut self.settings_draft.as_mut().unwrap().jira_entered);
        let todo = crate::secrets_view::token_changes(&self.settings, &mut s, jira_entered.trim(), crate::secrets::enabled());
        self.run_secrets(todo);
        self.settings = s.clone();
        if let Some(dr) = self.settings_draft.as_mut() {
            dr.s = s;
        }
        crate::git::set_program(&self.settings.git_path);
        self.skills_for = None;
        // Read the issues again with the new JIRA settings (also after a token was fixed).
        self.jira.forget();
        self.hosting.clear();
        if let Some(k) = self.repo_key() {
            crate::details_view::set_issue_rules(&self.settings.issue_rules(&k));
        }
        if let Some(r) = self.repo.as_mut() {
            if log_changed {
                r.topo = self.settings.log_topo;
                r.first_parent = self.settings.log_first_parent;
                r.no_merges = self.settings.log_no_merges;
                r.log_limit = self.settings.log_limit();
                App::reload_log(r);
            }
        }
        if self.screenshot.is_none() {
            self.settings.save();
        }
    }

    pub fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        if self.settings_draft.is_none() {
            self.open_settings(None);
        }
        let screen = ctx.content_rect();
        let base = vec2((screen.width() - 80.0).clamp(560.0, 940.0), (screen.height() - 80.0).clamp(420.0, 660.0));
        // Moved and resized like the other windows (crate::form).
        const WKEY: &str = "settings_window";
        let size = (base + crate::form::delta(ctx, WKEY)).max(vec2(560.0, 420.0)).min(screen.size() - vec2(8.0, 8.0));
        let title = match &self.repo {
            Some(r) => format!("{} – {}", tr("Settings"), r.name),
            None => tr("Settings").to_string(),
        };
        let mut action: Option<&str> = None;
        let dirty = self.draft_dirty();
        let m = egui::Modal::new(egui::Id::unique("settings_window"))
            .area(crate::form::area(ctx, "settings_window", WKEY))
            .frame(egui::Frame::new().fill(theme::BG()).stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(8))
            .show(ctx, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                let full = Rect::from_min_size(ui.cursor().min, size);
                // Title
                let title_r = Rect::from_min_size(full.min, vec2(size.x, 36.0));
                crate::form::title_drag(ui, title_r.with_max_x(title_r.right() - 36.0), WKEY);
                ui.painter().rect_filled(title_r, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, theme::BG_BAR());
                ui.painter().text(title_r.center(), egui::Align2::CENTER_CENTER, &title, FontId::proportional(14.0), theme::TEXT());
                let close_r = Rect::from_center_size(pos2(title_r.right() - 20.0, title_r.center().y), vec2(24.0, 24.0));
                let cr = ui.interact(close_r, egui::Id::unique("settings_x"), Sense::click());
                if cr.hovered() {
                    ui.painter().rect_filled(close_r, 4.0, theme::HOVER());
                }
                theme::paint_icon(ui.painter(), close_r.shrink(6.0), Icon::Close, theme::TEXT_MID());
                if cr.clicked() {
                    action = Some("cancel");
                }
                // Bottom buttons
                let foot = Rect::from_min_max(pos2(full.left(), full.bottom() - 50.0), full.max);
                theme::hline(ui, foot.top() + 0.5, full.left(), full.right());
                let left = Rect::from_min_max(pos2(full.left(), title_r.bottom()), pos2(full.left() + 240.0, foot.top()));
                ui.painter().rect_filled(left, 0.0, theme::BG_BAR());
                theme::vline(ui, left.right() - 0.5, left.top(), left.bottom());
                let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(left.shrink2(vec2(8.0, 8.0))));
                lu.set_clip_rect(left.shrink2(vec2(0.0, 2.0)));
                // Scrolls when the window is made small.
                egui::ScrollArea::vertical().id_salt("settings_nav_scroll").auto_shrink([false, false]).show(&mut lu, |ui| self.settings_nav(ui));
                let right = Rect::from_min_max(pos2(left.right() + 18.0, title_r.bottom() + 12.0), pos2(full.right() - 18.0, foot.top() - 8.0));
                let mut ru = ui.new_child(egui::UiBuilder::new().max_rect(right));
                ru.set_clip_rect(right.expand(2.0));
                self.settings_page(&mut ru);
                let mut fu = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(foot.shrink2(vec2(16.0, 10.0)))
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                if fu.add_enabled(dirty, egui::Button::new(tr("  Apply  "))).clicked() {
                    action = Some("apply");
                }
                if fu.button(tr("  Cancel  ")).clicked() {
                    action = Some("cancel");
                }
                if fu
                    .add(egui::Button::new(RichText::new(tr("     OK     ")).color(Color32::WHITE)).fill(theme::ACCENT()))
                    .clicked()
                {
                    action = Some("ok");
                }
                ui.allocate_rect(full, Sense::hover());
                crate::form::resize_grip(ui, full, WKEY, vec2(560.0, 420.0) - base);
            });
        crate::form::keep_on_screen(ctx, WKEY, m.response.rect);
        let editing = self.settings_draft.as_ref().is_some_and(|d| d.tool_edit.is_some());
        self.ext_tool_window(ctx);
        if m.should_close() && action.is_none() && !editing {
            action = Some("cancel");
        }
        if std::mem::take(&mut self.restart_requested) {
            self.apply_settings();
            self.show_settings = false;
            self.restart(ctx);
        }
        match action {
            Some("apply") => self.apply_settings(),
            Some("ok") => {
                self.apply_settings();
                self.show_settings = false;
            }
            Some("cancel") => {
                self.show_settings = false;
            }
            _ => {}
        }
        if !self.show_settings {
            // Keep the last page for next time, drop the edits.
            if let Some(d) = self.settings_draft.as_mut() {
                d.s = self.settings.clone();
            }
            let page = self.settings_draft.as_ref().map(|d| d.page);
            self.settings_draft = None;
            self.last_settings_page = page;
        }
    }

    fn settings_nav(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        crate::details_view::search_box(ui, &mut d.search, tr("Search settings"), ui.available_width(), None);
        ui.add_space(8.0);
        let q = d.search.trim().to_lowercase();
        let matches = |p: Page| {
            q.is_empty()
                || p.title().to_lowercase().contains(&q)
                || p.group().to_lowercase().contains(&q)
                || p.title_en().to_lowercase().contains(&q)
                || p.keywords().contains(&q)
        };
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut last_group = "";
        let w = ui.available_width();
        for p in PAGES {
            if !matches(p) {
                continue;
            }
            if p.group() != last_group {
                last_group = p.group();
                let (r, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
                theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 8.0, r.center().y), vec2(11.0, 11.0)), Icon::ChevronDown, theme::TEXT_MID());
                ui.painter().text(pos2(r.left() + 20.0, r.center().y), egui::Align2::LEFT_CENTER, last_group, FontId::proportional(13.0), theme::TEXT());
            }
            let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
            let sel = d.page == p;
            if sel {
                ui.painter().rect_filled(r, 4.0, theme::SELECT());
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 4.0, theme::HOVER());
            }
            ui.painter().text(pos2(r.left() + 34.0, r.center().y), egui::Align2::LEFT_CENTER, p.title(), FontId::proportional(13.0), theme::TEXT());
            if resp.clicked() {
                d.page = p;
            }
        }
    }

    fn settings_page(&mut self, ui: &mut Ui) {
        let Some(page) = self.settings_draft.as_ref().map(|d| d.page) else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new(page.group()).size(14.0).color(theme::TEXT_MID()));
            ui.label(RichText::new("›").size(14.0).color(theme::TEXT_DIM()));
            ui.label(RichText::new(page.title()).size(14.0).color(theme::TEXT()));
        });
        ui.add_space(10.0);
        egui::ScrollArea::vertical().id_salt("settings_page").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width() - 6.0);
            match page {
                Page::Window => self.page_window(ui),
                Page::Keymap => self.page_keymap(ui),
                Page::Credentials => self.page_credentials(ui),
                Page::IssueNavigation => self.page_issues(ui),
                Page::Jira => self.page_jira(ui),
                Page::Hosting => self.page_hosting(ui),
                Page::Log => self.page_log(ui),
                Page::Commit => self.page_commit(ui),
                Page::MergeConflicts => self.page_conflicts(ui),
                Page::Git => self.page_git(ui),
                Page::BranchPrefixes => self.page_prefixes(ui),
                Page::AiAgents => self.page_agents(ui),
                Page::ExternalTools => self.page_ext_tools(ui),
                Page::OpenFiles => self.page_open_files(ui),
                Page::FileManager => self.page_file_manager(ui),
            }
        });
    }

    fn page_issues(&mut self, ui: &mut Ui) {
        let repo_name = self.repo.as_ref().map(|r| r.name.clone());
        let Some(d) = self.settings_draft.as_mut() else { return };
        let SettingsDraft { issues: shared, own_issues, issues_for_repo, sel_rule, sample, .. } = d;
        ui.label(
            RichText::new(tr("RefBraid looks for these patterns in commit messages and shows them as links to your issue tracker. Click a link to open it."))
                .color(theme::TEXT_MID()),
        );
        ui.add_space(8.0);
        // One set of patterns for all repositories; a repository can have its own instead.
        if let Some(name) = &repo_name {
            ui.horizontal(|ui| {
                if ui.selectable_value(issues_for_repo, false, tr("All Repositories")).clicked() {
                    *sel_rule = None;
                }
                if ui.selectable_value(issues_for_repo, true, format!("{}: {name}", tr("This Repository"))).clicked() {
                    *sel_rule = None;
                }
            });
            ui.add_space(6.0);
        } else {
            *issues_for_repo = false;
        }
        let editable;
        let issues: &mut Vec<IssueRule> = if *issues_for_repo {
            let mut on = own_issues.is_some();
            if ui.checkbox(&mut on, tr("Use its own patterns instead of the ones for all repositories")).changed() {
                *own_issues = if on { Some(shared.clone()) } else { None };
                *sel_rule = None;
            }
            if !on {
                ui.label(RichText::new(tr("It uses the patterns for all repositories:")).color(theme::TEXT_DIM()).size(12.5));
            }
            ui.add_space(6.0);
            editable = on;
            match own_issues.as_mut() {
                Some(v) => v,
                None => shared,
            }
        } else {
            ui.label(RichText::new(tr("Used by every repository that has no patterns of its own.")).color(theme::TEXT_DIM()).size(12.5));
            ui.add_space(6.0);
            editable = true;
            shared
        };
        let frame_w = ui.available_width();
        ui.scope(|ui| {
        if !editable {
            ui.disable();
        }
        egui::Frame::new().stroke(Stroke::new(1.0, theme::BORDER())).corner_radius(4).show(ui, |ui| {
            ui.set_width(frame_w - 2.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            // Toolbar: add, remove, reset
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                if theme::icon_button_sized(ui, Icon::Plus, tr("Add a pattern"), 24.0, false, true).clicked() {
                    issues.push(IssueRule { pattern: String::new(), link: String::new() });
                    *sel_rule = Some(issues.len() - 1);
                }
                let can_remove = sel_rule.map(|i| i < issues.len()).unwrap_or(false);
                if theme::icon_button_sized(ui, Icon::Minus, tr("Remove the selected pattern"), 24.0, false, can_remove).clicked() {
                    if let Some(i) = sel_rule.take() {
                        if i < issues.len() {
                            issues.remove(i);
                        }
                    }
                }
                if theme::icon_button_sized(ui, Icon::Undo, tr("Reset to the default pattern"), 24.0, false, true).clicked() {
                    *issues = default_issue_rules();
                    *sel_rule = None;
                }
            });
            let w = ui.available_width();
            let col1 = (w * 0.42).max(160.0);
            let (hr, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
            ui.painter().rect_filled(hr, 0.0, theme::BG_BAR());
            theme::hline(ui, hr.top() + 0.5, hr.left(), hr.right());
            theme::hline(ui, hr.bottom() - 0.5, hr.left(), hr.right());
            ui.painter().text(pos2(hr.left() + 8.0, hr.center().y), egui::Align2::LEFT_CENTER, tr("Issue"), FontId::proportional(13.0), theme::TEXT());
            ui.painter().text(pos2(hr.left() + col1 + 8.0, hr.center().y), egui::Align2::LEFT_CENTER, tr("Link"), FontId::proportional(13.0), theme::TEXT());
            theme::vline(ui, hr.left() + col1, hr.top(), hr.bottom());
            for i in 0..issues.len() {
                let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
                let sel = *sel_rule == Some(i);
                if sel {
                    ui.painter().rect_filled(r, 0.0, theme::SELECT());
                }
                if resp.clicked() {
                    *sel_rule = Some(i);
                }
                let bad = regex::Regex::new(&issues[i].pattern).err().filter(|_| !issues[i].pattern.is_empty());
                let pr = Rect::from_min_max(pos2(r.left() + 4.0, r.top() + 3.0), pos2(r.left() + col1 - 4.0, r.bottom() - 3.0));
                let lr = Rect::from_min_max(pos2(r.left() + col1 + 4.0, r.top() + 3.0), pos2(r.right() - 4.0, r.bottom() - 3.0));
                for (k, rect) in [(0, pr), (1, lr)] {
                    let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    let rule = &mut issues[i];
                    let (text, hint) = if k == 0 {
                        (&mut rule.pattern, r"[A-Z]+\-\d+")
                    } else {
                        (&mut rule.link, "https://jira.example.com/browse/$0")
                    };
                    let te = egui::TextEdit::singleline(text)
                        .id(egui::Id::unique(("issue_cell", i, k)))
                        .hint_text(RichText::new(hint).color(theme::TEXT_DIM()))
                        .font(if k == 0 { FontId::monospace(12.5) } else { FontId::proportional(13.0) })
                        .desired_width(rect.width());
                    let er = theme::edit_menu(cu.add(te));
                    if er.has_focus() || er.clicked() {
                        *sel_rule = Some(i);
                    }
                    if k == 0 {
                        if let Some(e) = &bad {
                            cu.painter().rect_stroke(er.rect, 3.0, Stroke::new(1.0, theme::RED()), egui::StrokeKind::Outside);
                            er.on_hover_text(format!("This pattern is not a valid regular expression:\n{e}"));
                        }
                    }
                }
                theme::vline(ui, r.left() + col1, r.top(), r.bottom());
            }
            if issues.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, tr("No patterns: issue keys are not highlighted"), FontId::proportional(13.0), theme::TEXT_DIM());
            }
            ui.add_space(6.0);
        });
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new(tr("Issue is a regular expression. In Link, $0 is the whole match and $1, $2… are the parts in brackets of the pattern. Leave Link empty to only highlight the text."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        ui.add_space(14.0);
        ui.label(RichText::new(tr("Check")).strong().color(theme::TEXT()));
        ui.add_space(4.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut (*sample))
                .hint_text(tr("Type a commit message to try the patterns"))
                .desired_width(f32::INFINITY),
        );
        theme::edit_menu(r);
        ui.add_space(6.0);
        let compiled: Vec<(regex::Regex, String)> = issues
            .iter()
            .filter(|r| !r.pattern.trim().is_empty())
            .filter_map(|r| regex::Regex::new(&r.pattern).ok().map(|re| (re, r.link.trim().to_string())))
            .collect();
        let found = links_with(&compiled, &(*sample));
        if found.is_empty() {
            ui.label(RichText::new(tr("Nothing found in this text.")).color(theme::TEXT_DIM()).size(12.5));
        }
        for (range, url) in found {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&(*sample)[range]).color(theme::LINK()).underline());
                ui.label(RichText::new("→").color(theme::TEXT_DIM()));
                if url.is_empty() {
                    ui.label(RichText::new(tr("highlighted, no link")).color(theme::TEXT_DIM()).size(12.5));
                } else {
                    ui.hyperlink_to(RichText::new(&url).size(12.5), &url);
                }
            });
        }
    }

    /// Settings → Credentials: the helper that remembers passwords, and this computer's SSH keys.
    fn page_credentials(&mut self, ui: &mut Ui) {
        use crate::credentials as cr;
        let dir = self.repo.as_ref().map(|r| r.path.clone()).unwrap_or_else(|| std::env::temp_dir());
        let ssh = cr::ssh_dir();
        if self.cred.is_none() {
            let email = crate::git::run(&dir, &["config", "user.email"]).stdout.trim().to_string();
            self.cred = Some(CredState {
                helper: cr::helper(&dir),
                keys: ssh.as_deref().map(cr::list_keys).unwrap_or_default(),
                new_name: ssh.as_deref().map(cr::free_key_name).unwrap_or_default(),
                new_comment: email,
                note: None,
            });
        }
        let Some(st) = self.cred.as_mut() else { return };
        let mut reload = false;
        ui.label(RichText::new(tr("Passwords and tokens (HTTPS)")).strong().color(theme::TEXT()));
        ui.add(egui::Label::new(RichText::new(tr("Git itself remembers what you type when a credential helper is set; without one it asks every time. This is git's setting for all your repositories.")).color(theme::TEXT_DIM()).size(12.0)).wrap());
        ui.add_space(4.0);
        let known = cr::HELPERS.iter().find(|h| h.0 == st.helper);
        let shown = match known {
            Some(h) => tr(h.1).to_string(),
            None => st.helper.clone(),
        };
        let mut set: Option<&'static str> = None;
        egui::ComboBox::from_id_salt("settings_cred_helper").selected_text(shown).width(260.0).show_ui(ui, |ui| {
            for h in cr::HELPERS.iter() {
                if ui.selectable_label(st.helper == h.0, tr(h.1)).on_hover_text(tr(h.2)).clicked() {
                    set = Some(h.0);
                }
            }
        });
        if let Some(h) = known {
            ui.label(RichText::new(tr(h.2)).color(theme::TEXT_DIM()).size(12.0));
        }
        if let Some(value) = set.filter(|v| *v != st.helper) {
            let args = cr::set_helper_args(value);
            let out = crate::git::run(&dir, &args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
            // Removing a helper that is not set is not a failure worth a word.
            st.note = (!out.ok && !value.is_empty()).then(|| (out.stderr.trim().to_string(), true));
            st.helper = cr::helper(&dir);
        }

        ui.add_space(18.0);
        ui.label(RichText::new(tr("SSH keys of this computer")).strong().color(theme::TEXT()));
        ui.add(egui::Label::new(RichText::new(tr("A server lets you in over SSH when your public key is added to your account there: copy it and paste it into the server's settings (SSH Keys). The private half never leaves this computer.")).color(theme::TEXT_DIM()).size(12.0)).wrap());
        ui.add_space(4.0);
        if st.keys.is_empty() {
            ui.label(RichText::new(tr("No keys yet — make one below.")).color(theme::TEXT_MID()));
        }
        for k in &st.keys {
            ui.horizontal(|ui| {
                let name = k.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                ui.add_sized([170.0, 22.0], egui::Label::new(RichText::new(name).color(theme::TEXT())).truncate().halign(egui::Align::Min)).on_hover_text(k.file.display().to_string());
                ui.add_sized([100.0, 22.0], egui::Label::new(RichText::new(&k.kind).color(theme::TEXT_MID()).size(12.0)).truncate().halign(egui::Align::Min));
                if ui.button(tr("Copy Public Key")).on_hover_text(&k.public).clicked() {
                    ui.ctx().copy_text(k.public.clone());
                    st.note = Some((crate::i18n::trf("The public key {} is copied: paste it into your account on the server.", &[&k.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()]), false));
                }
                ui.label(RichText::new(&k.comment).color(theme::TEXT_DIM()).size(12.0));
            });
        }
        ui.add_space(10.0);
        ui.label(RichText::new(tr("New key")).color(theme::TEXT()));
        ui.horizontal(|ui| {
            ui.label(tr("File:"));
            theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut st.new_name).desired_width(160.0)));
            ui.label(tr("Comment:"));
            theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut st.new_comment).hint_text("you@example.com").desired_width(200.0)));
            let ok = cr::good_key_name(st.new_name.trim()) && ssh.is_some();
            if ui.add_enabled(ok, egui::Button::new(tr("Generate"))).on_hover_text(tr("Makes an Ed25519 key without a passphrase in your .ssh folder (ssh-keygen)")).clicked() {
                if let Some(d) = &ssh {
                    match cr::generate_key(d, st.new_name.trim(), st.new_comment.trim()) {
                        Ok(k) => {
                            ui.ctx().copy_text(k.public.clone());
                            st.note = Some((tr("The key is made and its public half is copied: paste it into your account on the server.").to_string(), false));
                            reload = true;
                        }
                        Err(e) => st.note = Some((e, true)),
                    }
                }
            }
        });
        if let Some(d) = &ssh {
            ui.label(RichText::new(d.display().to_string()).color(theme::TEXT_DIM()).size(12.0));
        }
        if let Some((text, bad)) = &st.note {
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(text).color(if *bad { theme::RED() } else { theme::GREEN() })).wrap());
        }
        if reload {
            if let Some(d) = &ssh {
                st.keys = cr::list_keys(d);
                st.new_name = cr::free_key_name(d);
            }
        }
    }

    /// Settings → Keymap: every action with its shortcut; a click on the shortcut waits for
    /// the new keys.
    fn page_keymap(&mut self, ui: &mut Ui) {
        use crate::keymap;
        let Some(d) = self.settings_draft.as_mut() else { return };
        let cap_id = egui::Id::unique("keymap_capture");
        let mut capturing: Option<String> = ui.ctx().data(|x| x.get_temp(cap_id));
        ui.add(egui::Label::new(RichText::new(tr("Click a shortcut and press the new keys. Esc leaves it as it is, Backspace removes it. Every action is also in Help → Find Action.")).color(theme::TEXT_MID())).wrap());
        ui.add_space(8.0);
        if let Some(id) = capturing.clone() {
            // The next key press is the new shortcut; it goes nowhere else.
            let pressed = ui.input_mut(|i| {
                let found = i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                });
                i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
                found
            });
            if let Some((key, mods)) = pressed {
                let plain = !(mods.command || mods.ctrl || mods.alt || mods.shift || mods.mac_cmd);
                if !(plain && key == egui::Key::Escape) {
                    let shortcut = if plain && matches!(key, egui::Key::Backspace | egui::Key::Delete) { String::new() } else { keymap::format(mods, key) };
                    keymap::set(&mut d.s.keys, &id, &shortcut);
                }
                capturing = None;
            }
        }
        // A table: the action, its shortcut (a button that takes new keys) and the default one.
        const NAME_W: f32 = 300.0;
        const KEYS_W: f32 = 160.0;
        const ROW_H: f32 = 26.0;
        let head = |ui: &mut Ui, text: &str, w: f32| {
            ui.allocate_ui_with_layout(vec2(w, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_width(w);
                ui.label(RichText::new(text).size(12.0).color(theme::TEXT_DIM()));
            });
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.add_space(12.0);
            head(ui, tr("Action"), NAME_W - 12.0);
            head(ui, tr("Shortcut"), KEYS_W);
            head(ui, tr("Default"), 120.0);
        });
        theme::hline(ui, ui.cursor().top() + 1.0, ui.max_rect().left(), ui.max_rect().right());
        ui.add_space(3.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut group = "";
        let mut odd = false;
        for a in keymap::ACTIONS {
            if a.group != group {
                group = a.group;
                odd = false;
                ui.add_space(10.0);
                ui.label(RichText::new(tr(group)).strong().color(theme::TEXT()));
                ui.add_space(4.0);
            }
            let now = keymap::binding(&d.s.keys, a.id);
            let changed = now != a.default;
            // Every other row on a faint band, so the eye keeps the line across the columns.
            let row = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ROW_H));
            if odd {
                ui.painter().rect_filled(row, 3.0, theme::HOVER());
            }
            odd = !odd;
            ui.allocate_ui_with_layout(vec2(row.width(), ROW_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_height(ROW_H);
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.add_space(12.0);
                ui.allocate_ui_with_layout(vec2(NAME_W - 12.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_width(NAME_W - 12.0);
                    ui.add(egui::Label::new(RichText::new(tr(a.name).trim_end_matches('…')).color(theme::TEXT())).truncate());
                });
                let waiting = capturing.as_deref() == Some(a.id);
                let text = if waiting { tr("Press the keys…").to_string() } else if now.is_empty() { "—".to_string() } else { keymap::shown(&now).into_owned() };
                let b = ui.add_sized([KEYS_W, 22.0], egui::Button::new(RichText::new(text).color(if waiting { theme::LINK() } else if changed { theme::STAR() } else { theme::TEXT() })).selected(waiting));
                if b.on_hover_text(tr("Click to change the shortcut")).clicked() {
                    capturing = if waiting { None } else { Some(a.id.to_string()) };
                }
                // The default one is named only where the shortcut differs from it.
                if changed {
                    let default = if a.default.is_empty() { "—".to_string() } else { keymap::shown(a.default).into_owned() };
                    ui.label(RichText::new(default).color(theme::TEXT_DIM()).size(12.0));
                    if ui.link(tr("Reset")).on_hover_text(tr("Put the default shortcut back")).clicked() {
                        keymap::set(&mut d.s.keys, a.id, a.default);
                    }
                }
                if let Some(other) = keymap::taken_by(&d.s.keys, a.id, &now) {
                    ui.label(RichText::new(crate::i18n::trf("also: {}", &[&tr(other.name).trim_end_matches('…')])).color(theme::RED()).size(12.0));
                }
            });
        }
        ui.add_space(10.0);
        if !d.s.keys.is_empty() && ui.link(tr("Reset all shortcuts to the defaults")).clicked() {
            d.s.keys.clear();
        }
        ui.ctx().data_mut(|x| match &capturing {
            Some(id) => {
                x.insert_temp(cap_id, id.clone());
            }
            None => {
                x.remove_temp::<String>(cap_id);
            }
        });
    }

    fn page_window(&mut self, ui: &mut Ui) {
        let running_builtin = self.started_builtin_title_bar;
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.label(RichText::new(tr("Interface language")).strong().color(theme::TEXT()));
        ui.label(
            RichText::new(tr("Menus, buttons, windows and the changelog are shown in this language."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        ui.add_space(4.0);
        let now = crate::i18n::Lang::from_code(&d.s.language);
        egui::ComboBox::from_id_salt("settings_language").selected_text(now.name()).width(180.0).show_ui(ui, |ui| {
            for l in crate::i18n::ALL {
                if ui.selectable_label(l == now, l.name()).clicked() {
                    d.s.language = l.code().to_string();
                }
            }
        });
        ui.add_space(18.0);
        ui.label(RichText::new(tr("Theme")).strong().color(theme::TEXT()));
        ui.add_space(4.0);
        const THEMES: [&str; 3] = ["Dark", "Light", "As the system"];
        egui::ComboBox::from_id_salt("settings_theme").selected_text(tr(THEMES[d.s.theme.min(2) as usize])).width(180.0).show_ui(ui, |ui| {
            for (k, name) in THEMES.iter().enumerate() {
                if ui.selectable_label(d.s.theme as usize == k, tr(name)).clicked() {
                    d.s.theme = k as u8;
                }
            }
        });
        ui.add_space(18.0);
        ui.label(RichText::new(tr("Repository tabs")).strong().color(theme::TEXT()));
        ui.label(RichText::new(tr("The strip under the title bar with a tab for every open repository.")).color(theme::TEXT_DIM()).size(12.0));
        ui.add_space(4.0);
        const TABS: [(&str, &str); 3] = [
            ("Always", "Also with a single repository open"),
            ("When several repositories are open", "With one repository open the strip is hidden and its room goes to the window"),
            ("Never", "Repositories are still switched with Ctrl+Tab and from Favorites"),
        ];
        egui::ComboBox::from_id_salt("settings_repo_tabs").selected_text(tr(TABS[d.s.repo_tabs.min(2) as usize].0)).width(280.0).show_ui(ui, |ui| {
            for (k, (name, tip)) in TABS.iter().enumerate() {
                if ui.selectable_label(d.s.repo_tabs as usize == k, tr(name)).on_hover_text(tr(tip)).clicked() {
                    d.s.repo_tabs = k as u8;
                }
            }
        });
        ui.add_space(18.0);
        ui.label(RichText::new(tr("Diff")).strong().color(theme::TEXT()));
        ui.add_space(4.0);
        let mut on = !d.s.syntax_off;
        if ui.checkbox(&mut on, tr("Syntax highlighting")).on_hover_text(tr("Colour keywords, strings, comments and numbers of code in diffs and in Annotate")).changed() {
            d.s.syntax_off = !on;
        }
        ui.add_space(6.0);
        ui.label(tr("Files that are not UTF-8 are read as:"));
        egui::ComboBox::from_id_salt("settings_encoding").selected_text(tr(crate::encoding::NAMES[(d.s.fallback_enc as usize).min(crate::encoding::NAMES.len() - 1)])).width(240.0).show_ui(ui, |ui| {
            for (k, name) in crate::encoding::NAMES.iter().enumerate() {
                if ui.selectable_label(d.s.fallback_enc as usize == k, tr(name)).clicked() {
                    d.s.fallback_enc = k as u8;
                }
            }
        });
        ui.label(RichText::new(tr("Only the lines that are not valid UTF-8 are read this way; open diffs change after they are opened again.")).color(theme::TEXT_DIM()).size(12.0));
        ui.add_space(18.0);
        tint_palette(ui, &mut d.s);
        ui.add_space(18.0);
        ui.label(RichText::new(tr("Window title bar")).strong().color(theme::TEXT()));
        ui.label(
            RichText::new(tr("The bar above the window with the minimise, maximise and close buttons."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        ui.add_space(6.0);
        if !cfg!(target_os = "linux") {
            ui.label(RichText::new(tr("On this system the window always uses the system title bar.")).color(theme::TEXT_MID()));
            return;
        }
        ui.radio_value(&mut d.s.builtin_title_bar, false, tr("System"))
            .on_hover_text(tr("Drawn by the desktop, the same as in other applications."));
        ui.radio_value(&mut d.s.builtin_title_bar, true, tr("Built-in"))
            .on_hover_text(tr("Drawn by RefBraid itself. Text can be sharper on screens with fractional scaling."));
        if d.s.builtin_title_bar != running_builtin {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Takes effect after RefBraid restarts.")).color(theme::UNVERSIONED()));
                if ui.button(tr("Apply and Restart")).clicked() {
                    self.restart_requested = true;
                }
            });
        }
        ui.add_space(14.0);
        ui.label(RichText::new(tr("Updates")).strong().color(theme::TEXT()));
        ui.add_space(4.0);
        if let Some(d) = self.settings_draft.as_mut() {
            let mut look = !d.s.no_update_check;
            if ui
                .checkbox(&mut look, tr("Look for a newer version of RefBraid when it starts"))
                .on_hover_text(tr("Once a day the program asks GitHub for the latest release (nothing about you or your repositories is sent) and tells when there is a newer one. Help → Check for Updates looks at any time."))
                .changed()
            {
                d.s.no_update_check = !look;
            }
        }
    }

    fn page_log(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        let s = &mut d.s;
        ui.label(RichText::new(tr("Used for the log now and for every repository you open.")).color(theme::TEXT_MID()));
        ui.add_space(10.0);
        ui.label(RichText::new(tr("Sort")).strong().color(theme::TEXT()));
        ui.radio_value(&mut s.log_topo, false, tr("By commit date")).on_hover_text(tr("Newest commits first, whatever branch they are on."));
        ui.radio_value(&mut s.log_topo, true, tr("Topologically")).on_hover_text(tr("Keep the commits of each branch together."));
        ui.add_space(10.0);
        ui.label(RichText::new(tr("Options")).strong().color(theme::TEXT()));
        ui.checkbox(&mut s.log_first_parent, tr("First parent only"))
            .on_hover_text(tr("Follow only the first parent of merge commits: commits that came in from merged branches are hidden."));
        ui.checkbox(&mut s.log_no_merges, tr("Hide merge commits"));
        ui.add_space(10.0);
        ui.label(RichText::new(tr("Graph")).strong().color(theme::TEXT()));
        ui.horizontal_top(|ui| {
            for (fixed, title, about) in [
                (false, "Compact", "Lanes close up when a branch ends; lines slant from row to row. Very long lines are cut, with an arrow at each end."),
                (true, "Fixed lanes", "Every line keeps its own column from start to end; lines bend only where they join."),
            ] {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    ui.radio_value(&mut s.graph_fixed, fixed, tr(title));
                    let r = crate::log_view::graph_preview(ui, fixed, s.graph_fixed == fixed);
                    if r.clicked() {
                        s.graph_fixed = fixed;
                    }
                    ui.label(RichText::new(tr(about)).color(theme::TEXT_DIM()).size(12.0));
                });
                ui.add_space(16.0);
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Commits to load:")).color(theme::TEXT()));
            let mut n = s.log_limit();
            if ui.add(egui::DragValue::new(&mut n).range(100..=200_000).speed(50)).changed() {
                s.log_limit = n;
            }
            ui.label(RichText::new(tr("newest first; more takes longer in big repositories")).color(theme::TEXT_DIM()).size(12.0));
        });
    }

    fn page_commit(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.label(RichText::new(tr("AI Assistant")).strong().color(theme::TEXT()));
        ui.label(
            RichText::new(tr("How the AI agent writes commit messages (the spiral button next to Amend)."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        ui.add_space(6.0);
        crate::ai_view::prompt_editor(ui, &mut d.s.ai_prompt, None);
        if ui.small_button("Reset to default").clicked() {
            d.s.ai_prompt = ai::DEFAULT_PROMPT.to_string();
        }
    }

    fn page_conflicts(&mut self, ui: &mut Ui) {
        let repo = self.repo.as_ref().map(|r| r.path.clone());
        let Some(d) = self.settings_draft.as_mut() else { return };
        let key = (d.s.no_personal_skills, d.s.skill_dirs.clone());
        if d.skills_key.as_ref() != Some(&key) {
            d.skills = crate::skills::find(repo.as_deref(), !d.s.no_personal_skills, &d.s.skill_dirs);
            d.skills_key = Some(key);
        }
        ui.label(RichText::new(tr("AI rules for resolving conflicts")).strong().color(theme::TEXT()));
        ui.label(
            RichText::new(tr("Sent to the AI agent with each conflicted file when \"Rules from Settings\" is chosen. The agent's answer fills the merge editor; nothing is saved until you press Apply."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        ui.add_space(4.0);
        if d.s.conflict_prompt.trim().is_empty() {
            d.s.conflict_prompt = crate::merge::DEFAULT_CONFLICT_PROMPT.to_string();
        }
        egui::ScrollArea::vertical().id_salt("conflict_prompt_scroll").max_height(150.0).show(ui, |ui| {
            let r = ui.add(
                egui::TextEdit::multiline(&mut d.s.conflict_prompt)
                    .margin(egui::Margin::symmetric(8, 6))
                    .font(FontId::proportional(13.0))
                    .desired_rows(5)
                    .desired_width(f32::INFINITY),
            );
            theme::edit_menu(r);
        });
        if ui.small_button("Reset to default").clicked() {
            d.s.conflict_prompt = crate::merge::DEFAULT_CONFLICT_PROMPT.to_string();
        }
        ui.add_space(12.0);
        ui.label(RichText::new(tr("Skills")).strong().color(theme::TEXT()));
        ui.label(
            RichText::new(tr("Instead of the rules, the agent can follow a skill: a folder with a SKILL.md file, for example .claude/skills/conflict_resolver/SKILL.md. RefBraid looks for them:"))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        ui.add_space(2.0);
        ui.label(RichText::new(format!("• in the repository: {}", crate::skills::PROJECT_DIRS.join(", "))).color(theme::TEXT_MID()).size(12.0));
        let mut personal = !d.s.no_personal_skills;
        if ui
            .checkbox(&mut personal, RichText::new(tr("• in my folders: ~/.claude/skills, ~/.agents/skills, ~/.codex/skills, … and Claude Code plugins")).size(12.0))
            .changed()
        {
            d.s.no_personal_skills = !personal;
        }
        ui.label(RichText::new(tr("• in these folders (a skills folder or one SKILL.md):")).color(theme::TEXT_MID()).size(12.0));
        let mut remove = None;
        for (k, dir) in d.s.skill_dirs.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(RichText::new(dir).color(theme::TEXT()).size(12.5));
                if theme::icon_button_sized(ui, Icon::Close, tr("Remove"), 18.0, false, true).clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            d.s.skill_dirs.remove(k);
        }
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let w = (ui.available_width() - 150.0).max(160.0);
            let r = ui.add(egui::TextEdit::singleline(&mut d.new_skill_dir).hint_text(tr("~/work/shared-skills")).desired_width(w));
            theme::edit_menu(r);
            if ui.button(tr("Browse…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("Skills Folder").pick_folder() {
                    d.new_skill_dir = p.display().to_string();
                }
            }
            if ui.add_enabled(!d.new_skill_dir.trim().is_empty(), egui::Button::new(tr("Add"))).clicked() {
                let v = d.new_skill_dir.trim().to_string();
                if !d.s.skill_dirs.contains(&v) {
                    d.s.skill_dirs.push(v);
                }
                d.new_skill_dir.clear();
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Found ({})", d.skills.len())).color(theme::TEXT()));
            if ui.small_button("Search again").clicked() {
                d.skills_key = None;
            }
        });
        egui::Frame::new().stroke(Stroke::new(1.0, theme::BORDER())).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt("skills_found").max_height(130.0).show(ui, |ui| {
                if d.skills.is_empty() {
                    ui.label(RichText::new(tr("No skills found.")).color(theme::TEXT_DIM()).size(12.0));
                }
                for s in &d.skills {
                    ui.horizontal(|ui| {
                        if s.about_conflicts {
                            ui.label(RichText::new("★").color(theme::STAR()).size(12.0)).on_hover_text(tr("About conflicts or merging"));
                        }
                        ui.label(RichText::new(&s.name).color(theme::TEXT()).size(12.5)).on_hover_text(s.file.display().to_string());
                        ui.label(RichText::new(&s.source).color(theme::TEXT_DIM()).size(11.5));
                        if !s.description.is_empty() {
                            let mut t: String = s.description.chars().take(90).collect();
                            if s.description.chars().count() > 90 {
                                t.push('…');
                            }
                            ui.label(RichText::new(t).color(theme::TEXT_MID()).size(11.5));
                        }
                    });
                }
            });
        });
        let Some(repo) = repo else { return };
        let rk = repo.display().to_string();
        ui.add_space(12.0);
        ui.label(RichText::new(tr("This repository")).strong().color(theme::TEXT()));
        let cur = d.s.conflict_method.get(&rk).and_then(|k| crate::merge::Method::from_key(k));
        let label = |m: &Option<crate::merge::Method>| match m {
            None => "Not chosen yet (a conflict skill of the repository is picked automatically, otherwise the rules)".to_string(),
            Some(crate::merge::Method::Prompt) => "Rules from Settings".to_string(),
            Some(crate::merge::Method::Skill(p)) => format!(
                "Skill: {}",
                d.skills.iter().find(|s| &s.file == p).map(|s| s.name.clone()).unwrap_or_else(|| p.display().to_string())
            ),
        };
        let mut pick: Option<Option<crate::merge::Method>> = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Resolve conflicts with:")).color(theme::TEXT_MID()));
            egui::ComboBox::from_id_salt("settings_conflict_method").width(360.0).selected_text(label(&cur)).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_none(), "Choose automatically").clicked() {
                    pick = Some(None);
                }
                if ui.selectable_label(cur == Some(crate::merge::Method::Prompt), "Rules from Settings").clicked() {
                    pick = Some(Some(crate::merge::Method::Prompt));
                }
                for s in &d.skills {
                    let m = crate::merge::Method::Skill(s.file.clone());
                    if ui.selectable_label(cur.as_ref() == Some(&m), format!("{}   ·  {}", s.name, s.source)).clicked() {
                        pick = Some(Some(m));
                    }
                }
            });
        });
        ui.label(
            RichText::new(tr("Remembered each time you resolve conflicts with AI here, and chosen automatically next time."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        match pick {
            Some(None) => {
                d.s.conflict_method.remove(&rk);
            }
            Some(Some(m)) => {
                d.s.conflict_method.insert(rk, m.key());
            }
            None => {}
        }
    }

    fn page_prefixes(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.label(RichText::new(tr("Prefixes for new branches")).strong().color(theme::TEXT()));
        ui.label(
            RichText::new(tr("Offered in front of the name in Create New Branch; the description is shown under it. The selected circle is the prefix chosen first."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
        ui.add_space(8.0);
        let mut list = d.s.branch_prefixes();
        let mut default = d.s.default_branch_prefix();
        let mut remove: Option<usize> = None;
        let w = ui.available_width();
        egui::Grid::new("prefix_grid").num_columns(4).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            ui.label("");
            ui.label(RichText::new(tr("Prefix")).color(theme::TEXT_DIM()).size(12.0));
            ui.label(RichText::new(tr("Description")).color(theme::TEXT_DIM()).size(12.0));
            ui.end_row();
            ui.radio_value(&mut default, String::new(), "").on_hover_text(tr("Start with no prefix"));
            ui.label(RichText::new(tr("No prefix")).color(theme::TEXT_MID()));
            ui.label("");
            ui.end_row();
            for (i, p) in list.iter_mut().enumerate() {
                let was_default = !p.name.is_empty() && default == p.name;
                ui.radio_value(&mut default, p.name.clone(), "").on_hover_text(tr("Choose this prefix first"));
                let old = p.name.clone();
                theme::edit_menu(ui.add_sized(vec2(130.0, 24.0), egui::TextEdit::singleline(&mut p.name).hint_text(tr("feature/"))));
                if was_default && p.name != old {
                    default = p.name.clone();
                }
                theme::edit_menu(ui.add_sized(vec2((w - 220.0).max(200.0), 24.0), egui::TextEdit::singleline(&mut p.description).hint_text(tr("What these branches are for"))));
                if theme::icon_button(ui, Icon::Close, tr("Remove this prefix")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            if list.get(i).is_some_and(|p| p.name == default) {
                default = String::new();
            }
            list.remove(i);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.link(tr("+ Add Prefix")).clicked() {
                list.push(crate::settings::BranchPrefix { name: String::new(), description: String::new() });
            }
            ui.add_space(16.0);
            if ui.small_button("Reset to defaults").clicked() {
                list = crate::settings::default_branch_prefixes();
                default = crate::settings::DEFAULT_BRANCH_PREFIX.to_string();
            }
        });
        // Only remember a list once it differs from the built-in one.
        let defaults = crate::settings::default_branch_prefixes();
        d.s.branch_prefixes = if list == defaults && d.s.branch_prefixes.is_none() { None } else { Some(list) };
        if d.s.branch_prefixes.as_ref() == Some(&defaults) {
            d.s.branch_prefixes = None;
        }
        d.s.branch_prefix_default = if default == crate::settings::DEFAULT_BRANCH_PREFIX { None } else { Some(default) };
    }

    fn page_git(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.label(RichText::new(tr("Path to Git executable")).strong().color(theme::TEXT()));
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 170.0).max(160.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut d.s.git_path)
                    .hint_text(tr("git (found automatically)"))
                    .desired_width(w),
            );
            theme::edit_menu(r);
            if ui.button(tr("Browse…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().set_title("Git Executable").pick_file() {
                    d.s.git_path = p.display().to_string();
                    d.git_check = None;
                }
            }
            if ui.button(tr("Test")).clicked() {
                d.git_check = Some(crate::git::version_of(&d.s.git_path));
            }
        });
        match &d.git_check {
            Some(Ok(v)) => {
                ui.label(RichText::new(format!("✓ {v}")).color(theme::GREEN()));
            }
            Some(Err(e)) => {
                ui.label(RichText::new(format!("Could not run it: {e}")).color(theme::RED()));
            }
            None => {}
        }
        ui.add_space(12.0);
        let mut auto = !d.s.no_auto_refresh;
        if ui
            .checkbox(&mut auto, tr("Refresh automatically when the repository changes"))
            .on_hover_text(tr("Reload branches, the log and local changes when something changes them outside RefBraid (a terminal, another program)."))
            .changed()
        {
            d.s.no_auto_refresh = !auto;
        }
        ui.add_space(6.0);
        ui.checkbox(&mut d.s.smart_checkout, tr("Put local changes aside automatically when they block switching branches or updating"))
            .on_hover_text(tr("Your changes are put aside (stash), the branch is switched and the changes are applied again. When this is off, RefBraid asks what to do."));
    }

    fn page_jira(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.label(
            RichText::new(tr("Show the status of JIRA issues in the commit details. Issue keys (ABC-123) are taken from the commit message, or, when it has none, from the names of the branches that contain the commit."))
                .color(theme::TEXT_MID()),
        );
        ui.add_space(10.0);
        ui.checkbox(&mut d.s.jira_on, tr("Show issue statuses from JIRA"));
        ui.add_space(8.0);
        if let Some(n) = &self.secrets_note {
            ui.add(egui::Label::new(RichText::new(n).color(theme::STAR()).size(12.0)).wrap());
            ui.add_space(8.0);
        }
        let mut changed = false;
        ui.add_enabled_ui(d.s.jira_on, |ui| {
            egui::Grid::new("jira_grid").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                let w = (ui.available_width() - 150.0).clamp(200.0, 460.0);
                ui.label("Server URL:");
                let r = ui.add(egui::TextEdit::singleline(&mut d.s.jira_url).hint_text(tr("https://jira.example.com")).desired_width(w));
                changed |= r.changed();
                // The saved token belongs to the server it was entered for.
                if r.changed() && (!d.s.jira_token.is_empty() || d.s.jira_kept != crate::secrets::Kept::File) {
                    d.s.jira_token.clear();
                    d.s.jira_kept = crate::secrets::Kept::File;
                }
                theme::edit_menu(r);
                ui.end_row();
                ui.label("Token:");
                ui.horizontal(|ui| {
                    // Write only: a saved token is never shown, a new one replaces it.
                    let saved = !d.s.jira_token.is_empty() || d.s.jira_kept != crate::secrets::Kept::File;
                    let hint = if saved { tr("saved — type to replace") } else { tr("personal access token") };
                    let r = ui.add(egui::TextEdit::singleline(&mut d.jira_entered).password(true).hint_text(hint).desired_width(w - 70.0));
                    changed |= r.changed();
                    theme::edit_menu(r);
                    if ui.add_enabled(saved, egui::Button::new(tr("Forget")).min_size(vec2(62.0, 20.0))).on_hover_text(tr("Remove the saved token")).clicked() {
                        d.s.jira_token.clear();
                        d.jira_entered.clear();
                        d.s.jira_kept = crate::secrets::Kept::File;
                        changed = true;
                    }
                });
                ui.end_row();
                ui.label("E-mail:");
                let r = ui.add(egui::TextEdit::singleline(&mut d.s.jira_email).hint_text(tr("only for Jira Cloud")).desired_width(w));
                changed |= r.changed();
                theme::edit_menu(r);
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(tr("JIRA Server / Data Center: create a personal access token in your profile (Profile → Personal Access Tokens) and leave E-mail empty. Jira Cloud: create an API token at id.atlassian.com and enter the e-mail of your account. The token is kept in the password store of the system and is not shown again once saved; where there is no such store it is kept in the settings file."))
                    .color(theme::TEXT_DIM())
                    .size(12.0),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let busy = d.jira_wait.is_some();
                if ui.add_enabled(!busy, egui::Button::new(tr("Test Connection"))).clicked() {
                    let mut s = d.s.clone();
                    if !d.jira_entered.trim().is_empty() {
                        s.jira_token = d.jira_entered.trim().to_string();
                    }
                    s.jira_on = true;
                    let cfg = crate::jira::Config::from(&s).unwrap_or_default();
                    let (tx, rx) = std::sync::mpsc::channel();
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(crate::jira::test(&cfg));
                        ctx.request_repaint();
                    });
                    d.jira_wait = Some(rx);
                    d.jira_check = None;
                }
                if busy {
                    ui.spinner();
                    ui.label(RichText::new(tr("Connecting…")).color(theme::TEXT_MID()));
                }
            });
        });
        if let Some(rx) = &d.jira_wait {
            if let Ok(r) = rx.try_recv() {
                d.jira_check = Some(r);
                d.jira_wait = None;
            }
        }
        if changed {
            d.jira_check = None;
        }
        match &d.jira_check {
            Some(Ok(v)) => {
                ui.label(RichText::new(format!("✓ {v}")).color(theme::GREEN()));
            }
            Some(Err(e)) => {
                ui.label(RichText::new(format!("Could not connect: {e}")).color(theme::RED()));
            }
            None => {}
        }
    }

    fn page_agents(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.label(
            RichText::new(tr("Which installed agent writes commit messages (the spiral button next to Amend)."))
                .color(theme::TEXT_MID()),
        );
        ui.add_space(8.0);
        crate::ai_view::agent_radio_list(ui, &self.agents, &mut d.s.agent, false);
        ui.add_space(6.0);
        if ui.button(tr("Search again")).on_hover_text(tr("Look for newly installed agents")).clicked() {
            self.agents = ai::detect();
        }
    }

    fn page_open_files(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.label(RichText::new(tr("\"Open in Editor\" in the diff's right-click menu opens the file at the line you clicked. The changed-file lists open it too.")).color(theme::TEXT_MID()));
        ui.add_space(12.0);
        let mut custom = d.editor_custom;
        ui.radio_value(&mut custom, false, tr("The system's default application"));
        ui.indent("editor_default", |ui| {
            ui.label(
                RichText::new(tr("At the line when it is VS Code, a JetBrains IDE (PyCharm…), Sublime Text, Zed, gedit, GNOME Text Editor, Kate, Geany or Notepad++; other programs just open the file."))
                    .color(theme::TEXT_DIM())
                    .size(12.0),
            );
        });
        ui.add_space(8.0);
        ui.radio_value(&mut custom, true, tr("This command:"));
        if custom != d.editor_custom {
            d.editor_custom = custom;
            if custom && d.s.editor_cmd.trim().is_empty() {
                d.s.editor_cmd = "code -g %f:%l".into();
            } else if !custom {
                d.s.editor_cmd.clear();
            }
        }
        ui.add_enabled_ui(custom, |ui| {
            ui.indent("editor_cmd", |ui| {
                theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut d.s.editor_cmd).font(FontId::monospace(13.0)).hint_text(tr("code -g %f:%l")).desired_width(f32::INFINITY)));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                if ui.button(tr("Browse…")).clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_title("Editor Program").pick_file() {
                        let p = p.display().to_string();
                        let p = if p.contains(' ') { format!("\"{p}\"") } else { p };
                        d.s.editor_cmd = format!("{p} %f");
                    }
                }
                egui::ComboBox::from_id_salt("editor_presets").selected_text("Presets").width(110.0).show_ui(ui, |ui| {
                    for (name, cmd) in [
                        ("VS Code", "code -g %f:%l"),
                        ("PyCharm", "pycharm --line %l %f"),
                        ("IntelliJ IDEA", "idea --line %l %f"),
                        ("Sublime Text", "subl %f:%l"),
                        ("gedit", "gedit +%l %f"),
                        ("GNOME Text Editor", "gnome-text-editor +%l %f"),
                        ("Kate", "kate -l %l %f"),
                        ("Notepad++", "\"C:\\Program Files\\Notepad++\\notepad++.exe\" -n%l %f"),
                    ] {
                        if ui.selectable_label(false, name).on_hover_text(cmd).clicked() {
                            d.s.editor_cmd = cmd.to_string();
                        }
                    }
                });
                });
                ui.add_space(4.0);
                ui.label(RichText::new(tr("%f - the file, %l - the line. Put quotes around a program path with spaces.")).color(theme::TEXT_DIM()).size(12.0));
            });
        });
    }

    fn page_file_manager(&mut self, ui: &mut Ui) {
        if !crate::integrate::SUPPORTED {
            ui.label(RichText::new(tr("Not available on macOS yet: open a folder with File → Open.")).color(theme::TEXT_MID()));
            return;
        }
        let (text, button) = if cfg!(windows) {
            (
                "Adds \"Open in RefBraid\" to the right-click menu of folders in Explorer.",
                if crate::integrate::installed() { "Update Explorer integration" } else { "Add to Explorer" },
            )
        } else {
            (
                "Adds RefBraid to \"Open With\" for folders in Files (Nautilus) and a Scripts entry \"Open in RefBraid\".",
                if crate::integrate::installed() { "Update Files integration" } else { "Add to Files" },
            )
        };
        ui.label(RichText::new(text).color(theme::TEXT_MID()));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(button).clicked() {
                self.install_integration();
            }
            let label = if cfg!(windows) { "Remove from Explorer" } else { "Remove from Files" };
            if crate::integrate::installed() && ui.button(label).clicked() {
                let text = crate::integrate::uninstall().unwrap_or_else(|e| format!("Could not do it: {e}"));
                self.dialog = Some(crate::app::Dialog::Message { title: tr("File manager integration").into(), text });
            }
        });
    }
}

/// Settings → Window: the colour the title bar and the left stripe fade from. Ready-made
/// swatches, any colour with the picker, and a preview of the bar.
fn tint_palette(ui: &mut Ui, s: &mut Settings) {
    use crate::settings::{DEFAULT_TITLE_TINT, TITLE_TINTS};
    ui.label(RichText::new(tr("Accent colour")).strong().color(theme::TEXT()));
    ui.label(
        RichText::new(tr("The title bar and the strip of buttons at the left fade from this colour at the top-left corner."))
            .color(theme::TEXT_DIM())
            .size(12.0),
    );
    ui.add_space(8.0);
    let cur = s.title_tint();
    let rgb = |c: [u8; 3]| theme::rgb(c[0], c[1], c[2]);
    // Preview: a small title bar with the gradient, like the real one.
    let (pr, _) = ui.allocate_exact_size(vec2(ui.available_width().min(520.0), 34.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(pr, 6.0, theme::BG_BAR());
    let mut mesh = egui::Mesh::default();
    let mid = pr.left() + pr.width() * 0.6;
    let inner = pr.shrink(1.0);
    mesh.colored_vertex(inner.left_top(), rgb(cur));
    mesh.colored_vertex(pos2(mid, inner.top()), theme::BG_BAR());
    mesh.colored_vertex(pos2(mid, inner.bottom()), theme::BG_BAR());
    mesh.colored_vertex(inner.left_bottom(), rgb(cur));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
    p.rect_stroke(pr, 6.0, Stroke::new(1.0, theme::BORDER()), egui::StrokeKind::Inside);
    let badge = Rect::from_center_size(pos2(pr.left() + 18.0, pr.center().y), vec2(18.0, 18.0));
    p.rect_filled(badge, 4.0, theme::ACCENT());
    p.text(badge.center(), egui::Align2::CENTER_CENTER, "RB", FontId::proportional(8.5), Color32::WHITE);
    p.text(pos2(badge.right() + 12.0, pr.center().y), egui::Align2::LEFT_CENTER, tr("File   Git   View   Help"), FontId::proportional(12.5), theme::TEXT());
    ui.add_space(10.0);
    // Swatches
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (name, c) in TITLE_TINTS {
            let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
            let sel = cur == *c;
            let p = ui.painter();
            // The swatch shows the colour fading into the bar, as it looks on the window.
            let sw = r.shrink(3.0);
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(sw.left_top(), rgb(*c));
            mesh.colored_vertex(sw.right_top(), rgb(*c).lerp_to_gamma(theme::BG_BAR(), 0.45));
            mesh.colored_vertex(sw.right_bottom(), rgb(*c).lerp_to_gamma(theme::BG_BAR(), 0.65));
            mesh.colored_vertex(sw.left_bottom(), rgb(*c).lerp_to_gamma(theme::BG_BAR(), 0.2));
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            p.add(mesh);
            let ring = if sel { Stroke::new(2.0, theme::ACCENT()) } else if resp.hovered() { Stroke::new(1.0, theme::TEXT_MID()) } else { Stroke::new(1.0, theme::INPUT_BORDER()) };
            p.rect_stroke(if sel { r.shrink(1.0) } else { sw }, 6.0, ring, egui::StrokeKind::Inside);
            if sel {
                theme::paint_icon(p, Rect::from_center_size(sw.center(), vec2(14.0, 14.0)), crate::theme::Icon::Check, Color32::WHITE);
            }
            let tip = if *c == DEFAULT_TITLE_TINT { format!("{name} (default)") } else { name.to_string() };
            if resp.on_hover_text(tip).clicked() {
                s.title_tint = if *c == DEFAULT_TITLE_TINT { None } else { Some(*c) };
            }
        }
    });
    ui.add_space(8.0);
    // Any other colour: picker and hex code.
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Custom:")).color(theme::TEXT_MID()));
        let mut c = cur;
        if ui.color_edit_button_srgb(&mut c).on_hover_text(tr("Pick any colour")).changed() {
            s.title_tint = Some(c);
        }
        let id = ui.scope_id().with("tint_hex");
        let mut hex = ui.data(|dd| dd.get_temp::<String>(id)).unwrap_or_else(|| format!("#{:02x}{:02x}{:02x}", cur[0], cur[1], cur[2]));
        let r = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(80.0).font(FontId::monospace(12.5)));
        if r.changed() {
            if let Some(c) = crate::settings::parse_hex_color(&hex) {
                s.title_tint = Some(c);
            }
        }
        if r.has_focus() {
            ui.data_mut(|dd| dd.insert_temp(id, hex));
        } else {
            ui.data_mut(|dd| dd.remove::<String>(id));
        }
        let named = TITLE_TINTS.iter().find(|(_, c)| *c == cur).map(|(n, _)| *n);
        ui.label(RichText::new(named.unwrap_or("custom colour")).color(theme::TEXT_DIM()).size(12.0));
        if s.title_tint.is_some() && ui.link(tr("Reset to default")).clicked() {
            s.title_tint = None;
        }
    });
}
