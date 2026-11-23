//! Undo of the last action: what the branches, tags, HEAD and stashes were before a job and
//! after it, and the git commands that put them back.

use crate::git;
use crate::i18n::{tr, trf};
use std::collections::BTreeMap;
use std::path::Path;

/// The refs a job can move, at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// The commit HEAD is at (empty in a repository without commits).
    pub head: String,
    /// The checked-out branch; None when HEAD is detached.
    pub branch: Option<String>,
    /// Full name ("refs/heads/main", "refs/tags/v1") → (hash, upstream of a branch).
    pub refs: BTreeMap<String, (String, String)>,
    /// The stashes, newest first: (hash, message).
    pub stashes: Vec<(String, String)>,
    /// In the middle of a merge, rebase, cherry-pick or revert: nothing to put back until it
    /// is finished or aborted.
    pub busy: bool,
}

pub fn snapshot(dir: &Path) -> Snapshot {
    let mut s = Snapshot { head: git::run(dir, &["rev-parse", "-q", "--verify", "HEAD"]).stdout.trim().to_string(), ..Default::default() };
    let b = git::run(dir, &["symbolic-ref", "-q", "--short", "HEAD"]);
    s.branch = Some(b.stdout.trim().to_string()).filter(|n| b.ok && !n.is_empty());
    for l in git::run(dir, &["for-each-ref", "--format=%(refname)%00%(objectname)%00%(upstream:short)", "refs/heads", "refs/tags"]).stdout.lines() {
        let f: Vec<&str> = l.split('\0').collect();
        if f.len() >= 2 {
            s.refs.insert(f[0].to_string(), (f[1].to_string(), f.get(2).unwrap_or(&"").to_string()));
        }
    }
    // A bisect moves HEAD from commit to commit: nothing to take back step by step.
    s.busy = git::operation(dir).is_some() || crate::bisect::load(dir).is_some();
    s.stashes = git::run(dir, &["stash", "list", "--format=%H%x00%gs"])
        .stdout
        .lines()
        .filter_map(|l| l.split_once('\0').map(|(h, m)| (h.to_string(), m.to_string())))
        .collect();
    s
}

/// How the checked-out branch goes back to where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    /// `reset --soft`: after a commit or an amend — the changes stay staged.
    Soft,
    /// `reset` (mixed): after a soft or mixed reset — the files are left alone.
    Mixed,
    /// `reset --keep`: after a merge, pull, hard reset, cherry-pick… — the files follow, local
    /// changes are kept, and git stops if they would be overwritten.
    Keep,
}

/// The last action that can be undone.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// The action, as its job was called ("Commit", "Delete feature/x").
    pub title: String,
    /// It is itself an undo, so undoing it does the action again ("Redo").
    pub redo: bool,
    pub before: Snapshot,
    pub after: Snapshot,
    pub back: Back,
    /// A stash was dropped on purpose (bring it back), not popped.
    pub stash_dropped: bool,
    pub stash_pushed: bool,
}

/// Was anything sent to a server? That cannot be taken back from here.
pub fn pushes(cmds: &[Vec<String>]) -> bool {
    cmds.iter().any(|c| first_word(c) == Some("push"))
}

fn first_word(cmd: &[String]) -> Option<&str> {
    cmd.iter().map(String::as_str).find(|a| !a.starts_with("env:"))
}

fn back_mode(cmds: &[Vec<String>]) -> Back {
    let has = |w: &str| cmds.iter().any(|c| first_word(c) == Some(w));
    if ["merge", "pull", "cherry-pick", "revert", "rebase", "checkout", "switch"].iter().any(|w| has(w)) {
        return Back::Keep;
    }
    if let Some(reset) = cmds.iter().find(|c| first_word(c) == Some("reset")) {
        return if reset.iter().any(|a| a == "--soft") {
            Back::Soft
        } else if reset.iter().any(|a| a == "--hard" || a == "--keep" || a == "--merge") {
            Back::Keep
        } else {
            Back::Mixed
        };
    }
    if has("commit") { Back::Soft } else { Back::Keep }
}

impl Record {
    /// The record of a finished job, or None when it moved nothing that can be put back.
    pub fn new(title: &str, cmds: &[Vec<String>], before: Snapshot, after: Snapshot) -> Option<Record> {
        if before == after || before.busy || after.busy {
            return None;
        }
        let stash = |w: &str| cmds.iter().any(|c| first_word(c) == Some("stash") && c.iter().any(|a| a == w));
        let (title, redo) = match title.strip_prefix("Undo ") {
            Some(rest) => (rest.to_string(), true),
            None => (title.strip_prefix("Redo ").unwrap_or(title).to_string(), false),
        };
        let r = Record { title, redo, before, after, back: back_mode(cmds), stash_dropped: stash("drop"), stash_pushed: stash("push") };
        // Only a stash was applied or popped, say: nothing to put back.
        (!r.steps().is_empty()).then_some(r)
    }

