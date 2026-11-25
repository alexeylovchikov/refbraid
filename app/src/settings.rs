//! Remembers recent repositories and favourite branches between runs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// A pattern found in commit messages and the web address it opens (Issue Navigation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueRule {
    pub pattern: String,
    /// `$0` is the whole match, `$1`… the groups of the pattern.
    pub link: String,
}

/// Key in `Settings::issues` of the patterns for all repositories (a repository's own key
/// overrides them).
pub const ALL_REPOS: &str = "*";

/// Used when no patterns were ever set for all repositories.
pub fn default_issue_rules() -> Vec<IssueRule> {
    vec![IssueRule { pattern: r"\b[A-Z][A-Z0-9]{1,9}-\d+\b".into(), link: String::new() }]
}

pub const DEFAULT_LOG_LIMIT: usize = 5000;

/// A prefix offered when creating a branch ("feature/") and what it is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchPrefix {
    pub name: String,
    pub description: String,
}

/// The prefixes used until the list is edited in Settings.
pub fn default_branch_prefixes() -> Vec<BranchPrefix> {
    prefix_list(&[
        ("feature/", "Development of new functionality."),
        ("release/", "Preparing a release."),
        ("hotfix/", "Urgent fix of critical bugs in production."),
        ("bugfix/", "Fix of ordinary bugs during development."),
        ("support/", "Support of older release versions."),
    ])
}

/// The defaults of an earlier version (descriptions in Russian); a saved copy follows the new ones.
fn old_default_branch_prefixes() -> Vec<BranchPrefix> {
    prefix_list(&[
        ("feature/", "разработка нового функционала."),
        ("release/", "подготовка к релизу."),
        ("hotfix/", "срочное исправление критических багов в продакшене."),
        ("bugfix/", "исправление обычных багов в процессе разработки."),
        ("support/", "поддержка старых версий релизов."),
    ])
}

fn prefix_list(items: &[(&str, &str)]) -> Vec<BranchPrefix> {
    items.iter().map(|(n, d)| BranchPrefix { name: n.to_string(), description: d.to_string() }).collect()
}

pub const DEFAULT_BRANCH_PREFIX: &str = "feature/";

/// Diff or merge: what an external tool is used for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExtKind {
    #[default]
    Diff,
    Merge,
}

/// An external program for comparing files (e.g. Beyond Compare, Meld, KDiff3).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ExtTool {
    pub kind: ExtKind,
    pub name: String,
    pub path: String,
    /// Arguments; %1 left, %2 right, %3 base, %4 result (merge).
    pub args: String,
}

/// Which tools a type of file opens with ("" = the built-in viewer).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ExtAssoc {
    /// "Default" for every other file, or a file-name pattern such as "*.json".
    pub pattern: String,
    pub diff: String,
    pub merge: String,
}

pub const DEFAULT_ASSOC: &str = "Default";

/// A repository in the Favorites panel; `group` is empty when it is not in a group.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct FavRepo {
    pub path: PathBuf,
    pub group: String,
}

/// A group (folder) of the Favorites panel.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct FavGroup {
    pub name: String,
    pub collapsed: bool,
}

