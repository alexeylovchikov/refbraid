//! For tests: a throw-away repository in the temporary folder, removed again when the test
//! is over, and the few steps every test of a git operation takes.

use crate::app::App;
use crate::git;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct TestRepo {
    pub dir: PathBuf,
}

const WHO: [(&str, &str); 4] = [("GIT_AUTHOR_NAME", "Tester"), ("GIT_AUTHOR_EMAIL", "tester@example.com"), ("GIT_COMMITTER_NAME", "Tester"), ("GIT_COMMITTER_EMAIL", "tester@example.com")];

impl TestRepo {
    /// An empty repository `<temp>/ge-<tag>-<pid>-<n>` with a name and an e-mail of its own, so
    /// commits work whatever the machine's own git settings are.
    pub fn new(tag: &str) -> TestRepo {
        let r = TestRepo::folder(tag);
        r.git(&["init", "-q"]);
        r.identity();
        r
    }

    /// A folder for a repository made some other way (a clone, a bare one).
    pub fn folder(tag: &str) -> TestRepo {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("ge-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The path as git gives it back (on some systems the temporary folder is a link).
        TestRepo { dir: git::plain_path(dir.canonicalize().unwrap()) }
    }

    /// The name and e-mail commits are made with, and line endings left as they are written.
    pub fn identity(&self) {
        self.git(&["config", "user.name", "Tester"]);
        self.git(&["config", "user.email", "tester@example.com"]);
        self.git(&["config", "core.autocrlf", "false"]);
        self.git(&["config", "commit.gpgsign", "false"]);
    }

    /// Run git here; the test stops when it fails. Gives back what it printed.
    pub fn git(&self, args: &[&str]) -> String {
        let out = git::run_env(&self.dir, &WHO, args);
        assert!(out.ok, "git {args:?} failed:\n{}{}", out.stdout, out.stderr);
        out.stdout.trim_end().to_string()
    }

    /// Run git here and say whether it worked.
    pub fn try_git(&self, args: &[&str]) -> bool {
        git::run_env(&self.dir, &WHO, args).ok
    }

    /// Run the commands of a job the way the program does: one after another, each may start
    /// with `env:NAME=value` items; the test stops at the first that fails.
    pub fn run(&self, cmds: &[Vec<String>]) {
        for c in cmds {
            let env: Vec<(&str, &str)> = c.iter().map_while(|s| s.strip_prefix("env:").and_then(|x| x.split_once('='))).chain(WHO).collect();
            let args: Vec<&str> = c.iter().skip_while(|s| s.starts_with("env:")).map(|s| s.as_str()).collect();
            let out = git::run_env(&self.dir, &env, &args);
            assert!(out.ok, "git {args:?} failed:\n{}{}", out.stdout, out.stderr);
        }
    }

    pub fn write(&self, name: &str, text: &str) {
        let p = self.dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    pub fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }

    /// Commit everything there is; gives back the new commit.
    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.head()
    }

    /// Write a file and commit it.
    pub fn commit_file(&self, name: &str, text: &str, message: &str) -> String {
        self.write(name, text);
        self.commit(message)
    }

    pub fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    /// The branch that is checked out.
    pub fn branch(&self) -> String {
        self.git(&["rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// The subjects of the commits of `rev`, newest first.
    pub fn subjects(&self, rev: &str) -> Vec<String> {
        self.git(&["log", "--format=%s", rev]).lines().map(String::from).collect()
    }

    /// The changed files as the program reads them: "XY path".
    pub fn status(&self) -> Vec<String> {
        git::status(&self.dir).iter().map(|e| format!("{}{} {}", e.x, e.y, e.path)).collect()
    }

    /// The program with this repository open (no window, nothing saved to the settings).
    pub fn app(&self) -> App {
        App::new(&eframe::egui::Context::default(), Some(self.dir.clone()), Some(self.dir.join("no-screenshot.png")))
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Wait until the jobs the program started are done.
pub fn settle(app: &mut App) {
    let started = std::time::Instant::now();
    while app.busy() {
        assert!(started.elapsed() < std::time::Duration::from_secs(30), "a job did not finish");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.poll_jobs();
    }
}
