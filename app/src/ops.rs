//! The git commands behind the buttons, put together apart from the windows that ask for
//! them, so each can be run against a real repository in a test.

use crate::app::ResetMode;
use crate::git::{Operation, StatusEntry};

/// The commands of the Commit button: the checked files with `message`. In the middle of a
/// merge, rebase and the like (`op`) the operation's own commit is concluded with them; with
/// nothing checked and `amend` only the message of the last commit is changed.
pub fn commit_commands(checked: &[StatusEntry], op: Option<Operation>, message: &str, amend: bool) -> Vec<Vec<String>> {
    let mut cmds: Vec<Vec<String>> = vec![];
    let untracked: Vec<String> = checked.iter().filter(|e| e.is_untracked()).map(|e| e.path.clone()).collect();
    if op.is_some() {
        if !checked.is_empty() {
            let mut c = vec!["add".to_string(), "-A".into(), "--".into()];
            c.extend(checked.iter().map(|e| e.path.clone()));
            cmds.push(c);
        }
        cmds.push(vec!["commit".into(), "--no-edit".into(), "-m".into(), message.to_string()]);
    } else if checked.is_empty() {
        cmds.push(vec!["commit".into(), "--amend".into(), "--only".into(), "-m".into(), message.to_string()]);
    } else {
        if !untracked.is_empty() {
            let mut c = vec!["add".to_string(), "--".into()];
            c.extend(untracked);
            cmds.push(c);
        }
        let mut c = vec!["commit".to_string(), "-m".into(), message.to_string()];
        if amend {
            c.push("--amend".into());
        }
        c.push("--".into());
        for e in checked {
            c.push(e.path.clone());
            if let Some(o) = &e.old_path {
                c.push(o.clone());
            }
        }
        cmds.push(c);
    }
    cmds
}

/// `git reset` to a commit in the chosen way.
pub fn reset_args(mode: &ResetMode, hash: &str) -> Vec<String> {
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
        ResetMode::Keep => "--keep",
    };
    vec!["reset".into(), flag.into(), hash.to_string()]
}