#[derive(Default, Clone, PartialEq)]
pub struct Settings {
    pub recent: Vec<PathBuf>,
    /// The repositories open as tabs when the program was last used, and the one in front.
    pub open_tabs: Vec<PathBuf>,
    pub active_tab: Option<PathBuf>,
    /// "repo-path|branch-name"
    pub favorites: HashSet<String>,
    /// repo paths whose favourites were set up (main/master get a star by default)
    pub fav_init: HashSet<String>,
    /// Chosen AI agent ("claude", "codex", …); empty means none.
    pub agent: String,
    /// Extra instructions for generating commit messages.
    pub ai_prompt: String,
    /// Write the commit message with the AI agent as soon as the Commit panel is opened with
    /// an empty message.
    pub ai_auto_message: bool,
    /// The "connect an AI agent" question was already shown.
    pub ai_setup_done: bool,
    /// Recently used commit messages, newest first.
    pub messages: Vec<String>,
    /// Log options: topological order, first parent only, no merges.
    pub log_topo: bool,
    pub log_first_parent: bool,
    pub log_no_merges: bool,
    /// Commit graph style: every line keeps its own column ("Fixed lanes") instead of the
    /// compact layout with slanted lines.
    pub graph_fixed: bool,
    /// How many commits the log loads (0 = default).
    pub log_limit: usize,
    /// Issue patterns per project (repository path). Projects not listed use the default.
    pub issues: HashMap<String, Vec<IssueRule>>,
    /// The git program to run; empty means "git" from PATH.
    pub git_path: String,
    /// Command that opens a file at a line (%f file, %l line); empty = the system's default
    /// application (known editors still get the line).
    pub editor_cmd: String,
    /// Don't reload when the repository changes outside the app.
    pub no_auto_refresh: bool,
    /// When local changes block a checkout: stash them, switch and apply them again without asking.
    pub smart_checkout: bool,
    /// The Merge window's last choice: 0 = git's default, 1 = --no-ff, 2 = --ff-only, 3 = --squash.
    pub merge_mode: u8,
    pub merge_no_commit: bool,
    /// The folder the last clone was put into.
    pub clone_dir: String,
    /// Diffs are shown without syntax colours.
    pub syntax_off: bool,
    /// Commit options kept from one commit to the next: add Signed-off-by, sign the commit.
    pub commit_signoff: bool,
    pub commit_sign: bool,
    /// JIRA: server address (https://jira.example.com), personal access token or API token, and
    /// the account e-mail for Jira Cloud (empty: the token is sent as a Bearer token).
    pub jira_url: String,
    pub jira_token: String,
    pub jira_email: String,
    /// Where the JIRA token is kept (in the store of the system, `jira_token` is empty until read).
    pub jira_kept: crate::secrets::Kept,
    /// Show the JIRA status of issues in the commit details.
    pub jira_on: bool,
    /// Don't look on GitHub for a newer version when the program starts.
    pub no_update_check: bool,
    /// When the program last looked (seconds since 1970), and the version not to tell about again.
    pub update_checked: i64,
    pub update_skip: String,
    /// Git hostings: which program runs on a host and the token for its API.
    pub hosting: Vec<crate::hosting::Account>,
    /// Height of the commit details panel under the changed files (0 = automatic).
    pub info_height: f32,
    /// The same in a file history tab, where the details are under the commit list.
    pub history_info_height: f32,
    /// Widths of the branches tree, the changes/details panel and the commit panel (0 = default).
    pub branches_width: f32,
    pub details_width: f32,
    pub commit_width: f32,
    /// Height of the commit message area under the changes (0 = default).
    pub message_height: f32,
    /// Linux: the app draws the window title bar itself (the default) instead of the desktop.
    pub builtin_title_bar: bool,
    /// External diff / merge tools (Settings → Tools → Diff & Merge Tools).
    pub ext_enabled: bool,
    pub ext_tools: Vec<ExtTool>,
    /// File types and their tools; the first row is always "Default".
    pub ext_assoc: Vec<ExtAssoc>,
    /// Colour the title bar and the left stripe fade from at the top-left corner (None = default).
    pub title_tint: Option<[u8; 3]>,
    /// Shortcuts changed in Settings → Keymap: action id → shortcut ("" = none).
    pub keys: Vec<(String, String)>,
    /// The strip of repository tabs: 0 = always (with a repository open), 1 = when more than
    /// one repository is open, 2 = never.
    pub repo_tabs: u8,
    /// Diffs leave white-space changes out: 0 = no, 1 = changes in its amount, 2 = all of it.
    pub diff_ws: u8,
    /// Lines that are not UTF-8 are read in this encoding (`encoding::NAMES`; 0 = not at all).
    pub fallback_enc: u8,
    /// The window was maximised when the program was last closed: it opens that way.
    pub maximized: bool,
    /// The colours of the window: 0 = dark, 1 = light, 2 = as the system.
    pub theme: u8,
    /// Interface language: "en" (default) or "ru".
    pub language: String,
    /// Rules the AI agent follows when it resolves merge conflicts (empty = default).
    pub conflict_prompt: String,
    /// How conflicts were last resolved with AI, per repository: "prompt" or "skill:<SKILL.md>".
    pub conflict_method: HashMap<String, String>,
    /// Extra folders (or SKILL.md files) to look for skills in.
    pub skill_dirs: Vec<String>,
    /// Don't look for skills in the person's own folders (~/.claude/skills, …).
    pub no_personal_skills: bool,
    /// Changes view: a flat list instead of grouping by directory.
    pub changes_flat: bool,
    /// Changes view: also list files .gitignore hides.
    pub changes_show_ignored: bool,
    /// Branch prefixes for "New Branch" (None = the defaults).
    pub branch_prefixes: Option<Vec<BranchPrefix>>,
    /// The prefix chosen first in "New Branch" (None = "feature/", "" = no prefix).
    pub branch_prefix_default: Option<String>,
    /// Favorite repositories, in the order shown, and the groups they can be put in.
    pub fav_repos: Vec<FavRepo>,
    pub fav_groups: Vec<FavGroup>,
    /// Width of the Favorites panel (0 = default).
    pub favorites_width: f32,
}

