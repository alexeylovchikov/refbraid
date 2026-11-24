//! Application state, loading data from git and background git jobs.

use crate::git::{self, Commit, CommitDetails, RefKind, RepoState, StatusEntry};
use crate::i18n::tr;
use crate::graph;
use crate::settings::Settings;
use crate::theme;
use eframe::egui::{self, Color32, Stroke, Ui};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DateFilter {
    #[default]
    All,
    Day,
    Week,
    Month,
    Year,
}

impl DateFilter {
    pub fn label(self) -> &'static str {
        match self {
            DateFilter::All => "All",
            DateFilter::Day => "Last 24 hours",
            DateFilter::Week => "Last 7 days",
            DateFilter::Month => "Last 30 days",
            DateFilter::Year => "Last year",
        }
    }
    pub fn seconds(self) -> Option<i64> {
        match self {
            DateFilter::All => None,
            DateFilter::Day => Some(86400),
            DateFilter::Week => Some(7 * 86400),
            DateFilter::Month => Some(30 * 86400),
            DateFilter::Year => Some(365 * 86400),
        }
    }
}

/// A file changed by one or more of the selected commits.
#[derive(Clone, Debug)]
pub struct MultiFile {
    pub file: git::ChangedFile,
    /// The diff spans from `base` (before the oldest selected commit that touched it) to `head`.
    pub base: Option<String>,
    pub head: String,
}

/// What one Log tab shows: its commits, filters and selection. The open tab's state lives in
/// the Repo; the other tabs keep theirs here until they are opened again.
#[derive(Default)]
pub struct LogState {
    commits: Vec<Commit>,
    rows: Vec<graph::Row>,
    index: HashMap<String, usize>,
    visible: Vec<usize>,
    selected: Option<usize>,
    details: Option<CommitDetails>,
    details_file: Option<usize>,
    details_part: usize,
    branch_filter: Option<String>,
    user_filter: Option<String>,
    date_filter: DateFilter,
    path_filter: Option<String>,
    path_input: String,
    follow: bool,
    file_names: HashMap<String, String>,
    text_filter: String,
    regex: bool,
    case: bool,
    topo: bool,
    first_parent: bool,
    no_merges: bool,
    not_merged: bool,
    unmerged: HashSet<String>,
    collapse_linear: bool,
    multi: BTreeSet<usize>,
    multi_files: Vec<MultiFile>,
    multi_commits: Vec<CommitDetails>,
    more: bool,
    deep: u8,
    line_range: Option<(usize, usize)>,
    history_all: bool,
}

macro_rules! log_fields {
    ($m:ident) => {
        $m!(commits, rows, index, visible, selected, details, details_file, details_part, branch_filter, user_filter,
            date_filter, path_filter, path_input, follow, file_names, text_filter, regex, case, topo, first_parent, no_merges, not_merged,
            unmerged, collapse_linear, multi, multi_files, multi_commits, more, deep, line_range, history_all)
    };
}

/// A Log tab: the first one is "Log"; "+" adds more, which can be closed.
pub struct LogTab {
    pub number: usize,
    /// None for the open tab (its state is in the Repo).
    pub state: Option<LogState>,
}

impl Repo {
    /// Take the open tab's log state out of the repository.
    pub fn take_log(&mut self) -> LogState {
        let mut s = LogState::default();
        macro_rules! take {
            ($($f:ident),*) => { $( s.$f = std::mem::take(&mut self.$f); )* };
        }
        log_fields!(take);
        s
    }

    /// Make a tab's log state the open one.
    pub fn put_log(&mut self, s: LogState) {
        macro_rules! put {
            ($($f:ident),*) => { $( self.$f = s.$f; )* };
        }
        log_fields!(put);
    }
}

pub struct Repo {
    pub path: PathBuf,
    pub name: String,
    pub state: RepoState,
    pub commits: Vec<Commit>,
    pub rows: Vec<graph::Row>,
    pub index: HashMap<String, usize>,
    pub visible: Vec<usize>,
    pub selected: Option<usize>,
    pub details: Option<CommitDetails>,
    pub details_file: Option<usize>,
    /// Which list details_file points into: 0 = the commit's own files (for a merge: the files
    /// it changed against every parent), 1.. = "Changes to" parent 1, 2, ….
    pub details_part: usize,
    pub status: Vec<StatusEntry>,
    pub unchecked: HashSet<String>,
    /// Files committed only in part: the changes left out (by their hash) and how many there are.
    pub partial: HashMap<String, PartialSel>,
    /// Ignored files, loaded only while "Show Ignored Files" is on.
    pub ignored: Vec<String>,
    pub status_selected: Option<String>,
    /// All selected files in the Changes view (Ctrl/Shift+click); `status_selected` is the last one.
    pub status_multi: HashSet<String>,
    pub stashes: Vec<git::Stash>,
    pub commit_msg: String,
    pub amend: bool,
    pub branch_filter: Option<String>,
    pub user_filter: Option<String>,
    pub date_filter: DateFilter,
    pub path_filter: Option<String>,
    pub path_input: String,
    /// File history tab: `path_filter` is one file, followed through its renames.
    pub follow: bool,
    /// With `follow`: the file's name in each commit of the list.
    pub file_names: HashMap<String, String>,
    pub text_filter: String,
    pub regex: bool,
    pub case: bool,
    pub topo: bool,
    pub first_parent: bool,
    pub no_merges: bool,
    /// Log shows only the branches' commits that are not merged into the current branch.
    pub not_merged: bool,
    /// With `not_merged`: the branches that have such commits ("L:name" / "R:remote/name").
    pub unmerged: HashSet<String>,
    pub collapse_linear: bool,
    pub branch_search: String,
    pub tree_closed: HashSet<String>,
    /// Folders inside Local / a remote that are open (they start closed).
    pub tree_open: HashSet<String>,
    /// The current branch the tree was last opened for.
    pub tree_head: Option<String>,
    pub log_limit: usize,
    /// Several selected commits (Ctrl/Shift+click); empty or one means a single selection.
    pub multi: BTreeSet<usize>,
    /// Combined changes of the selected commits.
    pub multi_files: Vec<MultiFile>,
    /// Details of each selected commit, newest first.
    pub multi_commits: Vec<CommitDetails>,
    pub tree_selected: Option<String>,
    pub scroll_to: Option<usize>,
    pub fingerprint: u64,
    pub authors: Vec<String>,
    /// The log may go on: as many commits were read as were asked for.
    pub more: bool,
    /// The text filter is searched by git over the whole history: 1 = in commit messages,
    /// 2 = in the changes (`-S`); 0 = only among the loaded commits.
    pub deep: u8,
    /// The log is being read on another thread (a big repository): the list shows what was
    /// read before until this arrives.
    pub log_pending: Option<Receiver<LogData>>,
    /// The changed (and ignored) files being read on another thread.
    pub status_pending: Option<Receiver<StatusData>>,
    /// The submodules of the repository and the state of each (read with the changed files).
    pub submodules: Vec<crate::submodule::Submodule>,
    /// Submodules taken out of the checked files by the program itself (only changes inside).
    pub auto_unchecked: HashSet<String>,
    /// Changed files that are kept in Git LFS.
    pub lfs_paths: HashSet<String>,
    /// The repository uses Git LFS at all (read with the changed files).
    pub lfs_in_use: bool,
    /// Which files of the commit shown in the details are kept in LFS: (what was asked, the answer).
    pub details_lfs: (u64, HashSet<String>),
    /// Changed files whose lines differ only by their endings (CRLF against LF).
    pub eol_only: HashSet<String>,
    /// File history of these lines only (Show History for Selection), 1-based, inclusive.
    pub line_range: Option<(usize, usize)>,
    /// File history: the commits of every branch, not only of the current one.
    pub history_all: bool,
}

/// What reading the log needs, so it can be done on another thread.
struct LogRequest {
    path: PathBuf,
    q: git::LogQuery,
    path_filter: Option<String>,
    no_merges: bool,
    collapse_linear: bool,
    not_merged: bool,
    state: RepoState,
}

/// The log as read and laid out.
pub struct LogData {
    commits: Vec<Commit>,
    rows: Vec<graph::Row>,
    index: HashMap<String, usize>,
    authors: Vec<String>,
    file_names: HashMap<String, String>,
    unmerged: HashSet<String>,
    more: bool,
}

/// How long `reload_log` waits for the log before letting the window go on without it
/// (milliseconds). An ordinary repository answers well within it, so nothing changes there;
/// screenshots and tests wait as long as it takes.
pub static LOG_WAIT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(if cfg!(test) { u64::MAX } else { 250 });
/// Branches and tags of commits being read in the background: (repository, commit, where
/// they arrive).
static REFS_LOADS: std::sync::Mutex<Vec<(PathBuf, String, Receiver<git::CommitRefs>)>> = std::sync::Mutex::new(Vec::new());

/// How long a background load is waited for: `LOG_WAIT_MS`, but no longer than `most`.
fn short_wait(most: u64) -> u64 {
    match LOG_WAIT_MS.load(std::sync::atomic::Ordering::Relaxed) {
        u64::MAX => u64::MAX,
        w => w.min(most),
    }
}

fn recv_within<T>(rx: &Receiver<T>, wait: u64) -> Option<T> {
    if wait == u64::MAX { rx.recv().ok() } else { rx.recv_timeout(std::time::Duration::from_millis(wait)).ok() }
}

/// A commit for the details pane. What is quick to read comes at once; the branches that
/// contain the commit and its tags are looked for on another thread and, when that takes
/// longer than a moment, arrive later (`App::poll_refs`). Until then those of `old` are shown
/// when it is the same commit.
pub fn details_of(dir: &Path, hash: &str, old: Option<&CommitDetails>) -> Option<CommitDetails> {
    details_within(dir, hash, old, short_wait(120))
}

/// `details_of` waiting at most `wait` milliseconds for the branches and tags.
fn details_within(dir: &Path, hash: &str, old: Option<&CommitDetails>, wait: u64) -> Option<CommitDetails> {
    let mut d = git::commit_details_light(dir, hash)?;
    let (tx, rx) = channel();
    let (path, full) = (dir.to_path_buf(), d.hash.clone());
    // Going down the list with the arrow keys asks for one commit after another: no more
    // than two searches run at a time, and of those waiting only the newest is done.
    static NEWEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    use std::sync::atomic::Ordering::SeqCst;
    let me = NEWEST.fetch_add(1, SeqCst) + 1;
    std::thread::spawn(move || {
        loop {
            let n = RUNNING.load(SeqCst);
            if n < 2 {
                if RUNNING.compare_exchange(n, n + 1, SeqCst, SeqCst).is_ok() {
                    break;
                }
                continue;
            }
            if NEWEST.load(SeqCst) != me {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let refs = git::commit_refs(&path, &full);
        RUNNING.fetch_sub(1, SeqCst);
        let _ = tx.send(refs);
        if let Some(ctx) = REPAINT.get() {
            ctx.request_repaint();
        }
    });
    match recv_within(&rx, wait) {
        Some((containing, tags, signature)) => (d.containing, d.tags, d.signature) = (containing, tags, signature),
        None => {
            if let Some(o) = old.filter(|o| o.hash == d.hash) {
                (d.containing, d.tags, d.signature) = (o.containing.clone(), o.tags.clone(), o.signature.clone());
            }
            d.refs_loading = true;
            let mut loads = REFS_LOADS.lock().unwrap();
            // The person has moved on from all but the last few commits.
            let n = loads.len();
            if n >= 3 {
                loads.drain(..n - 2);
            }
            loads.push((dir.to_path_buf(), d.hash.clone(), rx));
        }
    }
    Some(d)
}

/// The window, to draw again when a background load is done.
static REPAINT: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

/// Ask for another frame from a thread that has something for it.
pub fn repaint() {
    if let Some(ctx) = REPAINT.get() {
        ctx.request_repaint();
    }
}

impl Repo {
    /// A repository at `root` with nothing loaded yet.
    pub fn new(root: PathBuf, name: String, settings: &Settings) -> Repo {
        Repo {
            log_limit: settings.log_limit(),
            multi: BTreeSet::new(),
            multi_files: vec![],
            multi_commits: vec![],
            tree_open: HashSet::new(),
            tree_head: None,
            path: root,
            name,
            state: RepoState::default(),
            commits: vec![],
            rows: vec![],
            index: HashMap::new(),
            visible: vec![],
            selected: None,
            details: None,
            details_file: None,
            details_part: 0,
            status: vec![],
            unchecked: HashSet::new(),
            partial: HashMap::new(),
            ignored: vec![],
            status_selected: None,
            status_multi: HashSet::new(),
            stashes: vec![],
            commit_msg: String::new(),
            amend: false,
            branch_filter: None,
            user_filter: None,
            date_filter: DateFilter::All,
            path_filter: None,
            path_input: String::new(),
            follow: false,
            file_names: HashMap::new(),
            text_filter: String::new(),
            regex: false,
            case: false,
            topo: settings.log_topo,
            first_parent: settings.log_first_parent,
            no_merges: settings.log_no_merges,
            not_merged: false,
            unmerged: HashSet::new(),
            collapse_linear: false,
            branch_search: String::new(),
            tree_closed: HashSet::new(),
            tree_selected: None,
            scroll_to: None,
            fingerprint: 0,
            authors: vec![],
            more: false,
            deep: 0,
            log_pending: None,
            status_pending: None,
            submodules: vec![],
            lfs_paths: HashSet::new(),
            auto_unchecked: HashSet::new(),
            lfs_in_use: false,
            details_lfs: (0, HashSet::new()),
            eol_only: HashSet::new(),
            line_range: None,
            history_all: false,
        }
    }

    pub fn current_branch(&self) -> Option<&git::Branch> {
        self.state
            .branches
            .iter()
            .find(|b| b.kind == RefKind::Local && b.is_head)
    }
    pub fn selected_commit(&self) -> Option<&Commit> {
        self.selected.and_then(|i| self.commits.get(i))
    }
}

pub struct JobDone {
    pub id: u64,
    pub title: String,
    pub ok: bool,
    /// Stopped with the Cancel button.
    pub cancelled: bool,
    /// The job's commands, and the refs before and after it (jobs of the open repository).
    pub cmds: Vec<Vec<String>>,
    pub snaps: Option<(crate::undo::Snapshot, crate::undo::Snapshot)>,
    pub log: Vec<(String, String, bool)>,
    pub then: Then,
}

/// What is read together with the changed files, on another thread.
pub struct StatusData {
    pub status: Vec<StatusEntry>,
    pub ignored: Vec<String>,
    pub eol: HashSet<String>,
    pub submodules: Vec<crate::submodule::Submodule>,
    pub lfs: HashSet<String>,
    pub lfs_in_use: bool,
}

impl StatusData {
    fn put_into(self, repo: &mut Repo) {
        repo.status = self.status;
        repo.ignored = self.ignored;
        repo.eol_only = self.eol;
        // A submodule with changes inside but the recorded commit checked out has nothing
        // this repository could commit: it is never among the checked files.
        let inner: HashSet<String> = self
            .submodules
            .iter()
            .filter(|m| !m.pointer_moved() && m.state != crate::submodule::State::Conflict)
            .filter(|m| repo.status.iter().any(|e| e.path == m.path && e.x == ' '))
            .map(|m| m.path.clone())
            .collect();
        // Once its pointer moves (a commit made inside it) there is something to commit again.
        for p in repo.auto_unchecked.difference(&inner) {
            repo.unchecked.remove(p);
        }
        for p in inner.difference(&repo.auto_unchecked) {
            repo.unchecked.insert(p.clone());
        }
        repo.auto_unchecked = inner;
        repo.submodules = self.submodules;
        repo.lfs_paths = self.lfs;
        repo.lfs_in_use = self.lfs_in_use;
    }
}

/// The branch of the demo repository the `mrdemo` screenshot shows a merge request for.
const MR_DEMO_BRANCH: &str = "origin/bugfix/APP-1220-null-check";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Then {
    Nothing,
    OpenPush,
    ClearMessage,
    /// Open the Commit panel with the message kept in `App::pending_commit_msg` (after a
    /// squash merge or a cherry-pick / revert without a commit).
    ShowCommit,
    /// Open the repository in `App::pending_open` (after a clone or `git init`).
    OpenRepo,
    /// Open the Create Pull Request window for `App::pending_request` (after a push).
    NewRequest,
}

pub struct Job {
    pub title: String,
    pub cmds: Vec<Vec<String>>,
    pub then: Then,
    /// Commands that undo the earlier steps when command number `.0` fails (e.g. apply the
    /// stash again when a smart checkout could not switch).
    pub rescue: Option<(usize, Vec<Vec<String>>)>,
    /// Where to run; None = the open repository. A clone runs before there is one.
    pub dir: Option<PathBuf>,
}

/// A job that is running: its console lines and progress are read from `live`.
pub struct LiveJob {
    pub id: u64,
    pub title: String,
    pub live: git::Live,
}

impl Job {
    pub fn one(title: &str, args: &[&str]) -> Self {
        Job {
            title: title.into(),
            cmds: vec![args.iter().map(|s| s.to_string()).collect()],
            then: Then::Nothing,
            rescue: None,
            dir: None,
        }
    }
    pub fn many(title: &str, cmds: Vec<Vec<String>>) -> Self {
        Job {
            title: title.into(),
            cmds,
            then: Then::Nothing,
            rescue: None,
            dir: None,
        }
    }
    pub fn rescue(mut self, step: usize, cmds: Vec<Vec<String>>) -> Self {
        self.rescue = Some((step, cmds));
        self
    }
    /// Smart checkout: put the local changes aside, switch, and apply them again. When the
    /// switch itself fails, the changes are applied back right away.
    /// The same for a pull or merge that local changes stand in the way of ("Smart update").
    pub fn smart_checkout(args: &[String]) -> Self {
        let words: Vec<&String> = args.iter().skip(1).filter(|a| !a.starts_with('-')).collect();
        let target = words.last().map(|s| s.to_string()).unwrap_or_default();
        let title = match args.first().map(String::as_str) {
            Some("checkout") => format!("Smart checkout {target}"),
            Some("merge") => format!("Smart merge {target}"),
            _ => "Smart update".to_string(),
        };
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let cmds = vec![
            s(&["stash", "push", "-m", &format!("RefBraid: {}", title.to_lowercase())]),
            args.to_vec(),
            s(&["stash", "pop"]),
        ];
        Job::many(&title, cmds).rescue(1, vec![s(&["stash", "pop"])])
    }
    pub fn then(mut self, t: Then) -> Self {
        self.then = t;
        self
    }
    /// Run in `dir` instead of the open repository.
    pub fn in_dir(mut self, dir: &Path) -> Self {
        self.dir = Some(dir.to_path_buf());
        self
    }
}

pub struct ConsoleEntry {
    pub time: i64,
    pub cmd: String,
    pub output: String,
    pub ok: bool,
    /// Still running: the output grows as git prints it.
    pub running: bool,
    /// The job and the number of its command this entry shows.
    pub job: Option<(u64, usize)>,
}

/// Changes of a file left out of the next commit.
#[derive(Clone, Debug, Default)]
pub struct PartialSel {
    pub excluded: HashSet<u64>,
    /// Number of changes the file had when last looked at.
    pub total: usize,
}

pub struct DiffTab {
    pub title: String,
    pub subtitle: String,
    pub diff: git::DiffText,
    pub side_by_side: bool,
    pub jump: Option<usize>,
    /// Picture files: the decoded pictures, zoom and rotation.
    pub image: Option<crate::image_view::ImageState>,
    /// A local change (HEAD vs the working folder): its path; changes can be left out or reverted.
    pub worktree: Option<String>,
    /// Fold unchanged lines, and the folds opened by a click.
    pub collapse: bool,
    pub expanded: HashSet<usize>,
    /// The quick-view tab ("<Diff: name>"): a single click on another file shows that file here;
    /// a second double-click on its file keeps it as an ordinary tab.
    pub preview: bool,
    /// When a single click put the file into the quick view. A double-click whose first click
    /// was that very click only shows the file; any later double-click on it keeps the tab.
    pub preview_followed: Option<Instant>,
    /// Opened from a commit's changed files: the commit's files and details stay on the right,
    /// so the next file is one click away.
    pub from_commit: bool,
    /// What is compared (for "Open in external tool").
    pub source: Option<CompareSource>,
    /// Side-by-side: where the divider sits (share of the width for the left side).
    pub split: f32,
    /// How far the text is scrolled to the right (both sides together).
    pub hscroll: f32,
    /// Text selected with the mouse (copied with Ctrl+C).
    pub text_sel: Option<crate::commit_view::TextSel>,
    /// "Annotate with Git Blame": the blame of the new side, and what it was read for.
    pub blame: Option<(String, crate::blame::State)>,
    /// The search line (Ctrl+F), while it is open.
    pub find: Option<crate::find::Find>,
    /// Syntax colours of the lines, worked out when the diff is first drawn.
    pub syntax: Option<crate::syntax::DiffColors>,
}

impl DiffTab {
    /// "Diff: name", or "<Diff: name>" for the quick-view tab.
    pub fn tab_title(&self) -> String {
        if self.preview { format!("<Diff: {}>", self.title) } else { format!("Diff: {}", self.title) }
    }
}

/// What a diff tab compares, so it can be shown in an external tool too.
#[derive(Clone, Debug, PartialEq)]
pub enum CompareSource {
    /// HEAD (or nothing, for a new file) against the file in the working folder.
    Worktree { path: String, old_path: Option<String>, tracked: bool },
    /// A file of commit `hash` against `parent` (None = the file is new in it).
    Commit { hash: String, parent: Option<String>, path: String, old_path: Option<String> },
}

impl CompareSource {
    /// The file being compared (its current name).
    pub fn path(&self) -> &str {
        match self {
            CompareSource::Worktree { path, .. } | CompareSource::Commit { path, .. } => path,
        }
    }
}

/// Whether the light theme is to be shown: the setting (0 dark, 1 light, 2 as the system) and
/// what the system says (dark when it says nothing).
pub fn wants_light(setting: u8, system: Option<egui::Theme>) -> bool {
    match setting {
        1 => true,
        2 => system == Some(egui::Theme::Light),
        _ => false,
    }
}

/// A line of the console ("git commit -m …", "GIT_INDEX_FILE=… git commit …") is a commit.
pub fn is_commit_command(cmd: &str) -> bool {
    cmd.split_whitespace().skip_while(|w| *w != "git").nth(1) == Some("commit")
}

/// A branch or a commit a file can be compared with.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareItem {
    pub rev: String,
    pub label: String,
    pub note: String,
    /// The file's name in that revision, when it was renamed since.
    pub old_path: Option<String>,
}

/// The commits that changed a file (renames followed), newest first, for choosing one to
/// compare with.
pub fn file_revisions(dir: &Path, path: &str, limit: usize) -> Vec<CompareItem> {
    let names = git::file_names(dir, "HEAD", path, limit);
    let n = format!("-n{limit}");
    git::run(dir, &["log", "--follow", "--format=%H%x00%s%x00%an%x00%at", &n, "HEAD", "--", path])
        .stdout
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\0').collect();
            if f.len() < 4 {
                return None;
            }
            let old_path = names.get(f[0]).filter(|n| n.as_str() != path).cloned();
            Some(CompareItem { rev: f[0].to_string(), label: f[1].to_string(), note: format!("{}  ·  {}  ·  {}", short(f[0]), f[2], format_date(f[3].parse().unwrap_or(0))), old_path })
        })
        .collect()
}

/// How a file list opens a diff.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenDiff {
    /// An ordinary tab (toolbar buttons, menus, other windows).
    Normal,
    /// Double-click: in the quick-view tab, or keep it there for good when it already shows
    /// this file.
    DoubleClick,
    /// Single click: only when a quick-view tab is open, show the file there (stay on the
    /// current tab).
    Follow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Log,
    Console,
    Diff(usize),
    /// An "Annotate: file" tab (git blame).
    Blame(usize),
    /// The Reflog tab (Git → Reflog).
    Reflog,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommitTab {
    Changes,
    Stash,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
    Keep,
}