/// Delete a branch: a local one (`force`: even when it is not merged), or one on a server
/// ("origin/name": pushed away from its remote).
pub fn delete_branch_args(name: &str, remote: bool, force: bool) -> Vec<String> {
    if remote {
        let (r, b) = name.split_once('/').unwrap_or(("origin", name));
        vec!["push".into(), r.to_string(), "--delete".into(), b.to_string()]
    } else {
        vec!["branch".into(), if force { "-D" } else { "-d" }.into(), name.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Job;
    use crate::git;
    use crate::testrepo::{TestRepo, settle};

    fn entries(r: &TestRepo, names: &[&str]) -> Vec<StatusEntry> {
        git::status(&r.dir).into_iter().filter(|e| names.contains(&e.path.as_str())).collect()
    }

    #[test]
    fn commits_the_checked_files_and_amends() {
        let r = TestRepo::new("ops-commit");
        r.commit_file("a.txt", "one\n", "base");
        r.write("a.txt", "two\n");
        r.write("new file.txt", "fresh\n");
        r.write("left out.txt", "later\n");
        r.git(&["mv", "a.txt", "b.txt"]);
        r.write("b.txt", "two\n");
        // The renamed file and the new one are checked; the third stays out of the commit.
        let checked = entries(&r, &["b.txt", "new file.txt"]);
        assert_eq!(checked.len(), 2);
        r.run(&commit_commands(&checked, None, "second", false));
        assert_eq!(r.subjects("HEAD"), ["second", "base"]);
        // Both names of the moved file went in (its text changed too, so git shows it as one
        // file gone and one new).
        assert_eq!(r.git(&["show", "--name-status", "--format=", "HEAD"]).lines().map(|l| l.replace('\t', " ")).collect::<Vec<_>>(), ["D a.txt", "A b.txt", "A new file.txt"]);
        assert_eq!(r.status(), ["?? left out.txt"]);

        // Amend with nothing checked: only the message changes.
        let before = r.git(&["rev-parse", "HEAD^{tree}"]);
        r.run(&commit_commands(&[], None, "second, reworded", true));
        assert_eq!((r.subjects("HEAD"), r.git(&["rev-parse", "HEAD^{tree}"])), (vec!["second, reworded".to_string(), "base".to_string()], before));
        // Amend with a file: it joins the last commit.
        r.run(&commit_commands(&entries(&r, &["left out.txt"]), None, "second, with the third file", true));
        assert_eq!(r.subjects("HEAD").len(), 2);
        assert!(r.status().is_empty() && r.git(&["show", "--name-only", "--format=", "HEAD"]).contains("left out.txt"));
    }

    #[test]
    fn concludes_a_merge_stopped_by_a_conflict() {
        let r = TestRepo::new("ops-merge");
        r.commit_file("a.txt", "base\n", "base");
        let main = r.branch();
        r.git(&["checkout", "-q", "-b", "side"]);
        r.commit_file("a.txt", "theirs\n", "on side");
        r.git(&["checkout", "-q", &main]);
        r.commit_file("a.txt", "ours\n", "on main");
        assert!(!r.try_git(&["merge", "side"]));
        assert_eq!((git::operation(&r.dir), r.status()), (Some(Operation::Merge), vec!["UU a.txt".to_string()]));
        r.write("a.txt", "both\n");
        r.run(&commit_commands(&entries(&r, &["a.txt"]), Some(Operation::Merge), "Merge side", false));
        assert_eq!(git::operation(&r.dir), None);
        assert_eq!(r.git(&["log", "-1", "--format=%s|%P"]).split('|').nth(1).unwrap().split_whitespace().count(), 2);
        assert_eq!(r.read("a.txt"), "both\n");
    }

    #[test]
    fn resets_in_every_way() {
        let r = TestRepo::new("ops-reset");
        let first = r.commit_file("a.txt", "one\n", "first");
        let second = r.commit_file("a.txt", "two\n", "second");
        // Soft: the commit is gone, its change stays staged. Mixed: it stays, unstaged.
        r.run(&[reset_args(&ResetMode::Soft, &first)]);
        assert_eq!((r.head(), r.status(), r.read("a.txt")), (first.clone(), vec!["M  a.txt".to_string()], "two\n".to_string()));
        r.run(&[reset_args(&ResetMode::Mixed, &first)]);
        assert_eq!(r.status(), [" M a.txt"]);
        // Hard: the change is gone too.
        r.run(&[reset_args(&ResetMode::Hard, &second)]);
        r.run(&[reset_args(&ResetMode::Hard, &first)]);
        assert_eq!((r.head(), r.status().len(), r.read("a.txt")), (first.clone(), 0, "one\n".to_string()));
        // Keep: refuses when a local change would be lost, and moves when it would not.
        r.write("b.txt", "mine\n");
        r.run(&[reset_args(&ResetMode::Keep, &second)]);
        assert_eq!((r.head(), r.read("a.txt"), r.read("b.txt")), (second, "two\n".to_string(), "mine\n".to_string()));
        r.write("a.txt", "local\n");
        let keep = reset_args(&ResetMode::Keep, &first);
        assert!(!r.try_git(&keep.iter().map(|s| s.as_str()).collect::<Vec<_>>()));
    }

    #[test]
    fn deletes_branches_here_and_on_the_server() {
        let origin = TestRepo::folder("ops-origin");
        origin.git(&["init", "-q", "--bare"]);
        let r = TestRepo::new("ops-branches");
        r.commit_file("a.txt", "one\n", "base");
        let main = r.branch();
        r.git(&["remote", "add", "origin", &origin.dir.display().to_string()]);
        r.git(&["checkout", "-q", "-b", "merged"]);
        r.git(&["checkout", "-q", "-b", "unmerged"]);
        r.commit_file("b.txt", "x\n", "not merged");
        r.git(&["push", "-q", "-u", "origin", &main, "unmerged"]);
        r.git(&["checkout", "-q", &main]);
        let has = |name: &str| r.try_git(&["rev-parse", "-q", "--verify", name]);
        // A merged branch goes; an unmerged one only by force.
        r.run(&[delete_branch_args("merged", false, false)]);
        assert!(!has("refs/heads/merged"));
        let soft = delete_branch_args("unmerged", false, false);
        // It is pushed, so git lets it go without force only because its upstream has it.
        r.run(&[soft]);
        assert!(!has("refs/heads/unmerged") && has("refs/remotes/origin/unmerged"));
        r.git(&["branch", "local-only", "origin/unmerged", "--no-track"]);
        assert!(!r.try_git(&["branch", "-d", "local-only"]));
        r.run(&[delete_branch_args("local-only", false, true)]);
        assert!(!has("refs/heads/local-only"));
        // The branch on the server.
        assert_eq!(delete_branch_args("origin/unmerged", true, false), ["push", "origin", "--delete", "unmerged"]);
        r.run(&[delete_branch_args("origin/unmerged", true, false)]);
        assert!(!has("refs/remotes/origin/unmerged") && !origin.try_git(&["rev-parse", "-q", "--verify", "refs/heads/unmerged"]));
    }

    #[test]
    fn smart_checkout_and_smart_update_carry_local_changes_over() {
        let origin = TestRepo::folder("ops-smart-origin");
        origin.git(&["init", "-q", "--bare"]);
        let r = TestRepo::new("ops-smart");
        r.commit_file("a.txt", "one\n", "base");
        r.write("notes.txt", "start\n");
        r.commit("notes");
        let main = r.branch();
        r.git(&["remote", "add", "origin", &origin.dir.display().to_string()]);
        r.git(&["push", "-q", "-u", "origin", &main]);
        r.git(&["checkout", "-q", "-b", "other"]);
        r.commit_file("a.txt", "other\n", "a on other");
        r.git(&["checkout", "-q", &main]);

        // A local change to a file the other branch has changed: a plain checkout refuses.
        r.write("a.txt", "one\nmine\n");
        r.write("notes.txt", "start\nmy note\n");
        assert!(!r.try_git(&["checkout", "other"]));
        let mut app = r.app();
        // Only notes.txt is carried over cleanly; a.txt clashes, so try with it reverted.
        r.git(&["checkout", "--", "a.txt"]);
        app.refresh();
        app.run_job(Job::smart_checkout(&["checkout".to_string(), "other".to_string()]));
        settle(&mut app);
        assert_eq!((r.branch(), r.read("notes.txt"), r.read("a.txt")), ("other".to_string(), "start\nmy note\n".to_string(), "other\n".to_string()));
        assert!(r.git(&["stash", "list"]).is_empty());

        // Smart update: someone else pushed; the local change is put aside and comes back.
        r.git(&["checkout", "-q", "--", "notes.txt"]);
        r.git(&["checkout", "-q", &main]);
        let other = TestRepo::folder("ops-smart-clone");
        other.git(&["clone", "-q", &origin.dir.display().to_string(), "."]);
        other.identity();
        other.commit_file("a.txt", "one\nfrom the server\n", "server change");
        other.git(&["push", "-q"]);
        r.write("notes.txt", "start\nstill mine\n");
        app.refresh();
        app.run_job(Job::smart_checkout(&["pull".to_string(), "--no-edit".into(), "--no-rebase".into(), "origin".into(), main.clone()]));
        settle(&mut app);
        assert_eq!((r.read("a.txt"), r.read("notes.txt")), ("one\nfrom the server\n".to_string(), "start\nstill mine\n".to_string()));
        assert_eq!(r.subjects("HEAD")[0], "server change");
        assert!(r.git(&["stash", "list"]).is_empty());
    }

    #[test]
    fn cherry_picks_and_reverts_with_options() {
        let r = TestRepo::new("ops-pick");
        r.commit_file("a.txt", "one\n", "base");
        let main = r.branch();
        r.git(&["checkout", "-q", "-b", "side"]);
        let fix = r.commit_file("b.txt", "fix\n", "the fix");
        r.git(&["checkout", "-q", &main]);
        // -x records where the commit came from.
        r.run(&[crate::dialogs::pick_args(false, &[fix.clone()], true, false, None)]);
        assert!(r.git(&["log", "-1", "--format=%B"]).contains(&format!("(cherry picked from commit {fix})")));
        assert_eq!(r.read("b.txt"), "fix\n");
        // Revert without committing leaves the reversal staged.
        let picked = r.head();
        r.run(&[crate::dialogs::pick_args(true, &[picked], false, true, None)]);
        assert_eq!(r.status(), ["D  b.txt"]);
        assert_eq!(r.subjects("HEAD")[0], "the fix");
    }

    #[test]
    fn unusual_file_names_are_read_right() {
        let r = TestRepo::new("ops-names");
        r.write("docs/отчёт за год.txt", "строка\n");
        r.write("src/with space.rs", "fn a() {}\n");
        r.write("quote\"d.txt", "q\n");
        r.commit("base");
        r.git(&["mv", "src/with space.rs", "src/renamed file.rs"]);
        r.write("docs/отчёт за год.txt", "строка\nвторая\n");
        r.write("новый файл.md", "# привет\n");
        std::fs::remove_file(r.dir.join("quote\"d.txt")).unwrap();
        let st = git::status(&r.dir);
        let line = |p: &str| st.iter().find(|e| e.path == p).map(|e| (e.x, e.y, e.old_path.clone()));
        assert_eq!(line("docs/отчёт за год.txt"), Some((' ', 'M', None)));
        assert_eq!(line("src/renamed file.rs"), Some(('R', ' ', Some("src/with space.rs".to_string()))));
        assert_eq!(line("новый файл.md"), Some(('?', '?', None)));
        assert_eq!(line("quote\"d.txt"), Some((' ', 'D', None)));
        assert_eq!(st.len(), 4);
        // The diff of a file with such a name, and of the renamed one.
        let e = st.iter().find(|e| e.path == "docs/отчёт за год.txt").unwrap();
        let d = git::worktree_diff(&r.dir, e, true);
        assert!(d.unified.iter().any(|(t, l)| *t == '+' && l == "вторая") && d.unified.iter().any(|(t, l)| *t == ' ' && l == "строка"));
        let e = st.iter().find(|e| e.path == "новый файл.md").unwrap();
        assert!(git::worktree_diff(&r.dir, e, true).unified.iter().any(|(t, l)| *t == '+' && l == "# привет"));
        // A commit with them: the names and the rename as the details pane reads them.
        r.commit("names");
        let det = git::commit_details(&r.dir, "HEAD").unwrap();
        let mut names: Vec<(char, String, Option<String>)> = det.files.iter().map(|f| (f.status, f.path.clone(), f.old_path.clone())).collect();
        names.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(names, [
            ('M', "docs/отчёт за год.txt".to_string(), None),
            ('D', "quote\"d.txt".to_string(), None),
            ('R', "src/renamed file.rs".to_string(), Some("src/with space.rs".to_string())),
            ('A', "новый файл.md".to_string(), None),
        ]);
        // Branches and tags with slashes and non-Latin letters.
        r.git(&["branch", "feature/отчёт-2026"]);
        r.git(&["tag", "-a", "v1.0-релиз", "-m", "Первый выпуск"]);
        let state = git::load_state(&r.dir);
        assert!(state.branches.iter().any(|b| b.name == "feature/отчёт-2026" && b.kind == git::RefKind::Local));
        assert!(state.tags.iter().any(|t| t.name == "v1.0-релиз"));
        assert_eq!(state.head_branch.as_deref(), Some(r.branch().as_str()));
    }

    #[test]
    fn white_space_and_line_endings_are_told_apart_from_real_changes() {
        let r = TestRepo::new("ops-space");
        r.commit_file("code.py", "def f():\n  a = 1\n  return a\n", "base");
        r.write("eol.txt", "one\ntwo\n");
        r.write("real.txt", "one\ntwo\n");
        r.commit("more");
        // Re-indented, with one real change in the middle.
        r.write("code.py", "def f():\n    a = 1\n    return a + 1\n");
        r.write("eol.txt", "one\r\ntwo\r\n");
        r.write("real.txt", "one\r\nthree\r\n");
        let st = git::status(&r.dir);
        let e = st.iter().find(|e| e.path == "code.py").unwrap();
        let changed = |d: &git::DiffText| d.unified.iter().filter(|(t, _)| *t == '+').map(|(_, l)| l.trim().to_string()).collect::<Vec<_>>();
        assert_eq!(changed(&git::worktree_diff_exact(&r.dir, e, true)), ["a = 1", "return a + 1"]);
        // What the diff view shows with each choice (asked for directly: the choice itself is
        // one for the whole program, and other tests run beside this one).
        let view = |flag: &str| git::parse_diff(&r.git(&["diff", "--no-ext-diff", "--unified=100000", flag, "HEAD", "--", "code.py"]));
        assert_eq!((git::ws_flag(0), git::ws_flag(1), git::ws_flag(2)), (None, Some("-b"), Some("-w")));
        assert_eq!(changed(&view("-b")), ["return a + 1"]);
        assert_eq!(changed(&view("-w")), ["return a + 1"]);
        // Only the file whose lines differ by their endings alone is marked.
        assert_eq!(git::eol_only(&r.dir).into_iter().collect::<Vec<_>>(), ["eol.txt"]);
    }
}
