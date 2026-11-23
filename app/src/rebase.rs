//! Interactive rebase: the plan (what happens to each commit) and the todo list git runs.
//!
//! The todo uses only `pick`, `fixup`, `drop` and `exec`. A new message (a reworded commit,
//! or the joined message of squashed ones) is set by an `exec git commit --amend -F <file>`
//! line after the commit, so it survives a stop on a conflict and "Continue" — git's own
//! `reword` / `squash` would open an editor in the middle of the run.

use crate::git;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Keep the commit (with a new message when `Entry::edited` is set).
    Pick,
    /// Join into the kept commit before it; its message is added to that commit's message.
    Squash,
    /// Join into the kept commit before it; its message is thrown away.
    Fixup,
    /// Remove the commit.
    Drop,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub hash: String,
    pub author: String,
    /// The commit's own full message.
    pub message: String,
    pub action: Action,
    /// The message typed in the window, when it differs from the one the plan gives by itself.
    pub edited: Option<String>,
    /// The commit is already on a remote: rewriting it needs a force push afterwards.
    pub pushed: bool,
}

/// What the Interactive Rebase window edits.
pub struct Plan {
    /// The commit the rebased ones sit on; None when the first of them is the root commit.
    pub base: Option<String>,
    /// Oldest first — the order git applies them in.
    pub entries: Vec<Entry>,
    /// The order the window opened with, to tell whether anything was changed.
    pub original: Vec<String>,
    pub selected: usize,
    pub focus_message: bool,
    pub error: Option<String>,
    /// The plan as it was before the AI's suggestion was applied ("Undo").
    pub ai_before: Option<Vec<Entry>>,
    /// The AI's explanation of its suggestion.
    pub ai_note: Option<String>,
}

/// What a quick menu item sets up before the window opens.
pub enum Preset {
    Nothing,
    /// Select this commit and put the cursor into its message.
    Reword(String),
    /// Join these commits into the oldest of them.
    Squash(Vec<String>),
    Drop(Vec<String>),
}

/// The commits from `from` (included) up to HEAD, ready for the window.
pub fn load(dir: &Path, from: &str) -> Result<Plan, String> {
    if !git::run(dir, &["merge-base", "--is-ancestor", from, "HEAD"]).ok {
        return Err("This commit is not part of the current branch, so the branch cannot be rebased from it.".into());
    }
    let parent = git::run(dir, &["rev-parse", "--verify", "--quiet", &format!("{from}^")]);
    let base = parent.ok.then(|| parent.stdout.trim().to_string()).filter(|s| !s.is_empty());
    let range = match &base {
        Some(b) => format!("{b}..HEAD"),
        None => "HEAD".to_string(),
    };
    let out = git::run(dir, &["log", "--reverse", "--topo-order", "--format=%H%x00%P%x00%an%x00%B%x1e", &range]);
    if !out.ok {
        return Err(out.text());
    }
    let unpushed: HashSet<String> = git::run(dir, &["rev-list", &range, "--not", "--remotes"]).stdout.lines().map(|l| l.trim().to_string()).collect();
    let mut entries = Vec::new();
    for rec in out.stdout.split('\x1e') {
        let rec = rec.trim_start_matches('\n');
        let f: Vec<&str> = rec.split('\0').collect();
        if f.len() < 4 {
            continue;
        }
        if f[1].split_whitespace().count() > 1 {
            return Err("There is a merge commit between this commit and the top of the branch. An interactive rebase would flatten it, so it is not offered here; choose a commit above the merge.".into());
        }
        entries.push(Entry {
            hash: f[0].to_string(),
            author: f[2].to_string(),
            message: f[3].trim_end().to_string(),
            action: Action::Pick,
            edited: None,
            pushed: !unpushed.contains(f[0]),
        });
    }
    if entries.is_empty() {
        return Err("There are no commits to rebase.".into());
    }
    let original = entries.iter().map(|e| e.hash.clone()).collect();
    let selected = entries.len() - 1;
    Ok(Plan { base, entries, original, selected, focus_message: false, error: None, ai_before: None, ai_note: None })
}

