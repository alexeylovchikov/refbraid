//! Submodules: what `.gitmodules` lists, the commit the repository has recorded for each and
//! the one that is checked out in its folder, and the commits between two such pointers.

use crate::git::{self, DiffRow, DiffSide, DiffText};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Its folder is empty: `git submodule update --init` fetches it.
    NotInit,
    /// The recorded commit is checked out.
    Same,
    /// Another commit than the recorded one is checked out.
    Moved,
    /// A merge left two recorded commits.
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submodule {
    pub name: String,
    /// Its folder, from the root of the repository, with forward slashes.
    pub path: String,
    pub url: String,
    /// The commit the repository has recorded for it (in the index).
    pub recorded: String,
    /// The commit checked out in its folder (None: not initialised).
    pub actual: Option<String>,
    pub state: State,
    /// Files of its own are changed and not committed there.
    pub modified: bool,
    /// It has new files of its own that are not committed there.
    pub untracked: bool,
}

impl Submodule {
    /// Whether the repository itself has something to commit here: a moved pointer. Changes
    /// inside the submodule alone are committed in the submodule, not here.
    pub fn pointer_moved(&self) -> bool {
        self.state == State::Moved
    }
}

/// Read `git config -z -f .gitmodules --get-regexp '^submodule\..*\.(path|url)$'` (a key, a
/// line break, the value, a zero byte): (name, path, url).
pub fn parse_modules(text: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = vec![];
    for item in text.split('\0') {
        let Some((key, value)) = item.split_once('\n') else { continue };
        let Some(key) = key.strip_prefix("submodule.") else { continue };
        let (name, what) = if let Some(n) = key.strip_suffix(".path") {
            (n, 0)
        } else if let Some(n) = key.strip_suffix(".url") {
            (n, 1)
        } else {
            continue;
        };
        let at = match out.iter().position(|m| m.0 == name) {
            Some(i) => i,
            None => {
                out.push((name.to_string(), String::new(), String::new()));
                out.len() - 1
            }
        };
        if what == 0 {
            out[at].1 = value.to_string();
        } else {
            out[at].2 = value.to_string();
        }
    }
    out.retain(|m| !m.1.is_empty());
    out
}

/// Read `git submodule status`: (mark, commit, path). The mark is ' ', '+', '-' or 'U'.
pub fn parse_status(text: &str) -> Vec<(char, String, String)> {
    text.lines()
        .filter_map(|l| {
            let mark = l.chars().next()?;
            let rest = l.get(1..)?;
            let (hash, path) = rest.split_once(' ')?;
            if hash.len() < 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            // "path (describe)": the note in brackets is not part of the name.
            let path = match path.rfind(" (") {
                Some(i) if path.ends_with(')') => &path[..i],
                _ => path,
            };
            Some((mark, hash.to_string(), path.to_string()))
        })
        .collect()
}

/// Read `git ls-files -s -- <paths>`: the recorded commit of each submodule folder.
pub fn parse_recorded(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let (meta, path) = l.split_once('\t')?;
            let mut f = meta.split(' ');
            (f.next()? == "160000").then_some(())?;
            Some((path.to_string(), f.next()?.to_string()))
        })
        .collect()
}

/// Read `git status --porcelain=v2 -- <paths>`: (path, modified content, untracked files).
pub fn parse_dirty(text: &str) -> Vec<(String, bool, bool)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(9, ' ').collect();
            if f.len() < 9 || f[0] != "1" || !f[2].starts_with('S') {
                return None;
            }
            let sub: Vec<char> = f[2].chars().collect();
            Some((f[8].to_string(), sub.get(2) == Some(&'M'), sub.get(3) == Some(&'U')))
        })
        .collect()
}