    /// "Undo Commit" / "Redo Commit".
    pub fn label(&self) -> String {
        format!("{} {}", if self.redo { "Redo" } else { "Undo" }, self.title)
    }

    /// What undoing does, step by step: (explanation, git command).
    pub fn steps(&self) -> Vec<(String, Vec<String>)> {
        let (b, a) = (&self.before, &self.after);
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let short = |h: &str| h.chars().take(8).collect::<String>();
        let name = |r: &str| r.strip_prefix("refs/heads/").or_else(|| r.strip_prefix("refs/tags/")).unwrap_or(r).to_string();
        let tag = |r: &str| r.starts_with("refs/tags/");
        let mut out: Vec<(String, Vec<String>)> = vec![];
        // A stash made by the action: its changes come back first.
        if self.stash_pushed && a.stashes.len() == b.stashes.len() + 1 && a.stashes.first().is_some_and(|x| !b.stashes.contains(x)) {
            out.push((tr("Apply the stashed changes again and remove the stash").into(), s(&["stash", "pop"])));
        }
        // Back to the branch (or commit) that was checked out.
        let switched = a.branch != b.branch || (b.branch.is_none() && a.head != b.head);
        if switched && !b.head.is_empty() {
            match &b.branch {
                Some(n) => out.push((trf("Switch back to the branch '{}'", &[n]), s(&["checkout", n]))),
                None => out.push((trf("Switch back to the commit {}", &[&short(&b.head)]), s(&["checkout", "--detach", &b.head]))),
            }
        }
        // The branch checked out before the action, if the action moved it.
        let cur = b.branch.as_ref().map(|n| format!("refs/heads/{n}"));
        if let Some(r) = &cur {
            if let (Some((old, _)), Some((new, _))) = (b.refs.get(r), a.refs.get(r)) {
                if old != new {
                    let (flag, what) = match self.back {
                        Back::Soft => ("--soft", "its changes stay in the Changes list"),
                        Back::Mixed => ("--mixed", "the files are left as they are"),
                        Back::Keep => ("--keep", "your local changes are kept"),
                    };
                    out.push((trf("Move '{}' back from {} to {} ({})", &[&name(r), &short(new), &short(old), &tr(what)]), s(&["reset", flag, old])));
                }
            }
        }
        for (r, (old, up)) in &b.refs {
            if Some(r) == cur.as_ref() {
                continue;
            }
            match a.refs.get(r) {
                None => {
                    out.push((trf(if tag(r) { "Bring back the tag '{}' at {}" } else { "Bring back the branch '{}' at {}" }, &[&name(r), &short(old)]), s(&["update-ref", r, old])));
                    if !up.is_empty() && r.starts_with("refs/heads/") {
                        out.push((trf("Track '{}' again", &[up]), s(&["branch", &format!("--set-upstream-to={up}"), &name(r)])));
                    }
                }
                Some((new, _)) if new != old => out.push((trf(if tag(r) { "Move the tag '{}' back from {} to {}" } else { "Move the branch '{}' back from {} to {}" }, &[&name(r), &short(new), &short(old)]), s(&["update-ref", r, old, new]))),
                _ => {}
            }
        }
        for (r, (new, _)) in &a.refs {
            if !b.refs.contains_key(r) {
                out.push((trf(if tag(r) { "Delete the tag '{}' the action created" } else { "Delete the branch '{}' the action created" }, &[&name(r)]), s(&["update-ref", "-d", r, new])));
            }
        }
        if self.stash_dropped {
            for (h, m) in b.stashes.iter().filter(|x| !a.stashes.contains(x)) {
                out.push((trf("Bring back the stash \"{}\"", &[m]), s(&["stash", "store", "-m", m, h])));
            }
        }
        out
    }

    /// The repository is still as the action left it? Otherwise undoing could lose newer work.
    pub fn still_valid(&self, now: &Snapshot) -> bool {
        *now == self.after
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ge-undo-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        g(&dir, &["init", "-q", "-b", "main"]);
        for m in ["one", "two", "three"] {
            std::fs::write(dir.join("f.txt"), m).unwrap();
            g(&dir, &["add", "-A"]);
            g(&dir, &["commit", "-qm", m]);
        }
        dir
    }

    fn g(dir: &Path, args: &[&str]) {
        assert!(git::run_env(dir, &[("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")], args).ok, "{args:?}");
    }

    /// Run `cmds` as a job would, undo them, and give back the record.
    fn act(dir: &Path, title: &str, cmds: &[&[&str]]) -> Record {
        let before = snapshot(dir);
        for c in cmds {
            g(dir, c);
        }
        let owned: Vec<Vec<String>> = cmds.iter().map(|c| c.iter().map(|s| s.to_string()).collect()).collect();
        Record::new(title, &owned, before, snapshot(dir)).expect("something to undo")
    }