impl Plan {
    pub fn apply(&mut self, preset: Preset) {
        let pos = |entries: &[Entry], h: &str| entries.iter().position(|e| e.hash == h);
        match preset {
            Preset::Nothing => {}
            Preset::Reword(h) => {
                if let Some(i) = pos(&self.entries, &h) {
                    self.selected = i;
                    self.focus_message = true;
                }
            }
            Preset::Drop(list) => {
                for h in &list {
                    if let Some(i) = pos(&self.entries, h) {
                        self.entries[i].action = Action::Drop;
                        self.selected = i;
                    }
                }
            }
            Preset::Squash(list) => {
                let mut idx: Vec<usize> = list.iter().filter_map(|h| pos(&self.entries, h)).collect();
                idx.sort_unstable();
                let Some(&first) = idx.first() else { return };
                // Commits that are not next to each other are moved right after the oldest one.
                let moved: Vec<Entry> = idx[1..].iter().rev().map(|&i| self.entries.remove(i)).collect();
                for (k, mut e) in moved.into_iter().rev().enumerate() {
                    e.action = Action::Squash;
                    self.entries.insert(first + 1 + k, e);
                }
                self.entries[first].action = Action::Pick;
                self.selected = first;
                self.focus_message = true;
            }
        }
    }

    /// Move the selected commit one place up (earlier) or down (later).
    pub fn shift(&mut self, down: bool) {
        let i = self.selected;
        let j = if down { i + 1 } else { i.wrapping_sub(1) };
        if j < self.entries.len() {
            self.entries.swap(i, j);
            self.selected = j;
        }
    }

    /// Something differs from the history as it is now.
    pub fn changed(&self) -> bool {
        self.entries.iter().map(|e| &e.hash).ne(self.original.iter())
            || self.entries.iter().any(|e| e.action != Action::Pick)
            || (0..self.entries.len()).any(|i| final_message(&self.entries, i).is_some())
    }

    /// Why the plan cannot run as it is.
    pub fn problem(&self) -> Option<String> {
        match self.entries.iter().find(|e| e.action != Action::Drop) {
            None => return Some("Every commit is dropped. To remove them all, reset the branch instead.".into()),
            Some(e) if e.action != Action::Pick => return Some("The first kept commit has no commit before it to be joined into.".into()),
            Some(_) => {}
        }
        let empty = (0..self.entries.len()).any(|i| self.entries[i].action == Action::Pick && text(&self.entries, i).trim().is_empty());
        empty.then(|| "A commit message is empty.".into())
    }
}

/// The message the kept commit at `i` gets when nothing is typed: its own, then the messages
/// of the commits squashed into it.
pub fn default_message(entries: &[Entry], i: usize) -> String {
    let mut m = entries[i].message.clone();
    for e in entries[i + 1..].iter().take_while(|e| e.action != Action::Pick) {
        if e.action == Action::Squash {
            m.push_str("\n\n");
            m.push_str(&e.message);
        }
    }
    m
}

/// The message shown in the editor for the kept commit at `i`.
pub fn text(entries: &[Entry], i: usize) -> String {
    entries[i].edited.clone().unwrap_or_else(|| default_message(entries, i))
}

/// The new message of the kept commit at `i`, or None when it keeps its own.
pub fn final_message(entries: &[Entry], i: usize) -> Option<String> {
    if entries[i].action != Action::Pick {
        return None;
    }
    let t = text(entries, i);
    (t.trim() != entries[i].message.trim()).then(|| t.trim().to_string() + "\n")
}