/// The submodules of the repository in `dir` and the state of each. Nothing is run when the
/// repository has no `.gitmodules`.
pub fn read(dir: &Path) -> Vec<Submodule> {
    if !dir.join(".gitmodules").is_file() {
        return vec![];
    }
    let modules = parse_modules(&git::run(dir, &["config", "-z", "-f", ".gitmodules", "--get-regexp", r"^submodule\..*\.(path|url)$"]).stdout);
    if modules.is_empty() {
        return vec![];
    }
    let status = parse_status(&git::run(dir, &["submodule", "status"]).stdout);
    let mut args = vec!["ls-files", "-s", "--"];
    args.extend(modules.iter().map(|m| m.1.as_str()));
    let recorded = parse_recorded(&git::run(dir, &args).stdout);
    let mut args = vec!["status", "--porcelain=v2", "--"];
    args.extend(modules.iter().map(|m| m.1.as_str()));
    let dirty = parse_dirty(&git::run(dir, &args).stdout);
    let mut out: Vec<Submodule> = modules
        .into_iter()
        .filter_map(|(name, path, url)| {
            let rec = recorded.iter().find(|r| r.0 == path).map(|r| r.1.clone());
            let st = status.iter().find(|s| s.2 == path);
            // Listed in .gitmodules but not in the index: not a submodule of this commit.
            let recorded = rec.or_else(|| st.filter(|s| s.0 != '+').map(|s| s.1.clone()))?;
            let (state, actual) = match st {
                Some(('-', _, _)) | None => (State::NotInit, None),
                Some(('+', h, _)) => (State::Moved, Some(h.clone())),
                Some(('U', _, _)) => (State::Conflict, None),
                Some((_, h, _)) => (State::Same, Some(h.clone())),
            };
            let d = dirty.iter().find(|d| d.0 == path);
            Some(Submodule { name, path, url, recorded, actual, state, modified: d.is_some_and(|d| d.1), untracked: d.is_some_and(|d| d.2) })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// A commit on the way from one pointer to another: `added` is false for a commit the new
/// pointer no longer has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub added: bool,
    pub hash: String,
    pub subject: String,
}

/// Read `git log --left-right --format=%m%x09%h%x09%s old...new`.
pub fn parse_steps(text: &str) -> Vec<Step> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            let mark = f.next()?;
            let hash = f.next()?.to_string();
            Some(Step { added: mark == ">", hash, subject: f.next().unwrap_or("").to_string() })
        })
        .collect()
}

/// The most commits listed between two pointers.
pub const MOST_STEPS: usize = 200;

/// The commits between two pointers of the submodule in `sub_dir`, newest first; Err with the
/// reason when they cannot be listed (not initialised, a commit that was never fetched).
pub fn steps(sub_dir: &Path, old: &str, new: &str) -> Result<Vec<Step>, String> {
    if !sub_dir.join(".git").exists() {
        return Err(crate::i18n::tr("The submodule is not initialised: its commits cannot be listed.").to_string());
    }
    let n = (MOST_STEPS + 1).to_string();
    let out = git::run(sub_dir, &["log", "--left-right", "--format=%m%x09%h%x09%s", "-n", &n, &format!("{old}...{new}")]);
    if !out.ok {
        return Err(crate::i18n::tr("One of these commits is not in the submodule's folder yet: fetch it there (Update) to see the commits between.").to_string());
    }
    Ok(parse_steps(&out.stdout))
}

/// The two commits of a diff of a submodule folder (`-Subproject commit …` / `+Subproject
/// commit …`), and whether the new side has uncommitted changes of its own. None when the
/// diff is of an ordinary file.
pub fn pointers(d: &DiffText) -> Option<(Option<String>, Option<String>, bool)> {
    let (mut old, mut new, mut dirty, mut seen) = (None, None, false, false);
    for (tag, line) in &d.unified {
        let Some(rest) = line.strip_prefix("Subproject commit ") else {
            if matches!(tag, '+' | '-' | ' ') {
                return None;
            }
            continue;
        };
        seen = true;
        let (hash, is_dirty) = match rest.strip_suffix("-dirty") {
            Some(h) => (h, true),
            None => (rest, false),
        };
        match tag {
            '-' => old = Some(hash.to_string()),
            '+' => {
                new = Some(hash.to_string());
                dirty = is_dirty;
            }
            _ => {
                old = Some(hash.to_string());
                new = Some(hash.to_string());
                dirty = is_dirty;
            }
        }
    }
    seen.then_some((old, new, dirty))
}