    fn undo(dir: &Path, r: &Record) {
        assert!(r.still_valid(&snapshot(dir)));
        for (_, c) in r.steps() {
            g(dir, &c.iter().map(String::as_str).collect::<Vec<_>>());
        }
        assert_eq!(snapshot(dir), r.before, "{:?}", r.steps());
    }

    #[test]
    fn undoes_a_commit_keeping_its_changes() {
        let dir = repo("commit");
        std::fs::write(dir.join("f.txt"), "four").unwrap();
        let r = act(&dir, "Commit", &[&["add", "-A"], &["commit", "-qm", "four"]]);
        assert_eq!((r.label().as_str(), r.back), ("Undo Commit", Back::Soft));
        undo(&dir, &r);
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "four");
        assert_eq!(git::run(&dir, &["status", "--porcelain"]).stdout.trim(), "M  f.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undoes_a_hard_reset_and_a_deleted_branch() {
        let dir = repo("reset");
        let r = act(&dir, "Reset", &[&["reset", "-q", "--hard", "HEAD~2"]]);
        assert_eq!(r.back, Back::Keep);
        undo(&dir, &r);
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "three");

        g(&dir, &["branch", "feature", "HEAD~1"]);
        g(&dir, &["tag", "v1"]);
        let r = act(&dir, "Delete feature", &[&["branch", "-D", "feature"], &["tag", "-d", "v1"]]);
        assert_eq!(r.steps().len(), 2);
        undo(&dir, &r);
        // The undo is an action too: undoing it deletes them again.
        let again = act(&dir, "Undo Delete feature", &[&["branch", "-D", "feature"]]);
        assert_eq!(again.label(), "Redo Delete feature");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undoes_checkouts_merges_and_new_branches() {
        let dir = repo("switch");
        g(&dir, &["branch", "side", "HEAD~1"]);
        let r = act(&dir, "Checkout side", &[&["checkout", "-q", "side"]]);
        assert_eq!(r.steps()[0].1, ["checkout", "main"]);
        undo(&dir, &r);
        // A new branch made and checked out: back, and the branch goes away.
        let r = act(&dir, "Create branch topic", &[&["checkout", "-q", "-b", "topic", "HEAD~2"]]);
        assert_eq!(r.steps().len(), 2);
        undo(&dir, &r);
        // A merge commit.
        g(&dir, &["checkout", "-q", "side"]);
        std::fs::write(dir.join("other.txt"), "x").unwrap();
        g(&dir, &["add", "-A"]);
        g(&dir, &["commit", "-qm", "side work"]);
        g(&dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("local.txt"), "mine").unwrap();
        let r = act(&dir, "Merge side", &[&["merge", "-q", "--no-edit", "side"]]);
        assert!(dir.join("other.txt").exists());
        undo(&dir, &r);
        // The merged file is gone, the local one is still there.
        assert!(!dir.join("other.txt").exists() && dir.join("local.txt").exists());
        // Detached HEAD and back.
        let r = act(&dir, "Checkout", &[&["checkout", "-q", "--detach", "HEAD~1"]]);
        undo(&dir, &r);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn brings_back_a_dropped_stash_and_refuses_a_changed_repository() {
        let dir = repo("stash");
        std::fs::write(dir.join("f.txt"), "work in progress").unwrap();
        let pushed = act(&dir, "Stash", &[&["stash", "push", "-q", "-m", "wip"]]);
        assert_eq!(pushed.steps()[0].1, ["stash", "pop"]);
        let r = act(&dir, "Drop stash", &[&["stash", "drop", "-q"]]);
        assert!(snapshot(&dir).stashes.is_empty());
        undo(&dir, &r);
        assert_eq!(snapshot(&dir).stashes.len(), 1);
        // Undo of the stash itself: the changes are in the files again.
        g(&dir, &["stash", "pop", "-q"]);
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "work in progress");
        // Popping a stash leaves nothing to undo.
        g(&dir, &["stash", "push", "-q"]);
        let before = snapshot(&dir);
        g(&dir, &["stash", "pop", "-q"]);
        assert_eq!(Record::new("Pop stash", &[vec!["stash".into(), "pop".into()]], before, snapshot(&dir)), None);

        g(&dir, &["checkout", "-q", "--", "."]);
        let r = act(&dir, "Reset", &[&["reset", "-q", "--hard", "HEAD~1"]]);
        g(&dir, &["commit", "-q", "--allow-empty", "-m", "newer work"]);
        assert!(!r.still_valid(&snapshot(&dir)));
        assert!(pushes(&[vec!["env:A=b".into(), "push".into(), "origin".into()]]) && !pushes(&[vec!["commit".into()]]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