pub enum Dialog {
    NewBranch { from: String, prefix: String, name: String, checkout: bool },
    RenameBranch { old: String, new: String },
    NewTag { rev: String, name: String, message: String },
    DeleteBranch { name: String, remote: bool, force: bool },
    Reset { hash: String, subject: String, mode: ResetMode },
    Pull { rebase: bool, source: String },
    /// `paths`: the files selected in Changes; with `only` just these are put aside.
    Stash { message: String, untracked: bool, keep_index: bool, paths: Vec<String>, only: bool },
    /// Compare with Branch… / Compare with Revision…: the file `path` against one of `items`
    /// (revision, its label, a note such as the date, the file's name there when it differs).
    CompareFile { path: String, branches: bool, filter: String, items: Vec<CompareItem> },
    /// Find Action… (the command palette), or with `branches` the list of branches to check
    /// out: what is typed, and the line the arrow keys are on.
    Palette { query: String, at: usize, branches: bool },
    /// Bisect…: the commit where the problem is and one where it is not yet.
    Bisect { bad: String, good: String },
    /// New Branch from Stash: `stash` is its commit, `label` its message.
    StashBranch { stash: String, label: String, name: String },
    Checkout { rev: String },
    Confirm { title: String, text: String, ok: String, job: Option<Job> },
    Message { title: String, text: String },
    /// What a command printed when it failed (a hook that refused a commit), in full.
    Output { title: String, note: String, text: String },
    /// A checkout stopped because local changes to `files` would be overwritten. `args` is the
    /// checkout command to run again.
    CheckoutProblem { args: Vec<String>, files: Vec<String>, always: bool },
    /// A pull stopped because the branches went different ways and git has no default for
    /// joining them: merge or rebase. `counts` = (local commits, incoming commits).
    DivergentPull { args: Vec<String>, target: String, counts: Option<(usize, usize)>, rebase: bool, remember: bool },
    /// New group in the Favorites panel, or rename of `old`.
    FavGroup { old: Option<String>, name: String },
    /// Help → About: the program's version and the git it uses.
    About { git: String },
    /// Help → Changelog: what changed in each version.
    Changelog,
    EditMessage { hash: String, message: String },
    /// Git → Create Patch / Apply Patch.
    CreatePatch(crate::patch::CreatePatch),
    ApplyPatch(crate::patch::ApplyPatch),
    /// The Interactive Rebase window.
    Rebase(crate::rebase::Plan),
    /// Merge with options: `mode` 0 = git's default, 1 = --no-ff, 2 = --ff-only, 3 = --squash.
    Merge { rev: String, name: String, mode: u8, no_commit: bool, message: String },
    /// Cherry-pick or revert with options. `commits`: (hash, subject) in the order to apply;
    /// `parents`: for a merge commit among them, its parents as (hash, subject) to choose the
    /// mainline from (`mainline` is 1-based).
    /// File → Clone…
    Clone(crate::clone_view::CloneForm),
    /// Git → Clean…: what `git clean` would delete, to confirm.
    Clean(crate::ignore::CleanForm),
    Pick { revert: bool, commits: Vec<(String, String)>, parents: Vec<(String, String)>, record: bool, no_commit: bool, mainline: usize },
}

pub struct Toast {
    pub title: String,
    pub text: String,
    pub ok: bool,
    pub at: Instant,
}

/// A past notification, kept for the bell button in the title bar.
pub struct Notice {
    pub title: String,
    pub text: String,
    pub ok: bool,
    pub when: i64,
}

/// The list the arrow keys move through.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KeyFocus {
    #[default]
    Log,
    Branches,
    Changes,
    /// The text of a diff: the arrows move the text cursor there.
    Text,
}

/// Everything the window holds about one open repository. The repository in front has it in
/// `App` itself; the other tabs keep theirs here, so going back to one shows it as it was
/// left — the log, the diff tabs, the conflicts being resolved — without reading it again.
#[derive(Default)]
pub struct Workspace {
    pub repo: Option<Repo>,
    pub tab: Tab,
    pub diffs: Vec<DiffTab>,
    pub compare: Option<crate::branch_menu::CompareView>,
    pub merges: Vec<crate::merge::MergeDoc>,
    pub blames: Vec<crate::blame::BlameTab>,
    pub reflog: Option<crate::reflog::View>,
    pub history_diff: Option<(String, Option<DiffTab>)>,
    pub show_conflicts: bool,
    pub conflicts_sel: Option<String>,
    pub merge_open: Option<String>,
    pub merge_confirm: Option<bool>,
    pub merge_find: Option<crate::find::Find>,
    pub stash_open: Option<(String, Vec<git::StashFile>)>,
    pub conflicts_done: Vec<crate::merge_view::DoneFile>,
    pub conflict_info: HashMap<String, (usize, usize)>,
    pub conflict_theirs: String,
    pub had_conflicts: bool,
    pub log_tabs: Vec<LogTab>,
    pub active_log: usize,
    pub undo: Option<crate::undo::Record>,
    pub undo_note: Option<String>,
    pub commit_opts: crate::commit_view::CommitOpts,
}

