//! Thin layer over the `git` command line: running commands and parsing their output.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Clone, Debug)]
pub struct GitOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl GitOutput {
    pub fn text(&self) -> String {
        let mut s = String::new();
        if !self.stdout.trim().is_empty() {
            s.push_str(self.stdout.trim_end());
        }
        if !self.stderr.trim().is_empty() {
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str(self.stderr.trim_end());
        }
        s
    }
}

static PROGRAM: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Use another git program (Settings → Git); empty means "git" from PATH.
pub fn set_program(path: &str) {
    if let Ok(mut p) = PROGRAM.write() {
        *p = path.trim().to_string();
    }
}

/// The git program to run.
pub fn program() -> String {
    let p = PROGRAM.read().map(|p| p.clone()).unwrap_or_default();
    if p.is_empty() { "git".into() } else { p }
}

/// `git --version` of a program, or the error.
pub fn version_of(program: &str) -> Result<String, String> {
    let prog = if program.trim().is_empty() { "git" } else { program.trim() };
    let mut c = Command::new(prog);
    no_console(&mut c);
    match c.arg("--version").stdin(Stdio::null()).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// On Windows, keep helper programs from flashing a console window.
pub fn no_console(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// The person's home folder on every system.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Is the `git` program installed and reachable?
pub fn available() -> bool {
    let mut c = Command::new(program());
    no_console(&mut c);
    c.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Remove Windows' `\\?\` prefix that canonicalize adds; git and other tools dislike it.
pub fn plain_path(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    p
}

/// Build a git command for `dir` with an environment that never blocks on a terminal.
pub fn command(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(program());
    no_console(&mut c);
    c.current_dir(dir)
        .args(["--no-optional-locks", "-c", "color.ui=false", "-c", "core.quotepath=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("LC_ALL", "C.UTF-8")
        .stdin(Stdio::null());
    if let Ok(exe) = std::env::current_exe() {
        c.env("REFBRAID_ASKPASS", "1")
            .env("GIT_ASKPASS", &exe)
            .env("SSH_ASKPASS", &exe)
            .env("SSH_ASKPASS_REQUIRE", "prefer");
    }
    c
}

pub fn run(dir: &Path, args: &[&str]) -> GitOutput {
    run_env(dir, &[], args)
}

/// Like [`run`] with extra environment variables (e.g. GIT_INDEX_FILE).
pub fn run_env(dir: &Path, env: &[(&str, &str)], args: &[&str]) -> GitOutput {
    let mut c = command(dir, args);
    c.envs(env.iter().copied());
    match c.output() {
        Ok(o) => GitOutput {
            ok: o.status.success(),
            stdout: crate::encoding::decode(&o.stdout),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        },
        Err(e) => GitOutput {
            ok: false,
            stdout: String::new(),
            stderr: format!("Could not start git: {e}"),
        },
    }
}

/// One command of a running job, as the Console shows it while it runs.
#[derive(Clone, Debug, Default)]
pub struct LiveCmd {
    pub cmd: String,
    /// What the command has printed so far (complete lines).
    pub output: String,
    /// None while it runs.
    pub ok: Option<bool>,
}

#[derive(Default)]
struct LiveState {
    progress: String,
    cmds: Vec<LiveCmd>,
    cancel: bool,
    pid: Option<u32>,
}

/// A running job seen from the window: what it prints, how far it is, and a way to stop it.
#[derive(Clone, Default)]
pub struct Live(std::sync::Arc<std::sync::Mutex<LiveState>>);

impl Live {
    fn with<T>(&self, f: impl FnOnce(&mut LiveState) -> T) -> T {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }
    /// A command starts.
    pub fn begin(&self, cmd: &str) {
        self.with(|s| {
            s.progress.clear();
            s.cmds.push(LiveCmd { cmd: cmd.to_string(), ..Default::default() });
        });
    }
    /// The command ended; `text` is everything it printed.
    pub fn end(&self, ok: bool, text: &str) {
        self.with(|s| {
            s.progress.clear();
            if let Some(c) = s.cmds.last_mut() {
                c.ok = Some(ok);
                c.output = text.to_string();
            }
        });
    }
    fn line(&self, line: &str) {
        self.with(|s| {
            if !line.trim().is_empty() {
                s.progress = line.trim().to_string();
            }
            if let Some(c) = s.cmds.last_mut() {
                if !c.output.is_empty() {
                    c.output.push('\n');
                }
                c.output.push_str(line);
            }
        });
    }
    fn set_progress(&self, line: &str) {
        if !line.trim().is_empty() {
            self.with(|s| s.progress = line.trim().to_string());
        }
    }
    /// A job caught in the middle of a command, for screenshots.
    pub fn demo(cmd: &str, lines: &[&str], progress: &str) -> Live {
        let l = Live::default();
        l.begin(cmd);
        for x in lines {
            l.line(x);
        }
        l.set_progress(progress);
        l
    }
    /// The newest progress line, e.g. "Receiving objects:  45% (450/1000), 1.2 MiB".
    pub fn progress(&self) -> String {
        self.with(|s| s.progress.clone())
    }
    pub fn cmds(&self) -> Vec<LiveCmd> {
        self.with(|s| s.cmds.clone())
    }
    pub fn cancelled(&self) -> bool {
        self.with(|s| s.cancel)
    }
    /// Stop the running command (and the programs it started) and the rest of the job.
    pub fn cancel(&self) {
        let pid = self.with(|s| {
            s.cancel = true;
            s.pid
        });
        if let Some(pid) = pid {
            kill_tree(pid);
        }
    }
}

/// End a process started by [`run_live`] together with its children (ssh, git-remote-https…).
fn kill_tree(pid: u32) {
    #[cfg(unix)]
    {
        // The command runs in its own process group (see `run_live`), named after its pid.
        let _ = Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(windows)]
    {
        let mut c = Command::new("taskkill");
        no_console(&mut c);
        let _ = c.args(["/PID", &pid.to_string(), "/T", "/F"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// The share done that a progress line tells: "Receiving objects:  45% (450/1000)" → 0.45.
pub fn progress_share(line: &str) -> Option<f32> {
    let at = line.find('%')?;
    let digits: String = line[..at].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    let n: f32 = digits.parse().ok()?;
    (n <= 100.0).then_some(n / 100.0)
}

/// Subcommands that report how far they are when asked with `--progress`.
pub fn with_progress(args: &[String]) -> Vec<String> {
    let mut out = args.to_vec();
    if matches!(args.first().map(String::as_str), Some("fetch" | "pull" | "push" | "clone")) && !args.iter().any(|a| a == "--progress" || a == "-q" || a == "--quiet") {
        out.insert(1, "--progress".into());
    }
    out
}

/// Like [`run_env`], for the long commands of a job: what git prints on stderr reaches `live`
/// as it comes (progress lines that git rewrites in place only as the current progress), and
/// `live.cancel()` stops the command. `wake` asks the window to draw again.
pub fn run_live(dir: &Path, env: &[(&str, &str)], args: &[&str], live: &Live, wake: &dyn Fn()) -> GitOutput {
    use std::io::Read;
    let mut c = command(dir, args);
    c.envs(env.iter().copied());
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => return GitOutput { ok: false, stdout: String::new(), stderr: format!("Could not start git: {e}") },
    };
    live.with(|s| s.pid = Some(child.id()));
    let out_thread = child.stdout.take().map(|mut so| {
        std::thread::spawn(move || {
            let mut v = vec![];
            let _ = so.read_to_end(&mut v);
            v
        })
    });
    let mut stderr = String::new();
    if let Some(mut se) = child.stderr.take() {
        let mut cur: Vec<u8> = vec![];
        // A carriage return was read: the text before it is progress, unless a newline follows
        // (Windows line ends).
        let mut cr = false;
        let mut buf = [0u8; 4096];
        let mut last_wake = std::time::Instant::now();
        loop {
            let n = match se.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for &b in &buf[..n] {
                if cr && b != b'\n' {
                    live.set_progress(&String::from_utf8_lossy(&cur));
                    cur.clear();
                }
                cr = false;
                match b {
                    b'\n' => {
                        let line = String::from_utf8_lossy(&cur).into_owned();
                        cur.clear();
                        stderr.push_str(&line);
                        stderr.push('\n');
                        live.line(&line);
                    }
                    b'\r' => cr = true,
                    _ => cur.push(b),
                }
            }
            if cr {
                live.set_progress(&String::from_utf8_lossy(&cur));
            }
            if last_wake.elapsed() > std::time::Duration::from_millis(80) {
                last_wake = std::time::Instant::now();
                wake();
            }
        }
        if !cur.is_empty() && !cr {
            stderr.push_str(&String::from_utf8_lossy(&cur));
        }
    }
    let stdout = out_thread.and_then(|t| t.join().ok()).unwrap_or_default();
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    live.with(|s| s.pid = None);
    GitOutput { ok, stdout: crate::encoding::decode(&stdout), stderr }
}

/// Find the top folder of the repository that contains `path`.
pub fn find_repo_root(path: &Path) -> Option<PathBuf> {
    let dir = if path.is_file() { path.parent()? } else { path };
    let out = run(dir, &["rev-parse", "--show-toplevel"]);
    if out.ok {
        let p = out.stdout.trim();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug)]
pub struct Branch {
    /// Short name, e.g. `main` or `origin/main`.
    pub name: String,
    pub kind: RefKind,
    pub hash: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub is_head: bool,
}

#[derive(Clone, Debug)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    // Read from the log with the rest; nothing shows it yet (the details load their own).
    #[allow(dead_code)]
    pub email: String,
    pub time: i64,
    pub subject: String,
}

#[derive(Clone, Debug, Default)]
pub struct RepoState {
    pub head_branch: Option<String>,
    pub head_hash: String,
    pub branches: Vec<Branch>,
    pub remotes: Vec<String>,
    pub tags: Vec<Branch>,
    /// hash -> labels to draw next to the commit
    pub labels: HashMap<String, Vec<RefLabel>>,
    pub user_name: String,
    pub operation: Option<Operation>,
    /// Every working folder of the repository: the main one first, then the added worktrees.
    pub worktrees: Vec<Worktree>,
    /// The bisect in progress.
    pub bisect: Option<crate::bisect::Bisect>,
}

/// A working folder of the repository (`git worktree list`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: String,
    /// The branch checked out there (None: a detached HEAD or a bare repository).
    pub branch: Option<String>,
    pub bare: bool,
    pub locked: bool,
    /// Its folder is gone: only the record is left (`git worktree prune` removes it).
    pub prunable: bool,
    /// The repository's own folder, not an added worktree.
    pub main: bool,
}

/// Read `git worktree list --porcelain`.
pub fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut v: Vec<Worktree> = vec![];
    for line in text.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key == "worktree" {
            v.push(Worktree { path: PathBuf::from(value), main: v.is_empty(), ..Default::default() });
            continue;
        }
        let Some(w) = v.last_mut() else { continue };
        match key {
            "HEAD" => w.head = value.to_string(),
            "branch" => w.branch = Some(value.trim_start_matches("refs/heads/").to_string()),
            "bare" => w.bare = true,
            "locked" => w.locked = true,
            "prunable" => w.prunable = true,
            _ => {}
        }
    }
    v
}

pub fn worktrees(dir: &Path) -> Vec<Worktree> {
    parse_worktrees(&run(dir, &["worktree", "list", "--porcelain"]).stdout)
}

/// The same folder, whatever way each path is written.
pub fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl Operation {
    pub fn title(self) -> &'static str {
        match self {
            Operation::Merge => "Merge",
            Operation::Rebase => "Rebase",
            Operation::CherryPick => "Cherry-pick",
            Operation::Revert => "Revert",
        }
    }
    pub fn command(self) -> &'static str {
        match self {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RefLabel {
    pub text: String,
    pub kind: LabelKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    Head,
    Local,
    Remote,
    Tag,
    /// A mark of the bisect in progress: good, bad, skipped, first bad.
    Bisect,
}

fn parse_track(s: &str) -> (u32, u32) {
    let mut ahead = 0;
    let mut behind = 0;
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    for part in s.split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

pub fn load_state(dir: &Path) -> RepoState {
    let mut st = RepoState::default();
    let remotes = run(dir, &["remote"]);
    st.remotes = remotes
        .stdout
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    st.user_name = run(dir, &["config", "user.name"]).stdout.trim().to_string();

    let head = run(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    if head.ok {
        st.head_branch = Some(head.stdout.trim().to_string());
    }
    st.head_hash = run(dir, &["rev-parse", "--verify", "--quiet", "HEAD"])
        .stdout
        .trim()
        .to_string();

    let refs = run(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track)%00%(HEAD)%00%(*objectname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    );
    for line in refs.stdout.lines() {
        let f: Vec<&str> = line.split('\0').collect();
        if f.len() < 6 {
            continue;
        }
        let refname = f[0];
        let (kind, name) = if let Some(n) = refname.strip_prefix("refs/heads/") {
            (RefKind::Local, n)
        } else if let Some(n) = refname.strip_prefix("refs/remotes/") {
            if n.ends_with("/HEAD") {
                continue;
            }
            (RefKind::Remote, n)
        } else if let Some(n) = refname.strip_prefix("refs/tags/") {
            (RefKind::Tag, n)
        } else {
            continue;
        };
        let hash = if !f[5].is_empty() { f[5] } else { f[1] };
        let (ahead, behind) = parse_track(f[3]);
        let b = Branch {
            name: name.to_string(),
            kind,
            hash: hash.to_string(),
            upstream: if f[2].is_empty() { None } else { Some(f[2].to_string()) },
            ahead,
            behind,
            is_head: f[4] == "*",
        };
        if kind == RefKind::Tag {
            st.tags.push(b);
        } else {
            st.branches.push(b);
        }
    }

    // Labels: group a local branch with its remote twin as "origin & name".
    let mut labels: HashMap<String, Vec<RefLabel>> = HashMap::new();
    let mut used_remote: std::collections::HashSet<String> = Default::default();
    for b in st.branches.iter().filter(|b| b.kind == RefKind::Local) {
        let mut twin_remotes = vec![];
        for r in &st.remotes {
            let rn = format!("{r}/{}", b.name);
            if st
                .branches
                .iter()
                .any(|x| x.kind == RefKind::Remote && x.name == rn && x.hash == b.hash)
            {
                twin_remotes.push(r.clone());
                used_remote.insert(rn);
            }
        }
        let text = if twin_remotes.is_empty() {
            b.name.clone()
        } else {
            format!("{} & {}", twin_remotes.join(" & "), b.name)
        };
        let kind = if b.is_head { LabelKind::Head } else { LabelKind::Local };
        labels.entry(b.hash.clone()).or_default().push(RefLabel { text, kind });
    }
    for b in st.branches.iter().filter(|b| b.kind == RefKind::Remote) {
        if used_remote.contains(&b.name) {
            continue;
        }
        labels.entry(b.hash.clone()).or_default().push(RefLabel {
            text: b.name.clone(),
            kind: LabelKind::Remote,
        });
    }
    for t in &st.tags {
        labels.entry(t.hash.clone()).or_default().push(RefLabel {
            text: t.name.clone(),
            kind: LabelKind::Tag,
        });
    }
    if st.head_branch.is_none() && !st.head_hash.is_empty() {
        labels.entry(st.head_hash.clone()).or_default().insert(
            0,
            RefLabel {
                text: "HEAD".into(),
                kind: LabelKind::Head,
            },
        );
    }
    st.bisect = crate::bisect::load(dir);
    if let Some(b) = &st.bisect {
        let marked = b.found.iter().chain(b.bad.iter()).chain(b.good.iter()).chain(b.skipped.iter());
        for hash in marked {
            let Some(m) = b.mark(hash) else { continue };
            let list = labels.entry(hash.clone()).or_default();
            if !list.iter().any(|l| l.kind == LabelKind::Bisect) {
                list.push(RefLabel { text: m.word().to_string(), kind: LabelKind::Bisect });
            }
        }
    }
    for v in labels.values_mut() {
        v.sort_by_key(|l| match l.kind {
            LabelKind::Bisect => 0,
            LabelKind::Head => 1,
            LabelKind::Local => 2,
            LabelKind::Remote => 3,
            LabelKind::Tag => 4,
        });
    }
    st.labels = labels;

    st.operation = operation(dir);
    st.worktrees = worktrees(dir);
    st
}

/// The merge, rebase, cherry-pick or revert the repository is in the middle of.
pub fn operation(dir: &Path) -> Option<Operation> {
    let git_dir = run(dir, &["rev-parse", "--git-dir"]).stdout.trim().to_string();
    let git_dir = if Path::new(&git_dir).is_absolute() {
        PathBuf::from(git_dir)
    } else {
        dir.join(git_dir)
    };
    if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        Some(Operation::Rebase)
    } else if git_dir.join("MERGE_HEAD").exists() {
        Some(Operation::Merge)
    } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
        Some(Operation::CherryPick)
    } else if git_dir.join("REVERT_HEAD").exists() {
        Some(Operation::Revert)
    } else {
        None
    }
}

#[derive(Clone, Debug, Default)]
pub struct LogQuery {
    pub first_parent: bool,
    pub branch: Option<String>,
    pub paths: Vec<String>,
    pub limit: usize,
    /// Only commits that are not in the current branch yet (not merged into HEAD).
    pub not_merged: bool,
    /// File history: `paths` is one file, followed through its renames (from `branch` or HEAD).
    pub follow: bool,
    /// `--topo-order` instead of `--date-order`: a branch's commits stay together.
    pub topo: bool,
    /// Search the whole history with git instead of the loaded commits.
    pub grep: Option<Grep>,
    /// File history of lines `from..=to` of the one file in `paths` (`git log -L`).
    pub lines: Option<(usize, usize)>,
    /// File history over every branch, not only the line HEAD (or `branch`) is on.
    pub all: bool,
}

/// A search git runs over the history: in commit messages (`--grep`) or in the text the
/// commits added or removed (`-S`).
#[derive(Clone, Debug, PartialEq)]
pub struct Grep {
    pub text: String,
    pub regex: bool,
    pub case: bool,
    pub in_changes: bool,
}

impl Grep {
    pub fn args(&self) -> Vec<String> {
        let mut a = vec![];
        if !self.case {
            a.push("-i".to_string());
        }
        if self.in_changes {
            a.push(format!("-S{}", self.text));
            if self.regex {
                a.push("--pickaxe-regex".into());
            }
        } else {
            a.push(if self.regex { "-E" } else { "-F" }.to_string());
            a.push(format!("--grep={}", self.text));
        }
        a
    }
}

/// File history: the name the followed file has in each commit that changed it.
pub fn file_names(dir: &Path, rev: &str, path: &str, limit: usize) -> std::collections::HashMap<String, String> {
    let limit = format!("-n{limit}");
    let out = run(dir, &["log", "--follow", "--name-status", "-z", "--format=%x1e%H", &limit, rev, "--", path]);
    // Each record: "<hash>\0\nM\0<path>\0", or for a rename "<hash>\0\nR100\0<old>\0<new>\0".
    out.stdout
        .split('\x1e')
        .filter_map(|rec| {
            let mut parts = rec.split('\0').filter(|t| !t.trim().is_empty());
            let hash = parts.next()?.trim().to_string();
            let name = parts.last()?.to_string();
            Some((hash, name))
        })
        .collect()
}

/// Branches with commits that are not in the current branch: "L:name" for local ones,
/// "R:remote/name" for remote ones.
pub fn unmerged_branches(dir: &Path) -> std::collections::HashSet<String> {
    run(dir, &["for-each-ref", "--no-merged=HEAD", "--format=%(refname)", "refs/heads", "refs/remotes"])
        .stdout
        .lines()
        .filter_map(|r| {
            r.strip_prefix("refs/heads/")
                .map(|n| format!("L:{n}"))
                .or_else(|| r.strip_prefix("refs/remotes/").filter(|n| !n.ends_with("/HEAD")).map(|n| format!("R:{n}")))
        })
        .collect()
}

pub fn load_log(dir: &Path, q: &LogQuery) -> Vec<Commit> {
    let limit = format!("-n{}", q.limit);
    let mut args: Vec<String> = vec![
        "log".into(),
        if q.topo { "--topo-order" } else { "--date-order" }.into(),
        limit,
        "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%s%x1e".into(),
    ];
    if q.first_parent {
        args.push("--first-parent".into());
    }
    let lines = q.lines.zip(q.paths.first());
    if let Some(((from, to), path)) = &lines {
        // The commits that touched these lines, without their patches.
        args.push("-s".into());
        args.push(format!("-L{from},{to}:{path}"));
    } else if q.follow {
        args.push("--follow".into());
    }
    if let Some(g) = &q.grep {
        args.extend(g.args());
    }
    match &q.branch {
        Some(b) => args.push(b.clone()),
        // Lines are traced back from one commit; renames are followed from the tips given.
        None if lines.is_some() => args.push("HEAD".into()),
        None if q.follow && q.all => args.push("--all".into()),
        None if q.follow => args.push("HEAD".into()),
        None if q.not_merged => {
            args.push("--branches".into());
            args.push("--remotes".into());
        }
        None => {
            args.push("--branches".into());
            args.push("--remotes".into());
            args.push("--tags".into());
            args.push("HEAD".into());
        }
    }
    if q.not_merged {
        args.push("^HEAD".into());
    }
    if !q.paths.is_empty() && lines.is_none() {
        args.push("--".into());
        args.extend(q.paths.iter().cloned());
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = run(dir, &refs);
    let mut v = Vec::new();
    for rec in out.stdout.split('\x1e') {
        let rec = rec.trim_start_matches('\n');
        if rec.is_empty() {
            continue;
        }
        let f: Vec<&str> = rec.split('\0').collect();
        if f.len() < 6 {
            continue;
        }
        v.push(Commit {
            hash: f[0].to_string(),
            parents: f[1].split_whitespace().take(if q.first_parent { 1 } else { usize::MAX }).map(String::from).collect(),
            author: f[2].to_string(),
            email: f[3].to_string(),
            time: f[4].parse().unwrap_or(0),
            subject: f[5].to_string(),
        });
    }
    v
}

#[derive(Clone, Debug)]
pub struct CommitDetails {
    pub hash: String,
    pub author: String,
    pub email: String,
    pub author_time: i64,
    pub committer: String,
    pub commit_time: i64,
    pub message: String,
    pub parents: Vec<String>,
    /// Changes against the first parent (all a normal commit changed).
    pub files: Vec<ChangedFile>,
    pub containing: Vec<String>,
    /// Merge commits: the changes against each parent ("Changes to <parent>")…
    pub parent_files: Vec<Vec<ChangedFile>>,
    /// …the subject line of each parent…
    pub parent_subjects: Vec<String>,
    /// …and the files that differ from every parent: what the merge itself changed
    /// (usually resolved conflicts).
    pub merge_files: Vec<ChangedFile>,
    /// Annotated tags on the commit: (name, tagger, time, message).
    pub tags: Vec<(String, String, i64, String)>,
    /// `containing`, `tags` and `signature` are still being read (slow in a big repository).
    pub refs_loading: bool,
    /// A signed commit: what checking the signature said (git's %G? letter) and the signer.
    pub signature: Option<(char, String)>,
}

/// What is read about a commit apart from the commit itself: the branches that contain it,
/// the annotated tags on it and its signature.
pub type CommitRefs = (Vec<String>, Vec<(String, String, i64, String)>, Option<(char, String)>);

/// The signature of a commit as git checks it: the %G? letter (G good, B bad, U good but the
/// key is not trusted, X / Y expired, R revoked, E cannot be checked) and the signer's name.
/// None for a commit that is not signed.
pub fn signature(dir: &Path, hash: &str) -> Option<(char, String)> {
    let out = run(dir, &["log", "-1", "--format=%G?%x00%GS", hash]);
    let (state, signer) = out.stdout.trim_end().split_once('\0')?;
    match state.chars().next()? {
        'N' => None,
        c => Some((c, signer.to_string())),
    }
}

/// What a signature's state means, in words; the bool says it can be relied on.
pub fn signature_words(state: char) -> (&'static str, bool) {
    match state {
        'G' => ("Good signature", true),
        'U' => ("Good signature, but the key is not marked as trusted", true),
        'X' => ("Good signature that has expired", false),
        'Y' => ("Good signature made with a key that has expired", false),
        'R' => ("Good signature made with a key that was revoked", false),
        'B' => ("Bad signature", false),
        _ => ("Signed, but the signature cannot be checked (the key is not known here)", false),
    }
}

/// The branches containing a commit and its annotated tags. Looking for the branches walks
/// the history, so in a very big repository it takes seconds.
pub fn commit_refs(dir: &Path, hash: &str) -> CommitRefs {
    let containing = run(dir, &["branch", "-a", "--contains", hash, "--format=%(refname:short)"])
        .stdout
        .lines()
        .filter(|s| !s.ends_with("/HEAD") && !s.is_empty())
        .map(String::from)
        .collect();
    (containing, annotated_tags(dir, hash), signature(dir, hash))
}

impl CommitDetails {
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

#[derive(Clone, Debug)]
pub struct ChangedFile {
    /// A, M, D, R, C, T, U, ?
    pub status: char,
    pub path: String,
    pub old_path: Option<String>,
}

pub fn parse_name_status(text: &str) -> Vec<ChangedFile> {
    let parts: Vec<&str> = text.split('\0').collect();
    let mut i = 0;
    let mut files = vec![];
    while i < parts.len() {
        let st = parts[i].trim();
        if st.is_empty() {
            i += 1;
            continue;
        }
        let c = st.chars().next().unwrap_or('M');
        if (c == 'R' || c == 'C') && i + 2 < parts.len() + 1 {
            let old = parts.get(i + 1).copied().unwrap_or("").to_string();
            let new = parts.get(i + 2).copied().unwrap_or("").to_string();
            files.push(ChangedFile {
                status: c,
                path: new,
                old_path: Some(old),
            });
            i += 3;
        } else {
            let p = parts.get(i + 1).copied().unwrap_or("").to_string();
            files.push(ChangedFile {
                status: c,
                path: p,
                old_path: None,
            });
            i += 2;
        }
    }
    files
}

/// Like [`commit_details`] but without the (slow) list of branches containing it.
pub fn commit_details_light(dir: &Path, hash: &str) -> Option<CommitDetails> {
    details(dir, hash, false)
}

pub fn commit_details(dir: &Path, hash: &str) -> Option<CommitDetails> {
    details(dir, hash, true)
}

fn details(dir: &Path, hash: &str, with_branches: bool) -> Option<CommitDetails> {
    let out = run(
        dir,
        &["show", "-s", "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%cn%x00%ct%x00%B", hash],
    );
    if !out.ok {
        return None;
    }
    let f: Vec<&str> = out.stdout.splitn(8, '\0').collect();
    if f.len() < 8 {
        return None;
    }
    let parents: Vec<String> = f[1].split_whitespace().map(String::from).collect();
    let base = parents.first().cloned().unwrap_or_else(|| EMPTY_TREE.to_string());
    let files = parse_name_status(
        &run(dir, &["diff", "--name-status", "-z", "-M", &base, hash]).stdout,
    );
    let (mut parent_files, mut parent_subjects, mut merge_files) = (vec![], vec![], vec![]);
    if parents.len() > 1 {
        parent_files.push(files.clone());
        for p in &parents[1..] {
            parent_files.push(parse_name_status(&run(dir, &["diff", "--name-status", "-z", "-M", p, hash]).stdout));
        }
        for p in &parents {
            parent_subjects.push(run(dir, &["log", "-1", "--format=%s", p]).stdout.trim().to_string());
        }
        merge_files = files
            .iter()
            .filter(|f| parent_files[1..].iter().all(|list| list.iter().any(|g| g.path == f.path)))
            .cloned()
            .collect();
    }
    let (containing, tags, signature) = if with_branches { commit_refs(dir, hash) } else { (vec![], vec![], None) };
    Some(CommitDetails {
        tags,
        signature,
        refs_loading: false,
        hash: f[0].to_string(),
        parents,
        author: f[2].to_string(),
        email: f[3].to_string(),
        author_time: f[4].parse().unwrap_or(0),
        committer: f[5].to_string(),
        commit_time: f[6].parse().unwrap_or(0),
        message: f[7].trim_end().to_string(),
        files,
        containing,
        parent_files,
        parent_subjects,
        merge_files,
    })
}

/// The annotated tags that point at `hash`: (name, tagger, time, message). Lightweight tags
/// have no message and are left out.
pub fn annotated_tags(dir: &Path, hash: &str) -> Vec<(String, String, i64, String)> {
    let out = run(dir, &["for-each-ref", "--points-at", hash, "--format=%(refname:short)%00%(objecttype)%00%(taggername)%00%(taggerdate:unix)%00%(contents)%1e", "refs/tags"]);
    out.stdout
        .split('\x1e')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim_start_matches('\n').splitn(5, '\0').collect();
            if f.len() < 5 || f[1] != "tag" {
                return None;
            }
            // A signed tag carries its signature after the message.
            let msg = f[4].split("-----BEGIN PGP SIGNATURE-----").next().unwrap_or("").split("-----BEGIN SSH SIGNATURE-----").next().unwrap_or("").trim();
            Some((f[0].to_string(), f[2].to_string(), f[3].trim().parse().unwrap_or(0), msg.to_string()))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct StatusEntry {
    pub path: String,
    pub old_path: Option<String>,
    pub x: char,
    pub y: char,
}

impl StatusEntry {
    pub fn is_untracked(&self) -> bool {
        self.x == '?'
    }
    pub fn is_conflict(&self) -> bool {
        matches!(
            (self.x, self.y),
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D')
        )
    }
    /// Single letter shown like the IDE colours files.
    pub fn kind(&self) -> char {
        if self.is_untracked() {
            '?'
        } else if self.is_conflict() {
            'U'
        } else if self.x == 'A' {
            'A'
        } else if self.x == 'D' || self.y == 'D' {
            'D'
        } else if self.x == 'R' {
            'R'
        } else {
            'M'
        }
    }
}

pub fn status(dir: &Path) -> Vec<StatusEntry> {
    let out = run(dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]);
    let parts: Vec<&str> = out.stdout.split('\0').collect();
    let mut v = vec![];
    let mut i = 0;
    while i < parts.len() {
        let p = parts[i];
        if p.len() < 4 {
            i += 1;
            continue;
        }
        let mut ch = p.chars();
        let x = ch.next().unwrap_or(' ');
        let y = ch.next().unwrap_or(' ');
        let path = p[3..].to_string();
        let mut old_path = None;
        if x == 'R' || x == 'C' {
            old_path = parts.get(i + 1).map(|s| s.to_string());
            i += 1;
        }
        v.push(StatusEntry { path, old_path, x, y });
        i += 1;
    }
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

/// Show ignored files in the Changes view (set from the view options).
pub static SHOW_IGNORED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Files and folders (ending in '/') that .gitignore hides.
pub fn ignored(dir: &Path) -> Vec<String> {
    let out = run(dir, &["ls-files", "--others", "--ignored", "--exclude-standard", "--directory", "-z"]);
    let mut v: Vec<String> = out.stdout.split('\0').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
    v.sort();
    v
}

#[derive(Clone, Debug)]
pub struct Stash {
    pub name: String,
    pub message: String,
    pub time: i64,
    /// The stash's commit: unlike `stash@{n}` it stays the same when other stashes come and go.
    pub hash: String,
}

/// A file kept in a stash.
#[derive(Clone, Debug)]
pub struct StashFile {
    pub file: ChangedFile,
    /// The file was not under Git when it was put aside (kept in the stash's third parent).
    pub untracked: bool,
}

/// What a stash holds: the changed files, then the unversioned ones put aside with them.
pub fn stash_files(dir: &Path, stash: &str) -> Vec<StashFile> {
    let base = format!("{stash}^1");
    let mut v: Vec<StashFile> = parse_name_status(&run(dir, &["diff", "--name-status", "-z", "-M", &base, stash]).stdout).into_iter().map(|file| StashFile { file, untracked: false }).collect();
    let extra = format!("{stash}^3");
    if run(dir, &["rev-parse", "-q", "--verify", &format!("{extra}^{{commit}}")]).ok {
        let out = run(dir, &["ls-tree", "-r", "-z", "--name-only", &extra]);
        v.extend(out.stdout.split('\0').filter(|p| !p.is_empty()).map(|p| StashFile { file: ChangedFile { status: 'A', path: p.to_string(), old_path: None }, untracked: true }));
    }
    v
}

/// The diff of one file of a stash against what it was made from.
pub fn stash_file_diff(dir: &Path, stash: &str, f: &StashFile) -> DiffText {
    if f.untracked {
        commit_file_diff(dir, &format!("{stash}^3"), None, &f.file)
    } else {
        commit_file_diff(dir, stash, Some(&format!("{stash}^1")), &f.file)
    }
}

/// The command that brings one file of a stash into the working folder as it is in the stash
/// (the index is left alone). None for a file the stash deletes.
pub fn stash_take_args(stash: &str, f: &StashFile) -> Option<Vec<String>> {
    if f.file.status == 'D' {
        return None;
    }
    let source = if f.untracked { format!("{stash}^3") } else { stash.to_string() };
    Some(vec!["restore".into(), format!("--source={source}"), "--worktree".into(), "--".into(), f.file.path.clone()])
}

/// `git stash push` with its options; `paths` (when not empty) are the only files put aside.
pub fn stash_push_args(message: &str, untracked: bool, keep_index: bool, paths: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec!["stash".into(), "push".into()];
    if untracked {
        args.push("--include-untracked".into());
    }
    if keep_index {
        args.push("--keep-index".into());
    }
    if !message.trim().is_empty() {
        args.push("-m".into());
        args.push(message.trim().to_string());
    }
    if !paths.is_empty() {
        args.push("--".into());
        args.extend(paths.iter().cloned());
    }
    args
}

pub fn stashes(dir: &Path) -> Vec<Stash> {
    run(dir, &["stash", "list", "--format=%gd%x00%gs%x00%ct%x00%H"])
        .stdout
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\0').collect();
            if f.len() < 3 {
                return None;
            }
            Some(Stash {
                name: f[0].to_string(),
                message: f[1].to_string(),
                time: f[2].parse().unwrap_or(0),
                hash: f.get(3).unwrap_or(&f[0]).to_string(),
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffSide {
    Context,
    Removed,
    Added,
    Changed,
}

#[derive(Clone, Debug)]
pub struct DiffRow {
    pub left: Option<(usize, String)>,
    pub right: Option<(usize, String)>,
    pub kind: DiffSide,
}

#[derive(Clone, Debug, Default)]
pub struct DiffText {
    pub rows: Vec<DiffRow>,
    pub binary: bool,
    pub unified: Vec<(char, String)>,
    /// For pictures: the file before and after, shown in the image viewer.
    pub images: Option<ImagePair>,
}

#[derive(Clone, Debug, Default)]
pub struct ImagePair {
    pub path: String,
    pub before: Option<std::sync::Arc<Vec<u8>>>,
    pub after: Option<std::sync::Arc<Vec<u8>>>,
}

/// File types the image viewer can show.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "jpe", "jfif", "gif", "bmp", "dib", "ico", "cur", "tif", "tiff", "webp", "tga", "pnm", "pbm", "pgm", "ppm", "pam",
    "qoi", "hdr", "dds", "exr", "ff", "farbfeld", "svg", "svgz",
];

pub fn is_image_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The bytes of a file in a commit ("rev:path"), or None if it is not there.
pub fn blob(dir: &Path, rev: &str, path: &str) -> Option<Vec<u8>> {
    let out = command(dir, &["show", &format!("{rev}:{path}")]).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Turn unified diff output (with full context) into side-by-side rows.
pub fn parse_diff(text: &str) -> DiffText {
    let mut d = DiffText::default();
    let mut in_hunk = false;
    let mut l = 0usize;
    let mut r = 0usize;
    let mut minus: Vec<String> = vec![];
    let mut plus: Vec<String> = vec![];
    fn flush(d: &mut DiffText, minus: &mut Vec<String>, plus: &mut Vec<String>, l: &mut usize, r: &mut usize) {
        let n = minus.len().max(plus.len());
        for i in 0..n {
            let left = minus.get(i).map(|s| {
                *l += 1;
                (*l, s.clone())
            });
            let right = plus.get(i).map(|s| {
                *r += 1;
                (*r, s.clone())
            });
            let kind = match (&left, &right) {
                (Some(_), Some(_)) => DiffSide::Changed,
                (Some(_), None) => DiffSide::Removed,
                _ => DiffSide::Added,
            };
            d.rows.push(DiffRow { left, right, kind });
        }
        minus.clear();
        plus.clear();
    }
    for line in text.lines() {
        if line.starts_with("Binary files") {
            d.binary = true;
            continue;
        }
        if let Some(h) = line.strip_prefix("@@ ") {
            flush(&mut d, &mut minus, &mut plus, &mut l, &mut r);
            // @@ -a,b +c,d @@
            let mut it = h.split_whitespace();
            let a = it.next().unwrap_or("-1");
            let c = it.next().unwrap_or("+1");
            let a: usize = a.trim_start_matches('-').split(',').next().unwrap_or("1").parse().unwrap_or(1);
            let c: usize = c.trim_start_matches('+').split(',').next().unwrap_or("1").parse().unwrap_or(1);
            if !d.rows.is_empty() {
                d.rows.push(DiffRow {
                    left: None,
                    right: None,
                    kind: DiffSide::Context,
                });
            }
            l = a.saturating_sub(1);
            r = c.saturating_sub(1);
            if a == 0 {
                l = 0;
            }
            if c == 0 {
                r = 0;
            }
            in_hunk = true;
            d.unified.push(('@', line.to_string()));
            continue;
        }
        if !in_hunk {
            continue;
        }
        let (tag, rest) = match line.chars().next() {
            Some(c) => (c, &line[c.len_utf8()..]),
            None => (' ', ""),
        };
        match tag {
            '-' => {
                minus.push(rest.to_string());
                d.unified.push(('-', rest.to_string()));
            }
            '+' => {
                plus.push(rest.to_string());
                d.unified.push(('+', rest.to_string()));
            }
            '\\' => {}
            _ => {
                flush(&mut d, &mut minus, &mut plus, &mut l, &mut r);
                l += 1;
                r += 1;
                d.rows.push(DiffRow {
                    left: Some((l, rest.to_string())),
                    right: Some((r, rest.to_string())),
                    kind: DiffSide::Context,
                });
                d.unified.push((' ', rest.to_string()));
            }
        }
    }
    flush(&mut d, &mut minus, &mut plus, &mut l, &mut r);
    d
}

/// Diff of a file inside a commit (against its first parent).
/// How diffs for viewing treat white space: 0 = every change counts, 1 = changes in the
/// amount of white space are left out (`-b`), 2 = all white space is (`-w`).
static DIFF_WS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn set_diff_ws(mode: u8) {
    DIFF_WS.store(mode.min(2), std::sync::atomic::Ordering::Relaxed);
}

pub fn diff_ws() -> u8 {
    DIFF_WS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The option of `git diff` for a white-space mode.
pub fn ws_flag(mode: u8) -> Option<&'static str> {
    match mode {
        1 => Some("-b"),
        2 => Some("-w"),
        _ => None,
    }
}

/// Changed files whose lines differ from HEAD only by their endings (CRLF against LF): git
/// lists them as changed, but with the endings left out nothing is different.
pub fn eol_only(dir: &Path) -> std::collections::HashSet<String> {
    let names = |extra: &[&str]| -> Option<std::collections::HashSet<String>> {
        let mut args = vec!["diff", "HEAD", "--name-only", "-z"];
        args.extend_from_slice(extra);
        let out = run(dir, &args);
        out.ok.then(|| out.stdout.split('\0').filter(|p| !p.is_empty()).map(String::from).collect())
    };
    match (names(&[]), names(&["--ignore-cr-at-eol"])) {
        (Some(all), Some(real)) => all.difference(&real).cloned().collect(),
        _ => Default::default(),
    }
}

pub fn commit_file_diff(dir: &Path, hash: &str, parent: Option<&str>, file: &ChangedFile) -> DiffText {
    let base = parent.unwrap_or(EMPTY_TREE);
    let mut args = vec!["diff", "--no-ext-diff", "-M", "--unified=100000"];
    args.extend(ws_flag(diff_ws()));
    args.extend([base, hash, "--"]);
    if let Some(o) = &file.old_path {
        args.push(o);
    }
    args.push(&file.path);
    let mut d = parse_diff(&run(dir, &args).stdout);
    if is_image_path(&file.path) {
        let old = file.old_path.as_deref().unwrap_or(&file.path);
        d.images = Some(ImagePair {
            path: file.path.clone(),
            before: if file.status == 'A' { None } else { parent.and_then(|p| blob(dir, p, old)).map(std::sync::Arc::new) },
            after: if file.status == 'D' { None } else { blob(dir, hash, &file.path).map(std::sync::Arc::new) },
        });
    }
    d
}

/// Diff of a local change against HEAD, for viewing (white space as chosen in the diff).
pub fn worktree_diff(dir: &Path, e: &StatusEntry, has_head: bool) -> DiffText {
    worktree_diff_with(dir, e, has_head, ws_flag(diff_ws()))
}

/// Diff of a local change against HEAD with every change in it: what partial commits and
/// reverting single changes are worked out from.
pub fn worktree_diff_exact(dir: &Path, e: &StatusEntry, has_head: bool) -> DiffText {
    worktree_diff_with(dir, e, has_head, None)
}

fn worktree_diff_with(dir: &Path, e: &StatusEntry, has_head: bool, ws: Option<&str>) -> DiffText {
    let mut d = worktree_text_diff(dir, e, has_head, ws);
    if is_image_path(&e.path) {
        let old = e.old_path.as_deref().unwrap_or(&e.path);
        let before = if e.is_untracked() || !has_head || e.x == 'A' { None } else { blob(dir, "HEAD", old) };
        let after = std::fs::read(dir.join(&e.path)).ok();
        d.images = Some(ImagePair { path: e.path.clone(), before: before.map(std::sync::Arc::new), after: after.map(std::sync::Arc::new) });
    }
    d
}

fn worktree_text_diff(dir: &Path, e: &StatusEntry, has_head: bool, ws: Option<&str>) -> DiffText {
    if e.is_untracked() {
        let out = run(dir, &["diff", "--no-index", "--unified=100000", "--", "/dev/null", &e.path]);
        return parse_diff(&out.stdout);
    }
    let base = if has_head { "HEAD" } else { EMPTY_TREE };
    let mut args = vec!["diff", "--no-ext-diff", "-M", "--unified=100000"];
    args.extend(ws);
    args.extend([base, "--"]);
    if let Some(o) = &e.old_path {
        args.push(o);
    }
    args.push(&e.path);
    parse_diff(&run(dir, &args).stdout)
}

/// Cheap fingerprint of the repository metadata, used to notice outside changes.
pub fn fingerprint(dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let gd = dir.join(".git");
    let gd = if gd.is_file() {
        // worktree: .git file points elsewhere
        std::fs::read_to_string(&gd)
            .ok()
            .and_then(|s| s.strip_prefix("gitdir:").map(|p| PathBuf::from(p.trim())))
            .unwrap_or(gd)
    } else {
        gd
    };
    fn visit(p: &Path, h: &mut impl Hasher, depth: usize) {
        if let Ok(m) = std::fs::metadata(p) {
            if let Ok(t) = m.modified() {
                t.hash(h);
            }
            m.len().hash(h);
            if m.is_dir() && depth < 6 {
                if let Ok(rd) = std::fs::read_dir(p) {
                    for e in rd.flatten() {
                        e.file_name().hash(h);
                        visit(&e.path(), h, depth + 1);
                    }
                }
            }
        }
    }
    for name in ["HEAD", "index", "packed-refs", "refs", "MERGE_HEAD", "FETCH_HEAD", "logs/HEAD"] {
        let p = gd.join(name);
        name.hash(&mut h);
        visit(&p, &mut h, 0);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_progress() {
        assert_eq!(progress_share("Receiving objects:  45% (450/1000), 1.2 MiB | 600 KiB/s"), Some(0.45));
        assert_eq!(progress_share("remote: Compressing objects: 100% (3/3), done."), Some(1.0));
        assert_eq!(progress_share("remote: Enumerating objects: 5, done."), None);
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(with_progress(&v(&["fetch", "--all"])), v(&["fetch", "--progress", "--all"]));
        assert_eq!(with_progress(&v(&["commit", "-m", "x"])), v(&["commit", "-m", "x"]));
        assert_eq!(with_progress(&v(&["push", "-q", "origin"])), v(&["push", "-q", "origin"]));
    }

    #[test]
    fn live_run_collects_output_and_can_be_cancelled() {
        let dir = std::env::temp_dir().join(format!("ge-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(run(&dir, &["init", "-q"]).ok);
        let live = Live::default();
        live.begin("git rev-parse");
        let out = run_live(&dir, &[], &["rev-parse", "--is-inside-work-tree", "HEAD"], &live, &|| {});
        live.end(out.ok, &out.text());
        // stdout and stderr are both there, as with `run`.
        assert!(!out.ok && out.stdout.starts_with("true") && out.stderr.contains("ambiguous argument"), "{out:?}");
        let cmds = live.cmds();
        assert_eq!((cmds.len(), cmds[0].ok), (1, Some(false)));

        // A command that waits for ever (reads a pipe nobody writes to) is stopped by cancel.
        #[cfg(unix)]
        {
            let fifo = dir.join("wait");
            assert!(Command::new("mkfifo").arg(&fifo).status().unwrap().success());
            let live = Live::default();
            let l2 = live.clone();
            let stopper = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                l2.cancel();
            });
            let started = std::time::Instant::now();
            let out = run_live(&dir, &[], &["hash-object", "wait"], &live, &|| {});
            stopper.join().unwrap();
            assert!(!out.ok && live.cancelled() && started.elapsed() < std::time::Duration::from_secs(10));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_the_list_of_worktrees() {
        let text = "worktree /work/app\nHEAD 1111\nbranch refs/heads/main\n\nworktree /work/app-fix\nHEAD 2222\nbranch refs/heads/fix/login\nlocked moving\n\nworktree /work/app-old\nHEAD 3333\ndetached\nprunable gitdir file points to non-existent location\n";
        let w = parse_worktrees(text);
        assert_eq!(w.len(), 3);
        assert_eq!((w[0].path.to_str(), w[0].branch.as_deref(), w[0].main, w[0].locked), (Some("/work/app"), Some("main"), true, false));
        assert_eq!((w[1].branch.as_deref(), w[1].main, w[1].locked, w[1].prunable), (Some("fix/login"), false, true, false));
        assert_eq!((w[2].branch.as_deref(), w[2].head.as_str(), w[2].prunable), (None, "3333", true));
        assert!(parse_worktrees("").is_empty());
    }

    #[test]
    fn stash_shows_its_files_and_gives_one_back() {
        let dir = std::env::temp_dir().join(format!("ge-stash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let who = [("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")];
        let g = |args: &[&str]| assert!(run_env(&dir, &who, args).ok, "{args:?}");
        let put = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
        let get = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        g(&["init", "-q"]);
        put("a.txt", "one\n");
        put("src/b.txt", "one\n");
        put("gone.txt", "x\n");
        g(&["add", "-A"]);
        g(&["commit", "-qm", "base"]);
        put("a.txt", "two\n");
        put("src/b.txt", "two\n");
        put("new.txt", "fresh\n");
        std::fs::remove_file(dir.join("gone.txt")).unwrap();

        // Only the chosen files are put aside; the new one makes git take unversioned files.
        let args = stash_push_args(" part ", true, false, &["a.txt".to_string(), "new.txt".to_string()]);
        assert_eq!(args, ["stash", "push", "--include-untracked", "-m", "part", "--", "a.txt", "new.txt"]);
        g(&args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        assert_eq!((get("a.txt").as_str(), get("src/b.txt").as_str(), dir.join("new.txt").exists()), ("one\n", "two\n", false));
        g(&["stash", "push", "-m", "rest"]);
        assert_eq!(stash_push_args("", false, true, &[]), ["stash", "push", "--keep-index"]);

        let list = stashes(&dir);
        assert_eq!(list.iter().map(|s| s.message.as_str()).collect::<Vec<_>>(), ["On master: rest", "On master: part"].map(|m| m.replace("master", &run(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).stdout.trim().to_string())));
        assert!(list.iter().all(|s| s.hash.len() == 40));
        let names = |s: &Stash| stash_files(&dir, &s.hash).iter().map(|f| format!("{}{} {}", f.file.status, if f.untracked { "?" } else { "" }, f.file.path)).collect::<Vec<_>>();
        assert_eq!(names(&list[0]), ["D gone.txt", "M src/b.txt"]);
        assert_eq!(names(&list[1]), ["M a.txt", "A? new.txt"]);

        // The diff of a file of the stash, and one file taken out of it; the stash stays.
        let files = stash_files(&dir, &list[1].hash);
        let d = stash_file_diff(&dir, &list[1].hash, &files[0]);
        assert!(d.unified.iter().any(|(t, l)| *t == '-' && l == "one") && d.unified.iter().any(|(t, l)| *t == '+' && l == "two"));
        assert!(stash_file_diff(&dir, &list[1].hash, &files[1]).unified.iter().any(|(t, l)| *t == '+' && l == "fresh"));
        for f in &files {
            let take = stash_take_args(&list[1].hash, f).unwrap();
            g(&take.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        }
        assert_eq!((get("a.txt").as_str(), get("new.txt").as_str(), get("src/b.txt").as_str()), ("two\n", "fresh\n", "one\n"));
        assert_eq!(stashes(&dir).len(), 2);
        // A file the stash deletes cannot be taken.
        let gone = &stash_files(&dir, &list[0].hash)[0];
        assert!(stash_take_args(&list[0].hash, gone).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_history_follows_renames() {
        let dir = std::env::temp_dir().join(format!("ge-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| assert!(run_env(&dir, &[("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")], args).ok, "{args:?}");
        g(&["init", "-q"]);
        let text: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("old name.txt"), &text).unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "add"]);
        std::fs::write(dir.join("other.txt"), "x\n").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "unrelated"]);
        g(&["mv", "old name.txt", "new.txt"]);
        g(&["commit", "-qm", "rename"]);
        std::fs::write(dir.join("new.txt"), format!("{text}more\n")).unwrap();
        g(&["commit", "-qam", "edit"]);

        let q = |follow| LogQuery { first_parent: false, branch: None, paths: vec!["new.txt".into()], limit: 100, not_merged: false, follow, ..Default::default() };
        let subjects = |follow| load_log(&dir, &q(follow)).iter().map(|c| c.subject.clone()).collect::<Vec<_>>();
        // The plain path filter stops at the rename; the history goes through it.
        assert_eq!(subjects(false), ["edit", "rename"]);
        assert_eq!(subjects(true), ["edit", "rename", "add"]);
        let log = load_log(&dir, &q(true));
        let names = file_names(&dir, "HEAD", "new.txt", 100);
        let at = |i: usize| names.get(&log[i].hash).map(String::as_str);
        assert_eq!((at(0), at(1), at(2)), (Some("new.txt"), Some("new.txt"), Some("old name.txt")));

        // The history of some lines: line 31 ("more") was added by the last commit only; line 1
        // goes back through the rename to the first commit.
        let by_lines = |a, b| load_log(&dir, &LogQuery { lines: Some((a, b)), ..q(true) }).iter().map(|c| c.subject.clone()).collect::<Vec<_>>();
        assert_eq!(by_lines(31, 31), ["edit"]);
        assert_eq!(by_lines(1, 2), ["add"]);
        // Every branch: a commit on another branch that changed the file is listed too.
        g(&["checkout", "-q", "-b", "side", "HEAD~1"]);
        std::fs::write(dir.join("new.txt"), "side\n").unwrap();
        g(&["commit", "-qam", "side edit"]);
        g(&["checkout", "-q", "-"]);
        assert_eq!(subjects(true), ["edit", "rename", "add"]);
        let all: Vec<String> = load_log(&dir, &LogQuery { all: true, ..q(true) }).iter().map(|c| c.subject.clone()).collect();
        assert!(all.contains(&"side edit".to_string()) && all.contains(&"edit".to_string()) && all.contains(&"add".to_string()), "{all:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
