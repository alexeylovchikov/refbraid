//! Reflog: where HEAD has been. Every commit, checkout, reset, rebase, merge and pull leaves a
//! record, so a commit that no branch holds any more (after a reset, a rebase or a deleted
//! branch) can still be found here and brought back.

use crate::git;
use std::collections::HashSet;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The commit HEAD pointed at after the action.
    pub hash: String,
    /// When the action happened.
    pub time: i64,
    /// What moved HEAD: "commit", "reset", "checkout", "rebase (finish)", …
    pub action: String,
    /// The rest of the record: "moving to HEAD~1", the commit subject, …
    pub detail: String,
    /// The subject of the commit.
    pub subject: String,
    /// No branch, tag or other ref reaches the commit: only the reflog still knows it.
    pub lost: bool,
}

/// Read the output of `git reflog --date=unix --format=%H%x00%gd%x00%gs%x00%s%x1e`.
pub fn parse(text: &str) -> Vec<Entry> {
    text.split('\x1e')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim_start_matches('\n').split('\0').collect();
            if f.len() < 4 || f[0].is_empty() {
                return None;
            }
            // "HEAD@{1700000000}" with --date=unix.
            let time = f[1].split_once("@{").and_then(|(_, t)| t.trim_end_matches('}').parse().ok()).unwrap_or(0);
            let (action, detail) = f[2].split_once(": ").unwrap_or((f[2], ""));
            Some(Entry { hash: f[0].to_string(), time, action: action.to_string(), detail: detail.to_string(), subject: f[3].to_string(), lost: false })
        })
        .collect()
}

/// The newest `limit` records of HEAD's reflog, with the unreachable commits marked.
pub fn load(dir: &Path, limit: usize) -> Vec<Entry> {
    let n = format!("-n{limit}");
    let out = git::run(dir, &["reflog", "show", "--date=unix", "--format=%H%x00%gd%x00%gs%x00%s%x1e", &n, "HEAD"]);
    let mut entries = parse(&out.stdout);
    let hashes: Vec<&str> = entries.iter().map(|e| e.hash.as_str()).collect::<HashSet<_>>().into_iter().collect();
    if hashes.is_empty() {
        return entries;
    }
    // Commits reachable from the reflog's commits but from no ref: the lost ones are among them.
    let mut args = vec!["rev-list"];
    args.extend(hashes);
    args.extend(["--not", "--all", "HEAD"]);
    let lost: HashSet<String> = git::run(dir, &args).stdout.lines().map(|l| l.trim().to_string()).collect();
    for e in entries.iter_mut() {
        e.lost = lost.contains(&e.hash);
    }
    entries
}

/// The Reflog tab.
pub struct View {
    pub entries: Vec<Entry>,
    pub selected: usize,
    /// Show only the records of commits no branch reaches.
    pub only_lost: bool,
    /// The selected record's commit.
    pub details: Option<git::CommitDetails>,
    /// The repository state the records were read for (they are read again when it changes).
    pub fingerprint: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records() {
        let h = "a".repeat(40);
        let text = format!("{h}\0HEAD@{{1700000100}}\0reset: moving to HEAD~1\0Add login\x1e\n{h}\0HEAD@{{1700000000}}\0commit (amend): Add login\0Add login\x1e\n");
        let e = parse(&text);
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].time, e[0].action.as_str(), e[0].detail.as_str(), e[0].subject.as_str()), (1700000100, "reset", "moving to HEAD~1", "Add login"));
        assert_eq!((e[1].action.as_str(), e[1].detail.as_str()), ("commit (amend)", "Add login"));
    }

    #[test]
    fn finds_a_commit_lost_by_reset() {
        let dir = std::env::temp_dir().join(format!("ge-reflog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| assert!(git::run_env(&dir, &[("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")], args).ok, "{args:?}");
        g(&["init", "-q"]);
        for m in ["one", "two", "three"] {
            std::fs::write(dir.join("f.txt"), m).unwrap();
            g(&["add", "-A"]);
            g(&["commit", "-qm", m]);
        }
        let three = git::run(&dir, &["rev-parse", "HEAD"]).stdout.trim().to_string();
        g(&["reset", "-q", "--hard", "HEAD~1"]);

        let e = load(&dir, 100);
        assert_eq!(e[0].action, "reset");
        assert_eq!((e[0].subject.as_str(), e[0].lost), ("two", false));
        let lost: Vec<&Entry> = e.iter().filter(|x| x.lost).collect();
        assert_eq!(lost.len(), 1);
        assert_eq!((lost[0].hash.as_str(), lost[0].subject.as_str(), lost[0].action.as_str()), (three.as_str(), "three", "commit"));
        // A branch on it makes it reachable again.
        g(&["branch", "saved", &three]);
        assert!(load(&dir, 100).iter().all(|x| !x.lost));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