/// The default title bar tint (navy) and ready-made ones for the palette in Settings → Window.
pub const DEFAULT_TITLE_TINT: [u8; 3] = [0x17, 0x26, 0x3e];
pub const TITLE_TINTS: &[(&str, [u8; 3])] = &[
    ("Navy", DEFAULT_TITLE_TINT),
    ("Ocean", [0x16, 0x34, 0x4a]),
    ("Teal", [0x2b, 0x3d, 0x45]),
    ("Pine", [0x17, 0x38, 0x33]),
    ("Forest", [0x1d, 0x38, 0x24]),
    ("Olive", [0x32, 0x36, 0x1c]),
    ("Amber", [0x40, 0x31, 0x17]),
    ("Rust", [0x42, 0x24, 0x18]),
    ("Wine", [0x3f, 0x1c, 0x28]),
    ("Plum", [0x34, 0x1e, 0x40]),
    ("Indigo", [0x25, 0x22, 0x4a]),
    ("Graphite", [0x26, 0x28, 0x2c]),
];

/// "17263e" or "#17263e" → [0x17, 0x26, 0x3e].
pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

impl Settings {
    /// The file-type rules with the "Default" row first.
    pub fn ext_assoc_rows(&mut self) -> &mut Vec<ExtAssoc> {
        if !self.ext_assoc.first().is_some_and(|a| a.pattern == DEFAULT_ASSOC) {
            self.ext_assoc.retain(|a| a.pattern != DEFAULT_ASSOC);
            self.ext_assoc.insert(0, ExtAssoc { pattern: DEFAULT_ASSOC.into(), ..ExtAssoc::default() });
        }
        &mut self.ext_assoc
    }

    pub fn title_tint(&self) -> [u8; 3] {
        self.title_tint.unwrap_or(DEFAULT_TITLE_TINT)
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The program's own folder for settings and logs (`~/.config/refbraid`, `%APPDATA%\refbraid`).
pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .or_else(|| if cfg!(windows) { std::env::var_os("APPDATA") } else { None })
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("refbraid"))
}

fn file() -> Option<PathBuf> {
    Some(config_dir()?.join("settings.txt"))
}

/// The program used to be called Git Explorer and kept its settings in a folder of that
/// name. On the first start under the new name they are copied over (the old folder stays).
fn bring_old_settings() {
    let (Some(new), Some(dir)) = (file(), config_dir()) else { return };
    let old = dir.with_file_name(concat!("git-", "explorer")).join("settings.txt");
    if !new.exists() && old.is_file() && std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::copy(&old, &new);
    }
}