pub struct App {
    /// The open repositories in the order of their tabs (the one in front among them), and
    /// what the ones behind hold. A tab with nothing here has not been read yet.
    pub tab_order: Vec<PathBuf>,
    pub parked: Vec<Workspace>,
    /// The tabs are remembered for the next start (not when the program was given a folder
    /// to open, or takes a screenshot).
    pub keep_tabs: bool,
    /// The quick-view tab: a repository opened with one click in Favorites. The next one opened
    /// that way takes its place; a double-click keeps it.
    pub preview_repo: Option<PathBuf>,
    /// Whether the window was maximised on the last frame (None: not looked at yet).
    pub was_maximized: Option<bool>,
    /// The list the arrow keys move through (the one last clicked or gone to with a shortcut).
    pub key_focus: KeyFocus,
    pub notices: Vec<Notice>,
    pub unread: usize,
    pub repo: Option<Repo>,
    pub settings: Settings,
    pub tx: Sender<JobDone>,
    pub rx: Receiver<JobDone>,
    pub running: Vec<String>,
    pub console: Vec<ConsoleEntry>,
    pub toasts: Vec<Toast>,
    pub tab: Tab,
    pub diffs: Vec<DiffTab>,
    pub show_commit: bool,
    /// The Favorites panel (favourite repositories) at the left.
    pub show_favorites: bool,
    pub fav: crate::favorites_view::FavState,
    pub show_details: bool,
    pub show_branches: bool,
    pub commit_tab: CommitTab,
    pub dialog: Option<Dialog>,
    pub open_input: String,
    pub screenshot: Option<PathBuf>,
    pub shot: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    pub frame: u64,
    /// Drawn off-screen (screenshot fallback): no real window to capture.
    pub headless: bool,
    pub last_focus: bool,
    pub last_check: Instant,
    pub focus_search: bool,
    pub focus_message: bool,
    pub ctx: egui::Context,
    pub error: Option<String>,
    /// Installed AI agents found on this computer.
    pub agents: Vec<(&'static str, PathBuf)>,
    pub ai_job: Option<crate::ai::Running>,
    pub ai_error: Option<String>,
    pub show_settings: bool,
    /// The title bar style this run started with (changing it needs a restart).
    pub started_builtin_title_bar: bool,
    pub restart_requested: bool,
    pub settings_draft: Option<crate::settings_view::SettingsDraft>,
    /// JIRA issues read for the commit details.
    pub jira: crate::jira::Cache,
    /// What the Git hostings told about the open repositories (pull requests, CI).
    pub hosting: crate::hosting::Cache,
    /// What the threads that talk to the password store of the system did, and what went wrong
    /// there (shown on the settings pages that hold tokens).
    pub secrets_tx: Sender<crate::secrets::Done>,
    pub secrets_rx: Receiver<crate::secrets::Done>,
    pub secrets_note: Option<String>,
    /// Looking for a newer version of the program.
    pub update: crate::update_view::State,
    /// The window with all the pull requests of a branch.
    pub branch_requests: Option<crate::hosting_view::BranchRequests>,
    /// The Create Pull Request window.
    pub new_request: Option<crate::hosting_view::NewRequest>,
    /// The branch to offer a pull request for once the running push is done (`Then::NewRequest`).
    pub pending_request: Option<(String, String)>,
    /// A hosting to show instead of the one the remotes name (screenshots).
    pub fake_site: Option<crate::hosting::Site>,
    /// Whether the commit asked about last is in a branch of a remote (until the next refresh).
    pub pushed_memo: Option<(String, bool)>,
    pub last_settings_page: Option<crate::settings_view::Page>,
    pub show_agent_setup: bool,
    pub msg_history: Vec<String>,
    pub compare: Option<crate::branch_menu::CompareView>,
    /// Git → Git Config… window.
    pub git_config: Option<crate::config_view::ConfigForm>,
    /// The Push window.
    pub push_form: Option<crate::push_view::PushForm>,
    /// The "Split into Atomic Commits" analysis and its plan window.
    pub split: Option<crate::split_view::SplitForm>,
    /// Screenshot helper: show the Changes view's AI Tasks menu.
    pub show_ai_tasks: bool,
    pub worktree: Option<crate::branch_menu::WorktreeForm>,
    /// The window with the patterns of files kept in Git LFS.
    pub lfs_form: Option<crate::lfs_view::LfsForm>,
    /// Open merge editors (one per conflicted file).
    pub merges: Vec<crate::merge::MergeDoc>,
    /// Annotate tabs.
    pub blames: Vec<crate::blame::BlameTab>,
    /// The Reflog tab, while it is open.
    pub reflog: Option<crate::reflog::View>,
    /// File history tab: the commit whose change of the file is shown at the right, and that
    /// diff (None when the commit did not change the file itself).
    pub history_diff: Option<(String, Option<DiffTab>)>,
    /// Diffs show who last changed each line of the new side (context menu → Annotate with Git
    /// Blame); stays on from one diff to the next.
    pub diff_annotate: bool,
    /// Interactive Rebase: the AI agent working out a plan, and when it started.
    pub rebase_ai: Option<(crate::ai::Running, Instant)>,
    /// AI agents resolving conflicts: (file, running agent).
    pub conflict_ai: Vec<(String, crate::ai::Running)>,
    /// Skills found for the open repository (and the repository they were looked up for).
    pub skills: Vec<crate::skills::Skill>,
    pub skills_for: Option<PathBuf>,
    pub show_conflicts: bool,
    pub conflicts_sel: Option<String>,
    /// The file shown in the merge window ("Merge Revisions for …").
    pub merge_open: Option<String>,
    /// "Apply Changes" asked: Some(true) when every conflict is resolved, Some(false) when not.
    pub merge_confirm: Option<bool>,
    /// The search line of the merge window (Ctrl+F).
    pub merge_find: Option<crate::find::Find>,
    /// Settings → Credentials: the helper and the keys as read for the page.
    pub cred: Option<crate::settings_view::CredState>,
    /// Options of the next commit that are not kept after it (skip hooks, author, date).
    pub commit_opts: crate::commit_view::CommitOpts,
    /// The stash whose files are shown in the Stash tab: its commit and its files.
    pub stash_open: Option<(String, Vec<git::StashFile>)>,
    /// Files resolved while the Conflicts window was in use ("Resolved" group).
    pub conflicts_done: Vec<crate::merge_view::DoneFile>,
    /// (conflicts, simple ones) per conflicted file, read once.
    pub conflict_info: HashMap<String, (usize, usize)>,
    /// The incoming side's name read from a conflicted file ("feature", a commit…).
    pub conflict_theirs: String,
    /// Whether the last status had conflicts (to open the window when they appear).
    pub had_conflicts: bool,
    /// Collapsed groups and folders in the Changes view ("g:<group>", "<group>:<folder>").
    pub changes_collapsed: HashSet<String>,
    /// Screenshot helper: keep the Changes view options menu open.
    pub show_view_options: bool,
    /// Collapsed folders in the commit's changed-files tree ("" is the repository row).
    pub details_collapsed: HashSet<String>,
    /// Log tabs ("+" adds one); `active_log` is the one whose state is in the Repo.
    pub log_tabs: Vec<LogTab>,
    pub active_log: usize,
    /// The message for the Commit panel once the running job is done (`Then::ShowCommit`).
    pub pending_commit_msg: Option<String>,
    /// The repository to open once the running clone / init is done (`Then::OpenRepo`).
    pub pending_open: Option<PathBuf>,
    /// The running jobs: progress for the status bar, output for the console, Cancel.
    pub live_jobs: Vec<LiveJob>,
    pub next_job: u64,
    /// The last action that can be undone (Git → Undo …).
    pub undo: Option<crate::undo::Record>,
    /// The last action was a push: its name, to say why there is nothing to undo.
    pub undo_note: Option<String>,
}

impl App {
    pub fn new(ctx: &egui::Context, path: Option<PathBuf>, screenshot: Option<PathBuf>) -> Self {
        let (tx, rx) = channel();
        let keep_tabs = path.is_none() && screenshot.is_none() && !cfg!(test);
        let (secrets_tx, secrets_rx) = channel();
        let settings = Settings::load();
        if !cfg!(test) {
            git::set_diff_ws(settings.diff_ws);
            crate::encoding::set_fallback(settings.fallback_enc);
        }
        let mut app = App {
            tab_order: vec![],
            parked: vec![],
            keep_tabs,
            preview_repo: None,
            was_maximized: None,
            key_focus: KeyFocus::Log,
            notices: vec![],
            unread: 0,
            repo: None,
            settings,
            tx,
            rx,
            running: vec![],
            console: vec![],
            toasts: vec![],
            tab: Tab::Log,
            diffs: vec![],
            show_commit: false,
            show_favorites: false,
            fav: Default::default(),
            show_details: true,
            show_branches: true,
            commit_tab: CommitTab::Changes,
            dialog: None,
            open_input: String::new(),
            screenshot,
            shot: Arc::new(Mutex::new(None)),
            frame: 0,
            headless: false,
            last_focus: true,
            last_check: Instant::now(),
            focus_search: false,
            focus_message: false,
            ctx: ctx.clone(),
            error: None,
            agents: crate::ai::detect(),
            ai_job: None,
            ai_error: None,
            show_settings: false,
            started_builtin_title_bar: false,
            restart_requested: false,
            settings_draft: None,
            jira: Default::default(),
            hosting: Default::default(),
            new_request: None,
            branch_requests: None,
            update: Default::default(),
            secrets_tx,
            secrets_rx,
            secrets_note: None,
            pending_request: None,
            fake_site: None,
            pushed_memo: None,
            last_settings_page: None,
            show_agent_setup: false,
            msg_history: vec![],
            compare: None,
            git_config: None,
            push_form: None,
            split: None,
            show_ai_tasks: false,
            worktree: None,
            lfs_form: None,
            merges: vec![],
            history_diff: None,
            diff_annotate: false,
            rebase_ai: None,
            blames: Vec::new(),
            reflog: None,
            conflict_ai: vec![],
            skills: vec![],
            skills_for: None,
            show_conflicts: false,
            conflicts_sel: None,
            merge_open: None,
            merge_confirm: None,
            merge_find: None,
            stash_open: None,
            commit_opts: Default::default(),
            cred: None,
            conflicts_done: vec![],
            conflict_info: HashMap::new(),
            conflict_theirs: String::new(),
            had_conflicts: false,
            changes_collapsed: HashSet::new(),
            show_view_options: false,
            details_collapsed: HashSet::new(),
            log_tabs: vec![LogTab { number: 1, state: None }],
            active_log: 0,
            pending_commit_msg: None,
            pending_open: None,
            live_jobs: vec![],
            next_job: 1,
            undo: None,
            undo_note: None,
        };
        let _ = REPAINT.set(ctx.clone());
        if app.screenshot.is_some() {
            // The password store of the system belongs to the user: a screenshot run leaves it alone.
            crate::secrets::turn_off();
        } else {
            app.start_secrets();
        }
        if app.screenshot.is_some() {
            app.hosting.offline = true;
            LOG_WAIT_MS.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
        }
        app.started_builtin_title_bar = app.settings.builtin_title_bar;
        crate::i18n::set(crate::i18n::Lang::from_code(&app.settings.language));
        if app.screenshot.is_none() {
            // Menu entries an earlier version left behind (e.g. after "dnf remove").
            crate::integrate::cleanup_stale();
        }
        git::SHOW_IGNORED.store(app.settings.changes_show_ignored, std::sync::atomic::Ordering::Relaxed);
        // An unchanged prompt from an earlier version follows the new default.
        if app.settings.ai_prompt.trim().is_empty() || crate::ai::OLD_DEFAULT_PROMPTS.contains(&app.settings.ai_prompt.trim()) {
            app.settings.ai_prompt = crate::ai::DEFAULT_PROMPT.to_string();
        }
        // First run: offer to connect one of the installed agents.
        if app.screenshot.is_none() && !app.settings.ai_setup_done {
            if app.agents.is_empty() {
                app.settings.ai_setup_done = true;
            } else {
                if app.settings.agent.is_empty() {
                    app.settings.agent = app.agents[0].0.to_string();
                }
                app.show_agent_setup = true;
            }
        }
        // Started from the menu or a desktop shortcut (no folder given): show the favourites.
        let plain_start = path.is_none() && app.screenshot.is_none();
        let start = match path {
            Some(p) => Some(p),
            // Screenshots always show the sample repository so the picture is predictable.
            None if app.screenshot.is_some() => crate::demo::ensure_demo(),
            None => std::env::current_dir()
                .ok()
                .filter(|d| d != Path::new("/") && git::find_repo_root(d).is_some()),
        };
        // The tabs of the last time, read only when one is brought to the front.
        if app.keep_tabs {
            app.tab_order = app.settings.open_tabs.iter().filter(|p| p.is_dir()).cloned().collect();
            let front = app.settings.active_tab.clone().filter(|p| app.tab_order.contains(p)).or_else(|| app.tab_order.first().cloned());
            if let Some(p) = front {
                app.open(&p);
            }
        }
        if let Some(p) = start {
            app.open(&p);
        }
        // Screenshots: diffs annotated with git blame (combine with "diff" or "filehistory").
        if std::env::var("REFBRAID_VIEW").is_ok_and(|v| v.contains("gitblame")) {
            app.diff_annotate = true;
        }
        if plain_start {
            app.show_favorites = true;
            app.load_favorites(false);
        }
        // Optional starting view, used to check other screens: selected, commit, diff, console.
        if let Ok(view) = std::env::var("REFBRAID_VIEW") {
            if app.repo.is_some() {
                let first = app.repo.as_ref().and_then(|r| r.visible.get(1).copied());
                if view.contains("selected") || view.contains("diff") {
                    if let Some(i) = first {
                        app.select_commit(i);
                    }
                }
                if view.contains("diff") {
                    app.open_commit_file_diff(0);
                }
                if view.contains("difffile2") {
                    app.open_commit_file_diff(1);
                }
                if view.contains("mergecommit") {
                    let m = app.repo.as_ref().and_then(|r| r.visible.iter().copied().find(|&i| r.commits[i].parents.len() > 1));
                    if let Some(i) = m {
                        app.select_commit(i);
                    }
                }
                if view.contains("commit") || view.contains("stash") {
                    app.show_commit = true;
                }
                if view.contains("stash") {
                    app.commit_tab = CommitTab::Stash;
                }
                // The files of the newest stash.
                if view.contains("shelfopen") {
                    app.show_commit = true;
                    app.commit_tab = CommitTab::Stash;
                    if let Some(h) = app.repo.as_ref().and_then(|r| r.stashes.first().map(|s| s.hash.clone())) {
                        app.toggle_stash_files(&h);
                    }
                }
                // Compare with Revision… for a file of the last commit; Compare Versions of two commits.
                if view.contains("filerevs") || view.contains("filebranches") {
                    let file = app.repo.as_ref().and_then(|r| git::commit_details_light(&r.path, "HEAD")).and_then(|d| d.files.first().map(|f| f.path.clone()));
                    if let Some(f) = file {
                        app.open_compare_file_dialog(&f, view.contains("filebranches"));
                    }
                }
                if view.contains("twoversions") {
                    let n = app.repo.as_ref().map(|r| r.commits.len()).unwrap_or(0);
                    if n > 3 {
                        app.compare_commits(0, 3);
                    }
                }
                // The options of a commit, and a commit refused by a hook.
                if view.contains("hookfail") {
                    app.dialog = Some(Dialog::Output { title: tr("Commit Failed").into(), note: tr("The commit was not made. This is what git or a hook of the repository said:").into(), text: "Running pre-commit checks...\nruff.....................................................Failed\n- hook id: ruff\n- exit code: 1\n\nsrc/waybill.py:14:5: F841 Local variable `total` is assigned to but never used\nsrc/waybill.py:22:1: E302 Expected 2 blank lines, found 1\nFound 2 errors.".into() });
                }
                // Two repositories open as tabs.
                if view.contains("twintabs") {
                    if let (Some(first), Some(second)) = (app.repo.as_ref().map(|r| r.path.clone()), crate::demo::conflict_example()) {
                        // The second one in the quick-view tab, the Favorites panel open.
                        app.open_preview(&second);
                        app.open(&first);
                        app.show_favorites = true;
                    }
                }
                // A repository with submodules: the section in the tree and a moved pointer's diff.
                // ("nestedpointer": the Changes list and the diff of a moved pointer.)
                if view.contains("nestedrepos") || view.contains("nestedpointer") {
                    if let Some(dir) = crate::demo::submodule_example() {
                        app.open(&dir);
                        if view.contains("nestedpointer") {
                            app.show_commit = true;
                            app.open_worktree_diff("libs/charts");
                        }
                    }
                }
                // The command palette with something typed; the list of branches to check out.
                if view.contains("bigfiles") {
                    app.lfs_form = Some(crate::lfs_view::LfsForm { patterns: vec!["*.psd".into(), "*.mp4".into(), "assets/textures/**".into()], new: String::new() });
                }
                if view.contains("findaction") {
                    app.dialog = Some(Dialog::Palette { query: "bra".into(), at: 1, branches: false });
                }
                if view.contains("pickbranch") {
                    app.dialog = Some(Dialog::Palette { query: String::new(), at: 2, branches: true });
                }
                // A diff with the text cursor in it and two lines selected with the keyboard.
                if view.contains("caretmove") {
                    app.open_commit_file_diff(0);
                    app.key_focus = KeyFocus::Text;
                    if let Some(d) = app.diffs.last_mut() {
                        let at = d.diff.rows.iter().position(|r| r.right.is_some() && r.kind != git::DiffSide::Context).unwrap_or(0);
                        d.text_sel = Some(crate::commit_view::TextSel { side: 1, anchor: (at.saturating_sub(2), 4), head: (at, 9) });
                    }
                }
                if view.contains("sshkeys") {
                    app.open_settings(Some(crate::settings_view::Page::Credentials));
                }
                if view.contains("signinfail") {
                    let out = "git@gitlab.example.com: Permission denied (publickey).\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.";
                    if let Some((title, advice)) = crate::credentials::auth_problem(out) {
                        app.dialog = Some(Dialog::Output { title: tr(title).into(), note: tr(advice).into(), text: out.into() });
                    }
                }
                if view.contains("keysetup") {
                    app.open_settings(Some(crate::settings_view::Page::Keymap));
                }
                if view.contains("halving") {
                    app.open_bisect_dialog(None, None);
                }
                if view.contains("shelfnew") {
                    app.open_stash_dialog();
                }
                if view.contains("viewopts") {
                    app.show_commit = true;
                    app.show_view_options = true;
                }
                if view.contains("exttools") || view.contains("extbutton") {
                    // Demo external tools (nothing is started): one diff and one merge tool,
                    // Default rule left on the built-in viewer.
                    use crate::settings::{ExtAssoc, ExtKind, ExtTool, DEFAULT_ASSOC};
                    let path = "/opt/beyondcompare/bcompare-run.sh".to_string();
                    app.settings.ext_enabled = true;
                    app.settings.ext_tools = vec![
                        ExtTool { kind: ExtKind::Diff, name: "Bcompare-run.sh".into(), path: path.clone(), args: "%1 %2 %3".into() },
                        ExtTool { kind: ExtKind::Merge, name: "Bcompare-run.sh".into(), path, args: "%1 %2 %3 %4".into() },
                    ];
                    app.settings.ext_assoc = vec![
                        ExtAssoc { pattern: DEFAULT_ASSOC.into(), diff: String::new(), merge: String::new() },
                        ExtAssoc { pattern: "*.json".into(), diff: "Bcompare-run.sh".into(), merge: String::new() },
                    ];
                }
                if view.contains("exttools") {
                    app.open_settings(Some(crate::settings_view::Page::ExternalTools));
                    if let Some(d) = app.settings_draft.as_mut() {
                        d.tool_sel = Some(0);
                        if view.contains("exttoolsedit") {
                            d.tool_edit = Some(crate::ext_tools_view::ToolEdit::existing(0, &d.s.ext_tools[0]));
                        }
                    }
                } else if view.contains("issuesrepo") {
                    // Issue Navigation, the open repository's tab (it uses the shared patterns).
                    app.open_settings(Some(crate::settings_view::Page::IssueNavigation));
                    if let Some(d) = app.settings_draft.as_mut() {
                        d.issues_for_repo = true;
                    }
                } else if view.contains("jirapage") {
                    app.settings.jira_on = true;
                    app.settings.jira_url = "https://jira.example.com".into();
                    app.settings.jira_token = "NjM4OTk0MzE1NTQ2OkV4YW1wbGVUb2tlbg".into();
                    app.open_settings(Some(crate::settings_view::Page::Jira));
                    if let Some(d) = app.settings_draft.as_mut() {
                        d.jira_check = Some(Ok("Connected as Alex Morgan".into()));
                    }
                } else if view.contains("hostpage") {
                    use crate::hosting::{Account, Kind};
                    app.settings.hosting = vec![
                        Account { kind: Kind::GitHub, host: "github.com".into(), token: "example-token-not-a-real-one".into(), ..Default::default() },
                        Account { kind: Kind::GitLab, host: "code.example.com".into(), token: "example-token-not-a-real-one".into(), ..Default::default() },
                        Account { kind: Kind::Bitbucket, host: "bitbucket.org".into(), ..Default::default() },
                    ];
                    app.open_settings(Some(crate::settings_view::Page::Hosting));
                    if let Some(d) = app.settings_draft.as_mut() {
                        d.host_check = Some((0, Ok("Connected as alex-morgan".into())));
                    }
                } else if view.contains("editorsettings") {
                    app.open_settings(Some(crate::settings_view::Page::OpenFiles));
                } else if view.contains("aisettings") {
                    app.open_settings(Some(crate::settings_view::Page::AiAgents));
                } else if view.contains("prefixsettings") {
                    app.open_settings(Some(crate::settings_view::Page::BranchPrefixes));
                } else if view.contains("commitsettings") {
                    app.open_settings(Some(crate::settings_view::Page::Commit));
                } else if view.contains("mergesettings") {
                    app.open_settings(Some(crate::settings_view::Page::MergeConflicts));
                } else if view.contains("joinopts") {
                    // The Merge window (branch menu → Merge with Options…).
                    app.open_merge_options("feature/demo", "feature/demo");
                } else if view.contains("pickopts") || view.contains("undoopts") {
                    // Cherry-Pick with Options… / Revert Commit… for the newest merge commit.
                    let idx = app.repo.as_ref().and_then(|r| r.commits.iter().position(|c| c.parents.len() > 1));
                    if let Some(i) = idx {
                        app.open_pick(&[i], view.contains("undoopts"));
                    }
                } else if view.contains("getrepo") {
                    // File → Clone…
                    app.open_clone();
                    if let Some(Dialog::Clone(f)) = app.dialog.as_mut() {
                        f.url = "https://example.com/team/shop-backend.git".into();
                        f.name = "shop-backend".into();
                        f.parent = "/home/user/work".into();
                    }
                } else if view.contains("jobrun") {
                    // A fetch in the middle: progress and Cancel in the status bar, live console.
                    let live = git::Live::demo("git fetch --progress --all --prune", &["remote: Enumerating objects: 1840, done.", "remote: Counting objects: 100% (1840/1840), done.", "remote: Compressing objects: 100% (612/612), done."], "Receiving objects:  45% (828/1840), 3.12 MiB | 1.04 MiB/s");
                    app.running.push("Fetch".into());
                    app.live_jobs.push(LiveJob { id: 0, title: "Fetch".into(), live });
                    app.tab = Tab::Console;
                } else if view.contains("takeback") {
                    // Git → Undo of a deleted branch and a moved one.
                    let snap = app.repo.as_ref().map(|r| crate::undo::snapshot(&r.path));
                    if let Some(after) = snap {
                        let mut before = after.clone();
                        before.refs.insert("refs/heads/feature/APP-1402-report-export".into(), ("3ef392b5c0ffee00000000000000000000000000".into(), "origin/feature/APP-1402-report-export".into()));
                        app.undo = crate::undo::Record::new("Delete feature/APP-1402-report-export", &[vec!["branch".into(), "-D".into()]], before, after);
                        app.open_undo();
                    }
                } else if view.contains("wipefiles") {
                    app.open_clean();
                } else if view.contains("reflog") {
                    app.open_reflog();
                } else if view.contains("annotate") {
                    // The file the newest commit changed first.
                    let file = app.repo.as_ref().and_then(|r| git::commit_details(&r.path, "HEAD")).and_then(|d| d.files.first().map(|f| f.path.clone()));
                    if let Some(f) = file {
                        app.open_blame(&f, None, None);
                    }
                } else if view.contains("rangelog") {
                    // Show History for Selection: lines 12-16 of the file the newest commit changed first.
                    let file = app.repo.as_ref().and_then(|r| git::commit_details(&r.path, "HEAD")).and_then(|d| d.files.first().map(|f| f.path.clone()));
                    if let Some(f) = file {
                        app.open_lines_history(&f, None, 12, 16);
                    }
                } else if view.contains("filehistory") {
                    // The file the newest commit changed first.
                    let file = app.repo.as_ref().and_then(|r| git::commit_details(&r.path, "HEAD")).and_then(|d| d.files.first().map(|f| f.path.clone()));
                    if let Some(f) = file {
                        app.open_file_history(&f, None);
                    }
                } else if view.contains("irebase") {
                    app.demo_rebase();
                } else if view.contains("createpatch") {
                    app.open_create_patch(vec![]);
                } else if view.contains("applypatch") {
                    // A patch made from the newest commit's parent, so it is a commit to apply.
                    let path = app.repo.as_ref().map(|r| r.path.clone());
                    if let Some(dir) = path {
                        let out = std::env::temp_dir().join("refbraid-demo.patch");
                        if let Ok(b) = crate::git::command(&dir, &["format-patch", "-1", "--stdout", "HEAD~1"]).output() {
                            let _ = std::fs::write(&out, b.stdout);
                            app.open_apply_patch(&out);
                        }
                    }
                } else if view.contains("graphstyle") {
                    app.open_settings(Some(crate::settings_view::Page::Log));
                } else if view.contains("winsettings") {
                    app.open_settings(Some(crate::settings_view::Page::Window));
                } else if view.contains("settings") {
                    app.open_settings(Some(crate::settings_view::Page::IssueNavigation));
                }
                if view.contains("irebaseai") {
                    app.demo_rebase_ai();
                }
                if view.contains("branchsearch") {
                    // Text in the branch search, so its clear button shows.
                    if let Some(r) = app.repo.as_mut() {
                        r.branch_search = "ma".into();
                    }
                }
                if view.contains("jirastatus") {
                    // Issue statuses as JIRA would answer them (nothing is asked over the network).
                    use crate::jira::{Issue, State};
                    app.settings.jira_on = true;
                    app.settings.jira_url = "https://jira.invalid".into();
                    app.settings.jira_token = "token".into();
                    if let Some(cfg) = crate::jira::Config::from(&app.settings) {
                        let fake = [
                            ("APP-1201", "Base form component", "Done", "done", "Story", "Sam Lee"),
                            ("APP-1220", "Null check in the paid form", "Closed", "done", "Bug", "Sam Lee"),
                            ("APP-1308", "Session loader and waybill fixes", "In Progress", "indeterminate", "Bug", "Alex Morgan"),
                            ("APP-1314", "Smaller font on the printable form", "Code Review", "indeterminate", "Task", "Dana Kim"),
                            ("APP-1344", "Accreditation badge on report 098", "To Do", "new", "Task", ""),
                        ];
                        for (key, summary, status, category, kind, who) in fake {
                            let i = Issue { key: key.into(), summary: summary.into(), status: status.into(), category: category.into(), kind: kind.into(), assignee: who.into() };
                            app.jira.put(&cfg, key, State::Found(i));
                        }
                    }
                    let i = app.repo.as_ref().and_then(|r| r.visible.iter().copied().find(|&i| r.commits[i].subject.starts_with("APP-1308 [fix] Waybill")));
                    if let Some(i) = i {
                        app.select_commit(i);
                    }
                }
                if view.contains("cistatus") || view.contains("newrequest") {
                    // A hosting as it would answer (nothing is asked over the network).
                    use crate::hosting::{Ci, CiRun, Info, Kind, Request, Site};
                    let site = Site { kind: Kind::GitHub, host: "github.com".into(), path: "example/webshop".into(), http: false, token: "token".into(), email: String::new() };
                    let mut info = Info { default_branch: "main".into(), ..Default::default() };
                    if let Some(r) = app.repo.as_ref() {
                        let states = [Ci::Running, Ci::Passed, Ci::Failed, Ci::Passed, Ci::Passed, Ci::Cancelled, Ci::Passed];
                        for (i, st) in r.visible.iter().take(states.len()).zip(states) {
                            let run = CiRun { state: st, url: "https://github.com/example/webshop/actions/runs/1".into(), name: "CI".into() };
                            info.ci.insert(r.commits[*i].hash.clone(), run);
                        }
                        if let Some(b) = r.current_branch().filter(|b| b.name != "main") {
                            info.requests.push(Request { number: "42".into(), title: r.commits.iter().find(|c| c.hash == b.hash).map(|c| c.subject.clone()).unwrap_or_default(), url: "https://github.com/example/webshop/pull/42".into(), source: b.name.clone(), target: "main".into(), sha: b.hash.clone(), author: "alex-morgan".into(), draft: false, ..Default::default() });
                        }
                    }
                    app.hosting.put(&site, info);
                    app.fake_site = Some(site);
                    if view.contains("newrequest") {
                        let other = app.repo.as_ref().and_then(|r| r.state.branches.iter().find(|b| b.kind == RefKind::Remote && !b.name.ends_with("/HEAD") && !b.name.ends_with("/main")).map(|b| b.name.clone()));
                        if let Some((remote, branch)) = other.as_deref().and_then(|n| n.split_once('/')) {
                            app.open_new_request(remote, branch);
                        }
                    } else if let Some(i) = app.repo.as_ref().and_then(|r| r.visible.first().copied()) {
                        app.select_commit(i);
                    }
                }
                if view.contains("mrdemo") {
                    // GitLab as it would answer: merge requests of two branches and the pipelines
                    // of their commits (nothing is asked over the network). The menu of the
                    // branch with a request is opened at the end of `draw`.
                    use crate::hosting::{Ci, CiRun, Info, Kind, Request, Site};
                    let site = Site { kind: Kind::GitLab, host: "gitlab.example.com".into(), path: "shop/webshop".into(), http: false, token: "token".into(), email: String::new() };
                    let web = "https://gitlab.example.com/shop/webshop/-";
                    let mut info = Info { default_branch: "main".into(), ..Default::default() };
                    let mut shown = None;
                    if let Some(r) = app.repo.as_ref() {
                        let states = [Ci::Passed, Ci::Running, Ci::Failed, Ci::Passed, Ci::Passed];
                        for (n, (i, st)) in r.visible.iter().take(states.len()).zip(states).enumerate() {
                            info.ci.insert(r.commits[*i].hash.clone(), CiRun { state: st, url: format!("{web}/pipelines/{}", 911 - n), name: String::new() });
                        }
                        let tip = |name: &str| r.state.branches.iter().find(|b| b.kind == RefKind::Remote && b.name == name).map(|b| b.hash.clone());
                        let subject = |hash: &str| r.commits.iter().find(|c| c.hash == hash).map(|c| c.subject.clone()).unwrap_or_default();
                        if let Some(h) = tip(MR_DEMO_BRANCH) {
                            info.requests.push(Request { number: "128".into(), title: subject(&h), url: format!("{web}/merge_requests/128"), source: MR_DEMO_BRANCH.trim_start_matches("origin/").into(), target: "main".into(), sha: h.clone(), author: "sam.patel".into(), draft: false, ..Default::default() });
                            shown = r.index.get(&h).copied();
                        }
                        if let Some(h) = tip("origin/stage") {
                            info.requests.push(Request { number: "131".into(), title: "Release candidate from stage".into(), url: format!("{web}/merge_requests/131"), source: "stage".into(), target: "master".into(), sha: h, author: "alex.morgan".into(), draft: true, ..Default::default() });
                        }
                    }
                    app.hosting.put(&site, info);
                    if view.contains("mrlist") {
                        // Every request there was out of the branch, as the window lists them.
                        use crate::hosting::Fate;
                        let day = |d: i64| chrono::Local::now().timestamp() - d * 86400;
                        let branch = MR_DEMO_BRANCH.trim_start_matches("origin/");
                        let mut list: Vec<Request> = [
                            ("128", "APP-1220 [fix] Null check in the paid form", "main", "sam.patel", Fate::Open, false, 0),
                            ("131", "APP-1220 Port the null check to the release branch", "master", "sam.patel", Fate::Open, true, 1),
                            ("119", "APP-1220 [fix] Null check, first attempt", "main", "sam.patel", Fate::Closed, false, 9),
                            ("122", "APP-1220 [fix] Guard the paid form against an empty order", "stage", "sam.patel", Fate::Merged, false, 4),
                            ("97", "APP-1188 [feat] Paid form: totals by currency", "main", "dana.lee", Fate::Merged, false, 31),
                            ("84", "APP-1150 [fix] Paid form keeps the chosen period", "main", "alex.morgan", Fate::Merged, false, 58),
                        ]
                        .into_iter()
                        .map(|(n, title, target, who, fate, draft, days)| Request { number: n.into(), title: title.into(), url: format!("{web}/merge_requests/{n}"), source: branch.into(), target: target.into(), author: who.into(), fate, draft, when: day(days), ..Default::default() })
                        .collect();
                        crate::hosting::sort_requests(&mut list);
                        app.branch_requests = Some(crate::hosting_view::BranchRequests { site: site.clone(), branch: branch.into(), wait: None, list: Some(Ok(list)) });
                    }
                    app.fake_site = Some(site);
                    if let Some(i) = shown {
                        app.select_commit(i);
                    }
                }
                if view.contains("newversion") {
                    // A newer release as GitHub would describe it (nothing is asked over the network).
                    let v = env!("CARGO_PKG_VERSION");
                    let next = crate::update::version_of(v).map(|[a, b, c]| format!("{a}.{b}.{}", c + 1)).unwrap_or_else(|| "9.9.9".into());
                    let base = format!("https://github.com/{}/releases", crate::update::REPO);
                    let files = ["refbraid-{v}-1.el10.x86_64.rpm", "refbraid-{v}-1.el9.x86_64.rpm", "refbraid_{v}-1_amd64.deb", "refbraid-{v}-x86_64.AppImage", "refbraid-{v}-windows-x64.msi", "refbraid-{v}-macos-universal.dmg"];
                    app.update.latest = Some(crate::update::Release {
                        url: format!("{base}/tag/v{next}"),
                        body: "## Downloads\n\n| File | For |\n|---|---|\n| `a` | b |\n\n## What's new\n\n### New\n- **Hostings**: GitHub, GitLab and Bitbucket are recognised by the address of the remote. **Open on …** in the menu of a commit, a branch and a file opens it in the browser.\n- **Create Pull Request…** in the Git menu, in the menu of a branch and in the Push window; with a token in Settings → Hosting the request is created from the program.\n- The result of CI is shown before each commit in the Log and in the commit details.\n- Help → **Check for Updates…**: the program tells when a newer version is published.\n\n### Fixed\n- On macOS the shortcuts are shown with its own keys (`⌘K`, `⌥⌘A`).\n".into(),
                        assets: files.iter().map(|f| f.replace("{v}", &next)).map(|f| (f.clone(), format!("{base}/download/v{next}/{f}"))).collect(),
                        version: next,
                    });
                    app.update.open = true;
                }
                if view.contains("welcome") {
                    app.repo = None;
                }
                if view.contains("multisel") {
                    // Several files selected in the Changes view (Ctrl/Shift+click).
                    app.show_commit = true;
                    if let Some(r) = app.repo.as_mut() {
                        let paths: Vec<String> = r.status.iter().map(|e| e.path.clone()).collect();
                        r.status_multi = paths.iter().filter(|p| p.starts_with("notes/") || p.starts_with("src/")).cloned().collect();
                        r.status_selected = paths.iter().find(|p| p.starts_with("notes/")).cloned();
                    }
                }
                if view.contains("wtdiff") {
                    // A local change with two separate edits; the second one left out of the commit.
                    app.show_commit = true;
                    let p = app.repo.as_ref().and_then(|r| r.status.iter().find(|e| e.path.ends_with("app.py")).map(|e| e.path.clone()));
                    if let Some(p) = p {
                        app.open_worktree_diff(&p);
                        if let (Some(r), Some(d)) = (app.repo.as_mut(), app.diffs.last()) {
                            let bl = crate::partial::blocks(&d.diff.rows);
                            if bl.len() > 1 {
                                r.partial.insert(p.clone(), PartialSel { excluded: [bl[1].hash].into_iter().collect(), total: bl.len() });
                            }
                        }
                    }
                }
                if view.contains("quickview") {
                    // Quick-view diff: a double-click opens "<Diff: …>", a click on another file
                    // shows that file in it.
                    app.show_commit = true;
                    let files: Vec<String> = app.repo.as_ref().map(|r| r.status.iter().filter(|e| !e.is_untracked()).map(|e| e.path.clone()).collect()).unwrap_or_default();
                    if let Some(first) = files.first() {
                        app.open_worktree_diff_as(first, OpenDiff::DoubleClick);
                    }
                    if let Some(second) = files.iter().find(|p| p.ends_with("app.py")).or(files.get(1)) {
                        if let Some(r) = app.repo.as_mut() {
                            r.status_selected = Some(second.clone());
                        }
                        app.open_worktree_diff_as(second, OpenDiff::Follow);
                    }
                }
                if view.contains("newimage") {
                    // A picture added since the last commit: it has no "before".
                    app.show_commit = true;
                    let p = app.repo.as_ref().and_then(|r| r.status.iter().find(|e| crate::git::is_image_path(&e.path) && (e.is_untracked() || e.x == 'A')).map(|e| e.path.clone()));
                    if let Some(p) = p {
                        app.open_worktree_diff(&p);
                    }
                }
                if view.contains("splitter") {
                    // The divider moved left and the long lines scrolled to the right.
                    if let Some(d) = app.diffs.last_mut() {
                        d.split = 0.35;
                        d.hscroll = 260.0;
                    }
                }
                if view.contains("gitconfig") {
                    app.open_git_config();
                }
                if view.contains("atomicmenu") {
                    app.show_commit = true;
                    app.show_ai_tasks = true;
                }
                if view.contains("atomicplan") {
                    app.show_commit = true;
                    app.demo_split(crate::split_view::DEMO_PLAN);
                }
                if view.contains("favorites") || view.contains("favloading") {
                    let mut paths: Vec<PathBuf> = app.repo.iter().map(|r| r.path.clone()).collect();
                    for d in ["target/multisel-demo", "target/image-demo"] {
                        if let Ok(p) = std::fs::canonicalize(d) {
                            paths.push(p);
                        }
                    }
                    app.demo_favorites(&paths);
                    if view.contains("favloading") {
                        app.demo_favorites_loading();
                    }
                }
                if view.contains("about") {
                    app.open_about();
                }
                if view.contains("changelog") {
                    app.dialog = Some(Dialog::Changelog);
                }
                if view.contains("textsel") {
                    // A selection in the open diff, from the middle of one line into another.
                    for d in app.diffs.iter_mut() {
                        d.text_sel = Some(crate::commit_view::TextSel { side: if d.side_by_side { 1 } else { 2 }, anchor: (2, 13), head: (5, 9) });
                    }
                }
                if view.contains("unifiedview") {
                    for d in app.diffs.iter_mut() {
                        d.side_by_side = false;
                    }
                }
                if view.contains("wholehistory") || view.contains("loadedonly") {
                    // A text searched in the log: among the loaded commits, or by git in the
                    // whole history (the text comes from REFBRAID_SEARCH, "feat" by default).
                    let text = std::env::var("REFBRAID_SEARCH").unwrap_or_else(|_| "feat".into());
                    if let Some(r) = app.repo.as_mut() {
                        r.text_filter = text;
                        r.deep = if view.contains("wholehistory") { 1 } else { 0 };
                        App::reload_log(r);
                    }
                }
                if view.contains("findtext") {
                    // The search line of a diff (Ctrl+F) with something found.
                    for d in app.diffs.iter_mut() {
                        d.find = Some(crate::find::Find { query: "name".into(), current: 1, ..Default::default() });
                    }
                }
                if view.contains("addtogit") {
                    // Ctrl+Alt+A on a selected new file: it is added and turns green.
                    app.show_commit = true;
                    let first = app.repo.as_ref().and_then(|r| r.status.iter().find(|e| e.is_untracked()).map(|e| e.path.clone()));
                    if let Some(r) = app.repo.as_mut() {
                        r.status_selected = first;
                    }
                    app.add_selected_to_git();
                }
                if view.contains("divergedpull") {
                    app.dialog = Some(Dialog::DivergentPull { args: vec!["pull".into(), "--no-edit".into()], target: "origin/main".into(), counts: Some((2, 3)), rebase: false, remember: false });
                } else if view.contains("pullblocked") {
                    app.dialog = Some(Dialog::CheckoutProblem { args: vec!["pull".into(), "--no-edit".into()], files: vec!["Makefile".into()], always: false });
                } else if view.contains("checkoutproblem") {
                    app.dialog = Some(Dialog::CheckoutProblem { args: vec!["checkout".into(), "master".into()], files: vec!["VERSION".into(), "src/waybill.py".into()], always: false });
                } else if view.contains("dlgmsg") {
                    app.dialog = Some(Dialog::Message { title: tr("Push").into(), text: tr("This repository has no remotes to push to.").into() });
                } else if view.contains("dlgpush") {
                    app.open_push_dialog();
                    if view.contains("diff") {
                        if let Some(f) = app.push_form.as_mut() {
                            f.pending_diff = true;
                        }
                    }
                } else if view.contains("dlgbranch") {
                    app.dialog = Some(Dialog::NewBranch { from: "master".into(), prefix: "feature/".into(), name: "APP-1400".into(), checkout: true });
                } else if view.contains("agentsetup") {
                    app.show_agent_setup = true;
                }
                if view.contains("end") {
                    if let Some(r) = app.repo.as_mut() {
                        r.scroll_to = r.visible.last().copied();
                    }
                }
                if view.contains("multi") {
                    let picks: Vec<usize> = app.repo.as_ref().map(|r| r.visible.iter().skip(3).take(3).copied().collect()).unwrap_or_default();
                    if let Some(&first) = picks.first() {
                        app.select_commit(first);
                        if let Some(&last) = picks.last() {
                            app.select_more(last, true);
                        }
                    }
                }
                if view.contains("logtabs") {
                    app.new_log_tab();
                    if let Some(r) = app.repo.as_mut() {
                        r.branch_filter = Some("HEAD".into());
                        App::reload_log(r);
                    }
                    app.new_log_tab();
                    app.switch_log(1);
                }
                if view.contains("notmerged") {
                    if let Some(r) = app.repo.as_mut() {
                        r.not_merged = true;
                        App::reload_log(r);
                    }
                }
                if view.contains("collapse") || view.contains("nomerges") {
                    if let Some(r) = app.repo.as_mut() {
                        r.collapse_linear = view.contains("collapse");
                        r.no_merges = view.contains("nomerges");
                        App::reload_log(r);
                    }
                }
                if view.contains("console") {
                    app.tab = Tab::Console;
                }
                if view.contains("merge") || view.contains("conflicts") {
                    app.had_conflicts = true;
                    app.show_conflicts = true;
                    if view.contains("conflictsdone") {
                        for f in app.conflicted_files() {
                            app.conflicts_done.push(crate::merge_view::DoneFile { path: f.path.clone(), x: f.x, y: f.y, conflicts: 1 });
                        }
                    } else if view.contains("conflictshalf") {
                        if let Some(f) = app.conflicted_files().first() {
                            app.conflicts_done.push(crate::merge_view::DoneFile { path: f.path.clone(), x: f.x, y: f.y, conflicts: 2 });
                        }
                    }
                    if view.contains("merge") {
                        let files = app.conflicted_files();
                        // For the search, a file whose language is known.
                        let code = files.iter().find(|f| view.contains("mergelookup") && f.path.ends_with(".py"));
                        if let Some(f) = code.or(files.last()).map(|f| f.path.clone()) {
                            app.open_merge(&f, true);
                            if view.contains("mergeai") {
                                if let Some(d) = app.merges.first_mut() {
                                    let n = d.conflicts();
                                    d.take_all(true);
                                    d.note = Some((format!("Claude Code filled in {n} conflict(s) — review the result, edit it if needed and press Apply."), false));
                                }
                            }
                            // The search line with a word found in all three columns.
                            if view.contains("mergelookup") {
                                app.merge_find = Some(crate::find::Find { query: std::env::var("REFBRAID_SEARCH").unwrap_or_else(|_| "total".into()), current: 1, go: true, ..Default::default() });
                            }
                            if view.contains("mergeconfirm") {
                                app.merge_confirm = Some(false);
                            }
                            // The columns scrolled sideways (long lines).
                            if view.contains("mergewide") {
                                app.ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("merge_hscroll", &f)), 420.0f32));
                            }
                        }
                    }
                }
            }
        }
        app
    }

    /// Help → Open Merge Conflict Example: a fresh sample repository in the middle of a merge;
    /// the Conflicts window opens on it.
    pub fn open_conflict_example(&mut self) {
        match crate::demo::conflict_example() {
            Some(dir) => {
                self.open(&dir);
                self.had_conflicts = false;
                self.merge_open = None;
                self.show_conflicts = true;
            }
            None => self.notify(Toast { title: tr("Example not created").into(), text: tr("Git could not build the sample repository.").into(), ok: false, at: Instant::now() }),
        }
    }

    pub fn open(&mut self, p: &Path) {
        let p = git::plain_path(std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));
        if !git::available() {
            self.error = Some(if cfg!(windows) {
                "Git is not installed. Install Git for Windows (git-scm.com), then open the folder again.".to_string()
            } else {
                "Git is not installed. Install the \"git\" package, then open the folder again.".to_string()
            });
            self.repo = None;
            return;
        }
        let Some(root) = git::find_repo_root(&p) else {
            let text = format!("\"{}\" is not inside a Git repository.", p.display());
            // The repository that is open stays open.
            if self.repo.is_some() {
                self.dialog = Some(Dialog::Message { title: tr("Open").into(), text });
            } else {
                self.error = Some(text);
            }
            // A folder that is not a repository yet: offer to make it one.
            if p.is_dir() && self.screenshot.is_none() {
                self.offer_init(&p);
            }
            return;
        };
        // The repository of another tab comes to the front; a new one gets a tab of its own.
        if self.repo.as_ref().is_some_and(|r| r.path != root) {
            if !self.can_switch() {
                self.notify(Toast { title: tr("Wait for the running operation").into(), text: tr("Another repository can be opened when it is done.").into(), ok: false, at: Instant::now() });
                return;
            }
            self.park();
        }
        if !self.tab_order.contains(&root) {
            self.tab_order.push(root.clone());
        }
        if let Some(i) = self.parked.iter().position(|w| w.repo.as_ref().is_some_and(|r| r.path == root)) {
            let mut w = self.parked.remove(i);
            self.swap_workspace(&mut w);
            self.error = None;
            self.settings.add_recent(&root);
            self.came_to_front(&root);
            self.refresh_soon();
            return;
        }
        self.error = None;
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let key = root.display().to_string();
        git::set_program(&self.settings.git_path);
        graph::set_fixed_lanes(self.settings.graph_fixed);
        // The last action belongs to the repository it was done in.
        if self.repo.as_ref().is_none_or(|r| r.path != root) {
            self.undo = None;
            self.undo_note = None;
        }
        let mut repo = Repo::new(root.clone(), name, &self.settings);
        Self::load_everything(&mut repo, short_wait(250));
        if !self.settings.fav_init.contains(&key) {
            self.settings.fav_init.insert(key.clone());
            for b in ["main", "master"] {
                self.settings.favorites.insert(format!("{key}|{b}"));
                for r in &repo.state.remotes {
                    self.settings.favorites.insert(format!("{key}|{r}/{b}"));
                }
            }
        }
        self.settings.add_recent(&root);
        if self.screenshot.is_none() {
            self.settings.save();
        }
        crate::details_view::set_issue_rules(&self.settings.issue_rules(&key));
        self.repo = Some(repo);
        self.diffs.clear();
        self.merges.clear();
        self.blames.clear();
        self.reflog = None;
        self.log_tabs = vec![LogTab { number: 1, state: None }];
        self.active_log = 0;
        for (_, job) in self.conflict_ai.drain(..) {
            crate::ai::cancel(&job);
        }
        self.show_conflicts = false;
        self.merge_open = None;
        self.merge_confirm = None;
        self.conflicts_done.clear();
        self.conflict_info.clear();
        self.conflict_theirs.clear();
        self.tab = Tab::Log;
        self.came_to_front(&root);
    }

    /// Nothing is running that belongs to the repository in front, so another one can take
    /// its place.
    pub fn can_switch(&self) -> bool {
        !self.busy() && self.ai_job.is_none() && self.conflict_ai.is_empty() && self.rebase_ai.is_none()
    }

    /// Exchange what the window holds about the repository in front with `w`.
    fn swap_workspace(&mut self, w: &mut Workspace) {
        macro_rules! swap {
            ($($f:ident),*) => { $(std::mem::swap(&mut self.$f, &mut w.$f);)* };
        }
        swap!(repo, tab, diffs, compare, merges, blames, reflog, history_diff, show_conflicts, conflicts_sel, merge_open, merge_confirm, merge_find, stash_open, conflicts_done, conflict_info, conflict_theirs, had_conflicts, log_tabs, active_log, undo, undo_note, commit_opts);
    }

    /// Put the repository in front aside, with everything open in it.
    fn park(&mut self) {
        let mut w = Workspace::default();
        self.swap_workspace(&mut w);
        // Windows that belong to it are closed.
        self.dialog = None;
        self.push_form = None;
        self.split = None;
        self.worktree = None;
        self.lfs_form = None;
        self.git_config = None;
        if w.repo.is_some() {
            self.parked.push(w);
        }
    }

    /// What follows a repository coming to the front: the window title, its own settings,
    /// the remembered tabs.
    fn came_to_front(&mut self, root: &Path) {
        crate::details_view::set_issue_rules(&self.settings.issue_rules(&root.display().to_string()));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} — RefBraid", root.display())));
        self.remember_tabs();
    }

    /// Open a repository in the quick-view tab (one click in Favorites): it takes the place of
    /// the repository that was looked at that way before. One that has a tab already just
    /// comes to the front.
    pub fn open_preview(&mut self, p: &Path) {
        let Some(root) = git::find_repo_root(&git::plain_path(std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))) else {
            self.open(p);
            return;
        };
        if self.tab_order.contains(&root) {
            self.open(&root);
            return;
        }
        let old = self.preview_repo.clone();
        self.open(&root);
        if self.repo.as_ref().is_none_or(|r| r.path != root) {
            return;
        }
        if let Some(old) = old.filter(|o| *o != root) {
            // The new one stands where the old quick view stood.
            if let Some(at) = self.tab_order.iter().position(|t| *t == old) {
                self.tab_order.retain(|t| *t != old && *t != root);
                self.tab_order.insert(at.min(self.tab_order.len()), root.clone());
            }
            self.parked.retain(|w| w.repo.as_ref().is_none_or(|r| r.path != old));
        }
        self.preview_repo = Some(root);
        self.remember_tabs();
    }

    /// Keep the quick-view tab as an ordinary one (a double-click on it or in Favorites).
    pub fn keep_repo_tab(&mut self, p: &Path) {
        if self.preview_repo.as_deref().is_some_and(|q| git::same_dir(q, p)) {
            self.preview_repo = None;
            self.remember_tabs();
        }
    }

    /// The window opens maximised next time when it is maximised now.
    fn remember_maximized(&mut self, ctx: &egui::Context) {
        let Some(now) = ctx.input(|i| i.viewport().maximized) else { return };
        // Only a change seen while the program runs counts: on the first frames the window
        // is not yet in the state it was asked to open in.
        if self.was_maximized.replace(now).is_some_and(|before| before != now) && now != self.settings.maximized {
            self.settings.maximized = now;
            if self.screenshot.is_none() && !cfg!(test) {
                self.settings.save();
            }
        }
    }

    fn remember_tabs(&mut self) {
        if !self.keep_tabs {
            return;
        }
        // The quick-view tab is not kept for the next start.
        self.settings.open_tabs = self.tab_order.iter().filter(|t| Some(*t) != self.preview_repo.as_ref()).cloned().collect();
        self.settings.active_tab = self.repo.as_ref().map(|r| r.path.clone());
        self.settings.save();
    }

    /// Close a repository's tab. When it is the one in front, its neighbour takes its place.
    pub fn close_repo_tab(&mut self, root: &Path) {
        let Some(at) = self.tab_order.iter().position(|p| p == root) else { return };
        let front = self.repo.as_ref().is_some_and(|r| r.path == root);
        if front && !self.can_switch() {
            self.notify(Toast { title: tr("Wait for the running operation").into(), text: tr("The repository can be closed when it is done.").into(), ok: false, at: Instant::now() });
            return;
        }
        self.tab_order.remove(at);
        if self.preview_repo.as_deref() == Some(root) {
            self.preview_repo = None;
        }
        self.parked.retain(|w| w.repo.as_ref().is_none_or(|r| r.path != root));
        if front {
            let mut gone = Workspace::default();
            self.swap_workspace(&mut gone);
            self.dialog = None;
            self.push_form = None;
            self.split = None;
            self.worktree = None;
            self.lfs_form = None;
            self.git_config = None;
            match self.tab_order.get(at.min(self.tab_order.len().saturating_sub(1))).cloned() {
                Some(next) => self.open(&next),
                None => self.ctx.send_viewport_cmd(egui::ViewportCommand::Title("RefBraid".into())),
            }
        }
        self.remember_tabs();
    }

    /// What a repository's tab says besides its name, as Favorites does: commits to push,
    /// commits to pull, and whether tracked files are changed. Taken from the repository as
    /// it is open here; for a tab not read yet, from what Favorites knows.
    pub fn tab_marks(&self, path: &Path) -> (usize, usize, bool) {
        let of = |r: &Repo| {
            let (ahead, behind) = r.current_branch().map(|b| (b.ahead as usize, b.behind as usize)).unwrap_or((0, 0));
            (ahead, behind, r.status.iter().any(|e| !e.is_untracked()))
        };
        let open = self.repo.iter().chain(self.parked.iter().filter_map(|w| w.repo.as_ref())).find(|r| r.path == path);
        match (open, self.fav.info_of(path)) {
            (Some(r), _) => of(r),
            (None, Some(i)) => (i.ahead, i.behind, i.changes > 0),
            (None, None) => (0, 0, false),
        }
    }

    /// The strip of repository tabs under the title bar (shown when more than one is open).
    pub fn repo_tabs(&mut self, ui: &mut Ui) {
        let front = self.repo.as_ref().map(|r| r.path.clone());
        let can = self.can_switch();
        let preview = self.preview_repo.clone();
        let mut go: Option<PathBuf> = None;
        let mut close: Option<PathBuf> = None;
        let mut keep_tab: Option<PathBuf> = None;
        ui.scope(|ui| {
            // Spaced like the Log / Console tabs below.
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            for p in self.tab_order.clone() {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
                // The quick-view tab is written in angle brackets, like the quick-view diff tab.
                let quick = preview.as_ref() == Some(&p);
                let name = if quick { format!("<{name}>") } else { name };
                let is_front = front.as_ref() == Some(&p);
                let font = egui::FontId::proportional(13.0);
                let w = theme::text_width(ui, &name, &font).min(220.0);
                // Commits to push and to pull, and a dot for changed files, as in Favorites.
                let (ahead, behind, changed) = self.tab_marks(&p);
                let small = egui::FontId::proportional(11.5);
                let marks: Vec<(String, egui::Color32)> = [(ahead > 0, format!("↗{ahead}"), theme::GREEN()), (behind > 0, format!("↙{behind}"), theme::LINK())].into_iter().filter(|m| m.0).map(|m| (m.1, m.2)).collect();
                let marks_w: f32 = marks.iter().map(|(t, _)| theme::text_width(ui, t, &small) + 6.0).sum::<f32>() + if changed { 12.0 } else { 0.0 };
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(w + 44.0 + marks_w, 24.0), egui::Sense::click());
                let hovered = resp.hovered();
                if is_front {
                    ui.painter().rect_filled(rect, 5.0, theme::TAB_SEL());
                } else if hovered {
                    ui.painter().rect_filled(rect, 5.0, theme::HOVER());
                }
                let color = if is_front { theme::TEXT() } else if can { theme::TEXT_MID() } else { theme::TEXT_DIM() };
                theme::paint_clipped(ui.painter(), egui::pos2(rect.left() + 10.0, rect.center().y), &name, font, color, w + 2.0);
                let mut mx = rect.left() + 10.0 + w + 8.0;
                for (text, c) in &marks {
                    ui.painter().text(egui::pos2(mx, rect.center().y), egui::Align2::LEFT_CENTER, text, small.clone(), *c);
                    mx += theme::text_width(ui, text, &small) + 6.0;
                }
                if changed {
                    ui.painter().circle_filled(egui::pos2(mx + 3.0, rect.center().y), 3.5, theme::BLUE_FILE());
                }
                // The cross, on the tab in front and under the pointer.
                let x = egui::Rect::from_center_size(egui::pos2(rect.right() - 13.0, rect.center().y), egui::vec2(16.0, 16.0));
                let over_x = resp.hover_pos().is_some_and(|h| x.contains(h));
                if is_front || hovered {
                    if over_x {
                        ui.painter().rect_filled(x, 3.0, theme::BORDER());
                    }
                    theme::paint_icon(ui.painter(), x.shrink(4.0), theme::Icon::Close, theme::TEXT_MID());
                }
                if resp.middle_clicked() || (resp.clicked() && over_x) {
                    close = Some(p.clone());
                } else if resp.double_clicked() && quick {
                    keep_tab = Some(p.clone());
                } else if resp.clicked() && !is_front {
                    go = Some(p.clone());
                }
                let tip = if !can && !is_front {
                    format!("{}\n{}", p.display(), tr("Wait for the running operation"))
                } else if quick {
                    format!("{}\n{}", p.display(), tr("Quick view: the next repository opened from Favorites takes this tab. Double-click to keep it."))
                } else {
                    p.display().to_string()
                };
                let mut tip = tip;
                for (on, text) in [(ahead > 0, crate::i18n::trf("{} commit(s) to push", &[&ahead])), (behind > 0, crate::i18n::trf("{} commit(s) to pull", &[&behind])), (changed, tr("There are changed files").to_string())] {
                    if on {
                        tip.push_str(&format!("\n{text}"));
                    }
                }
                resp.on_hover_text(tip).context_menu(|ui| {
                    if quick && ui.button(tr("Keep Tab")).clicked() {
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("keep_repo_tab"), p.clone()));
                        ui.close();
                    }
                    if ui.button(tr("Close Tab")).clicked() {
                        close = Some(p.clone());
                        ui.close();
                    }
                    if ui.button(tr("Close Other Tabs")).clicked() {
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("close_other_tabs"), p.clone()));
                        ui.close();
                    }
                });
            }
        });
        let keep: Option<PathBuf> = ui.ctx().data_mut(|d| d.remove_temp(egui::Id::unique("close_other_tabs")));
        if let Some(keep) = keep {
            if front.as_ref() != Some(&keep) {
                self.open(&keep);
            }
            if self.repo.as_ref().is_some_and(|r| r.path == keep) {
                self.parked.clear();
                self.preview_repo = self.preview_repo.take().filter(|q| *q == keep);
                self.tab_order = vec![keep];
                self.remember_tabs();
            }
        }
        let kept: Option<PathBuf> = ui.ctx().data_mut(|d| d.remove_temp(egui::Id::unique("keep_repo_tab")));
        if let Some(p) = keep_tab.or(kept) {
            self.keep_repo_tab(&p);
        }
        if let Some(p) = close {
            self.close_repo_tab(&p);
        }
        if let Some(p) = go {
            self.open(&p);
        }
    }

    /// Read the repository again. The list of changed files is waited for at most `wait`
    /// milliseconds (u64::MAX = until it is read): looking through a very big working folder
    /// takes git a while, and the window must not stand still for it.
    fn load_everything(repo: &mut Repo, wait: u64) {
        repo.state = git::load_state(&repo.path);
        Self::reload_log(repo);
        Self::reload_status(repo, wait);
        repo.stashes = git::stashes(&repo.path);
        repo.fingerprint = git::fingerprint(&repo.path);
    }

    /// Read the changed files on another thread; when they don't arrive within `wait`
    /// milliseconds the old list stays until `take_pending_status` gets the new one.
    pub fn reload_status(repo: &mut Repo, wait: u64) {
        let path = repo.path.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let ignored = if git::SHOW_IGNORED.load(std::sync::atomic::Ordering::Relaxed) { git::ignored(&path) } else { vec![] };
            let status = git::status(&path);
            // Worth a look only when something tracked is changed.
            let eol = if status.iter().any(|e| !e.is_untracked()) { git::eol_only(&path) } else { HashSet::new() };
            let submodules = crate::submodule::read(&path);
            let lfs_in_use = crate::lfs::in_use(&path);
            let lfs = if lfs_in_use { crate::lfs::tracked(&path, &status.iter().map(|e| e.path.clone()).collect::<Vec<_>>()) } else { HashSet::new() };
            let _ = tx.send(StatusData { status, ignored, eol, submodules, lfs, lfs_in_use });
            if let Some(ctx) = REPAINT.get() {
                ctx.request_repaint();
            }
        });
        match recv_within(&rx, wait) {
            Some(d) => {
                repo.status_pending = None;
                d.put_into(repo);
            }
            // An earlier load still on its way is dropped with its receiver.
            None => repo.status_pending = Some(rx),
        }
    }

    /// Put the changed files read in the background into `repo` if they have arrived.
    pub fn take_pending_status(repo: &mut Repo) -> bool {
        let Some(rx) = repo.status_pending.as_ref() else { return false };
        match rx.try_recv() {
            Ok(d) => {
                repo.status_pending = None;
                d.put_into(repo);
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                repo.status_pending = None;
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
        }
    }

    /// Fill in the branches and tags of the shown commits that were read in the background.
    pub fn poll_refs(&mut self) {
        // None: the search was given up for a newer one.
        let mut got: Vec<(PathBuf, String, Option<git::CommitRefs>)> = vec![];
        REFS_LOADS.lock().unwrap().retain(|(path, hash, rx)| match rx.try_recv() {
            Ok(refs) => {
                got.push((path.clone(), hash.clone(), Some(refs)));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                got.push((path.clone(), hash.clone(), None));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
        });
        let Some(repo) = self.repo.as_mut() else { return };
        for (path, hash, refs) in got {
            if path != repo.path {
                continue;
            }
            let shown = [repo.details.as_mut(), self.reflog.as_mut().and_then(|v| v.details.as_mut())];
            for d in shown.into_iter().flatten().filter(|d| d.hash == hash) {
                d.refs_loading = false;
                if let Some((containing, tags, signature)) = refs.clone() {
                    (d.containing, d.tags, d.signature) = (containing, tags, signature);
                }
            }
        }
    }

    /// Read the log again. The reading runs on another thread; when it takes longer than a
    /// moment (a very big repository) the window goes on with the old list and `poll_log`
    /// shows the new one when it arrives.
    pub fn reload_log(repo: &mut Repo) {
        Self::reload_log_within(repo, LOG_WAIT_MS.load(std::sync::atomic::Ordering::Relaxed));
    }

    /// `reload_log` waiting at most `wait` milliseconds for the log (u64::MAX = until it is read).
    pub fn reload_log_within(repo: &mut Repo, wait: u64) {
        let text = repo.text_filter.trim().to_string();
        if text.is_empty() || repo.follow {
            repo.deep = 0;
        }
        let req = LogRequest {
            path: repo.path.clone(),
            q: git::LogQuery {
                first_parent: repo.first_parent,
                branch: repo.branch_filter.clone().filter(|b| !b.is_empty()),
                paths: repo.path_filter.iter().cloned().collect(),
                limit: repo.log_limit,
                not_merged: repo.not_merged && !repo.follow,
                follow: repo.follow && repo.path_filter.is_some(),
                topo: repo.topo && !repo.follow,
                grep: (repo.deep > 0).then(|| git::Grep { text, regex: repo.regex, case: repo.case, in_changes: repo.deep == 2 }),
                lines: repo.line_range.filter(|_| repo.follow),
                all: repo.history_all && repo.follow,
            },
            path_filter: repo.path_filter.clone(),
            no_merges: repo.no_merges,
            collapse_linear: repo.collapse_linear,
            not_merged: repo.not_merged,
            state: repo.state.clone(),
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(Self::compute_log(&req));
            if let Some(ctx) = REPAINT.get() {
                ctx.request_repaint();
            }
        });
        let got = if wait == u64::MAX { rx.recv().ok() } else { rx.recv_timeout(std::time::Duration::from_millis(wait)).ok() };
        match got {
            Some(d) => {
                repo.log_pending = None;
                Self::apply_log(repo, d);
            }
            // An earlier load still on its way is dropped with its receiver.
            None => repo.log_pending = Some(rx),
        }
    }

    /// Show a log that was read in the background, once it is there. True when it arrived.
    pub fn poll_log(&mut self) -> bool {
        let Some(repo) = self.repo.as_mut() else { return false };
        if !Self::take_pending_log(repo) {
            return false;
        }
        // A file history starts at the newest version of the file.
        let first = (repo.follow && repo.selected.is_none()).then(|| repo.visible.first().copied()).flatten();
        if let Some(i) = first {
            self.select_commit(i);
        }
        true
    }

    /// Put a log read in the background into `repo` if it has arrived.
    pub fn take_pending_log(repo: &mut Repo) -> bool {
        let Some(rx) = repo.log_pending.as_ref() else { return false };
        match rx.try_recv() {
            Ok(d) => {
                repo.log_pending = None;
                Self::apply_log(repo, d);
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                repo.log_pending = None;
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
        }
    }

    /// The log is still being read for the first time or again.
    pub fn log_loading(&self) -> bool {
        self.repo.as_ref().is_some_and(|r| r.log_pending.is_some())
    }

    /// Read more of the history: the next portion of older commits.
    pub fn load_more_log(&mut self) {
        let step = self.settings.log_limit();
        if let Some(repo) = self.repo.as_mut() {
            if repo.more && repo.log_pending.is_none() {
                repo.log_limit += step;
                Self::reload_log(repo);
            }
        }
    }

    /// Search the text filter with git over the whole history (1 = messages, 2 = changes), or
    /// go back to searching the loaded commits (0).
    pub fn set_deep_search(&mut self, mode: u8) {
        if let Some(repo) = self.repo.as_mut() {
            repo.deep = mode;
            Self::reload_log(repo);
        }
    }

    fn compute_log(req: &LogRequest) -> LogData {
        let q = &req.q;
        let mut commits = git::load_log(&req.path, q);
        let more = commits.len() >= q.limit;
        let file_names = match (&req.path_filter, q.follow) {
            (Some(p), true) => git::file_names(&req.path, q.branch.as_deref().unwrap_or("HEAD"), p, q.limit),
            _ => HashMap::new(),
        };
        let unmerged = if req.not_merged { git::unmerged_branches(&req.path) } else { HashSet::new() };
        let mut dashed = HashSet::new();
        if (req.no_merges || req.collapse_linear) && !q.follow {
            let (c, d) = simplify(commits, req.no_merges, req.collapse_linear, &req.state);
            commits = c;
            dashed = d;
        }
        // The current branch's line (HEAD and its first parents) is drawn in the leftmost column.
        let mut main = HashSet::new();
        {
            let pos: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, c)| (c.hash.as_str(), i)).collect();
            let mut cur = pos.get(req.state.head_hash.as_str()).copied();
            while let Some(i) = cur {
                if !main.insert(commits[i].hash.clone()) {
                    break;
                }
                cur = commits[i].parents.first().and_then(|p| pos.get(p.as_str()).copied());
            }
        }
        let rows = if q.follow {
            // The file's versions one after another: a single line, whatever lies between them.
            let mut line = commits.clone();
            for i in 0..line.len() {
                line[i].parents = commits.get(i + 1).map(|c| vec![c.hash.clone()]).unwrap_or_default();
            }
            graph::layout_chosen(&line, &dashed, &line.iter().map(|c| c.hash.clone()).collect())
        } else {
            graph::layout_chosen(&commits, &dashed, &main)
        };
        let index = commits.iter().enumerate().map(|(i, c)| (c.hash.clone(), i)).collect();
        let mut authors: Vec<String> = commits.iter().map(|c| c.author.clone()).collect();
        authors.sort();
        authors.dedup();
        LogData { commits, rows, index, authors, file_names, unmerged, more }
    }

    fn apply_log(repo: &mut Repo, d: LogData) {
        let keep = repo.selected_commit().map(|c| c.hash.clone());
        let keep_multi: Vec<String> = repo.multi.iter().filter_map(|&i| repo.commits.get(i).map(|c| c.hash.clone())).collect();
        repo.file_names = d.file_names;
        repo.unmerged = d.unmerged;
        repo.rows = d.rows;
        repo.index = d.index;
        repo.authors = d.authors;
        repo.commits = d.commits;
        repo.more = d.more;
        repo.selected = keep.and_then(|h| repo.index.get(&h).copied());
        repo.multi = keep_multi.iter().filter_map(|h| repo.index.get(h).copied()).collect();
        if repo.multi.len() < 2 {
            repo.multi.clear();
        }
        if repo.selected.is_none() {
            repo.details = None;
        }
        Self::apply_filters(repo);
    }

    pub fn apply_filters(repo: &mut Repo) {
        let now = chrono::Utc::now().timestamp();
        let text = repo.text_filter.trim().to_string();
        let re = if repo.regex && !text.is_empty() {
            regex::RegexBuilder::new(&text)
                .case_insensitive(!repo.case)
                .build()
                .ok()
        } else {
            None
        };
        let needle = if repo.case { text.clone() } else { text.to_lowercase() };
        let hash_needle = text.to_lowercase();
        repo.visible = repo
            .commits
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                if let Some(u) = &repo.user_filter {
                    if &c.author != u {
                        return false;
                    }
                }
                if let Some(s) = repo.date_filter.seconds() {
                    if c.time < now - s {
                        return false;
                    }
                }
                // With a search over the whole history git has chosen the commits already
                // (by the full message or by their changes).
                if !text.is_empty() && repo.deep == 0 {
                    if c.hash.starts_with(&hash_needle) {
                        return true;
                    }
                    if repo.regex {
                        return re.as_ref().map(|r| r.is_match(&c.subject)).unwrap_or(false);
                    }
                    let subj = if repo.case { c.subject.clone() } else { c.subject.to_lowercase() };
                    return subj.contains(&needle);
                }
                true
            })
            .map(|(i, _)| i)
            .collect();
    }

    pub fn filtered(repo: &Repo) -> bool {
        repo.visible.len() != repo.commits.len()
    }

    /// Compare with Branch… (`branches`) or Compare with Revision… for a file: the list to
    /// choose from — every branch, or the commits that changed the file.
    pub fn open_compare_file_dialog(&mut self, path: &str, branches: bool) {
        let Some(repo) = self.repo.as_ref() else { return };
        let items = if branches {
            repo.state.branches.iter().filter(|b| b.kind != RefKind::Tag && !b.name.ends_with("/HEAD")).map(|b| CompareItem { rev: b.name.clone(), label: b.name.clone(), note: if b.is_head { tr("current").to_string() } else { String::new() }, old_path: None }).collect()
        } else {
            file_revisions(&repo.path, path, 500)
        };
        self.dialog = Some(Dialog::CompareFile { path: path.to_string(), branches, filter: String::new(), items });
    }

    /// The menu items of a file asked for a comparison (see `compare_file_items`).
    pub fn take_compare_request(&mut self, ctx: &egui::Context) {
        let id = egui::Id::unique("compare_file_request");
        let asked: Option<(String, bool)> = ctx.data_mut(|d| d.remove_temp(id));
        if let Some((path, branches)) = asked {
            self.open_compare_file_dialog(&path, branches);
        }
    }

    /// Compare Versions: what differs between two commits of the Log (the older one is the base).
    pub fn compare_commits(&mut self, a: usize, b: usize) {
        let Some(repo) = self.repo.as_ref() else { return };
        // The list is newest first.
        let (new, old) = (a.min(b), a.max(b));
        let (Some(new), Some(old)) = (repo.commits.get(new).map(|c| c.hash.clone()), repo.commits.get(old).map(|c| c.hash.clone())) else { return };
        self.open_compare(short(&old), Some(short(&new)));
        if let Some(c) = self.compare.as_mut() {
            c.title = crate::i18n::trf("Compare {} with {}", &[&short(&new), &short(&old)]);
            c.show_files = true;
        }
    }

    /// Stash Changes…: the files selected in Changes can be the only ones put aside.
    pub fn open_stash_dialog(&mut self) {
        let mut paths: Vec<String> = self.repo.as_ref().map(|r| if r.status_multi.is_empty() { r.status_selected.iter().cloned().collect() } else { r.status_multi.iter().cloned().collect() }).unwrap_or_default();
        paths.sort();
        self.dialog = Some(Dialog::Stash { message: String::new(), untracked: false, keep_index: false, paths, only: false });
    }

    /// Show the files of a stash in the Stash tab, or hide them when they are shown.
    pub fn toggle_stash_files(&mut self, stash: &str) {
        if self.stash_open.as_ref().is_some_and(|(h, _)| h == stash) {
            self.stash_open = None;
        } else if let Some(repo) = self.repo.as_ref() {
            self.stash_open = Some((stash.to_string(), git::stash_files(&repo.path, stash)));
        }
    }

    /// Open the diff of a stash's file against what the stash was made from.
    pub fn open_stash_file(&mut self, stash: &git::Stash, f: &git::StashFile) {
        let Some(repo) = self.repo.as_ref() else { return };
        let diff = git::stash_file_diff(&repo.path, &stash.hash, f);
        let subtitle = format!("{}  ·  {} {}", f.file.path, stash.name, stash.message);
        self.push_diff_as(file_name(&f.file.path), subtitle, diff, OpenDiff::Normal);
    }

    /// Bring one file of a stash into the working folder; asks first when the file has changes
    /// of its own that would be lost.
    pub fn take_stash_file(&mut self, stash: &git::Stash, f: &git::StashFile) {
        let Some(args) = git::stash_take_args(&stash.hash, f) else { return };
        let job = Job::many("Take file from stash", vec![args]);
        let changed = self.repo.as_ref().is_some_and(|r| r.status.iter().any(|e| e.path == f.file.path));
        if changed {
            self.dialog = Some(Dialog::Confirm {
                title: tr("Take File from Stash").into(),
                text: crate::i18n::trf("'{}' has changes in the working folder. They will be replaced by the file from the stash.", &[&f.file.path]),
                ok: tr("Replace").into(),
                job: Some(job),
            });
        } else {
            self.run_job(job);
        }
    }

    /// Read everything again and wait for it: after an operation the next step may need the
    /// new list of changed files at once.
    pub fn refresh(&mut self) {
        self.refresh_within(u64::MAX);
    }

    /// Read everything again without holding the window up: what takes long (the changed
    /// files of a very big working folder) is shown when it arrives. For refreshing on the
    /// program's own initiative and by F5.
    pub fn refresh_soon(&mut self) {
        self.refresh_within(short_wait(250));
    }

    fn refresh_within(&mut self, wait: u64) {
        self.pushed_memo = None;
        if let Some(repo) = self.repo.as_mut() {
            Self::load_everything(repo, wait);
            if let Some(i) = repo.selected {
                let h = repo.commits[i].hash.clone();
                repo.details = details_of(&repo.path, &h, repo.details.as_ref());
            }
        }
        if let Some(p) = self.repo.as_ref().map(|r| r.path.clone()) {
            self.refresh_favorite(&p);
        }
    }

    /// Ctrl+click (toggle one commit) or Shift+click (a range from the current one).
    pub fn select_more(&mut self, idx: usize, range: bool) {
        let Some(repo) = self.repo.as_mut() else { return };
        let Some(anchor) = repo.selected else {
            self.select_commit(idx);
            return;
        };
        if range {
            let a = repo.visible.iter().position(|&v| v == anchor);
            let b = repo.visible.iter().position(|&v| v == idx);
            if let (Some(a), Some(b)) = (a, b) {
                repo.multi = repo.visible[a.min(b)..=a.max(b)].iter().copied().collect();
            }
        } else {
            if repo.multi.is_empty() {
                repo.multi.insert(anchor);
            }
            if !repo.multi.remove(&idx) {
                repo.multi.insert(idx);
                repo.selected = Some(idx);
            } else if repo.selected == Some(idx) {
                repo.selected = repo.multi.iter().next().copied();
            }
        }
        match repo.multi.len() {
            0 => {
                repo.selected = None;
                repo.details = None;
                Self::clear_multi(repo);
            }
            1 => {
                let one = *repo.multi.iter().next().unwrap();
                self.select_commit(one);
            }
            _ => {
                if let Some(s) = repo.selected {
                    let h = repo.commits[s].hash.clone();
                    repo.details = details_of(&repo.path, &h, repo.details.as_ref());
                }
                Self::load_multi(repo);
            }
        }
    }

    pub fn clear_multi(repo: &mut Repo) {
        repo.multi.clear();
        repo.multi_files.clear();
        repo.multi_commits.clear();
        repo.details_file = None;
    }

    /// Combine the changes of the selected commits, oldest first, file by file.
    pub fn load_multi(repo: &mut Repo) {
        repo.details_file = None;
        let hashes: Vec<String> = repo.multi.iter().take(300).map(|&i| repo.commits[i].hash.clone()).collect();
        let mut commits = vec![];
        let mut files: Vec<MultiFile> = vec![];
        for h in hashes.iter().rev() {
            let Some(d) = git::commit_details_light(&repo.path, h) else { continue };
            let base = d.parents.first().cloned();
            for f in &d.files {
                let prev = files.iter().position(|m| m.file.path == f.path).or_else(|| {
                    f.old_path.as_ref().and_then(|o| files.iter().position(|m| &m.file.path == o))
                });
                match prev {
                    None => files.push(MultiFile { file: f.clone(), base: base.clone(), head: h.clone() }),
                    Some(i) => {
                        let m = &mut files[i];
                        let status = match (m.file.status, f.status) {
                            ('A', 'D') => None,
                            ('A', _) => Some('A'),
                            ('D', 'A') => Some('M'),
                            (_, 'D') => Some('D'),
                            (_, 'R') => Some('R'),
                            (p, _) if p == 'R' => Some('R'),
                            _ => Some('M'),
                        };
                        match status {
                            None => {
                                files.remove(i);
                            }
                            Some(s) => {
                                if f.path != m.file.path {
                                    if m.file.old_path.is_none() && m.file.status != 'A' {
                                        m.file.old_path = Some(m.file.path.clone());
                                    }
                                    m.file.path = f.path.clone();
                                }
                                m.file.status = s;
                                m.head = h.clone();
                            }
                        }
                    }
                }
            }
            commits.push(d);
        }
        files.sort_by(|a, b| a.file.path.cmp(&b.file.path));
        commits.reverse();
        repo.multi_files = files;
        repo.multi_commits = commits;
    }

    pub fn open_multi_file_diff(&mut self, i: usize) {
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(m) = repo.multi_files.get(i) else { return };
        let diff = git::commit_file_diff(&repo.path, &m.head, m.base.as_deref(), &m.file);
        let title = file_name(&m.file.path);
        let subtitle = format!(
            "{}  ·  {} vs {}  ·  {} commits",
            m.file.path,
            short(&m.head),
            m.base.as_deref().map(short).unwrap_or("(empty)"),
            repo.multi.len()
        );
        let source = CompareSource::Commit { hash: m.head.clone(), parent: m.base.clone(), path: m.file.path.clone(), old_path: m.file.old_path.clone() };
        if let Some(k) = self.push_diff_as(title, subtitle, diff, OpenDiff::Normal) {
            self.diffs[k].from_commit = true;
            self.diffs[k].source = Some(source);
        }
    }

    pub fn select_commit(&mut self, idx: usize) {
        if let Some(repo) = self.repo.as_mut() {
            Self::clear_multi(repo);
            repo.selected = Some(idx);
            repo.details_file = None;
            let h = repo.commits[idx].hash.clone();
            repo.details = details_of(&repo.path, &h, repo.details.as_ref());
            // File history: the file is already picked among the commit's files.
            if let (Some(name), Some(d)) = (repo.file_names.get(&h), &repo.details) {
                repo.details_file = d.files.iter().position(|f| &f.path == name);
                repo.details_part = 0;
            }
        }
    }

    /// "Show History": a Log tab with the commits that changed the file, renames included.
    /// The history is read from HEAD, or from `rev` when the file is not in HEAD any more.
    /// "Show History for Selection": the commits that changed lines `from..=to` of the file as
    /// it is in `rev` (None = HEAD).
    pub fn open_lines_history(&mut self, path: &str, rev: Option<&str>, from: usize, to: usize) {
        if self.repo.is_none() {
            return;
        }
        let number = self.log_tabs.iter().map(|t| t.number).max().unwrap_or(1) + 1;
        let state = LogState {
            path_filter: Some(path.to_string()),
            path_input: path.to_string(),
            follow: true,
            branch_filter: rev.map(String::from),
            line_range: Some((from.max(1), to.max(from).max(1))),
            ..Default::default()
        };
        self.log_tabs.push(LogTab { number, state: Some(state) });
        self.switch_log(self.log_tabs.len() - 1);
        if let Some(&first) = self.repo.as_ref().and_then(|r| r.visible.first()) {
            self.select_commit(first);
        }
    }

    pub fn open_file_history(&mut self, path: &str, rev: Option<&str>) {
        let Some(repo) = self.repo.as_ref() else { return };
        let in_head = git::run(&repo.path, &["cat-file", "-e", &format!("HEAD:{path}")]).ok;
        let number = self.log_tabs.iter().map(|t| t.number).max().unwrap_or(1) + 1;
        let state = LogState {
            path_filter: Some(path.to_string()),
            path_input: path.to_string(),
            follow: true,
            branch_filter: rev.filter(|_| !in_head).map(String::from),
            ..Default::default()
        };
        self.log_tabs.push(LogTab { number, state: Some(state) });
        self.switch_log(self.log_tabs.len() - 1);
        // Start at the newest version of the file.
        if let Some(&first) = self.repo.as_ref().and_then(|r| r.visible.first()) {
            self.select_commit(first);
        }
    }

    pub fn navigate_to(&mut self, rev: &str) {
        let Some(repo) = self.repo.as_mut() else { return };
        let hash = git::run(
            &repo.path,
            &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")],
        )
        .stdout
        .trim()
        .to_string();
        if let Some(&i) = repo.index.get(&hash) {
            if !repo.visible.contains(&i) {
                repo.text_filter.clear();
                repo.user_filter = None;
                repo.date_filter = DateFilter::All;
                Self::apply_filters(repo);
            }
            repo.scroll_to = Some(i);
            self.select_commit(i);
        }
    }

    // ---------------------------------------------------------------- jobs

    pub fn run_job(&mut self, job: Job) {
        let dir = match (&job.dir, self.repo.as_ref()) {
            (Some(d), _) => d.clone(),
            (None, Some(repo)) => repo.path.clone(),
            (None, None) => return,
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        self.running.push(job.title.clone());
        let id = self.next_job;
        self.next_job += 1;
        let live = git::Live::default();
        self.live_jobs.push(LiveJob { id, title: job.title.clone(), live: live.clone() });
        std::thread::spawn(move || {
            let mut log = vec![];
            let mut ok = true;
            let wake = || ctx.request_repaint();
            // For "Undo": where the branches, tags and HEAD are before the job…
            let before = job.dir.is_none().then(|| crate::undo::snapshot(&dir));
            for (step, c) in job.cmds.iter().enumerate() {
                // Leading "env:NAME=value" items set environment variables for this command.
                let env: Vec<(&str, &str)> = c.iter().map_while(|s| s.strip_prefix("env:").and_then(|e| e.split_once('='))).collect();
                // Fetch, pull, push and clone tell how far they are.
                let shown: Vec<String> = git::with_progress(&c.iter().skip(env.len()).cloned().collect::<Vec<_>>());
                let args: Vec<&str> = shown.iter().map(|s| s.as_str()).collect();
                let cmd = format!("git {}", shown.join(" "));
                live.begin(&cmd);
                wake();
                let out = git::run_live(&dir, &env, &args, &live, &wake);
                live.end(out.ok, &out.text());
                log.push((cmd, out.text(), out.ok));
                if !out.ok || live.cancelled() {
                    ok = false;
                    if let Some((_, undo)) = job.rescue.as_ref().filter(|(at, _)| *at == step) {
                        for u in undo {
                            let args: Vec<&str> = u.iter().map(|s| s.as_str()).collect();
                            let cmd = format!("git {}", u.join(" "));
                            live.begin(&cmd);
                            let out = git::run_env(&dir, &[], &args);
                            live.end(out.ok, &out.text());
                            log.push((cmd, out.text(), out.ok));
                        }
                    }
                    break;
                }
            }
            // …and after it.
            let snaps = before.map(|b| (b, crate::undo::snapshot(&dir)));
            let _ = tx.send(JobDone {
                id,
                title: job.title,
                ok,
                cmds: job.cmds,
                snaps,
                cancelled: live.cancelled(),
                log,
                then: job.then,
            });
            ctx.request_repaint();
        });
    }

    /// Git → Undo: explain what will be put back and ask.
    pub fn open_undo(&mut self) {
        let (Some(rec), Some(repo)) = (self.undo.clone(), self.repo.as_ref()) else { return };
        let label = rec.label();
        if let Some(op) = repo.state.operation {
            self.dialog = Some(Dialog::Message { title: label, text: crate::i18n::trf("A {} is in progress. Finish or abort it first.", &[&op.title().to_lowercase()]) });
            return;
        }
        if !rec.still_valid(&crate::undo::snapshot(&repo.path)) {
            self.undo = None;
            self.dialog = Some(Dialog::Message { title: label, text: tr("The branches have changed since this action (in a terminal or another program), so it can no longer be undone safely. The Reflog tab (Git → Reflog) still has the earlier commits.").into() });
            return;
        }
        let steps = rec.steps();
        let mut text = String::new();
        for (what, _) in &steps {
            text.push_str(&format!("•  {what}\n"));
        }
        text.push_str(&format!("\n{}\n", tr("Commands:")));
        for (_, cmd) in &steps {
            text.push_str(&format!("git {}\n", cmd.join(" ")));
        }
        self.dialog = Some(Dialog::Confirm {
            title: label.clone(),
            text: text.trim_end().to_string(),
            ok: tr(if rec.redo { "Redo Now" } else { "Undo Now" }).into(),
            job: Some(Job::many(&label, steps.into_iter().map(|(_, c)| c).collect())),
        });
    }

    /// Stop the running jobs (the Cancel button in the status bar).
    pub fn cancel_jobs(&mut self) {
        for j in &self.live_jobs {
            j.live.cancel();
        }
    }

    /// The newest progress line of the running jobs, and the share done when git tells it.
    pub fn job_progress(&self) -> Option<(String, Option<f32>)> {
        let j = self.live_jobs.last()?;
        let line = j.live.progress();
        let share = git::progress_share(&line);
        Some((line, share))
    }

    /// Show in the console what the commands of job `id` have printed so far.
    fn sync_console(&mut self, id: u64, cmds: &[git::LiveCmd]) {
        let now = chrono::Utc::now().timestamp();
        for (step, c) in cmds.iter().enumerate() {
            let at = self.console.iter().rposition(|e| e.job == Some((id, step)));
            let e = match at {
                Some(i) => &mut self.console[i],
                None => {
                    self.console.push(ConsoleEntry { time: now, cmd: c.cmd.clone(), output: String::new(), ok: true, running: true, job: Some((id, step)) });
                    self.console.last_mut().unwrap()
                }
            };
            if e.output.len() != c.output.len() {
                e.output = c.output.clone();
            }
            e.running = c.ok.is_none();
            e.ok = c.ok.unwrap_or(true);
        }
    }

    pub fn poll_jobs(&mut self) {
        let mut any = false;
        // A job of the open repository that worked and was not about submodules itself.
        let mut moved_refs = false;
        // What the running commands have printed since the last frame.
        let live: Vec<(u64, Vec<git::LiveCmd>)> = self.live_jobs.iter().map(|j| (j.id, j.live.cmds())).collect();
        for (id, cmds) in &live {
            self.sync_console(*id, cmds);
        }
        while let Ok(done) = self.rx.try_recv() {
            any = true;
            if let Some(i) = self.running.iter().position(|t| t == &done.title) {
                self.running.remove(i);
            }
            if let Some(i) = self.live_jobs.iter().position(|j| j.id == done.id) {
                let j = self.live_jobs.remove(i);
                self.sync_console(done.id, &j.live.cmds());
            }
            // What the job moved, for Git → Undo. A push cannot be taken back from here.
            if let Some((before, after)) = done.snaps.clone() {
                if crate::undo::pushes(&done.cmds) {
                    self.undo = None;
                    self.undo_note = Some(done.title.clone());
                } else if before.busy || after.busy {
                    self.undo = None;
                    self.undo_note = None;
                } else if let Some(r) = crate::undo::Record::new(&done.title, &done.cmds, before, after) {
                    self.undo = Some(r);
                    self.undo_note = None;
                }
            }
            if done.cancelled {
                self.notify(Toast { title: crate::i18n::trf("{} cancelled", &[&done.title]), text: tr("The command was stopped.").into(), ok: false, at: Instant::now() });
                if matches!(done.then, Then::ShowCommit) {
                    self.pending_commit_msg = None;
                }
                if matches!(done.then, Then::OpenRepo) {
                    self.pending_open = None;
                }
                continue;
            }
            // Local changes stand in the way of a checkout: smart checkout, or ask what to do.
            if !done.ok {
                if let Some((args, mut files)) = done.log.last().and_then(|(cmd, out, _)| blocked_checkout(cmd, out)) {
                    if files.is_empty() {
                        // "cannot pull with rebase: You have unstaged changes" names no files.
                        files = self.repo.as_ref().map(|r| r.status.iter().filter(|e| !e.is_untracked()).map(|e| e.path.clone()).collect()).unwrap_or_default();
                    }
                    if self.settings.smart_checkout {
                        self.run_job(Job::smart_checkout(&args));
                    } else {
                        self.dialog = Some(Dialog::CheckoutProblem { args, files, always: false });
                    }
                    continue;
                }
                if let Some(args) = done.log.last().and_then(|(cmd, out, _)| divergent_pull(cmd, out)) {
                    self.open_divergent_pull(args);
                    continue;
                }
            }
            // Signing in or reaching the server went wrong: what it means and what to do, with
            // git's own words below.
            if !done.ok && self.dialog.is_none() {
                if let Some((out, (title, advice))) = done.log.last().and_then(|(_, out, _)| crate::credentials::auth_problem(out).map(|p| (out, p))) {
                    self.dialog = Some(Dialog::Output { title: tr(title).into(), note: tr(advice).into(), text: out.trim_end().to_string() });
                }
            }
            // A commit that was refused (a hook, a missing key, a wrong author): what was printed
            // is shown whole — a hook's explanation doesn't fit a notification.
            if !done.ok && self.dialog.is_none() {
                if let Some((_, out, _)) = done.log.last().filter(|(cmd, out, _)| is_commit_command(cmd) && out.lines().count() > 1) {
                    self.dialog = Some(Dialog::Output { title: tr("Commit Failed").into(), note: tr("The commit was not made. This is what git or a hook of the repository said:").into(), text: out.trim_end().to_string() });
                }
            }
            let switch = done.title.starts_with("Smart checkout ");
            let smart = switch || done.title.starts_with("Smart update") || done.title.starts_with("Smart merge ");
            let last = done.log.last().map(|l| l.1.clone()).unwrap_or_default();
            let text = if done.ok && smart {
                if switch {
                    "Your local changes were put aside, the branch was switched and the changes were applied again.".to_string()
                } else {
                    "Your local changes were put aside, the branch was updated and the changes were applied again.".to_string()
                }
            } else if smart && done.log.len() == 3 && done.log[2].0 == "git stash pop" {
                let what = if switch { "Switched, but your local changes clash with this branch." } else { "Updated, but your local changes clash with the incoming ones." };
                format!("{what} Resolve the conflicts in Changes; the stash is kept until you drop it.")
            } else if smart && done.log.len() > 3 {
                // The checkout or pull itself failed and the changes were applied back.
                format!("{}\nYour local changes were put back.", done.log[1].1.lines().take(4).collect::<Vec<_>>().join("\n"))
            } else if done.ok {
                summarize_success(&done.title, &last)
            } else {
                last.lines().take(6).collect::<Vec<_>>().join("\n")
            };
            self.notify(Toast {
                title: if done.ok {
                    done.title.clone()
                } else {
                    format!("{} failed", done.title)
                },
                text,
                ok: done.ok,
                at: Instant::now(),
            });
            // What was sent or brought changes what the hosting shows: read it again.
            if done.cmds.iter().any(|c| c.iter().any(|a| matches!(a.as_str(), "push" | "fetch" | "pull"))) {
                self.hosting.forget();
            }
            if done.ok && done.snaps.is_some() && !done.cmds.iter().any(|c| c.iter().any(|a| a == "submodule")) {
                moved_refs = true;
            }
            if done.ok {
                match done.then {
                    Then::OpenPush => {
                        self.clear_message();
                        self.refresh();
                        self.open_push_dialog();
                    }
                    Then::ClearMessage => self.clear_message(),
                    Then::ShowCommit => {
                        let msg = self.pending_commit_msg.take().unwrap_or_default();
                        if let Some(r) = self.repo.as_mut() {
                            if r.commit_msg.trim().is_empty() {
                                r.commit_msg = msg;
                            }
                        }
                        self.open_commit();
                    }
                    Then::OpenRepo => {
                        if let Some(p) = self.pending_open.take() {
                            self.open(&p);
                        }
                    }
                    Then::NewRequest => {
                        self.refresh();
                        self.hosting.forget();
                        if let Some((remote, branch)) = self.pending_request.take() {
                            self.open_new_request(&remote, &branch);
                        }
                    }
                    Then::Nothing => {}
                }
            } else if done.then == Then::NewRequest {
                self.pending_request = None;
            } else if done.then == Then::ShowCommit {
                self.pending_commit_msg = None;
            } else if done.then == Then::OpenRepo {
                self.pending_open = None;
            }
        }
        if any {
            let before = self.repo.as_ref().map(|r| r.submodules.clone()).unwrap_or_default();
            self.refresh();
            if moved_refs && self.dialog.is_none() {
                self.offer_submodule_update(&before);
            }
        }
    }

    /// Which of these files of a commit are kept in Git LFS (by the attributes of the working
    /// folder); asked from git once per list of files.
    pub fn lfs_marks(&mut self, paths: &[String]) -> HashSet<String> {
        use std::hash::{Hash, Hasher};
        let Some(repo) = self.repo.as_mut().filter(|r| r.lfs_in_use) else { return HashSet::new() };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        paths.hash(&mut h);
        let key = h.finish() | 1;
        if repo.details_lfs.0 != key {
            repo.details_lfs = (key, crate::lfs::tracked(&repo.path, paths));
        }
        repo.details_lfs.1.clone()
    }

    /// After a pull, a checkout or a merge the repository may record other commits for its
    /// submodules than are checked out in their folders: offer to bring them along.
    fn offer_submodule_update(&mut self, before: &[crate::submodule::Submodule]) {
        let Some(repo) = self.repo.as_ref() else { return };
        let behind = crate::submodule::behind_after(before, &repo.submodules);
        if behind.is_empty() {
            return;
        }
        let list = behind.iter().take(8).map(|p| format!("  {p}")).collect::<Vec<_>>().join("\n");
        let more = if behind.len() > 8 { format!("\n  … {}", behind.len() - 8) } else { String::new() };
        self.dialog = Some(Dialog::Confirm {
            title: tr("Update Submodules").into(),
            text: format!("{}\n\n{list}{more}", tr("The repository now records other commits for these submodules than are in their folders. Update them?")),
            ok: tr("Update").into(),
            job: Some(Job::many("Update submodules", crate::submodule::update_args(None, true))),
        });
    }

    fn clear_message(&mut self) {
        if let Some(r) = self.repo.as_mut() {
            r.commit_msg.clear();
            r.amend = false;
            r.unchecked.clear();
        }
        // Skipping the hooks, another author and a date are for one commit only.
        self.commit_opts = Default::default();
    }

    pub fn busy(&self) -> bool {
        !self.running.is_empty()
    }

    // ---------------------------------------------------------------- actions

    pub fn fetch(&mut self) {
        self.run_job(Job::one("Fetch", &["fetch", "--all", "--prune"]));
    }

    pub fn open_pull_dialog(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        let source = repo
            .current_branch()
            .and_then(|b| b.upstream.clone())
            .unwrap_or_else(|| {
                let r = repo.state.remotes.first().cloned().unwrap_or_else(|| "origin".into());
                format!(
                    "{r}/{}",
                    repo.state.head_branch.clone().unwrap_or_else(|| "main".into())
                )
            });
        self.dialog = Some(Dialog::Pull {
            rebase: false,
            source,
        });
    }

    pub fn open_push_dialog(&mut self) {
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(head) = repo.state.head_branch.clone() else {
            self.dialog = Some(Dialog::Message {
                title: tr("Push").into(),
                text: tr("HEAD is detached. Check out a branch before pushing.").into(),
            });
            return;
        };
        let up = repo.current_branch().and_then(|b| b.upstream.clone());
        let Some(remote) = repo.state.remotes.first().cloned() else {
            self.dialog = Some(Dialog::Message {
                title: tr("Push").into(),
                text: tr("This repository has no remotes to push to.").into(),
            });
            return;
        };
        let range = match &up {
            Some(u) => format!("{u}..HEAD"),
            None => "HEAD".into(),
        };
        let mut args = vec!["log", "--format=%H%x00%s", "-n", "200", range.as_str()];
        if up.is_none() {
            args.push("--not");
            args.push("--remotes");
        }
        let commits = git::run(&repo.path, &args)
            .stdout
            .lines()
            .filter_map(|l| l.split_once('\0').map(|(a, b)| crate::push_view::PushCommit { hash: a.to_string(), subject: b.to_string() }))
            .collect();
        let target = up.unwrap_or_else(|| format!("{remote}/{head}"));
        self.open_push_window(commits, target, head);
    }

    pub fn open_new_branch(&mut self, from: &str) {
        self.dialog = Some(Dialog::NewBranch {
            from: from.to_string(),
            prefix: self.settings.default_branch_prefix(),
            name: String::new(),
            checkout: true,
        });
    }

    pub fn checkout_branch(&mut self, name: &str, kind: RefKind) {
        let Some(repo) = self.repo.as_ref() else { return };
        if kind == RefKind::Remote {
            let local = name.split_once('/').map(|x| x.1).unwrap_or(name).to_string();
            if repo
                .state
                .branches
                .iter()
                .any(|b| b.kind == RefKind::Local && b.name == local)
            {
                self.run_job(Job::one(&format!("Checkout {local}"), &["checkout", &local]));
            } else {
                self.run_job(Job::one(&format!("Checkout {name}"), &["checkout", "--track", name]));
            }
        } else if kind == RefKind::Tag {
            self.run_job(Job::one(
                &format!("Checkout {name}"),
                &["checkout", &format!("tags/{name}")],
            ));
        } else {
            // A branch can be checked out in one working folder at a time.
            if let Some(w) = self.worktree_with(name) {
                self.dialog = Some(Dialog::Message {
                    title: tr("Checkout").into(),
                    text: crate::i18n::trf("Branch '{}' is checked out in another worktree:\n{}\n\nA branch can be open in one working folder at a time. Open that folder (Worktrees in the branch tree), or create a new branch from this one here.", &[&name, &w.path.display()]),
                });
                return;
            }
            self.run_job(Job::one(&format!("Checkout {name}"), &["checkout", name]));
        }
    }

    /// The other working folder that has the local branch `name` checked out.
    pub fn worktree_with(&self, name: &str) -> Option<git::Worktree> {
        let repo = self.repo.as_ref()?;
        repo.state.worktrees.iter().find(|w| w.branch.as_deref() == Some(name) && !git::same_dir(&w.path, &repo.path)).cloned()
    }

    pub fn is_favorite(&self, branch: &str) -> bool {
        match &self.repo {
            Some(r) => self
                .settings
                .favorites
                .contains(&format!("{}|{branch}", r.path.display())),
            None => false,
        }
    }

    pub fn toggle_favorite(&mut self, branch: &str) {
        let Some(r) = &self.repo else { return };
        let k = format!("{}|{branch}", r.path.display());
        if !self.settings.favorites.remove(&k) {
            self.settings.favorites.insert(k);
        }
        self.settings.save();
    }

    pub fn open_commit_file_diff(&mut self, file_idx: usize) {
        self.open_commit_part_diff(0, file_idx);
    }

    /// Diff of a file of the selected commit: part 0 = its own files (against the first parent),
    /// part k = "Changes to" parent k.
    pub fn open_commit_part_diff(&mut self, part: usize, file_idx: usize) {
        self.open_commit_part_diff_as(part, file_idx, OpenDiff::Normal);
    }

    pub fn open_commit_part_diff_as(&mut self, part: usize, file_idx: usize, how: OpenDiff) {
        if how == OpenDiff::Follow && !self.diffs.iter().any(|d| d.preview) {
            return;
        }
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(d) = repo.details.as_ref() else { return };
        let (f, parent) = match part {
            0 if d.is_merge() => (d.merge_files.get(file_idx), d.parents.first()),
            0 => (d.files.get(file_idx), d.parents.first()),
            k => (d.parent_files.get(k - 1).and_then(|l| l.get(file_idx)), d.parents.get(k - 1)),
        };
        let Some(f) = f else { return };
        let source = CompareSource::Commit { hash: d.hash.clone(), parent: parent.cloned(), path: f.path.clone(), old_path: f.old_path.clone() };
        // A double-click follows the file-type rules of Settings → Diff & Merge Tools.
        if how == OpenDiff::DoubleClick {
            if let Some(tool) = crate::external::tool_for(&self.settings, &f.path, crate::settings::ExtKind::Diff).cloned() {
                self.open_in_external(&source, &tool);
                return;
            }
        }
        let diff = git::commit_file_diff(&repo.path, &d.hash, parent.map(|s| s.as_str()), f);
        let title = file_name(&f.path);
        let subtitle = format!(
            "{}  ·  {} vs {}",
            f.path,
            short(&d.hash),
            parent.map(|p| short(p)).unwrap_or("(empty)")
        );
        if let Some(i) = self.push_diff_as(title, subtitle, diff, how) {
            self.diffs[i].from_commit = true;
            self.diffs[i].source = Some(source);
        }
    }

    pub fn open_worktree_diff(&mut self, path: &str) {
        self.open_worktree_diff_as(path, OpenDiff::Normal);
    }

    pub fn open_worktree_diff_as(&mut self, path: &str, how: OpenDiff) {
        if how == OpenDiff::Follow && !self.diffs.iter().any(|d| d.preview) {
            return;
        }
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(e) = repo.status.iter().find(|e| e.path == path) else { return };
        let has_head = !repo.state.head_hash.is_empty();
        let source = CompareSource::Worktree { path: e.path.clone(), old_path: e.old_path.clone(), tracked: has_head && !e.is_untracked() && e.x != 'A' };
        // A double-click follows the file-type rules of Settings → Diff & Merge Tools.
        if how == OpenDiff::DoubleClick {
            if let Some(tool) = crate::external::tool_for(&self.settings, path, crate::settings::ExtKind::Diff).cloned() {
                self.open_in_external(&source, &tool);
                return;
            }
        }
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(e) = repo.status.iter().find(|e| e.path == path) else { return };
        let diff = git::worktree_diff(&repo.path, e, has_head);
        let title = file_name(&e.path);
        let subtitle = format!("{}  ·  HEAD vs Local Changes", e.path);
        let path = e.path.clone();
        // Renamed files are committed whole (their two paths make partial commits unclear).
        // And single changes are picked from the exact diff, not from one that leaves white space out.
        let partial_ok = e.old_path.is_none() && git::diff_ws() == 0 && !repo.submodules.iter().any(|m| m.path == e.path) && !repo.lfs_paths.contains(&e.path);
        if let Some(i) = self.push_diff_as(title, subtitle, diff, how) {
            self.diffs[i].worktree = partial_ok.then_some(path);
            self.diffs[i].from_commit = false;
            self.diffs[i].source = Some(source);
        }
    }

    /// Show a comparison in an external tool: the old version as a copy, the new one as the
    /// file itself when it is in the working folder (so changes made in the tool are kept).
    pub fn open_in_external(&mut self, src: &CompareSource, tool: &crate::settings::ExtTool) {
        let Some(dir) = self.repo.as_ref().map(|r| r.path.clone()) else { return };
        let res = (|| -> Result<String, String> {
            use crate::external::write_copy;
            let scratch = crate::external::scratch_dir();
            let (left, right) = match src {
                CompareSource::Worktree { path, old_path, tracked } => {
                    let old = if *tracked { git::blob(&dir, "HEAD", old_path.as_deref().unwrap_or(path)).unwrap_or_default() } else { vec![] };
                    let left = write_copy(&scratch, "HEAD", path, &old)?;
                    let here = dir.join(path);
                    let right = if here.is_file() { here } else { write_copy(&scratch, "deleted", path, b"")? };
                    (left, right)
                }
                CompareSource::Commit { hash, parent, path, old_path } => {
                    let old = parent.as_deref().and_then(|p| git::blob(&dir, p, old_path.as_deref().unwrap_or(path))).unwrap_or_default();
                    let new = git::blob(&dir, hash, path).unwrap_or_default();
                    let left = write_copy(&scratch, parent.as_deref().map(short).unwrap_or("empty"), path, &old)?;
                    let right = write_copy(&scratch, short(hash), path, &new)?;
                    (left, right)
                }
            };
            crate::external::launch(tool, [Some(&left), Some(&right), None, None], None)
        })();
        self.report_external(tool, res);
    }

    /// Open a file of the working folder for editing (at `line` when the program allows it).
    pub fn open_file_at(&mut self, path: &std::path::Path, line: Option<usize>) {
        if let Err(e) = crate::external::open_file(path, line, &self.settings.editor_cmd) {
            self.notify(Toast { title: tr("Open file failed").into(), text: e, ok: false, at: Instant::now() });
        }
    }

    /// Several selected commits: a double-click on a file uses the external diff tool for its
    /// type, if there is one.
    pub fn try_external_multi_diff(&mut self, i: usize) -> bool {
        let Some(m) = self.repo.as_ref().and_then(|r| r.multi_files.get(i)) else { return false };
        let Some(tool) = crate::external::tool_for(&self.settings, &m.file.path, crate::settings::ExtKind::Diff).cloned() else { return false };
        let source = CompareSource::Commit { hash: m.head.clone(), parent: m.base.clone(), path: m.file.path.clone(), old_path: m.file.old_path.clone() };
        self.open_in_external(&source, &tool);
        true
    }

    /// A conflicted file in the external merge tool chosen for its type: ours (%1), theirs
    /// (%2), base (%3) and the file itself as the result (%4). False when no tool is set.
    pub fn try_external_merge(&mut self, path: &str) -> bool {
        let Some(tool) = crate::external::tool_for(&self.settings, path, crate::settings::ExtKind::Merge).cloned() else { return false };
        self.external_merge_with(path, &tool)
    }

    /// A conflicted file in the given external merge tool (see [`Self::try_external_merge`]).
    pub fn external_merge_with(&mut self, path: &str, tool: &crate::settings::ExtTool) -> bool {
        let tool = tool.clone();
        let Some(dir) = self.repo.as_ref().map(|r| r.path.clone()) else { return false };
        let res = (|| -> Result<String, String> {
            use crate::external::write_copy;
            let scratch = crate::external::scratch_dir();
            let ours = write_copy(&scratch, "ours", path, &git::blob(&dir, ":2", path).unwrap_or_default())?;
            let theirs = write_copy(&scratch, "theirs", path, &git::blob(&dir, ":3", path).unwrap_or_default())?;
            let base = git::blob(&dir, ":1", path).map(|b| write_copy(&scratch, "base", path, &b)).transpose()?;
            let out = dir.join(path);
            crate::external::launch(&tool, [Some(&ours), Some(&theirs), base.as_deref(), Some(&out)], None)
        })();
        let ok = res.is_ok();
        self.report_external(&tool, res);
        if ok {
            self.notify(Toast {
                title: format!("{} opened", tool.name),
                text: crate::i18n::trf("Save the merged {} there, then choose Mark as Resolved in the Conflicts window.", &[&file_name(path)]),
                ok: true,
                at: Instant::now(),
            });
        }
        true
    }

    fn report_external(&mut self, tool: &crate::settings::ExtTool, res: Result<String, String>) {
        match res {
            Ok(cmd) => self.console.push(ConsoleEntry { time: chrono::Local::now().timestamp(), cmd: format!("({}) {cmd}", tool.name), output: String::new(), ok: true, running: false, job: None }),
            Err(e) => self.notify(Toast { title: format!("{} could not start", tool.name), text: e, ok: false, at: Instant::now() }),
        }
    }

    pub fn push_diff(&mut self, title: String, subtitle: String, diff: git::DiffText) {
        self.push_diff_as(title, subtitle, diff, OpenDiff::Normal);
    }

    /// Show a diff in a tab; returns the tab used.
    pub fn push_diff_as(&mut self, title: String, subtitle: String, diff: git::DiffText, how: OpenDiff) -> Option<usize> {
        let jump = diff.rows.iter().position(|r| r.kind != git::DiffSide::Context);
        let same = self.diffs.iter().position(|d| d.subtitle == subtitle);
        let preview = self.diffs.iter().position(|d| d.preview);
        // Reuse a tab: the one already showing this diff, or the quick-view tab.
        let reuse = match how {
            OpenDiff::Normal => same,
            OpenDiff::DoubleClick => same.or(preview),
            // A file that has its own ordinary tab is not copied into the quick view.
            // Nor is the file the quick view already shows loaded again.
            OpenDiff::Follow => match same {
                Some(_) => return None,
                None => preview,
            },
        };
        if let Some(i) = reuse {
            let d = &mut self.diffs[i];
            // The first click of a double-click lands just before it (within the double-click time).
            let same_gesture = d.preview_followed.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(450));
            if how == OpenDiff::DoubleClick && same == Some(i) && d.preview && !same_gesture {
                // Double-click on the file the quick view shows: keep the tab.
                d.preview = false;
            }
            d.preview_followed = (how == OpenDiff::Follow).then(Instant::now);
            if d.subtitle != subtitle {
                d.title = title;
                d.subtitle = subtitle;
                d.worktree = None;
                d.hscroll = 0.0;
            }
            d.diff = diff;
            // The file may have changed since it was annotated.
            d.blame = None;
            d.syntax = None;
            if let Some(f) = d.find.as_mut() {
                f.cache = None;
            }
            d.jump = jump;
            d.image = None;
            d.text_sel = None;
            d.expanded.clear();
            if how != OpenDiff::Follow {
                self.tab = Tab::Diff(i);
            }
            return Some(i);
        }
        if how == OpenDiff::Follow {
            return None;
        }
        self.diffs.push(DiffTab {
            title,
            subtitle,
            diff,
            side_by_side: true,
            jump,
            image: None,
            worktree: None,
            collapse: true,
            expanded: HashSet::new(),
            preview: how == OpenDiff::DoubleClick,
            preview_followed: None,
            split: 0.5,
            hscroll: 0.0,
            from_commit: false,
            source: None,
            text_sel: None,
            find: None,
            syntax: None,
            blame: None,
        });
        self.tab = Tab::Diff(self.diffs.len() - 1);
        Some(self.diffs.len() - 1)
    }

    /// Open Log tab `i` (its own filters and selection).
    pub fn switch_log(&mut self, i: usize) {
        self.tab = Tab::Log;
        if i == self.active_log || i >= self.log_tabs.len() {
            return;
        }
        let Some(repo) = self.repo.as_mut() else { return };
        self.log_tabs[self.active_log].state = Some(repo.take_log());
        let next = self.log_tabs[i].state.take().unwrap_or_default();
        repo.put_log(next);
        self.active_log = i;
        // The repository may have changed while the tab was in the background.
        App::reload_log(repo);
    }

    /// "+": another Log tab showing all branches, with the default log options.
    pub fn new_log_tab(&mut self) {
        let number = self.log_tabs.iter().map(|t| t.number).max().unwrap_or(1) + 1;
        let state = LogState {
            topo: self.settings.log_topo,
            first_parent: self.settings.log_first_parent,
            no_merges: self.settings.log_no_merges,
            ..Default::default()
        };
        self.log_tabs.push(LogTab { number, state: Some(state) });
        self.switch_log(self.log_tabs.len() - 1);
    }

    /// Close an extra Log tab (the first one stays).
    pub fn close_log_tab(&mut self, i: usize) {
        if i == 0 || i >= self.log_tabs.len() {
            return;
        }
        if i == self.active_log {
            let on_log = self.tab == Tab::Log;
            self.switch_log(i - 1);
            if !on_log {
                self.tab = Tab::Log;
            }
        }
        self.log_tabs.remove(i);
        if self.active_log > i {
            self.active_log -= 1;
        }
    }

    /// "Log", or for the extra tabs "Log: <branch>" when filtered by a branch, else "Log-<n>".
    pub fn log_tab_title(&self, i: usize) -> String {
        if i == 0 {
            return crate::i18n::tr("Log").into();
        }
        let history = if i == self.active_log {
            self.repo.as_ref().filter(|r| r.follow).and_then(|r| r.path_filter.clone().map(|p| (p, r.line_range)))
        } else {
            self.log_tabs[i].state.as_ref().filter(|s| s.follow).and_then(|s| s.path_filter.clone().map(|p| (p, s.line_range)))
        };
        if let Some((p, lines)) = history {
            let lines = match lines {
                Some((a, b)) if a == b => format!(":{a}"),
                Some((a, b)) => format!(":{a}-{b}"),
                None => String::new(),
            };
            return format!("{}: {}{lines}", crate::i18n::tr("History"), file_name(&p));
        }
        let filter = if i == self.active_log {
            self.repo.as_ref().and_then(|r| r.branch_filter.clone())
        } else {
            self.log_tabs[i].state.as_ref().and_then(|s| s.branch_filter.clone())
        };
        let head = self.repo.as_ref().and_then(|r| r.state.head_branch.clone()).unwrap_or_else(|| "HEAD".into());
        match filter.filter(|b| !b.is_empty()) {
            Some(b) if b == "HEAD" => format!("Log: {head}"),
            Some(b) => format!("Log: {b}"),
            None => format!("Log-{}", self.log_tabs[i].number),
        }
    }

    pub fn close_diff(&mut self, i: usize) {
        if i < self.diffs.len() {
            self.diffs.remove(i);
        }
        self.tab = match self.tab {
            Tab::Diff(j) if j == i => {
                if self.diffs.is_empty() {
                    Tab::Log
                } else {
                    Tab::Diff(j.min(self.diffs.len() - 1))
                }
            }
            Tab::Diff(j) if j > i => Tab::Diff(j - 1),
            t => t,
        };
    }

    pub fn choose_folder(&mut self) {
        let start = self
            .repo
            .as_ref()
            .and_then(|r| r.path.parent().map(|p| p.to_path_buf()))
            .or_else(git::home_dir);
        let mut dlg = rfd::FileDialog::new().set_title("Open Git Repository");
        if let Some(s) = start {
            dlg = dlg.set_directory(s);
        }
        if let Some(p) = dlg.pick_folder() {
            self.open(&p);
        }
    }

    /// Show a pop-up notification and keep it in the bell's list.
    pub fn notify(&mut self, t: Toast) {
        self.notices.insert(
            0,
            Notice { title: t.title.clone(), text: t.text.clone(), ok: t.ok, when: chrono::Local::now().timestamp() },
        );
        self.notices.truncate(100);
        self.unread += 1;
        self.toasts.push(t);
    }

    pub fn install_integration(&mut self) {
        let text = match crate::integrate::install() {
            Ok(t) => t,
            Err(e) => format!("Could not set it up: {e}"),
        };
        self.dialog = Some(Dialog::Message {
            title: if cfg!(windows) { "Explorer integration".into() } else { "Files app integration".into() },
            text,
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.screenshot.is_some() {
            raw_input.events.extend(screenshot_events());
        }
    }
}

/// Mouse events for screenshot mode, one call per frame: `REFBRAID_POINTER` keeps the mouse
/// still; `REFBRAID_DRAG=x1,y1,x2,y2` presses at the first point on frame 40, drags to the
/// second over ten frames and lets go (to capture text selection and the like).
pub fn screenshot_events() -> Vec<egui::Event> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static FRAME: AtomicUsize = AtomicUsize::new(0);
    let f = FRAME.fetch_add(1, Ordering::Relaxed);
    // REFBRAID_RCLICK=x,y: a right-click there on frame 40 (to capture context menus).
    if let Some((x, y)) = std::env::var("REFBRAID_RCLICK").ok().and_then(|v| v.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?)))) {
        let p = egui::pos2(x, y);
        let button = |pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE };
        return match f {
            40 => vec![egui::Event::PointerMoved(p), button(true)],
            41 => vec![button(false)],
            _ => vec![egui::Event::PointerMoved(p)],
        };
    }
    if let Some(v) = std::env::var("REFBRAID_DRAG").ok() {
        let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if n.len() == 4 {
            let (a, b) = (egui::pos2(n[0], n[1]), egui::pos2(n[2], n[3]));
            let button = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            return match f {
                0..=39 => vec![egui::Event::PointerMoved(a)],
                40 => vec![egui::Event::PointerMoved(a), button(a, true)],
                41..=50 => {
                    let t = (f - 40) as f32 / 10.0;
                    vec![egui::Event::PointerMoved(a + (b - a) * t)]
                }
                51 => vec![button(b, false)],
                _ => vec![],
            };
        }
    }
    screenshot_pointer().map(|p| vec![egui::Event::PointerMoved(p)]).unwrap_or_default()
}

