//! Bisect: finding the commit that broke something by halving the history between a good and
//! a bad commit. This reads the state git keeps while a bisect is going on.

use crate::git;
use std::path::{Path, PathBuf};

/// A bisect in progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bisect {
    /// The commit known to be bad.
    pub bad: Option<String>,
    /// The commits known to be good, and those that could not be tested.
    pub good: Vec<String>,
    pub skipped: Vec<String>,
    /// The first bad commit, once git has found it.
    pub found: Option<String>,
    /// How many commits may still be the first bad one (the bad commit among them).
    pub left: usize,
}

/// What a commit is in a bisect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Good,
    Bad,
    Skipped,
    /// The first bad commit.
    Found,
}

impl Mark {
    pub fn word(self) -> &'static str {
        match self {
            Mark::Good => "good",
            Mark::Bad => "bad",
            Mark::Skipped => "skipped",
            Mark::Found => "first bad",
        }
    }
}

impl Bisect {
    pub fn mark(&self, hash: &str) -> Option<Mark> {
        if self.found.as_deref() == Some(hash) {
            Some(Mark::Found)
        } else if self.bad.as_deref() == Some(hash) {
            Some(Mark::Bad)
        } else if self.good.iter().any(|g| g == hash) {
            Some(Mark::Good)
        } else if self.skipped.iter().any(|g| g == hash) {
            Some(Mark::Skipped)
        } else {
            None
        }
    }

    /// Both ends are known, so git is offering commits to test.
    pub fn testing(&self) -> bool {
        self.found.is_none() && self.bad.is_some() && !self.good.is_empty()
    }

    /// About how many more commits will have to be tested.
    pub fn steps(&self) -> usize {
        steps(self.left)
    }
}

/// Tests needed to find one commit among `left`: each test halves what is left.
pub fn steps(left: usize) -> usize {
    let mut n = 0;
    let mut room = 1usize;
    while room < left {
        room *= 2;
        n += 1;
    }
    n
}

/// The first bad commit named in git's bisect log ("# first bad commit: [<hash>] subject").
pub fn found_in_log(log: &str) -> Option<String> {
    let line = log.lines().rev().find(|l| l.starts_with("# first bad commit: ["))?;
    let hash = line.split_once('[')?.1.split_once(']')?.0;
    (!hash.is_empty()).then(|| hash.to_string())
}

/// Sort the refs git keeps under refs/bisect ("<refname>\0<hash>" per line) into bad, good
/// and skipped commits.
pub fn sort_refs(text: &str) -> (Option<String>, Vec<String>, Vec<String>) {
    let (mut bad, mut good, mut skipped) = (None, vec![], vec![]);
    for l in text.lines() {
        let Some((name, hash)) = l.split_once('\0') else { continue };
        let name = name.trim_start_matches("refs/bisect/");
        if name == "bad" || name == "new" {
            bad = Some(hash.to_string());
        } else if name.starts_with("good-") || name.starts_with("old-") {
            good.push(hash.to_string());
        } else if name.starts_with("skip-") {
            skipped.push(hash.to_string());
        }
    }
    (bad, good, skipped)
}

fn git_dir(dir: &Path) -> PathBuf {
    let d = git::run(dir, &["rev-parse", "--git-dir"]).stdout.trim().to_string();
    if Path::new(&d).is_absolute() { PathBuf::from(d) } else { dir.join(d) }
}

/// The bisect the repository is in the middle of, if any.
pub fn load(dir: &Path) -> Option<Bisect> {
    let log = std::fs::read_to_string(git_dir(dir).join("BISECT_LOG")).ok()?;
    let refs = git::run(dir, &["for-each-ref", "--format=%(refname)%00%(objectname)", "refs/bisect"]);
    let (bad, good, skipped) = sort_refs(&refs.stdout);
    let mut b = Bisect { bad, good, skipped, found: found_in_log(&log), left: 0 };
    if let (Some(bad), false) = (&b.bad, b.good.is_empty()) {
        let mut args = vec!["rev-list", "--count", bad.as_str(), "--not"];
        args.extend(b.good.iter().map(|g| g.as_str()));
        b.left = git::run(dir, &args).stdout.trim().parse().unwrap_or(0);
        // One commit left between the good ones and the bad one: that is the first bad commit.
        // (Git also writes it to its bisect log, but not every version does it the same way.)
        if b.found.is_none() && b.left == 1 && b.skipped.is_empty() {
            b.found = Some(bad.clone());
        }
    }
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_refs_and_the_log() {
        let (bad, good, skipped) = sort_refs("refs/bisect/bad\0bbb\nrefs/bisect/good-aaa\0aaa\nrefs/bisect/good-ccc\0ccc\nrefs/bisect/skip-ddd\0ddd\n");
        assert_eq!((bad.as_deref(), good, skipped), (Some("bbb"), vec!["aaa".to_string(), "ccc".to_string()], vec!["ddd".to_string()]));
        assert_eq!(found_in_log("git bisect start\n# bad: [bbb] broke it\ngit bisect bad bbb\n"), None);
        assert_eq!(found_in_log("# good: [aaa] fine\ngit bisect good aaa\n# first bad commit: [eee] the one\n").as_deref(), Some("eee"));
        assert_eq!([0, 1, 2, 3, 4, 5, 8, 9, 100].map(steps), [0, 0, 1, 2, 2, 3, 3, 4, 7]);
    }

    #[test]
    fn finds_the_first_bad_commit() {
        let dir = std::env::temp_dir().join(format!("ge-bisect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let who = [("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")];
        let g = |args: &[&str]| assert!(git::run_env(&dir, &who, args).ok, "{args:?}");
        let rev = |r: &str| git::run(&dir, &["rev-parse", r]).stdout.trim().to_string();
        g(&["init", "-q"]);
        for i in 1..=12 {
            std::fs::write(dir.join("n.txt"), format!("{i}\n")).unwrap();
            g(&["add", "-A"]);
            g(&["commit", "-qm", &format!("commit {i}")]);
        }
        assert!(load(&dir).is_none());
        // Everything from commit 8 on is "broken".
        let culprit = rev("HEAD~4");
        let (bad, good) = (rev("HEAD"), rev("HEAD~11"));
        g(&["bisect", "start", &bad, &good]);
        let b = load(&dir).unwrap();
        assert_eq!((b.bad.as_deref(), b.good.as_slice(), b.left, b.steps()), (Some(bad.as_str()), &[good.clone()][..], 11, 4));
        assert!(b.testing() && b.mark(&bad) == Some(Mark::Bad) && b.mark(&good) == Some(Mark::Good) && b.mark(&culprit).is_none());
        let mut tests = 0;
        while load(&dir).unwrap().testing() {
            let n: u32 = std::fs::read_to_string(dir.join("n.txt")).unwrap().trim().parse().unwrap();
            g(&["bisect", if n >= 8 { "bad" } else { "good" }]);
            tests += 1;
            assert!(tests <= 5);
        }
        let b = load(&dir).unwrap();
        assert_eq!(b.found.as_deref(), Some(culprit.as_str()));
        assert_eq!(b.mark(&culprit), Some(Mark::Found));
        g(&["bisect", "reset"]);
        assert!(load(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
