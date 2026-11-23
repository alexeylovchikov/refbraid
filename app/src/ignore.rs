//! Ignoring and cleaning: lines for `.gitignore` / `.git/info/exclude`, and the list of files
//! `git clean` would delete.

use crate::git;
use std::path::{Path, PathBuf};

/// What to ignore, starting from a file's path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum What {
    /// Exactly this file or folder.
    Path,
    /// The folder the file is in.
    Folder,
    /// Every file with the same extension, anywhere.
    Extension,
}

/// The ignore line for `path` (relative to the repository, '/' separators; a folder ends in '/').
/// None when there is nothing of that kind: a file at the root has no folder, a file may have
/// no extension.
pub fn pattern(path: &str, what: What) -> Option<String> {
    let clean = path.trim_end_matches('/');
    let is_dir = path.ends_with('/');
    match what {
        What::Path => Some(format!("/{}{}", escape(clean), if is_dir { "/" } else { "" })),
        What::Folder => clean.rsplit_once('/').map(|(dir, _)| format!("/{}/", escape(dir))),
        What::Extension => {
            if is_dir {
                return None;
            }
            let name = clean.rsplit('/').next().unwrap_or(clean);
            let (stem, ext) = name.rsplit_once('.')?;
            if stem.is_empty() || ext.is_empty() {
                return None;
            }
            Some(format!("*.{}", escape(ext)))
        }
    }
}

/// Characters that mean something in an ignore pattern are taken literally.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        if matches!(c, '*' | '?' | '[' | '\\') || (i == 0 && matches!(c, '#' | '!')) {
            out.push('\\');
        }
        out.push(c);
    }
    if out.ends_with(' ') {
        out.insert(out.len() - 1, '\\');
    }
    out
}

/// `text` of an ignore file with `lines` added at the end; lines already there are not repeated.
/// Returns None when nothing new was added.
pub fn with_lines(text: &str, lines: &[String]) -> Option<String> {
    let have: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut new: Vec<&String> = vec![];
    for l in lines {
        if !have.contains(&l.as_str()) && !new.contains(&l) {
            new.push(l);
        }
    }
    if new.is_empty() {
        return None;
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for l in new {
        out.push_str(l);
        out.push('\n');
    }
    Some(out)
}

/// The ignore file to write: the repository's `.gitignore`, or `info/exclude` in its git folder
/// (ignored on this computer only, not committed).
pub fn target(dir: &Path, exclude: bool) -> PathBuf {
    if !exclude {
        return dir.join(".gitignore");
    }
    let p = git::run(dir, &["rev-parse", "--git-path", "info/exclude"]).stdout.trim().to_string();
    let p = PathBuf::from(if p.is_empty() { ".git/info/exclude".to_string() } else { p });
    if p.is_absolute() { p } else { dir.join(p) }
}

/// Add `lines` to the ignore file. Ok(false) when they were all there already.
pub fn add(dir: &Path, exclude: bool, lines: &[String]) -> Result<bool, String> {
    let file = target(dir, exclude);
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let Some(new) = with_lines(&text, lines) else { return Ok(false) };
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&file, new).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(true)
}

/// The `git clean` command: untracked files and folders, and with `ignored` the ignored ones too.
pub fn clean_args(dry: bool, ignored: bool) -> Vec<String> {
    let mut a = vec!["clean".to_string(), if dry { "-n" } else { "-f" }.to_string(), "-d".into()];
    if ignored {
        a.push("-x".into());
    }
    a
}

/// What `git clean` would delete (folders end in '/').
pub fn clean_preview(dir: &Path, ignored: bool) -> Vec<String> {
    let args = clean_args(true, ignored);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // Paths with unusual characters come quoted; -c core.quotePath=false keeps the rest readable.
    let mut full = vec!["-c", "core.quotePath=false"];
    full.extend(args);
    git::run(dir, &full).stdout.lines().filter_map(|l| l.strip_prefix("Would remove ")).map(|p| p.trim_matches('"').to_string()).collect()
}

/// The Clean window.
pub struct CleanForm {
    pub ignored: bool,
    pub files: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_patterns() {
        assert_eq!(pattern("build/out.log", What::Path).as_deref(), Some("/build/out.log"));
        assert_eq!(pattern("build/cache/", What::Path).as_deref(), Some("/build/cache/"));
        assert_eq!(pattern("build/tmp/out.log", What::Folder).as_deref(), Some("/build/tmp/"));
        assert_eq!(pattern("out.log", What::Folder), None);
        assert_eq!(pattern("build/out.log", What::Extension).as_deref(), Some("*.log"));
        assert_eq!(pattern(".env", What::Extension), None);
        assert_eq!(pattern("Makefile", What::Extension), None);
        assert_eq!(pattern("notes [1]*.txt", What::Path).as_deref(), Some("/notes \\[1]\\*.txt"));
        assert_eq!(pattern("#draft", What::Path).as_deref(), Some("/\\#draft"));
    }

    #[test]
    fn adds_only_new_lines() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(with_lines("", &l(&["*.log"])).as_deref(), Some("*.log\n"));
        assert_eq!(with_lines("target/", &l(&["*.log", "target/", "*.log"])).as_deref(), Some("target/\n*.log\n"));
        assert_eq!(with_lines("*.log\n", &l(&["*.log"])), None);
    }

    #[test]
    fn ignores_and_cleans_in_a_real_repository() {
        let dir = std::env::temp_dir().join(format!("ge-ignore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("build")).unwrap();
        assert!(git::run(&dir, &["init", "-q"]).ok);
        std::fs::write(dir.join("build/out.log"), "x").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join("local.cfg"), "x").unwrap();
        let untracked = |d: &Path| git::run(d, &["status", "--porcelain", "-uall"]).stdout.lines().map(|l| l[3..].to_string()).collect::<Vec<_>>();
        assert_eq!(untracked(&dir), ["build/out.log", "local.cfg", "notes.txt"]);

        assert_eq!(add(&dir, false, &[pattern("build/out.log", What::Extension).unwrap()]), Ok(true));
        assert_eq!(add(&dir, false, &["*.log".to_string()]), Ok(false));
        assert_eq!(add(&dir, true, &[pattern("local.cfg", What::Path).unwrap()]), Ok(true));
        // .gitignore itself is new; local.cfg is hidden by info/exclude, which is not listed.
        assert_eq!(untracked(&dir), [".gitignore", "notes.txt"]);

        assert_eq!(clean_preview(&dir, false), [".gitignore", "notes.txt"]);
        let mut all = clean_preview(&dir, true);
        all.sort();
        assert_eq!(all, [".gitignore", "build/", "local.cfg", "notes.txt"]);
        let args = clean_args(false, false);
        assert!(git::run(&dir, &args.iter().map(String::as_str).collect::<Vec<_>>()).ok);
        assert!(!dir.join("notes.txt").exists() && dir.join("local.cfg").exists() && dir.join("build/out.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