/// Screenshot mode: `REFBRAID_POINTER=x,y` keeps the mouse there, to capture hover effects
/// and tooltips.
pub fn screenshot_pointer() -> Option<egui::Pos2> {
    let v = std::env::var("REFBRAID_POINTER").ok()?;
    let (x, y) = v.split_once(',')?;
    Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

impl App {
    /// Draws the whole window (also used by the off-screen fallback in screenshot mode).
    /// How diffs treat white space (see `git::set_diff_ws`): remembered, and the open diffs
    /// are read again that way.
    pub fn set_diff_ws(&mut self, mode: u8) {
        git::set_diff_ws(mode);
        self.settings.diff_ws = mode.min(2);
        if self.screenshot.is_none() {
            self.settings.save();
        }
        for i in 0..self.diffs.len() {
            self.reload_diff(i);
        }
    }

    /// Read a diff tab's comparison again (pictures are left as they are).
    pub fn reload_diff(&mut self, i: usize) {
        let Some(repo) = self.repo.as_ref() else { return };
        let Some(d) = self.diffs.get(i) else { return };
        if d.diff.images.is_some() {
            return;
        }
        let (diff, partial) = match d.source.clone() {
            Some(CompareSource::Worktree { path, .. }) => {
                let Some(e) = repo.status.iter().find(|e| e.path == path) else { return };
                let has_head = !repo.state.head_hash.is_empty();
                (git::worktree_diff(&repo.path, e, has_head), (e.old_path.is_none() && git::diff_ws() == 0 && !repo.submodules.iter().any(|m| m.path == e.path) && !repo.lfs_paths.contains(&e.path)).then_some(path))
            }
            Some(CompareSource::Commit { hash, parent, path, old_path }) => {
                let f = git::ChangedFile { status: if old_path.is_some() { 'R' } else { 'M' }, path, old_path };
                (git::commit_file_diff(&repo.path, &hash, parent.as_deref(), &f), None)
            }
            None => return,
        };
        let d = &mut self.diffs[i];
        d.jump = diff.rows.iter().position(|r| r.kind != git::DiffSide::Context);
        d.diff = diff;
        if d.worktree.is_some() || partial.is_some() {
            d.worktree = partial;
        }
        d.syntax = None;
        d.blame = None;
        d.text_sel = None;
        d.expanded.clear();
        if let Some(f) = d.find.as_mut() {
            f.cache = None;
        }
    }

    /// Do what an action of the keymap table (`keymap::ACTIONS`) stands for.
    pub fn run_action(&mut self, id: &str) {
        let has_repo = self.repo.is_some();
        if crate::keymap::action(id).is_some_and(|a| a.repo && !has_repo) {
            return;
        }
        match id {
            "palette" => self.dialog = Some(Dialog::Palette { query: String::new(), at: 0, branches: false }),
            "checkout" => self.dialog = Some(Dialog::Palette { query: String::new(), at: 0, branches: true }),
            "open" => self.choose_folder(),
            "clone" => self.open_clone(),
            "settings" => self.show_settings = true,
            "shortcuts" => self.shortcuts(),
            "next_repo" | "prev_repo" => {
                let n = self.tab_order.len();
                let at = self.repo.as_ref().and_then(|r| self.tab_order.iter().position(|p| *p == r.path));
                if let (Some(at), true) = (at, n > 1) {
                    let next = if id == "next_repo" { (at + 1) % n } else { (at + n - 1) % n };
                    let p = self.tab_order[next].clone();
                    self.open(&p);
                }
            }
            "close_repo" => {
                if let Some(p) = self.repo.as_ref().map(|r| r.path.clone()) {
                    self.close_repo_tab(&p);
                }
            }
            "close_tab" => match self.tab {
                Tab::Diff(i) => self.close_diff(i),
                Tab::Blame(i) => self.close_blame(i),
                _ => {}
            },
            "commit" => self.open_commit(),
            "push" => self.open_push_dialog(),
            "pull" => self.open_pull_dialog(),
            "fetch" => self.fetch(),
            "new_branch" => {
                let from = self.repo.as_ref().and_then(|r| r.state.head_branch.clone()).unwrap_or_else(|| "HEAD".into());
                self.open_new_branch(&from);
            }
            "stash" => self.open_stash_dialog(),
            "undo" => {
                if !self.busy() && self.undo.is_some() {
                    self.open_undo();
                }
            }
            "add_to_git" => self.add_selected_to_git(),
            "rename_branch" => {
                if let Some(b) = self.selected_tree_branch().filter(|b| b.kind == RefKind::Local) {
                    self.dialog = Some(Dialog::RenameBranch { old: b.name.clone(), new: b.name.clone() });
                }
            }
            "reflog" => self.open_reflog(),
            "bisect" => self.open_bisect_dialog(None, None),
            "refresh" => self.refresh_soon(),
            "find" => self.find_action(),
            "focus_log" => {
                self.tab = Tab::Log;
                self.key_focus = KeyFocus::Log;
            }
            "focus_branches" => {
                self.tab = Tab::Log;
                self.show_branches = true;
                self.key_focus = KeyFocus::Branches;
                if let Some(r) = self.repo.as_mut() {
                    if r.tree_selected.is_none() {
                        r.tree_selected = Some("HEAD".into());
                    }
                }
            }
            "focus_changes" => {
                self.show_commit = true;
                self.commit_tab = CommitTab::Changes;
                self.key_focus = KeyFocus::Changes;
            }
            "console" => self.tab = Tab::Console,
            _ => {}
        }
    }

    /// The shortcut of an action as it is now, for menus and hints ("" when it has none).
    pub fn key_of(&self, id: &str) -> String {
        crate::keymap::shown(&crate::keymap::binding(&self.settings.keys, id)).into_owned()
    }

    /// A menu line: the action's name and, after a gap, its shortcut.
    pub fn menu_line(&self, id: &str) -> String {
        let name = crate::keymap::action(id).map(|a| tr(a.name)).unwrap_or(id);
        match self.key_of(id).as_str() {
            "" => name.to_string(),
            k => format!("{name}    {k}"),
        }
    }

    /// A hint: the given words and the action's shortcut in brackets.
    pub fn tip_with_key(&self, text: &str, id: &str) -> String {
        match self.key_of(id).as_str() {
            "" => text.to_string(),
            k => format!("{text}  ({k})"),
        }
    }

    /// The theme chosen in the settings (or the system's) is the one shown.
    fn sync_theme(&self, ctx: &egui::Context) {
        let want = match std::env::var("REFBRAID_THEME").ok().as_deref() {
            Some("light") => true,
            Some("dark") => false,
            _ => wants_light(self.settings.theme, ctx.input(|i| i.raw.system_theme)),
        };
        if want != theme::is_light() {
            theme::set_light(ctx, want);
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.sync_theme(&ctx);
        self.remember_maximized(&ctx);
        self.poll_jobs();
        self.update_tick(&ctx);
        self.poll_secrets();
        self.poll_log();
        self.poll_refs();
        if let Some(repo) = self.repo.as_mut() {
            Self::take_pending_status(repo);
        }
        self.handle_keys(&ctx);
        self.take_dropped_patch(&ctx);
        if self.screenshot.is_none() && !self.settings.no_auto_refresh {
            self.watch(&ctx);
        }

        egui::Panel::top("title")
            .exact_size(40.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(theme::BG_BAR()))
            .show(ui, |ui| self.title_bar(ui));
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG_BAR()))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("stripe")
            .exact_size(36.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::BG_BAR()))
            .show(ui, |ui| self.left_stripe(ui));
        // The repository tabs start after the stripe, which runs up to the title bar.
        if self.settings.shows_repo_tabs(self.tab_order.len()) {
            egui::Panel::top("repo_tabs")
                .exact_size(37.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::BG()))
                .show(ui, |ui| {
                    let r = ui.max_rect();
                    theme::hline(ui, r.bottom() - 0.5, r.left(), r.right());
                    // The same gap above the tabs and between them and the line below.
                    let inner = egui::Rect::from_min_max(egui::pos2(r.left(), r.top() + 6.0), egui::pos2(r.right(), r.bottom() - 7.0));
                    let mut tabs = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    self.repo_tabs(&mut tabs)
                });
        }

        // Favorites sit at the far left, before the Commit panel.
        if self.show_favorites {
            let w = if self.settings.favorites_width > 0.0 { self.settings.favorites_width } else { 300.0 };
            let r = egui::Panel::left("favorites")
                .default_size(w)
                .min_size(220.0)
                // Side panels give way so the Git window keeps room (a remembered width from a
                // wider screen must not squeeze it to nothing).
                .max_size((ui.available_width() - 600.0).max(220.0))
                .resizable(true)
                .frame(egui::Frame::new().fill(theme::BG()).stroke(Stroke::new(1.0, theme::BORDER())))
                .show(ui, |ui| self.favorites_panel(ui));
            let w = r.response.rect.width();
            self.remember_width(ui, w, |s| &mut s.favorites_width);
        }
        if self.repo.is_none() {
            let r = egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::BG()))
                .show(ui, |ui| self.welcome(ui));
            if self.show_favorites {
                let top = r.response.rect;
                ui.painter().hline(top.x_range(), top.top() + 0.5, Stroke::new(1.0, theme::BORDER()));
            }
        } else {
            if self.show_commit {
                let w = if self.settings.commit_width > 0.0 { self.settings.commit_width } else { 390.0 };
                let r = egui::Panel::left("commit")
                    .default_size(w)
                    .min_size(280.0)
                    .max_size((ui.available_width() - 560.0).max(280.0))
                    .resizable(true)
                    .frame(
                        egui::Frame::new()
                            .fill(theme::BG())
                            .stroke(Stroke::new(1.0, theme::BORDER()))
                            // Next to Favorites the two panels' borders would stand side by
                            // side and make that splitter wider than the others: they share one.
                            .outer_margin(egui::Margin { left: if self.show_favorites { -1 } else { 0 }, ..Default::default() }),
                    )
                    .show(ui, |ui| self.commit_panel(ui));
                let w = r.response.rect.width();
                self.remember_width(ui, w, |s| &mut s.commit_width);
            }
            let r = egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::BG()))
                .show(ui, |ui| self.tool_window(ui));
            // Same top border as the Favorites and Commit panels next to it (their frames add
            // a line under the title bar's own), so the tops line up.
            let top = r.response.rect;
            ui.painter().hline(top.x_range(), top.top() + 0.5, Stroke::new(1.0, theme::BORDER()));
        }

        self.poll_ai();
        self.poll_conflict_ai();
        self.poll_favorites();
        self.take_compare_request(&ctx);
        self.dialogs(&ctx);
        self.git_config_window(&ctx);
        self.push_window(&ctx);
        self.split_window(&ctx);
        self.watch_conflicts();
        self.conflicts_window(&ctx);
        self.merge_window(&ctx);
        // Screenshot helper: show the current branch's menu (REFBRAID_VIEW=branchmenu).
        if self.screenshot.is_some() && std::env::var("REFBRAID_VIEW").map(|v| v.contains("branchmenu")).unwrap_or(false) {
            let cur = self.repo.as_ref().and_then(|r| r.state.head_branch.clone());
            // With `mrdemo`: the menu of the remote branch that has a merge request.
            let other = std::env::var("REFBRAID_VIEW").is_ok_and(|v| v.contains("mrdemo"));
            let shown = if other { Some((MR_DEMO_BRANCH.to_string(), crate::git::RefKind::Remote)) } else { cur.clone().map(|n| (n, crate::git::RefKind::Local)) };
            if let Some((name, kind)) = shown {
                egui::Area::new(egui::Id::unique("demo_branch_menu"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(230.0, 160.0))
                    .show(&ctx, |ui| {
                        egui::Frame::menu(ui.style()).show(ui, |ui| {
                            egui::containers::menu::menu_style(ui.style_mut());
                            self.branch_actions(ui, &name, kind, cur.as_deref());
                        });
                    });
            }
        }
        // Screenshot helper: show the project menu with recent repositories (REFBRAID_VIEW=repomenu).
        if self.screenshot.is_some() && std::env::var("REFBRAID_VIEW").map(|v| v.contains("repomenu")).unwrap_or(false) {
            if self.settings.recent.len() < 3 {
                self.settings.recent = ["/tmp/refbraid-demo/sample-project", "/home/user/work/shop/shop-backend-api", "/home/user/work/shop/common", "/home/user/work/shop/shop-frontend-storefront-donation_hall", "/home/user/projects/refbraid"]
                    .iter()
                    .map(std::path::PathBuf::from)
                    .collect();
            }
            egui::Area::new(egui::Id::unique("demo_repo_menu"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(210.0, 36.0))
                .show(&ctx, |ui| {
                    egui::Frame::menu(ui.style()).show(ui, |ui| {
                        egui::containers::menu::menu_style(ui.style_mut());
                        self.repo_menu(ui);
                    });
                });
        }
        // Screenshot helper: show the branch list under the branch name (REFBRAID_VIEW=branchpopup).
        if self.screenshot.is_some() && std::env::var("REFBRAID_VIEW").map(|v| v.contains("branchpopup")).unwrap_or(false) {
            egui::Area::new(egui::Id::unique("demo_branch_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(340.0, 36.0))
                .show(&ctx, |ui| {
                    egui::Frame::menu(ui.style()).show(ui, |ui| {
                        egui::containers::menu::menu_style(ui.style_mut());
                        ui.set_max_width(340.0);
                        self.branches_popup(ui);
                    });
                });
        }
        // Screenshot helper: a commit's context menu in the Log, on a commit several branches
        // end at (REFBRAID_VIEW=revmenu).
        if self.screenshot.is_some() && std::env::var("REFBRAID_VIEW").map(|v| v.contains("revmenu")).unwrap_or(false) {
            let pick = self.repo.as_ref().and_then(|r| {
                let tips = |h: &str| r.state.branches.iter().filter(|b| b.hash == h && !b.is_head && !b.name.ends_with("/HEAD")).count();
                let best = r.commits.iter().enumerate().max_by_key(|(i, c)| (tips(&c.hash), std::cmp::Reverse(*i)))?;
                Some(best.0)
            });
            if let Some(idx) = pick {
                egui::Area::new(egui::Id::unique("demo_rev_menu"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(560.0, 140.0))
                    .show(&ctx, |ui| {
                        egui::Frame::menu(ui.style()).show(ui, |ui| {
                            egui::containers::menu::menu_style(ui.style_mut());
                            self.commit_actions(ui, idx);
                        });
                    });
            }
        }
        // Screenshot helper: show the log's sort menu (REFBRAID_VIEW=sortmenu).
        if self.screenshot.is_some() && std::env::var("REFBRAID_VIEW").map(|v| v.contains("sortmenu")).unwrap_or(false) {
            if let Some(repo) = self.repo.as_mut() {
                egui::Area::new(egui::Id::unique("demo_sort_menu"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(560.0, 160.0))
                    .show(&ctx, |ui| {
                        egui::Frame::menu(ui.style()).show(ui, |ui| {
                            egui::containers::menu::menu_style(ui.style_mut());
                            crate::log_view::sort_menu(ui, repo);
                        });
                    });
            }
        }
        self.ai_windows(&ctx);
        self.settings_window(&ctx);
        self.branch_windows(&ctx);
        self.lfs_window(&ctx);
        self.new_request_window(&ctx);
        self.branch_requests_window(&ctx);
        self.update_window(&ctx);
        self.toasts(&ctx);
        self.window_edges(&ctx);
        self.screenshot_step(&ctx);
    }
}

pub fn short(h: &str) -> &str {
    &h[..h.len().min(8)]
}

pub fn file_name(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

fn summarize_success(title: &str, out: &str) -> String {
    let first = out
        .lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with("hint:"))
        .unwrap_or("")
        .trim()
        .to_string();
    if first.is_empty() && title == "Fetch" {
        return "Everything is up to date.".into();
    }
    first.chars().take(160).collect()
}

pub fn format_date(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_opt(ts, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let d = dt.date_naive();
    if d == today {
        format!("Today {}", dt.format("%H:%M"))
    } else if Some(d) == today.pred_opt() {
        format!("Yesterday {}", dt.format("%H:%M"))
    } else {
        dt.format("%d.%m.%Y %H:%M").to_string()
    }
}

pub fn format_full_date(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%d.%m.%Y at %H:%M").to_string())
        .unwrap_or_default()
}

pub fn status_color(c: char) -> Color32 {
    match c {
        'A' => theme::GREEN(),
        'D' => theme::TEXT_DIM(),
        '?' => theme::UNVERSIONED(),
        'U' => theme::RED(),
        _ => theme::BLUE_FILE(),
    }
}

/// Hide merge commits and/or linear stretches of side branches, reconnecting the graph
/// through the hidden commits. Returns the remaining commits and those whose lines were shortened.
fn simplify(
    commits: Vec<Commit>,
    no_merges: bool,
    collapse_linear: bool,
    state: &RepoState,
) -> (Vec<Commit>, HashSet<String>) {
    let index: HashMap<String, usize> = commits.iter().enumerate().map(|(i, c)| (c.hash.clone(), i)).collect();
    let mut children = vec![0usize; commits.len()];
    for c in &commits {
        for p in &c.parents {
            if let Some(&i) = index.get(p.as_str()) {
                children[i] += 1;
            }
        }
    }
    // The current branch's own line always stays fully visible.
    let mut mainline: HashSet<usize> = HashSet::new();
    let mut cur = index.get(state.head_hash.as_str()).copied();
    while let Some(i) = cur {
        if !mainline.insert(i) {
            break;
        }
        cur = commits[i].parents.first().and_then(|p| index.get(p.as_str()).copied());
    }
    let hidden: Vec<bool> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if no_merges && c.parents.len() > 1 {
                return true;
            }
            collapse_linear
                && c.parents.len() == 1
                && children[i] == 1
                && !mainline.contains(&i)
                && !state.labels.contains_key(&c.hash)
                && index.contains_key(c.parents[0].as_str())
        })
        .collect();
    // Parents come after children in the list, so resolve from the bottom up.
    let mut resolved: Vec<Option<Vec<String>>> = vec![None; commits.len()];
    let expand = |parents: &[String], resolved: &Vec<Option<Vec<String>>>| -> (Vec<String>, bool) {
        let mut out: Vec<String> = vec![];
        let mut changed = false;
        for p in parents {
            match index.get(p.as_str()) {
                Some(&j) if hidden[j] => {
                    changed = true;
                    for q in resolved[j].clone().unwrap_or_default() {
                        if !out.contains(&q) {
                            out.push(q);
                        }
                    }
                }
                _ => {
                    if !out.contains(p) {
                        out.push(p.clone());
                    }
                }
            }
        }
        (out, changed)
    };
    for i in (0..commits.len()).rev() {
        if hidden[i] {
            let (ps, _) = expand(&commits[i].parents, &resolved);
            resolved[i] = Some(ps);
        }
    }
    let mut dashed = HashSet::new();
    let mut out = Vec::with_capacity(commits.len());
    for (i, mut c) in commits.into_iter().enumerate() {
        if hidden[i] {
            continue;
        }
        let (ps, changed) = expand(&c.parents, &resolved);
        if changed {
            dashed.insert(c.hash.clone());
        }
        c.parents = ps;
        out.push(c);
    }
    (out, dashed)
}

/// A failed `git checkout …`, `git pull …` or `git merge …` because local changes are in the
/// way: the command's arguments and the files git listed (none for a rebasing pull).
pub fn blocked_checkout(cmd: &str, out: &str) -> Option<(Vec<String>, Vec<String>)> {
    let args: Vec<String> = cmd.strip_prefix("git ")?.split_whitespace().map(String::from).collect();
    let head = match args.first().map(String::as_str) {
        Some("checkout") if !args.iter().any(|a| a == "-f" || a == "--force" || a == "--") => "Your local changes to the following files would be overwritten by checkout:",
        Some("pull" | "merge") => {
            if out.contains("cannot pull with rebase: You have unstaged changes") || out.contains("cannot rebase: You have unstaged changes") {
                return Some((args, vec![]));
            }
            "Your local changes to the following files would be overwritten by merge:"
        }
        _ => return None,
    };
    let at = out.find(head)?;
    let files: Vec<String> = out[at + head.len()..]
        .lines()
        .skip(1)
        .take_while(|l| l.starts_with('\t') || l.starts_with("  "))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Some((args, files))
}

/// A failed `git pull …` because the branches diverged and git was not told whether to merge
/// or rebase: the pull's arguments.
pub fn divergent_pull(cmd: &str, out: &str) -> Option<Vec<String>> {
    let args: Vec<String> = cmd.strip_prefix("git ")?.split_whitespace().map(String::from).collect();
    if args.first().map(String::as_str) != Some("pull") || !out.contains("Need to specify how to reconcile divergent branches") {
        return None;
    }
    Some(args)
}

/// The same pull, told to merge or to rebase.
pub fn pull_with(args: &[String], rebase: bool) -> Vec<String> {
    let mut v: Vec<String> = args.iter().filter(|a| !matches!(a.as_str(), "--rebase" | "--no-rebase" | "--ff-only" | "--ff" | "--no-ff" | "-r")).cloned().collect();
    v.insert(1, if rebase { "--rebase".into() } else { "--no-rebase".into() });
    v
}

impl App {
    /// Ask how to join diverged branches, with how many commits are on each side.
    pub fn open_divergent_pull(&mut self, args: Vec<String>) {
        let Some(repo) = self.repo.as_ref() else { return };
        // "git pull [options] [remote [branch]]": compare with that branch, else the upstream.
        let words: Vec<&String> = args.iter().skip(1).filter(|a| !a.starts_with('-')).collect();
        let target = match words.as_slice() {
            [remote, branch, ..] => format!("{remote}/{branch}"),
            _ => repo.current_branch().and_then(|b| b.upstream.clone()).unwrap_or_else(|| "@{u}".into()),
        };
        let out = git::run(&repo.path, &["rev-list", "--left-right", "--count", &format!("HEAD...{target}")]);
        let counts = if out.ok {
            let n: Vec<usize> = out.stdout.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            (n.len() == 2).then(|| (n[0], n[1]))
        } else {
            None
        };
        let target = if target == "@{u}" { "the remote branch".to_string() } else { target };
        self.dialog = Some(Dialog::DivergentPull { args, target, counts, rebase: false, remember: false });
    }
}

#[cfg(test)]
mod checkout_tests {
    use super::*;

    /// A repository with `n` commits "commit 1" … "commit n"; number 3 has "needle" in the
    /// body of its message, number 5 adds the word "haystack" to a file.
    fn long_repo(tag: &str, n: usize) -> Repo {
        let dir = std::env::temp_dir().join(format!("ge-log-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| assert!(git::run_env(&dir, &[("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")], args).ok, "{args:?}");
        g(&["init", "-q"]);
        for i in 1..=n {
            std::fs::write(dir.join("f.txt"), if i == 5 { "haystack\n".to_string() } else { format!("{i}\n") }).unwrap();
            g(&["add", "-A"]);
            let msg = if i == 3 { "commit 3\n\nthe needle is in the body".to_string() } else { format!("commit {i}") };
            g(&["commit", "-qm", &msg]);
        }
        // The folder as git names it: what the program has for an open repository.
        let dir = git::find_repo_root(&git::plain_path(dir.canonicalize().unwrap())).unwrap();
        let mut repo = Repo::new(dir, "t".into(), &Settings::default());
        repo.state = git::load_state(&repo.path);
        repo
    }

    #[test]
    fn loads_more_and_searches_the_whole_history() {
        let mut repo = long_repo("deep", 30);
        repo.log_limit = 10;
        App::reload_log(&mut repo);
        assert_eq!((repo.commits.len(), repo.more), (10, true));
        assert_eq!(repo.commits[0].subject, "commit 30");

        // The loaded commits do not have it; git finds it in the body of an old message.
        repo.text_filter = "NEEDLE".into();
        App::apply_filters(&mut repo);
        assert!(repo.visible.is_empty());
        repo.deep = 1;
        App::reload_log(&mut repo);
        assert_eq!(repo.visible.iter().map(|&i| repo.commits[i].subject.as_str()).collect::<Vec<_>>(), ["commit 3"]);
        assert!(!repo.more);
        // Match case: nothing.
        repo.case = true;
        App::reload_log(&mut repo);
        assert!(repo.commits.is_empty());

        // In the changes: the commit that added the word and the one that removed it.
        repo.case = false;
        repo.text_filter = "haystack".into();
        repo.deep = 2;
        App::reload_log(&mut repo);
        assert_eq!(repo.commits.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["commit 6", "commit 5"]);

        // No text: an ordinary log again, and the next portion.
        repo.text_filter.clear();
        App::reload_log(&mut repo);
        assert_eq!((repo.deep, repo.commits.len()), (0, 10));
        repo.log_limit += 10;
        App::reload_log(&mut repo);
        assert_eq!((repo.commits.len(), repo.more), (20, true));
        repo.log_limit += 20;
        App::reload_log(&mut repo);
        assert_eq!((repo.commits.len(), repo.more), (30, false));
        let _ = std::fs::remove_dir_all(&repo.path);
    }

    /// Wait for the running jobs of `app` to end.
    fn settle(app: &mut App) {
        let started = Instant::now();
        while app.busy() {
            assert!(started.elapsed() < std::time::Duration::from_secs(30));
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.poll_jobs();
        }
    }

    #[test]
    fn a_job_can_be_undone_and_done_again() {
        let repo = long_repo("job", 4);
        let dir = repo.path.clone();
        assert!(git::run(&dir, &["branch", "side", "HEAD~2"]).ok);
        let mut app = App::new(&egui::Context::default(), Some(dir.clone()), Some(dir.join("no-screenshot.png")));
        assert!(app.repo.is_some() && app.undo.is_none());
        let has_side = |d: &Path| git::run(d, &["rev-parse", "-q", "--verify", "refs/heads/side"]).ok;

        app.run_job(Job::one("Delete side", &["branch", "-D", "side"]));
        settle(&mut app);
        assert!(!has_side(&dir));
        // The command and its output went to the console, finished.
        let last = app.console.last().unwrap();
        assert!(last.cmd == "git branch -D side" && last.ok && !last.running && last.output.contains("Deleted branch side"), "{} / {}", last.cmd, last.output);
        assert_eq!(app.undo.as_ref().map(|r| r.label()), Some("Undo Delete side".to_string()));

        app.open_undo();
        let Some(Dialog::Confirm { job: Some(job), .. }) = app.dialog.take() else { panic!("no confirmation") };
        app.run_job(job);
        settle(&mut app);
        assert!(has_side(&dir));
        assert_eq!(app.undo.as_ref().map(|r| r.label()), Some("Redo Delete side".to_string()));

        // A job that moves nothing keeps the last undo; a failed command is reported.
        app.run_job(Job::one("Status", &["status", "--short"]));
        settle(&mut app);
        assert_eq!(app.undo.as_ref().map(|r| r.label()), Some("Redo Delete side".to_string()));
        app.run_job(Job::one("Delete nothing", &["branch", "-D", "no-such-branch"]));
        settle(&mut app);
        assert!(!app.console.last().unwrap().ok);
        assert!(app.toasts.last().is_some_and(|t| !t.ok && t.title == "Delete nothing failed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn actions_run_from_the_table_and_the_palette_lists_branches() {
        let dir = long_repo("keys", 3).path;
        assert!(git::run(&dir, &["branch", "side"]).ok);
        let mut app = App::new(&egui::Context::default(), Some(dir.clone()), Some(dir.join("no-screenshot.png")));
        // Every action of the table is known to run_action (none falls through silently
        // into a panic), and the ones that open something do.
        app.run_action("commit");
        assert!(app.show_commit && app.focus_message);
        app.run_action("focus_branches");
        assert!(app.key_focus == KeyFocus::Branches && app.repo.as_ref().unwrap().tree_selected.is_some());
        app.run_action("focus_changes");
        assert!(app.key_focus == KeyFocus::Changes && app.commit_tab == CommitTab::Changes);
        app.run_action("console");
        assert!(app.tab == Tab::Console);
        app.run_action("focus_log");
        assert!(app.tab == Tab::Log && app.key_focus == KeyFocus::Log);
        app.run_action("palette");
        assert!(matches!(app.dialog.take(), Some(Dialog::Palette { branches: false, .. })));
        app.run_action("checkout");
        assert!(matches!(app.dialog.take(), Some(Dialog::Palette { branches: true, .. })));
        app.run_action("new_branch");
        assert!(matches!(app.dialog.take(), Some(Dialog::NewBranch { .. })));
        app.run_action("stash");
        assert!(matches!(app.dialog.take(), Some(Dialog::Stash { .. })));
        // The branches to check out: the current one first, then the other local ones.
        let b = app.palette_branches();
        assert!(b[0].2 && b[1].0 == "side" && !b[1].2, "{b:?}");
        app.checkout_branch(&b[1].0, b[1].1);
        settle(&mut app);
        assert_eq!(app.repo.as_ref().unwrap().state.head_branch.as_deref(), Some("side"));
        // A shortcut of one's own is what the menus show.
        assert_eq!(app.menu_line("commit"), "Commit…    Ctrl+K");
        crate::keymap::set(&mut app.settings.keys, "commit", "Ctrl+Enter");
        crate::keymap::set(&mut app.settings.keys, "fetch", "");
        assert_eq!((app.key_of("commit"), app.menu_line("fetch")), ("Ctrl+Enter".to_string(), "Fetch".to_string()));
        assert_eq!(app.tip_with_key("Commit", "commit"), "Commit  (Ctrl+Enter)");
        // Without a repository the actions that need one do nothing.
        app.close_repo_tab(&dir);
        app.run_action("commit");
        app.run_action("checkout");
        assert!(app.dialog.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tab_strip_is_shown_as_the_setting_says() {
        let mut s = Settings::default();
        // Out of the box: always, once a repository is open.
        assert_eq!([0, 1, 2].map(|n| s.shows_repo_tabs(n)), [false, true, true]);
        s.repo_tabs = 1;
        assert_eq!([0, 1, 2].map(|n| s.shows_repo_tabs(n)), [false, false, true]);
        s.repo_tabs = 2;
        assert_eq!([0, 1, 2].map(|n| s.shows_repo_tabs(n)), [false, false, false]);
    }

    #[test]
    fn the_theme_follows_the_setting_or_the_system() {
        use egui::Theme::{Dark, Light};
        assert!(!wants_light(0, Some(Light)) && wants_light(1, Some(Dark)) && wants_light(1, None));
        assert!(wants_light(2, Some(Light)) && !wants_light(2, Some(Dark)) && !wants_light(2, None));
    }

    #[test]
    fn repositories_open_as_tabs_and_keep_what_was_open_in_them() {
        let (a, b) = (long_repo("tab-a", 3).path, long_repo("tab-b", 5).path);
        let mut app = App::new(&egui::Context::default(), Some(a.clone()), Some(a.join("no-screenshot.png")));
        assert_eq!(app.tab_order.as_slice(), std::slice::from_ref(&a));
        // Something open in the first one: a selected commit, a job that can be undone, the Console.
        app.select_commit(1);
        let picked = app.repo.as_ref().unwrap().commits[1].hash.clone();
        app.run_job(Job::one("Create branch x", &["branch", "x"]));
        settle(&mut app);
        assert!(app.undo.is_some());
        app.tab = Tab::Console;
        let read_at = app.repo.as_ref().unwrap().fingerprint;

        app.open(&b);
        assert_eq!(app.tab_order, [a.clone(), b.clone()]);
        assert_eq!((app.repo.as_ref().unwrap().path.clone(), app.repo.as_ref().unwrap().commits.len(), app.parked.len()), (b.clone(), 5, 1));
        // The second starts clean: its own tab, nothing to undo.
        assert!(app.tab == Tab::Log && app.undo.is_none() && app.repo.as_ref().unwrap().selected.is_none());

        // Back to the first: as it was left, without being opened anew.
        app.open(&a);
        let r = app.repo.as_ref().unwrap();
        assert_eq!((r.path.clone(), r.selected_commit().map(|c| c.hash.clone()), r.fingerprint), (a.clone(), Some(picked), read_at));
        assert!(app.tab == Tab::Console && app.undo.is_some() && app.parked.len() == 1 && app.tab_order.len() == 2);

        // While something runs the repository in front stays in front.
        app.running.push("Fetch".into());
        app.open(&b);
        assert_eq!(app.repo.as_ref().unwrap().path, a);
        app.close_repo_tab(&a);
        assert_eq!(app.tab_order.len(), 2);
        app.running.clear();

        // Closing the one in front brings its neighbour; closing the last leaves the start screen.
        app.close_repo_tab(&a);
        assert_eq!((app.tab_order.clone(), app.repo.as_ref().unwrap().path.clone(), app.parked.len()), (vec![b.clone()], b.clone(), 0));
        assert!(app.undo.is_none());
        app.close_repo_tab(&b);
        assert!(app.repo.is_none() && app.tab_order.is_empty() && app.diffs.is_empty());
        // A tab tells of changed files, also for the repository that is not in front.
        app.open(&a);
        app.open(&b);
        assert_eq!((app.tab_marks(&a), app.tab_marks(&b)), ((0, 0, false), (0, 0, false)));
        std::fs::write(b.join("f.txt"), "changed\n").unwrap();
        app.refresh();
        assert_eq!(app.tab_marks(&b), (0, 0, true));
        app.open(&a);
        assert_eq!((app.tab_marks(&a), app.tab_marks(&b)), ((0, 0, false), (0, 0, true)));
        assert!(git::run(&b, &["checkout", "--", "f.txt"]).ok);
        app.close_repo_tab(&b);
        app.close_repo_tab(&a);
        // The quick-view tab: the next repository opened that way takes its place, in the same
        // spot; one that is kept stays.
        let c = long_repo("tab-c", 2).path;
        app.open(&a);
        app.open_preview(&b);
        assert_eq!((app.tab_order.clone(), app.preview_repo.clone(), app.repo.as_ref().unwrap().path.clone()), (vec![a.clone(), b.clone()], Some(b.clone()), b.clone()));
        app.open_preview(&c);
        assert_eq!((app.tab_order.clone(), app.preview_repo.clone(), app.parked.len()), (vec![a.clone(), c.clone()], Some(c.clone()), 1));
        // Going to a repository that has a tab changes nothing about the quick view.
        app.open_preview(&a);
        assert_eq!((app.tab_order.len(), app.preview_repo.clone(), app.repo.as_ref().unwrap().path.clone()), (2, Some(c.clone()), a.clone()));
        app.keep_repo_tab(&c);
        assert!(app.preview_repo.is_none());
        app.open_preview(&b);
        assert_eq!((app.tab_order.clone(), app.preview_repo.clone()), (vec![a.clone(), c.clone(), b.clone()], Some(b.clone())));
        app.close_repo_tab(&b);
        assert!(app.preview_repo.is_none() && app.tab_order == vec![a.clone(), c.clone()]);
        app.close_repo_tab(&c);
        let _ = std::fs::remove_dir_all(&c);
        app.close_repo_tab(&a);
        // A folder that is not a repository does not take the open one away.
        app.open(&a);
        app.open(&std::env::temp_dir());
        assert_eq!(app.repo.as_ref().unwrap().path, a);
        for d in [a, b] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn worktrees_are_listed_guard_their_branch_and_can_be_removed() {
        let dir = long_repo("wt", 3).path;
        let side = dir.with_file_name(format!("ge-log-wt-side-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&side);
        assert!(git::run(&dir, &["worktree", "add", "-q", "-b", "side", &side.display().to_string()]).ok);
        let mut app = App::new(&egui::Context::default(), Some(dir.clone()), Some(dir.join("no-screenshot.png")));
        let trees = app.repo.as_ref().unwrap().state.worktrees.clone();
        assert_eq!(trees.len(), 2);
        assert!(trees[0].main && git::same_dir(&trees[0].path, &dir) && !trees[1].main && trees[1].branch.as_deref() == Some("side"));
        // The branch is open over there: checking it out here explains why it cannot be done.
        assert!(app.worktree_with("side").is_some_and(|w| git::same_dir(&w.path, &side)));
        let here = app.repo.as_ref().unwrap().state.head_branch.clone().unwrap();
        assert!(app.worktree_with(&here).is_none());
        app.checkout_branch("side", RefKind::Local);
        let Some(Dialog::Message { text, .. }) = app.dialog.take() else { panic!("no explanation") };
        assert!(text.contains("side") && app.running.is_empty(), "{text}");
        // A worktree with a new file in it goes only by force.
        std::fs::write(side.join("draft.txt"), "x\n").unwrap();
        let (_, args) = crate::branches_view::remove_worktree_plan(&side.display().to_string(), Some("side"), git::status(&side).len(), false);
        assert!(args.contains(&"--force".to_string()));
        app.run_job(Job::many("Remove worktree", vec![args]));
        settle(&mut app);
        assert!(!side.exists());
        assert_eq!(app.repo.as_ref().unwrap().state.worktrees.len(), 1);
        assert!(app.worktree_with("side").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bisect_is_seen_marks_its_commits_and_is_not_undone_step_by_step() {
        let dir = long_repo("bis", 6).path;
        let mut app = App::new(&egui::Context::default(), Some(dir.clone()), Some(dir.join("no-screenshot.png")));
        assert!(app.repo.as_ref().unwrap().state.bisect.is_none());
        let (bad, good) = (app.repo.as_ref().unwrap().commits[0].hash.clone(), app.repo.as_ref().unwrap().commits[5].hash.clone());
        app.run_job(Job::one("Bisect start", &["bisect", "start", &bad, &good]));
        settle(&mut app);
        let st = app.repo.as_ref().unwrap().state.clone();
        let b = st.bisect.clone().unwrap();
        assert!(b.testing() && b.left == 5 && b.bad.as_deref() == Some(bad.as_str()));
        let mark = |st: &git::RepoState, h: &str| st.labels.get(h).and_then(|l| l.iter().find(|l| l.kind == git::LabelKind::Bisect)).map(|l| l.text.clone());
        assert_eq!((mark(&st, &bad).as_deref(), mark(&st, &good).as_deref()), (Some("bad"), Some("good")));
        // Git has moved HEAD to a commit to test; that is not something Undo takes back.
        assert!(st.head_hash != bad && app.undo.is_none());
        app.run_job(Job::one("Bisect reset", &["bisect", "reset"]));
        settle(&mut app);
        let st = &app.repo.as_ref().unwrap().state;
        assert!(st.bisect.is_none() && st.head_hash == bad && mark(st, &bad).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_commit_refused_by_a_hook_shows_what_the_hook_said() {
        use std::os::unix::fs::PermissionsExt;
        let dir = long_repo("hook", 2).path;
        assert!(git::run(&dir, &["config", "user.name", "T"]).ok && git::run(&dir, &["config", "user.email", "t@e"]).ok);
        let hook = dir.join(".git/hooks/pre-commit");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho 'style check failed:' >&2\necho '  f.txt:1 trailing space' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut app = App::new(&egui::Context::default(), Some(dir.clone()), Some(dir.join("no-screenshot.png")));
        let count = |d: &Path| git::run(d, &["rev-list", "--count", "HEAD"]).stdout.trim().to_string();
        let commit = |flags: &[String]| {
            let mut cmds = vec![vec!["commit".to_string(), "--allow-empty".into(), "-m".into(), "try".into()]];
            crate::commit_view::add_commit_flags(&mut cmds, flags);
            Job::many("Commit", cmds).then(Then::ClearMessage)
        };
        app.commit_opts.author = "Someone Else <s@e>".into();
        app.run_job(commit(&[]));
        settle(&mut app);
        assert_eq!(count(&dir), "2");
        let Some(Dialog::Output { text, .. }) = app.dialog.take() else { panic!("the hook's words are not shown") };
        assert!(text.contains("style check failed:") && text.contains("f.txt:1 trailing space"), "{text}");
        // Nothing was committed, so the options for this commit are still set.
        assert_eq!(app.commit_opts.author, "Someone Else <s@e>");

        // Skipping the hooks, with another author and a sign-off.
        app.commit_opts.no_verify = true;
        let flags = crate::commit_view::commit_flags(true, false, &app.commit_opts);
        app.run_job(commit(&flags));
        settle(&mut app);
        assert!(app.dialog.is_none());
        assert_eq!(count(&dir), "3");
        let made = git::run(&dir, &["log", "-1", "--format=%an <%ae>%n%B"]).stdout;
        assert!(made.starts_with("Someone Else <s@e>") && made.contains("Signed-off-by: T <t@e>"), "{made}");
        // The one-commit options are cleared after it; an unsigned commit has no signature.
        assert_eq!(app.commit_opts.count(), 0);
        assert!(git::signature(&dir, "HEAD").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compares_two_commits_and_lists_the_revisions_of_a_file() {
        let dir = long_repo("cmp", 4).path;
        let g = |args: &[&str]| assert!(git::run_env(&dir, &[("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")], args).ok, "{args:?}");
        g(&["mv", "f.txt", "g.txt"]);
        g(&["commit", "-qm", "renamed"]);
        g(&["branch", "side", "HEAD~2"]);
        // The commits that changed the file, through its rename; the older ones know its old name.
        let revs = file_revisions(&dir, "g.txt", 100);
        assert_eq!(revs.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["renamed", "commit 4", "commit 3", "commit 2", "commit 1"]);
        assert_eq!(revs.iter().map(|r| r.old_path.as_deref()).collect::<Vec<_>>(), [None, Some("f.txt"), Some("f.txt"), Some("f.txt"), Some("f.txt")]);
        assert!(revs[1].note.contains("T"));

        let mut app = App::new(&egui::Context::default(), Some(dir.clone()), Some(dir.join("no-screenshot.png")));
        // Two commits of the Log, whichever is given first: the older one is the base.
        app.compare_commits(3, 1);
        let c = app.compare.take().unwrap();
        let hash = |i: usize| app.repo.as_ref().unwrap().commits[i].hash.clone();
        assert_eq!((c.base.as_str(), c.head.as_deref()), (short(&hash(3)), Some(short(&hash(1)))));
        assert!(c.show_files && c.files.len() == 1 && c.files[0].path == "f.txt" && c.only_head.len() == 2 && c.only_base.is_empty());
        // The file against a branch and against a revision where it had another name.
        app.open_compare_file_dialog("g.txt", true);
        let Some(Dialog::CompareFile { items, branches: true, .. }) = app.dialog.take() else { panic!("no window") };
        assert!(items.iter().any(|i| i.rev == "side") && items.iter().any(|i| i.note == "current"));
        std::fs::write(dir.join("g.txt"), "now\n").unwrap();
        app.open_compare_file(&revs[3].rev, None, &git::ChangedFile { status: 'R', path: "g.txt".into(), old_path: Some("f.txt".into()) });
        let d = &app.diffs.last().unwrap().diff;
        assert!(d.unified.iter().any(|(t, l)| *t == '-' && l == "2") && d.unified.iter().any(|(t, l)| *t == '+' && l == "now"), "{:?}", d.unified);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn branches_of_a_commit_and_changed_files_arrive_later() {
        let dir = long_repo("later", 3).path;
        let mut app = App::new(&egui::Context::default(), Some(dir.clone()), Some(dir.join("no-screenshot.png")));
        let rev = |a: &[&str]| git::run(&dir, a).stdout.trim().to_string();
        let (head, branch) = (rev(&["rev-parse", "HEAD"]), rev(&["rev-parse", "--abbrev-ref", "HEAD"]));
        let wait = |done: &mut dyn FnMut() -> bool| {
            let started = Instant::now();
            while !done() {
                assert!(started.elapsed() < std::time::Duration::from_secs(20));
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        // Not waited for: the commit is shown at once, its branches are filled in later.
        let d = details_within(&dir, &head, None, 0).unwrap();
        assert!(d.refs_loading && d.containing.is_empty() && d.files.len() == 1 && d.message.starts_with("commit 3"));
        app.repo.as_mut().unwrap().details = Some(d);
        wait(&mut || {
            app.poll_refs();
            !app.repo.as_ref().unwrap().details.as_ref().unwrap().refs_loading
        });
        assert_eq!(app.repo.as_ref().unwrap().details.as_ref().unwrap().containing, vec![branch]);
        // Waited for, they are there at once.
        assert!(!details_of(&dir, &head, None).unwrap().containing.is_empty());

        // The changed files: the old list stays until the new one is read.
        std::fs::write(dir.join("new.txt"), "x\n").unwrap();
        let repo = app.repo.as_mut().unwrap();
        assert!(repo.status.is_empty());
        App::reload_status(repo, 0);
        assert!(repo.status_pending.is_some() && repo.status.is_empty());
        wait(&mut || App::take_pending_status(repo));
        assert!(repo.status_pending.is_none());
        assert_eq!(repo.status.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["new.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_slow_log_arrives_later_and_keeps_the_selection() {
        let mut repo = long_repo("slow", 6);
        App::reload_log(&mut repo);
        repo.selected = Some(2);
        let hash = repo.commits[2].hash.clone();
        // Not waiting at all: the old list stays until the new one is read.
        repo.topo = true;
        App::reload_log_within(&mut repo, 0);
        assert!(repo.log_pending.is_some() && repo.commits.len() == 6);
        let started = Instant::now();
        while !App::take_pending_log(&mut repo) {
            assert!(started.elapsed() < std::time::Duration::from_secs(20));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(repo.log_pending.is_none());
        assert_eq!(repo.selected.map(|i| repo.commits[i].hash.clone()), Some(hash));
        let _ = std::fs::remove_dir_all(&repo.path);
    }

    #[test]
    fn finds_blocked_checkout() {
        let out = "error: Your local changes to the following files would be overwritten by checkout:\n\tVERSION\n\tsrc/a.rs\nPlease commit your changes or stash them before you switch branches.\nAborting\n";
        let (args, files) = blocked_checkout("git checkout master", out).unwrap();
        assert_eq!(args, vec!["checkout", "master"]);
        assert_eq!(files, vec!["VERSION", "src/a.rs"]);
        assert!(blocked_checkout("git checkout -f master", out).is_none());
        assert!(blocked_checkout("git pull", out).is_none());
        let job = Job::smart_checkout(&args);
        assert_eq!(job.cmds.len(), 3);
        assert_eq!(job.cmds[1], vec!["checkout", "master"]);
        assert_eq!(job.title, "Smart checkout master");
        let out = "Updating 384fb5e..1a5d4c4\nerror: Your local changes to the following files would be overwritten by merge:\n\tMakefile\nPlease commit your changes or stash them before you merge.\nAborting\n";
        let (args, files) = blocked_checkout("git pull --no-edit", out).unwrap();
        assert_eq!(files, vec!["Makefile"]);
        let job = Job::smart_checkout(&args);
        assert_eq!(job.title, "Smart update");
        assert_eq!(job.cmds[1], vec!["pull", "--no-edit"]);
        assert!(blocked_checkout("git pull --rebase", "error: cannot pull with rebase: You have unstaged changes.").is_some());
    }

    #[test]
    fn finds_divergent_pull() {
        let out = "hint: You have divergent branches and need to specify how to reconcile them.\nfatal: Need to specify how to reconcile divergent branches.\n";
        let args = divergent_pull("git pull --no-edit", out).unwrap();
        assert_eq!(pull_with(&args, false), vec!["pull", "--no-rebase", "--no-edit"]);
        assert_eq!(pull_with(&["pull".into(), "--no-edit".into(), "--ff-only".into(), "origin".into(), "main".into()], true), vec!["pull", "--rebase", "--no-edit", "origin", "main"]);
        assert!(divergent_pull("git fetch", out).is_none());
        assert!(divergent_pull("git pull", "Already up to date.").is_none());
    }
}