/// What the AI agent is asked when it suggests the plan: every commit (oldest first) with its
/// message, files and a cut-down diff, and the format of the answer.
pub fn ai_prompt(dir: &Path, branch: &str, entries: &[Entry], rules: &str) -> String {
    const LIMIT: usize = 90_000;
    const PER_COMMIT: usize = 6_000;
    let mut list = String::new();
    for e in entries {
        let stat = git::run(dir, &["show", "--stat=120", "--format=", &e.hash]).stdout;
        list += &format!(
            "\n=== COMMIT {}  (author: {}{})\nMessage:\n{}\nFiles:\n{}",
            short(&e.hash),
            e.author,
            if e.pushed { ", already pushed" } else { "" },
            indent(&e.message),
            stat.trim_end()
        );
        if list.len() < LIMIT {
            let diff = git::run(dir, &["show", "--format=", "-U2", "--no-color", &e.hash]).stdout;
            let cut: String = diff.chars().take(PER_COMMIT).collect();
            list += &format!("\nDiff:\n{cut}");
            if cut.len() < diff.len() {
                list += "\n… (diff cut)";
            }
        }
        list.push('\n');
    }
    if list.len() > LIMIT {
        list += "\n… (later diffs are left out; their messages and files are listed)\n";
    }
    format!(
        "You clean up the history of a git branch before it is merged, with an interactive rebase.\n\n\
Look at the commits below (oldest first) and suggest the best plan:\n\
- squash work-in-progress commits, fixes of a previous commit of this branch, and commits with meaningless messages \
(\"...\", \"wip\", \"fix\", \"update\") into the commit whose task they belong to; use fixup when their message adds nothing;\n\
- reword commits whose message does not say what they do;\n\
- drop only commits that are empty or whose change is completely undone later in this branch;\n\
- move a commit only to put it right after the commit it is squashed into, and only when the commits in between \
do not touch the same lines; otherwise keep the order;\n\
- keep commits that are fine as they are.\n\n\
Write new commit messages following these rules:\n{rules}\n\n\
Reply with ONLY the plan in exactly this format and no other text. One block per commit, in the new order \
(oldest first); every commit must appear exactly once:\n\n\
COMMIT: <short hash>\n\
ACTION: pick | reword | squash | fixup | drop\n\
MESSAGE: <new commit subject — for reword, and for a picked commit that others are squashed into>\n\n\
A squash or fixup commit goes into the nearest kept commit above it. After the blocks write one line:\n\
NOTE: <one or two sentences: what you changed and why>\n\n\
Do not run tools or change any files.\n\nBranch: {branch}\n{list}"
    )
}

fn indent(t: &str) -> String {
    t.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n")
}

fn short(h: &str) -> &str {
    &h[..h.len().min(9)]
}

/// Turn the agent's answer into entries in the new order, with its note. Fails when a commit is
/// missing, unknown or named twice, or the plan cannot run.
pub fn parse_ai(text: &str, entries: &[Entry]) -> Result<(Vec<Entry>, String), String> {
    let mut out: Vec<Entry> = vec![];
    let mut note = String::new();
    let mut used = HashSet::new();
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches(['*', '-', '#', ' ']).trim();
        let Some((key, val)) = line.split_once(':') else { continue };
        // Markdown the agent may add: **COMMIT:** `abc1234`.
        let val = val.trim().trim_matches(|c| c == '`' || c == '*').trim();
        match key.trim().to_ascii_uppercase().as_str() {
            "COMMIT" => {
                let h = val.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                if h.len() < 4 {
                    return Err(format!("The plan names a commit as \"{val}\"."));
                }
                let Some(e) = entries.iter().find(|e| e.hash.starts_with(&h)) else {
                    return Err(format!("The plan names a commit {h} that is not among the ones being rebased."));
                };
                if !used.insert(e.hash.clone()) {
                    return Err(format!("The plan names commit {h} twice."));
                }
                let mut e = e.clone();
                e.action = Action::Pick;
                e.edited = None;
                out.push(e);
            }
            "ACTION" => {
                let Some(e) = out.last_mut() else { continue };
                e.action = match val.split_whitespace().next().unwrap_or("").to_ascii_lowercase().as_str() {
                    "squash" | "s" => Action::Squash,
                    "fixup" | "f" => Action::Fixup,
                    "drop" | "d" => Action::Drop,
                    _ => Action::Pick,
                };
            }
            "MESSAGE" => {
                let Some(e) = out.last_mut() else { continue };
                if !val.is_empty() && e.action == Action::Pick {
                    // The answer gives the subject line only: the commit keeps the rest of its message.
                    let body = e.message.split_once('\n').map(|(_, b)| b.trim_matches('\n')).filter(|b| !b.trim().is_empty());
                    e.edited = Some(match body {
                        Some(b) => format!("{val}\n\n{b}"),
                        None => val.to_string(),
                    });
                }
            }
            "NOTE" => note = val.to_string(),
            _ => {}
        }
    }
    if out.is_empty() {
        return Err("The answer has no plan in the expected form.".into());
    }
    if out.len() != entries.len() {
        let missing: Vec<&str> = entries.iter().filter(|e| !used.contains(&e.hash)).map(|e| short(&e.hash)).collect();
        return Err(format!("The plan leaves out {} commit(s): {}.", missing.len(), missing.join(", ")));
    }
    // A message given for a commit that only keeps its own message changes nothing.
    for i in 0..out.len() {
        if out[i].edited.as_ref().is_some_and(|m| m.trim() == default_message(&out, i).trim()) {
            out[i].edited = None;
        }
    }
    Ok((out, note))
}

