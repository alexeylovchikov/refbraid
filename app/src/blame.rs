//! Annotate (git blame): which commit last changed each line of a file.

use crate::git;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Receiver;

/// A commit that some lines of the file come from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Origin {
    pub hash: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
    /// The commit before it and the file's name there ("Annotate Previous Revision").
    pub previous: Option<(String, String)>,
    /// The file's name in this commit.
    pub filename: String,
}

impl Origin {
    /// Lines changed in the working folder and not committed yet.
    pub fn uncommitted(&self) -> bool {
        self.hash.bytes().all(|b| b == b'0')
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Index into `Blame::origins`.
    pub origin: usize,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Blame {
    pub lines: Vec<Line>,
    pub origins: Vec<Origin>,
    /// For each origin: 0.0 for the oldest commit of the file … 1.0 for the newest.
    pub age: Vec<f32>,
}

/// Read the output of `git blame --porcelain`.
pub fn parse(text: &str) -> Blame {
    let mut b = Blame::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut cur = 0usize;
    for line in text.split('\n') {
        if let Some(content) = line.strip_prefix('\t') {
            b.lines.push(Line { origin: cur, text: content.trim_end_matches('\r').to_string() });
            continue;
        }
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        // "<hash> <line in that commit> <line now> [<lines in the group>]" starts every line's record.
        if key.len() >= 40 && key.bytes().all(|c| c.is_ascii_hexdigit()) {
            cur = *index.entry(key.to_string()).or_insert_with(|| {
                b.origins.push(Origin { hash: key.to_string(), ..Default::default() });
                b.origins.len() - 1
            });
            continue;
        }
        let Some(o) = b.origins.get_mut(cur) else { continue };
        match key {
            "author" => o.author = rest.to_string(),
            "author-time" => o.time = rest.trim().parse().unwrap_or(0),
            "summary" => o.summary = rest.to_string(),
            "filename" => o.filename = rest.to_string(),
            "previous" => o.previous = rest.split_once(' ').map(|(h, p)| (h.to_string(), p.to_string())),
            _ => {}
        }
    }
    // Rank the commits by date: the shade of a line tells how recent it is.
    let mut order: Vec<usize> = (0..b.origins.len()).collect();
    order.sort_by_key(|&i| if b.origins[i].uncommitted() { i64::MAX } else { b.origins[i].time });
    b.age = vec![1.0; b.origins.len()];
    let n = order.len();
    for (rank, &i) in order.iter().enumerate() {
        b.age[i] = if n > 1 { rank as f32 / (n - 1) as f32 } else { 1.0 };
    }
    b
}

/// Annotate `path` as it is in `rev`, or in the working folder when `rev` is None.
pub fn load(dir: &Path, rev: Option<&str>, path: &str) -> Result<Blame, String> {
    let mut args = vec!["blame", "--porcelain"];
    if let Some(r) = rev {
        args.push(r);
    }
    args.extend(["--", path]);
    let out = git::run(dir, &args);
    if !out.ok {
        return Err(out.text());
    }
    Ok(parse(&out.stdout))
}

pub enum State {
    Loading(Receiver<Result<Blame, String>>),
    Ready(Blame),
    Failed(String),
}

/// An "Annotate: file" tab.
pub struct BlameTab {
    pub path: String,
    pub rev: Option<String>,
    pub state: State,
    /// Line (0-based) to bring into view once the file is loaded.
    pub scroll_to: Option<usize>,
    /// Syntax colours of the lines, worked out once the file is loaded.
    pub colors: Option<Vec<Vec<crate::syntax::Span>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_groups() {
        let a = "a".repeat(40);
        let c = "c".repeat(40);
        let text = format!(
            "{a} 1 1 2\nauthor Ann Lee\nauthor-mail <ann@e>\nauthor-time 100\nauthor-tz +0000\nsummary first commit\nfilename old.txt\n\tone\n{a} 2 2\n\t\ttwo, indented\n\
             {c} 3 3 1\nauthor Bob\nauthor-time 200\nsummary second\nprevious {a} old.txt\nfilename new.txt\n\tthree\n{a} 4 4 1\n\tfour\n"
        );
        let b = parse(&text);
        let lines: Vec<(usize, &str)> = b.lines.iter().map(|l| (l.origin, l.text.as_str())).collect();
        assert_eq!(lines, [(0, "one"), (0, "\ttwo, indented"), (1, "three"), (0, "four")]);
        assert_eq!(b.origins.len(), 2);
        assert_eq!((b.origins[0].author.as_str(), b.origins[0].time, b.origins[0].summary.as_str()), ("Ann Lee", 100, "first commit"));
        assert_eq!(b.origins[1].previous, Some((a.clone(), "old.txt".to_string())));
        assert_eq!(b.origins[1].filename, "new.txt");
        assert_eq!(b.age, [0.0, 1.0]);
    }

    #[test]
    fn annotates_a_real_file_with_local_changes() {
        let dir = std::env::temp_dir().join(format!("ge-blame-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| assert!(git::run_env(&dir, &[("GIT_AUTHOR_NAME", "Ann"), ("GIT_AUTHOR_EMAIL", "a@e"), ("GIT_COMMITTER_NAME", "Ann"), ("GIT_COMMITTER_EMAIL", "a@e")], args).ok, "{args:?}");
        g(&["init", "-q"]);
        std::fs::write(dir.join("f.txt"), "one\ntwo\n").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "first"]);
        std::fs::write(dir.join("f.txt"), "one\ntwo\nthree\n").unwrap();
        g(&["commit", "-qam", "second"]);
        std::fs::write(dir.join("f.txt"), "one\ntwo\nthree\nlocal\n").unwrap();

        let b = load(&dir, None, "f.txt").unwrap();
        let of = |i: usize| &b.origins[b.lines[i].origin];
        assert_eq!(b.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["one", "two", "three", "local"]);
        assert_eq!((of(0).summary.as_str(), of(2).summary.as_str()), ("first", "second"));
        assert!(of(3).uncommitted() && !of(2).uncommitted());
        assert!(of(2).previous.is_some() && of(0).previous.is_none());
        // The committed version has no local line.
        assert_eq!(load(&dir, Some("HEAD"), "f.txt").unwrap().lines.len(), 3);
        assert!(load(&dir, None, "missing.txt").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