fn short(h: &str) -> &str {
    &h[..h.len().min(7)]
}

/// What a diff tab shows for a submodule folder in place of two "Subproject commit" lines:
/// the old and the new commit, then the commits between them.
pub fn describe(path: &str, old: Option<&str>, new: Option<&str>, dirty: bool, steps: Result<Vec<Step>, String>) -> DiffText {
    use crate::i18n::{tr, trf};
    let mut d = DiffText::default();
    let (mut ln, mut rn) = (0, 0);
    let mut push = |d: &mut DiffText, kind: DiffSide, text: String| {
        let (tag, left, right) = match kind {
            DiffSide::Removed => {
                ln += 1;
                ('-', Some((ln, text.clone())), None)
            }
            DiffSide::Added => {
                rn += 1;
                ('+', None, Some((rn, text.clone())))
            }
            _ => {
                ln += 1;
                rn += 1;
                (' ', Some((ln, text.clone())), Some((rn, text.clone())))
            }
        };
        d.unified.push((tag, text));
        d.rows.push(DiffRow { left, right, kind });
    };
    let head = match (old, new) {
        (Some(o), Some(n)) if o != n => trf("Submodule {}: {} → {}", &[&path, &short(o), &short(n)]),
        (Some(o), Some(_)) => trf("Submodule {} at {}", &[&path, &short(o)]),
        (None, Some(n)) => trf("Submodule {} added at {}", &[&path, &short(n)]),
        (Some(o), None) => trf("Submodule {} removed (was at {})", &[&path, &short(o)]),
        (None, None) => trf("Submodule {}", &[&path]),
    };
    push(&mut d, DiffSide::Context, head);
    if dirty {
        push(&mut d, DiffSide::Context, tr("It has uncommitted changes of its own: commit them inside the submodule (Open).").to_string());
    }
    if let (Some(o), Some(n)) = (old, new) {
        if o != n {
            match steps {
                Ok(list) => {
                    let (gone, came) = (list.iter().filter(|s| !s.added).count(), list.iter().filter(|s| s.added).count());
                    let more = list.len() > MOST_STEPS;
                    push(&mut d, DiffSide::Context, String::new());
                    push(&mut d, DiffSide::Context, match (came, gone) {
                        (0, 0) => tr("No commits between them.").to_string(),
                        (c, 0) => trf("New commits: {}", &[&c.to_string()]),
                        (0, g) => trf("Commits left behind (the pointer moved back): {}", &[&g.to_string()]),
                        (c, g) => trf("New commits: {}, left behind: {}", &[&c.to_string(), &g.to_string()]),
                    });
                    for s in list.iter().take(MOST_STEPS) {
                        push(&mut d, if s.added { DiffSide::Added } else { DiffSide::Removed }, format!("{}  {}", s.hash, s.subject));
                    }
                    if more {
                        push(&mut d, DiffSide::Context, trf("… only the first {} are listed", &[&MOST_STEPS.to_string()]));
                    }
                }
                Err(why) => {
                    push(&mut d, DiffSide::Context, String::new());
                    push(&mut d, DiffSide::Context, why);
                }
            }
        }
    }
    d
}

/// If `d` is the diff of a submodule folder, what to show in its place.
pub fn replace_diff(dir: &Path, path: &str, d: &DiffText) -> Option<DiffText> {
    let (old, new, dirty) = pointers(d)?;
    let list = match (&old, &new) {
        (Some(o), Some(n)) if o != n => steps(&dir.join(path), o, n),
        _ => Ok(vec![]),
    };
    Some(describe(path, old.as_deref(), new.as_deref(), dirty, list))
}