/// Quote for the shell git runs editors and `exec` lines with (also on Windows).
fn sh_quote(p: &Path) -> String {
    let s = p.display().to_string();
    let s = if cfg!(windows) { s.replace('\\', "/") } else { s };
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The todo list for git and the message files its `exec` lines read (kept in `work`).
pub fn todo(entries: &[Entry], work: &Path) -> (String, Vec<(PathBuf, String)>) {
    let mut out = String::new();
    let mut files = Vec::new();
    let mut target: Option<usize> = None;
    let mut flush = |out: &mut String, target: Option<usize>| {
        if let Some(msg) = target.and_then(|t| final_message(entries, t)) {
            let f = work.join(format!("message-{}.txt", files.len() + 1));
            out.push_str(&format!("exec git commit --amend --no-verify --allow-empty -q -F {}\n", sh_quote(&f)));
            files.push((f, msg));
        }
    };
    for (i, e) in entries.iter().enumerate() {
        match e.action {
            Action::Pick => {
                flush(&mut out, target);
                target = Some(i);
                out.push_str(&format!("pick {}\n", e.hash));
            }
            Action::Squash | Action::Fixup => out.push_str(&format!("fixup {}\n", e.hash)),
            Action::Drop => out.push_str(&format!("drop {}\n", e.hash)),
        }
    }
    flush(&mut out, target);
    (out, files)
}

/// Write the todo and the message files into the repository's git folder.
/// Returns the todo file and the `git rebase` arguments.
pub fn prepare(dir: &Path, plan: &Plan) -> Result<(PathBuf, Vec<String>), String> {
    let gd = git::run(dir, &["rev-parse", "--absolute-git-dir"]);
    if !gd.ok {
        return Err(gd.text());
    }
    let work = PathBuf::from(gd.stdout.trim()).join("refbraid-rebase");
    let _ = std::fs::remove_dir_all(&work);
    let io = |e: std::io::Error| format!("Could not write the rebase plan to {}: {e}", work.display());
    std::fs::create_dir_all(&work).map_err(io)?;
    let (text, files) = todo(&plan.entries, &work);
    for (f, msg) in &files {
        std::fs::write(f, msg).map_err(io)?;
    }
    let file = work.join("todo");
    std::fs::write(&file, text).map_err(io)?;
    let mut args: Vec<String> = ["rebase", "-i", "--autostash", "--no-autosquash"].iter().map(|s| s.to_string()).collect();
    args.push(plan.base.clone().unwrap_or_else(|| "--root".into()));
    Ok((file, args))
}

/// The GIT_SEQUENCE_EDITOR value: this program, started to hand git the prepared todo
/// (see `sequence_editor`). Git adds the path of its own todo file at the end.
pub fn editor_command(todo: &Path) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find the program's own file: {e}"))?;
    Ok(format!("{} --rebase-todo {}", sh_quote(&exe), sh_quote(todo)))
}