impl Settings {
    pub fn load() -> Self {
        if !cfg!(test) {
            bring_old_settings();
        }
        // The built-in title bar is the default; "titlebar=system" switches to the desktop's.
        let mut s = Settings { builtin_title_bar: true, ..Settings::default() };
        let Some(f) = file() else { return s };
        let Ok(text) = std::fs::read_to_string(f) else { return s };
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("recent=") {
                s.recent.push(PathBuf::from(v));
            } else if let Some(v) = line.strip_prefix("tab=") {
                s.open_tabs.push(PathBuf::from(v));
            } else if let Some(v) = line.strip_prefix("activetab=") {
                s.active_tab = Some(PathBuf::from(v));
            } else if let Some(v) = line.strip_prefix("favrepo=") {
                let (group, path) = v.split_once('\t').unwrap_or(("", v));
                s.fav_repos.push(FavRepo { path: PathBuf::from(unesc(path)), group: unesc(group) });
            } else if let Some(v) = line.strip_prefix("favgroup=") {
                let (name, collapsed) = v.split_once('\t').unwrap_or((v, "0"));
                s.fav_groups.push(FavGroup { name: unesc(name), collapsed: collapsed == "1" });
            } else if let Some(v) = line.strip_prefix("favwidth=") {
                s.favorites_width = v.parse().unwrap_or(0.0);
            } else if let Some(v) = line.strip_prefix("fav=") {
                s.favorites.insert(v.to_string());
            } else if let Some(v) = line.strip_prefix("favinit=") {
                s.fav_init.insert(v.to_string());
            } else if let Some(v) = line.strip_prefix("agent=") {
                s.agent = v.to_string();
            } else if let Some(v) = line.strip_prefix("aiprompt=") {
                s.ai_prompt = unesc(v);
            } else if line == "aiauto=1" {
                s.ai_auto_message = true;
            } else if let Some(v) = line.strip_prefix("logopts=") {
                s.log_topo = v.contains('t');
                s.log_first_parent = v.contains('f');
                s.log_no_merges = v.contains('n');
            } else if let Some(v) = line.strip_prefix("key=") {
                if let Some((id, shortcut)) = v.split_once('\t') {
                    crate::keymap::set(&mut s.keys, id, shortcut);
                }
            } else if let Some(v) = line.strip_prefix("repotabs=") {
                s.repo_tabs = v.trim().parse().unwrap_or(0).min(2);
            } else if let Some(v) = line.strip_prefix("diffws=") {
                s.diff_ws = v.trim().parse().unwrap_or(0).min(2);
            } else if let Some(v) = line.strip_prefix("encoding=") {
                s.fallback_enc = v.trim().parse().unwrap_or(0).min(crate::encoding::NAMES.len() as u8 - 1);
            } else if line == "maximized=1" {
                s.maximized = true;
            } else if let Some(v) = line.strip_prefix("theme=") {
                s.theme = v.trim().parse().unwrap_or(0).min(2);
            } else if let Some(v) = line.strip_prefix("lang=") {
                s.language = v.trim().to_string();
            } else if line == "graph=fixed" {
                s.graph_fixed = true;
            } else if let Some(v) = line.strip_prefix("loglimit=") {
                s.log_limit = v.parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("editorcmd=") {
                s.editor_cmd = v.to_string();
            } else if let Some(v) = line.strip_prefix("gitpath=") {
                s.git_path = v.to_string();
            } else if line == "exttools=1" {
                s.ext_enabled = true;
            } else if let Some(v) = line.strip_prefix("exttool=") {
                let f: Vec<&str> = v.split('\t').collect();
                if f.len() == 4 {
                    let kind = if f[0] == "m" { ExtKind::Merge } else { ExtKind::Diff };
                    s.ext_tools.push(ExtTool { kind, name: unesc(f[1]), path: unesc(f[2]), args: unesc(f[3]) });
                }
            } else if let Some(v) = line.strip_prefix("extassoc=") {
                let f: Vec<&str> = v.split('\t').collect();
                if f.len() == 3 {
                    s.ext_assoc.push(ExtAssoc { pattern: unesc(f[0]), diff: unesc(f[1]), merge: unesc(f[2]) });
                }
            } else if let Some(v) = line.strip_prefix("titletint=") {
                s.title_tint = parse_hex_color(v);
            } else if line == "titlebar=builtin" {
                s.builtin_title_bar = true;
            } else if line == "titlebar=system" {
                s.builtin_title_bar = false;
            } else if line == "updatecheck=0" {
                s.no_update_check = true;
            } else if let Some(v) = line.strip_prefix("updatechecked=") {
                s.update_checked = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("updateskip=") {
                s.update_skip = v.trim().to_string();
            } else if line == "autorefresh=0" {
                s.no_auto_refresh = true;
            } else if line == "smartcheckout=1" {
                s.smart_checkout = true;
            } else if let Some(v) = line.strip_prefix("mergemode=") {
                s.merge_mode = v.parse::<u8>().unwrap_or(0).min(3);
            } else if let Some(v) = line.strip_prefix("clonedir=") {
                s.clone_dir = v.to_string();
            } else if line == "syntax=0" {
                s.syntax_off = true;
            } else if line == "signoff=1" {
                s.commit_signoff = true;
            } else if line == "gpgsign=1" {
                s.commit_sign = true;
            } else if line == "mergenocommit=1" {
                s.merge_no_commit = true;
            } else if let Some(v) = line.strip_prefix("jiraurl=") {
                s.jira_url = v.to_string();
            } else if let Some(v) = line.strip_prefix("jiratoken=") {
                s.jira_token = v.to_string();
            } else if let Some(v) = line.strip_prefix("jiraemail=") {
                s.jira_email = v.to_string();
            } else if let Some(v) = line.strip_prefix("hosting=") {
                s.hosting.extend(crate::hosting::Account::from_line(v));
            } else if line == "jirakept=store" {
                s.jira_kept = crate::secrets::Kept::Store;
            } else if line == "jira=1" {
                s.jira_on = true;
            } else if let Some(v) = line.strip_prefix("infoheight=") {
                s.info_height = v.parse().unwrap_or(0.0);
            } else if let Some(v) = line.strip_prefix("historyinfoheight=") {
                s.history_info_height = v.parse().unwrap_or(0.0);
            } else if let Some(v) = line.strip_prefix("panels=") {
                let w: Vec<f32> = v.split(',').map(|x| x.parse().unwrap_or(0.0)).collect();
                s.branches_width = w.first().copied().unwrap_or(0.0);
                s.details_width = w.get(1).copied().unwrap_or(0.0);
                s.commit_width = w.get(2).copied().unwrap_or(0.0);
                s.message_height = w.get(3).copied().unwrap_or(0.0);
            } else if let Some(v) = line.strip_prefix("issues=") {
                s.issues.entry(unesc(v)).or_default();
            } else if let Some(v) = line.strip_prefix("issue=") {
                let f: Vec<&str> = v.split('\t').collect();
                if f.len() == 3 {
                    s.issues.entry(unesc(f[0])).or_default().push(IssueRule {
                        pattern: unesc(f[1]),
                        link: unesc(f[2]),
                    });
                }
            } else if let Some(v) = line.strip_prefix("conflictprompt=") {
                s.conflict_prompt = unesc(v);
            } else if let Some(v) = line.strip_prefix("conflictmethod=") {
                if let Some((repo, m)) = v.split_once('\t') {
                    s.conflict_method.insert(unesc(repo), unesc(m));
                }
            } else if let Some(v) = line.strip_prefix("skilldir=") {
                s.skill_dirs.push(unesc(v));
            } else if line == "bprefixes=1" {
                s.branch_prefixes.get_or_insert_with(Vec::new);
            } else if let Some(v) = line.strip_prefix("bprefix=") {
                let (n, d) = v.split_once('\t').unwrap_or((v, ""));
                s.branch_prefixes.get_or_insert_with(Vec::new).push(BranchPrefix { name: unesc(n), description: unesc(d) });
            } else if let Some(v) = line.strip_prefix("bprefixdefault=") {
                s.branch_prefix_default = Some(unesc(v));
            } else if line == "changes=flat" {
                s.changes_flat = true;
            } else if line == "changes=ignored" {
                s.changes_show_ignored = true;
            } else if line == "personalskills=0" {
                s.no_personal_skills = true;
            } else if line == "aisetup=1" {
                s.ai_setup_done = true;
            } else if let Some(v) = line.strip_prefix("msg=") {
                s.messages.push(unesc(v));
            }
        }
        if s.branch_prefixes.as_ref() == Some(&old_default_branch_prefixes()) {
            s.branch_prefixes = None;
        }
        s.share_issue_rules();
        s
    }

    /// Settings from before patterns for all repositories: the set most repositories use becomes
    /// the shared one, and repositories with just that set drop their own copy.
    fn share_issue_rules(&mut self) {
        if self.issues.contains_key(ALL_REPOS) || self.issues.is_empty() {
            return;
        }
        let mut counts: Vec<(Vec<IssueRule>, usize)> = vec![];
        for rules in self.issues.values() {
            match counts.iter_mut().find(|(r, _)| r == rules) {
                Some((_, n)) => *n += 1,
                None => counts.push((rules.clone(), 1)),
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        let Some((common, _)) = counts.into_iter().next() else { return };
        self.issues.retain(|_, r| *r != common);
        if common != default_issue_rules() {
            self.issues.insert(ALL_REPOS.to_string(), common);
        }
    }

    pub fn save(&self) {
        let Some(f) = file() else { return };
        if let Some(p) = f.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let mut out = String::new();
        for r in &self.recent {
            out.push_str(&format!("recent={}\n", r.display()));
        }
        for t in &self.open_tabs {
            out.push_str(&format!("tab={}\n", t.display()));
        }
        if let Some(t) = &self.active_tab {
            out.push_str(&format!("activetab={}\n", t.display()));
        }
        let mut favs: Vec<_> = self.favorites.iter().collect();
        favs.sort();
        for f in favs {
            out.push_str(&format!("fav={f}\n"));
        }
        for f in &self.fav_init {
            out.push_str(&format!("favinit={f}\n"));
        }
        let tab1 = |s: &str| esc(s).replace('\t', " ");
        for g in &self.fav_groups {
            out.push_str(&format!("favgroup={}\t{}\n", tab1(&g.name), if g.collapsed { 1 } else { 0 }));
        }
        for r in &self.fav_repos {
            out.push_str(&format!("favrepo={}\t{}\n", tab1(&r.group), tab1(&r.path.display().to_string())));
        }
        if self.favorites_width > 0.0 {
            out.push_str(&format!("favwidth={}\n", self.favorites_width.round()));
        }
        out.push_str(&format!("agent={}\n", self.agent));
        out.push_str(&format!("aiprompt={}\n", esc(&self.ai_prompt)));
        if self.ai_auto_message {
            out.push_str("aiauto=1\n");
        }
        out.push_str(&format!(
            "logopts={}{}{}\n",
            if self.log_topo { "t" } else { "" },
            if self.log_first_parent { "f" } else { "" },
            if self.log_no_merges { "n" } else { "" }
        ));
        for (id, shortcut) in &self.keys {
            out.push_str(&format!("key={id}\t{shortcut}\n"));
        }
        if self.maximized {
            out.push_str("maximized=1\n");
        }
        if self.diff_ws > 0 {
            out.push_str(&format!("diffws={}\n", self.diff_ws));
        }
        if self.fallback_enc > 0 {
            out.push_str(&format!("encoding={}\n", self.fallback_enc));
        }
        if self.repo_tabs > 0 {
            out.push_str(&format!("repotabs={}\n", self.repo_tabs));
        }
        if self.theme > 0 {
            out.push_str(&format!("theme={}\n", self.theme));
        }
        if !self.language.is_empty() && self.language != "en" {
            out.push_str(&format!("lang={}\n", self.language));
        }
        if self.graph_fixed {
            out.push_str("graph=fixed\n");
        }
        if self.log_limit > 0 {
            out.push_str(&format!("loglimit={}\n", self.log_limit));
        }
        if !self.editor_cmd.trim().is_empty() {
            out.push_str(&format!("editorcmd={}\n", self.editor_cmd.trim()));
        }
        if !self.git_path.is_empty() {
            out.push_str(&format!("gitpath={}\n", self.git_path));
        }
        out.push_str(if self.builtin_title_bar { "titlebar=builtin\n" } else { "titlebar=system\n" });
        if self.ext_enabled {
            out.push_str("exttools=1\n");
        }
        let t = |s: &str| esc(s).replace('\t', " ");
        for x in &self.ext_tools {
            let k = if x.kind == ExtKind::Merge { "m" } else { "d" };
            out.push_str(&format!("exttool={k}\t{}\t{}\t{}\n", t(&x.name), t(&x.path), t(&x.args)));
        }
        for a in &self.ext_assoc {
            out.push_str(&format!("extassoc={}\t{}\t{}\n", t(&a.pattern), t(&a.diff), t(&a.merge)));
        }
        if let Some([r, g, b]) = self.title_tint {
            out.push_str(&format!("titletint={r:02x}{g:02x}{b:02x}\n"));
        }
        if self.no_auto_refresh {
            out.push_str("autorefresh=0\n");
        }
        if self.no_update_check {
            out.push_str("updatecheck=0\n");
        }
        if self.update_checked > 0 {
            out.push_str(&format!("updatechecked={}\n", self.update_checked));
        }
        if !self.update_skip.trim().is_empty() {
            out.push_str(&format!("updateskip={}\n", self.update_skip.trim()));
        }
        if self.smart_checkout {
            out.push_str("smartcheckout=1\n");
        }
        if self.merge_mode > 0 {
            out.push_str(&format!("mergemode={}\n", self.merge_mode));
        }
        if self.merge_no_commit {
            out.push_str("mergenocommit=1\n");
        }
        if self.commit_signoff {
            out.push_str("signoff=1\n");
        }
        if self.commit_sign {
            out.push_str("gpgsign=1\n");
        }
        if self.syntax_off {
            out.push_str("syntax=0\n");
        }
        if !self.clone_dir.trim().is_empty() {
            out.push_str(&format!("clonedir={}\n", self.clone_dir.trim()));
        }
        if self.jira_on {
            out.push_str("jira=1\n");
        }
        // A token kept in the store of the system leaves only a mark here.
        let in_file = self.jira_kept == crate::secrets::Kept::File;
        let no_token = String::new();
        for (k, v) in [("jiraurl", &self.jira_url), ("jiratoken", if in_file { &self.jira_token } else { &no_token }), ("jiraemail", &self.jira_email)] {
            if !v.trim().is_empty() {
                out.push_str(&format!("{k}={}\n", v.trim()));
            }
        }
        if !in_file {
            out.push_str("jirakept=store\n");
        }
        for a in self.hosting.iter().filter(|a| !a.host.trim().is_empty()) {
            out.push_str(&format!("hosting={}\n", a.to_line()));
        }
        if self.info_height > 0.0 {
            out.push_str(&format!("infoheight={}\n", self.info_height.round()));
        }
        if self.history_info_height > 0.0 {
            out.push_str(&format!("historyinfoheight={}\n", self.history_info_height.round()));
        }
        out.push_str(&format!(
            "panels={},{},{},{}\n",
            self.branches_width.round(),
            self.details_width.round(),
            self.commit_width.round(),
            self.message_height.round()
        ));
        let mut projects: Vec<_> = self.issues.iter().collect();
        projects.sort_by(|a, b| a.0.cmp(b.0));
        let tab = |s: &str| esc(s).replace('\t', " ");
        for (repo, rules) in projects {
            out.push_str(&format!("issues={}\n", esc(repo)));
            for r in rules {
                out.push_str(&format!("issue={}\t{}\t{}\n", tab(repo), tab(&r.pattern), tab(&r.link)));
            }
        }
        if !self.conflict_prompt.trim().is_empty() {
            out.push_str(&format!("conflictprompt={}\n", esc(&self.conflict_prompt)));
        }
        let mut methods: Vec<_> = self.conflict_method.iter().collect();
        methods.sort();
        for (repo, m) in methods {
            out.push_str(&format!("conflictmethod={}\t{}\n", tab(repo), tab(m)));
        }
        for d in &self.skill_dirs {
            out.push_str(&format!("skilldir={}\n", esc(d)));
        }
        if let Some(list) = &self.branch_prefixes {
            out.push_str("bprefixes=1\n");
            for b in list {
                out.push_str(&format!("bprefix={}\t{}\n", tab(&b.name), tab(&b.description)));
            }
        }
        if let Some(d) = &self.branch_prefix_default {
            out.push_str(&format!("bprefixdefault={}\n", esc(d)));
        }
        if self.changes_flat {
            out.push_str("changes=flat\n");
        }
        if self.changes_show_ignored {
            out.push_str("changes=ignored\n");
        }
        if self.no_personal_skills {
            out.push_str("personalskills=0\n");
        }
        if self.ai_setup_done {
            out.push_str("aisetup=1\n");
        }
        for m in &self.messages {
            out.push_str(&format!("msg={}\n", esc(m)));
        }
        let _ = std::fs::write(&f, out);
        // The file may hold tokens of JIRA and of the hostings (where the system has no password
        // store): readable by its owner only.
        #[cfg(unix)]
        if (in_file && !self.jira_token.is_empty()) || self.hosting.iter().any(|a| a.kept == crate::secrets::Kept::File && !a.token.is_empty()) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
        }
    }

    /// Issue patterns of a project: its own, or else the ones for all repositories.
    pub fn issue_rules(&self, repo: &str) -> Vec<IssueRule> {
        self.issues.get(repo).cloned().unwrap_or_else(|| self.shared_issue_rules())
    }

    /// The issue patterns for all repositories (the built-in one if never edited).
    pub fn shared_issue_rules(&self) -> Vec<IssueRule> {
        self.issues.get(ALL_REPOS).cloned().unwrap_or_else(default_issue_rules)
    }

    /// Prefixes offered in "New Branch".
    pub fn branch_prefixes(&self) -> Vec<BranchPrefix> {
        self.branch_prefixes.clone().unwrap_or_else(default_branch_prefixes)
    }

    /// The prefix "New Branch" starts with ("" = no prefix).
    pub fn default_branch_prefix(&self) -> String {
        let d = self.branch_prefix_default.clone().unwrap_or_else(|| DEFAULT_BRANCH_PREFIX.to_string());
        if d.is_empty() || self.branch_prefixes().iter().any(|p| p.name == d) { d } else { String::new() }
    }

    pub fn log_limit(&self) -> usize {
        if self.log_limit == 0 { DEFAULT_LOG_LIMIT } else { self.log_limit }
    }

    pub fn add_message(&mut self, m: &str) {
        let m = m.trim().to_string();
        if m.is_empty() {
            return;
        }
        self.messages.retain(|x| x != &m);
        self.messages.insert(0, m);
        self.messages.truncate(20);
    }

    pub fn is_fav_repo(&self, p: &std::path::Path) -> bool {
        self.fav_repos.iter().any(|r| r.path == p)
    }

    /// Add a repository to the favorites, or take it out when it is there.
    pub fn toggle_fav_repo(&mut self, p: &std::path::Path) {
        if self.is_fav_repo(p) {
            self.fav_repos.retain(|r| r.path != p);
        } else {
            self.fav_repos.push(FavRepo { path: p.to_path_buf(), group: String::new() });
        }
    }

    /// Whether the strip of repository tabs is shown with `open` repositories open.
    pub fn shows_repo_tabs(&self, open: usize) -> bool {
        match self.repo_tabs {
            1 => open > 1,
            2 => false,
            _ => open >= 1,
        }
    }

    pub fn add_recent(&mut self, p: &std::path::Path) {
        self.recent.retain(|x| x != p);
        self.recent.insert(0, p.to_path_buf());
        self.recent.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_the_common_issue_patterns() {
        let jira = vec![IssueRule { pattern: "X-\\d+".into(), link: "https://jira/$0".into() }];
        let other = vec![IssueRule { pattern: "Y-\\d+".into(), link: String::new() }];
        let mut s = Settings::default();
        s.issues.insert("/a".into(), jira.clone());
        s.issues.insert("/b".into(), jira.clone());
        s.issues.insert("/c".into(), other.clone());
        s.share_issue_rules();
        assert_eq!(s.issues.get(ALL_REPOS), Some(&jira));
        assert!(!s.issues.contains_key("/a"));
        assert_eq!(s.issue_rules("/a"), jira);
        assert_eq!(s.issue_rules("/c"), other);
        assert_eq!(s.issue_rules("/new"), jira);
    }

    #[test]
    fn reads_tint_colours() {
        assert_eq!(parse_hex_color("#17263e"), Some([0x17, 0x26, 0x3e]));
        assert_eq!(parse_hex_color("2B3D45"), Some([0x2b, 0x3d, 0x45]));
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("zz0000"), None);
        assert_eq!(Settings::default().title_tint(), DEFAULT_TITLE_TINT);
    }
}