/// Which submodules a job left with another recorded commit than is checked out, when the
/// recorded commit itself changed during the job (a pull, a checkout, a merge) — the ones
/// worth offering an update for.
pub fn behind_after(before: &[Submodule], after: &[Submodule]) -> Vec<String> {
    after
        .iter()
        .filter(|s| matches!(s.state, State::Moved | State::NotInit))
        .filter(|s| before.iter().find(|b| b.path == s.path).is_none_or(|b| b.recorded != s.recorded))
        .map(|s| s.path.clone())
        .collect()
}

/// The command that brings submodules to their recorded commits: one (`path`) or all of them;
/// `recursive` also those inside them. Not yet fetched ones are fetched.
pub fn update_args(path: Option<&str>, recursive: bool) -> Vec<Vec<String>> {
    let mut a: Vec<String> = ["submodule", "update", "--init"].iter().map(|s| s.to_string()).collect();
    if recursive {
        a.push("--recursive".into());
    }
    if let Some(p) = path {
        a.push("--".into());
        a.push(p.to_string());
    }
    vec![a]
}

/// The command that copies the addresses from `.gitmodules` into the submodules' own settings.
pub fn sync_args(path: Option<&str>) -> Vec<Vec<String>> {
    let mut a: Vec<String> = ["submodule", "sync", "--recursive"].iter().map(|s| s.to_string()).collect();
    if let Some(p) = path {
        a.push("--".into());
        a.push(p.to_string());
    }
    vec![a]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testrepo::TestRepo;

    #[test]
    fn reads_what_git_prints() {
        let m = parse_modules("submodule.lib.path\nlib\0submodule.lib.url\n../lib\0submodule.third party/zlib.url\nhttps://example.com/z.git\0submodule.third party/zlib.path\nthird party/zlib\0submodule.ghost.url\nx\0");
        assert_eq!(m, [("lib".to_string(), "lib".to_string(), "../lib".to_string()), ("third party/zlib".into(), "third party/zlib".into(), "https://example.com/z.git".into())]);
        let h = "683111a92b3dde146f70dcdedf18cea6ffbe628d";
        let s = parse_status(&format!(" {h} lib (heads/master)\n+{h} third party/zlib (v1.2-3-g683111a)\n-{h} docs\n"));
        assert_eq!(s, [(' ', h.to_string(), "lib".to_string()), ('+', h.into(), "third party/zlib".into()), ('-', h.into(), "docs".into())]);
        assert_eq!(parse_recorded(&format!("160000 {h} 0\tlib\n100644 {h} 0\tREADME\n")), [("lib".to_string(), h.to_string())]);
        let v2 = format!("1 .M SCM. 160000 160000 160000 {h} {h} lib\n1 .M S..U 160000 160000 160000 {h} {h} third party/zlib\n1 .M N... 100644 100644 100644 {h} {h} file\n");
        assert_eq!(parse_dirty(&v2), [("lib".to_string(), true, false), ("third party/zlib".into(), false, true)]);
        assert_eq!(parse_steps(">\tabc1234\tAdd a thing\n<\tdef5678\tOld\twith a tab\n"), [Step { added: true, hash: "abc1234".into(), subject: "Add a thing".into() }, Step { added: false, hash: "def5678".into(), subject: "Old\twith a tab".into() }]);
    }

    #[test]
    fn tells_a_submodule_diff_from_a_file() {
        let h = "683111a92b3dde146f70dcdedf18cea6ffbe628d";
        let g = "0e1e5e589b9c2d163b08d1cc4f7e3507b54869ac";
        let d = git::parse_diff(&format!("diff --git a/lib b/lib\nindex 683111a..0e1e5e5 160000\n--- a/lib\n+++ b/lib\n@@ -1 +1 @@\n-Subproject commit {h}\n+Subproject commit {g}-dirty\n"));
        assert_eq!(pointers(&d), Some((Some(h.to_string()), Some(g.to_string()), true)));
        let file = git::parse_diff("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,2 @@\n-Subproject commit x\n+Subproject commit y\n+more\n");
        assert_eq!(pointers(&file), None);
        assert_eq!(pointers(&DiffText::default()), None);
    }

    /// A repository with one submodule `lib` that has two commits; gives back both and the library.
    fn with_lib(tag: &str) -> (TestRepo, TestRepo, String, String) {
        let lib = TestRepo::new(&format!("{tag}-lib"));
        lib.write("a.txt", "1\n");
        let one = lib.commit("One");
        lib.write("a.txt", "2\n");
        let two = lib.commit("Two");
        let top = TestRepo::new(tag);
        top.write("f.txt", "x\n");
        top.commit("Start");
        top.git(&["-c", "protocol.file.allow=always", "submodule", "add", "-q", &lib.dir.display().to_string(), "lib"]);
        top.commit("Add the library");
        // Commits inside the submodule need an author too.
        for (k, v) in [("user.name", "Tester"), ("user.email", "tester@example.com"), ("core.autocrlf", "false")] {
            assert!(git::run(&top.dir.join("lib"), &["config", k, v]).ok);
        }
        (top, lib, one, two)
    }

    #[test]
    fn follows_a_submodule_through_its_states() {
        let (top, _lib, one, two) = with_lib("submodule");
        let s = read(&top.dir);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].path.as_str(), s[0].state, s[0].recorded.as_str(), s[0].actual.as_deref()), ("lib", State::Same, two.as_str(), Some(two.as_str())));
        assert!(!s[0].modified && !s[0].untracked && !s[0].pointer_moved());
        let before = s;

        // Another commit checked out there: the pointer moved, one commit is left behind.
        let sub = top.dir.join("lib");
        assert!(git::run(&sub, &["checkout", "-q", &one]).ok);
        let s = read(&top.dir);
        assert_eq!((s[0].state, s[0].recorded.as_str(), s[0].actual.as_deref()), (State::Moved, two.as_str(), Some(one.as_str())));
        // The recorded commit is the same as before: nothing to offer an update for.
        assert!(behind_after(&before, &s).is_empty());
        let e = git::status(&top.dir).into_iter().find(|e| e.path == "lib").unwrap();
        let d = git::worktree_diff(&top.dir, &e, true);
        let text: Vec<&str> = d.unified.iter().map(|l| l.1.as_str()).collect();
        assert!(text[0].contains(&one[..7]) && text[0].contains(&two[..7]), "{text:?}");
        assert!(d.unified.iter().any(|(t, l)| *t == '-' && l.ends_with("Two")), "{text:?}");

        // Changes of its own, with the recorded commit checked out again.
        assert!(git::run(&sub, &["checkout", "-q", &two]).ok);
        std::fs::write(sub.join("a.txt"), "edited\n").unwrap();
        std::fs::write(sub.join("new.txt"), "x\n").unwrap();
        let s = read(&top.dir);
        assert_eq!((s[0].state, s[0].modified, s[0].untracked), (State::Same, true, true));
        assert!(git::run(&sub, &["checkout", "-q", "."]).ok);
        std::fs::remove_file(sub.join("new.txt")).unwrap();

        // The repository records the older commit (as a pull would bring): an update is due.
        assert!(git::run(&sub, &["checkout", "-q", &one]).ok);
        top.commit("Go back to One");
        assert!(git::run(&sub, &["checkout", "-q", &two]).ok);
        let s = read(&top.dir);
        assert_eq!(behind_after(&before, &s), ["lib"]);
        top.run(&update_args(Some("lib"), false));
        let s = read(&top.dir);
        assert_eq!((s[0].state, s[0].actual.as_deref()), (State::Same, Some(one.as_str())));

        // Emptied and fetched again.
        top.git(&["submodule", "deinit", "-q", "-f", "lib"]);
        let s = read(&top.dir);
        assert_eq!((s[0].state, s[0].actual.as_deref()), (State::NotInit, None));
        top.run(&update_args(Some("lib"), false));
        assert_eq!(read(&top.dir)[0].state, State::Same);
    }

    #[test]
    fn offers_an_update_when_a_job_moves_the_recorded_commit() {
        use crate::app::{Dialog, Job};
        use crate::testrepo::settle;
        let (top, _lib, one, two) = with_lib("submodule-offer");
        let main = top.branch();
        let sub = top.dir.join("lib");
        // A branch that records the older commit of the library.
        top.git(&["checkout", "-q", "-b", "old"]);
        assert!(git::run(&sub, &["checkout", "-q", &one]).ok);
        top.commit("Go back to One");
        top.git(&["checkout", "-q", &main]);
        top.run(&update_args(None, true));

        let mut app = top.app();
        assert_eq!(app.repo.as_ref().unwrap().submodules[0].state, State::Same);
        app.run_job(Job::one("Checkout old", &["checkout", "old"]));
        settle(&mut app);
        let Some(Dialog::Confirm { job: Some(job), text, .. }) = app.dialog.take() else { panic!("no offer to update") };
        assert!(text.contains("lib"), "{text}");
        app.run_job(job);
        settle(&mut app);
        // Updated, and the update itself asks nothing more.
        assert!(app.dialog.is_none());
        let m = app.repo.as_ref().unwrap().submodules[0].clone();
        assert_eq!((m.state, m.actual.as_deref(), m.recorded.as_str()), (State::Same, Some(one.as_str()), one.as_str()));
        assert_ne!(one, two);

        // Changes inside the submodule alone are nothing this repository can commit.
        std::fs::write(sub.join("a.txt"), "edited\n").unwrap();
        top.write("f.txt", "changed\n");
        app.refresh();
        let repo = app.repo.as_ref().unwrap();
        assert!(repo.submodules[0].modified && repo.status.iter().any(|e| e.path == "lib"));
        assert!(repo.unchecked.contains("lib") && !repo.unchecked.contains("f.txt"));
        // Committed inside it: the pointer moved, and that is checked for the commit again.
        assert!(git::run(&sub, &["commit", "-q", "-am", "Edit inside"]).ok);
        app.refresh();
        let repo = app.repo.as_ref().unwrap();
        assert!(repo.submodules[0].pointer_moved() && !repo.unchecked.contains("lib"));
    }

    #[test]
    fn reads_a_submodule_in_a_folder_with_other_letters() {
        let lib = TestRepo::new("submodule-names-lib");
        lib.write("a.txt", "1\n");
        let one = lib.commit("One");
        lib.write("a.txt", "2\n");
        lib.commit("Two");
        let top = TestRepo::new("submodule-names");
        top.write("f.txt", "x\n");
        top.commit("Start");
        let path = "библиотеки/my lib";
        top.git(&["-c", "protocol.file.allow=always", "submodule", "add", "-q", &lib.dir.display().to_string(), path]);
        top.commit("Add the library");
        assert!(git::run(&top.dir.join(path), &["checkout", "-q", &one]).ok);
        let s = read(&top.dir);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!((s[0].path.as_str(), s[0].state, s[0].actual.as_deref()), (path, State::Moved, Some(one.as_str())));
        std::fs::write(top.dir.join(path).join("new.txt"), "x\n").unwrap();
        assert!(read(&top.dir)[0].untracked);
    }

    #[test]
    fn a_repository_without_submodules_costs_nothing() {
        let r = TestRepo::new("nosubmodule");
        r.write("f.txt", "x\n");
        r.commit("Start");
        assert!(read(&r.dir).is_empty());
    }
}