/// `refbraid --rebase-todo <prepared> <git's todo>`: replace git's todo with ours.
pub fn sequence_editor(args: &[String]) -> i32 {
    match (args.first(), args.get(1)) {
        (Some(from), Some(to)) => match std::fs::copy(from, to) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("refbraid: could not write the rebase plan: {e}");
                1
            }
        },
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_ai_plan() {
        let e = |h: &str, m: &str| Entry { hash: h.repeat(40), author: "A".into(), message: m.into(), action: Action::Pick, edited: None, pushed: false };
        let entries = vec![e("a", "Add login"), e("b", "..."), e("c", "Add logout"), e("d", "wip")];
        let answer = "Here is the plan:\n\nCOMMIT: aaaaaaa\nACTION: pick\nMESSAGE: Add login form\n\n**COMMIT:** bbbbbbb\nACTION: fixup\n\nCOMMIT: ddddddd\nACTION: squash\n\nCOMMIT: `ccccccc`\nACTION: pick\nMESSAGE: Add logout\n\nNOTE: Joined the fixes into the login commit.";
        let (out, note) = parse_ai(answer, &entries).unwrap();
        let got: Vec<(char, Action)> = out.iter().map(|x| (x.hash.chars().next().unwrap(), x.action)).collect();
        assert_eq!(got, [('a', Action::Pick), ('b', Action::Fixup), ('d', Action::Squash), ('c', Action::Pick)]);
        assert_eq!(out[0].edited.as_deref(), Some("Add login form"));
        // The same message as the commit's own is no change.
        assert_eq!(out[3].edited, None);
        assert_eq!(note, "Joined the fixes into the login commit.");
        assert!(parse_ai("COMMIT: aaaaaaa\nACTION: pick", &entries).unwrap_err().contains("leaves out 3"));
        assert!(parse_ai("COMMIT: eeeeeee", &entries).is_err());
        assert!(parse_ai("no plan", &entries).is_err());
        // A reworded commit keeps the body of its message.
        let with_body = vec![e("a", "wip\n\nWhy it was needed.\nSecond line.")];
        let (out, _) = parse_ai("COMMIT: aaaaaaa\nACTION: reword\nMESSAGE: Add login", &with_body).unwrap();
        assert_eq!(out[0].edited.as_deref(), Some("Add login\n\nWhy it was needed.\nSecond line."));
    }

    const ID: [(&str, &str); 4] = [("GIT_AUTHOR_NAME", "T"), ("GIT_AUTHOR_EMAIL", "t@e"), ("GIT_COMMITTER_NAME", "T"), ("GIT_COMMITTER_EMAIL", "t@e")];

    /// A repository with commits "c1".."c5", each adding its own file (so any order applies).
    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ge-rebase-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| assert!(git::run_env(&dir, &ID, args).ok, "{args:?}");
        g(&["init", "-q"]);
        for i in 1..=5 {
            std::fs::write(dir.join(format!("f{i}.txt")), format!("{i}\n")).unwrap();
            g(&["add", "-A"]);
            g(&["commit", "-qm", &format!("c{i}")]);
        }
        dir
    }

    fn subjects(dir: &Path) -> Vec<String> {
        git::run(dir, &["log", "--reverse", "--format=%s"]).stdout.lines().map(String::from).collect()
    }

    /// Run the plan the way the app does, with `cp` standing in for the program itself.
    fn run(dir: &Path, plan: &Plan) {
        let (file, args) = prepare(dir, plan).unwrap();
        let editor = format!("cp {}", sh_quote(&file));
        let mut env = ID.to_vec();
        env.push(("GIT_SEQUENCE_EDITOR", &editor));
        let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let out = git::run_env(dir, &env, &args);
        assert!(out.ok, "{}", out.text());
    }

    #[test]
    fn loads_oldest_first_and_sees_no_change() {
        let dir = repo("load");
        let plan = load(&dir, "HEAD~2").unwrap();
        let msgs: Vec<&str> = plan.entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(msgs, ["c3", "c4", "c5"]);
        assert!(plan.base.is_some());
        assert!(!plan.changed());
        assert!(plan.problem().is_none());
        // No remote: nothing is pushed.
        assert!(plan.entries.iter().all(|e| !e.pushed));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn squash_reword_drop_and_reorder() {
        let dir = repo("all");
        let mut plan = load(&dir, "HEAD~3").unwrap(); // c2 c3 c4 c5
        plan.apply(Preset::Squash(vec![plan.entries[0].hash.clone(), plan.entries[2].hash.clone()])); // c2 + c4
        let order: Vec<&str> = plan.entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(order, ["c2", "c4", "c3", "c5"]);
        assert_eq!(text(&plan.entries, 0), "c2\n\nc4");
        plan.entries[2].action = Action::Drop; // c3
        plan.entries[3].edited = Some("five, reworded\n\nwith a body".into());
        assert!(plan.changed());
        assert!(plan.problem().is_none());
        run(&dir, &plan);
        assert_eq!(subjects(&dir), ["c1", "c2", "five, reworded"]);
        assert_eq!(git::run(&dir, &["log", "-1", "--format=%B", "HEAD~1"]).stdout.trim(), "c2\n\nc4");
        assert_eq!(git::run(&dir, &["log", "-1", "--format=%b"]).stdout.trim(), "with a body");
        assert!(dir.join("f4.txt").exists() && !dir.join("f3.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fixup_keeps_the_first_message_and_root_is_reachable() {
        let dir = repo("root");
        let first = git::run(&dir, &["rev-list", "--max-parents=0", "HEAD"]).stdout.trim().to_string();
        let mut plan = load(&dir, &first).unwrap();
        assert!(plan.base.is_none());
        assert_eq!(plan.entries.len(), 5);
        plan.entries[1].action = Action::Fixup;
        plan.selected = 4;
        plan.shift(false); // c5 before c4
        run(&dir, &plan);
        assert_eq!(subjects(&dir), ["c1", "c3", "c5", "c4"]);
        assert!(dir.join("f2.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_plans_that_cannot_run() {
        let e = |m: &str, action| Entry { hash: m.into(), author: String::new(), message: m.into(), action, edited: None, pushed: false };
        let plan = |entries: Vec<Entry>| Plan { base: None, original: entries.iter().map(|e| e.hash.clone()).collect(), entries, selected: 0, focus_message: false, error: None, ai_before: None, ai_note: None };
        assert!(plan(vec![e("a", Action::Drop), e("b", Action::Squash)]).problem().is_some());
        assert!(plan(vec![e("a", Action::Drop), e("b", Action::Drop)]).problem().is_some());
        assert!(plan(vec![e("a", Action::Drop), e("b", Action::Pick), e("c", Action::Fixup)]).problem().is_none());
        let mut p = plan(vec![e("a", Action::Pick)]);
        p.entries[0].edited = Some("  ".into());
        assert!(p.problem().is_some());
        // A dropped commit between a commit and its squash does not break the pair.
        let (t, files) = todo(&[e("a", Action::Pick), e("b", Action::Drop), e("c", Action::Squash)], Path::new("/w"));
        assert!(t.starts_with("pick a\ndrop b\nfixup c\nexec git commit --amend"), "{t}");
        assert_eq!(files[0].1, "a\n\nc\n");
    }

    #[test]
    fn merge_in_the_range_is_refused() {
        let dir = repo("merge");
        let g = |args: &[&str]| assert!(git::run_env(&dir, &ID, args).ok, "{args:?}");
        g(&["checkout", "-q", "-b", "side", "HEAD~2"]);
        std::fs::write(dir.join("side.txt"), "s\n").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "side"]);
        g(&["checkout", "-q", "-"]);
        g(&["merge", "-q", "--no-ff", "-m", "join", "side"]);
        assert!(load(&dir, "HEAD~3").is_err());
        assert!(load(&dir, "side").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
