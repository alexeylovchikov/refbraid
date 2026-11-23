//! A file with merge conflicts: the parts both sides agree on, the conflicting parts
//! (yours / theirs) and the result the person (or an AI agent) puts together.

use crate::git::{self, Operation};
use std::path::{Path, PathBuf};

/// Rules the AI agent follows when it resolves conflicts (Settings → Merge Conflicts).
pub const DEFAULT_CONFLICT_PROMPT: &str = "Resolve each conflict so that the intent of both sides is kept.\n\
- When both sides add different code, keep both in a sensible order.\n\
- When both sides change the same lines, combine the changes; if they truly contradict, prefer the incoming side (theirs) for behaviour and ours for formatting.\n\
- Keep the file's style, indentation and naming. Do not add comments about the merge.\n\
- The result must compile / parse: check imports, brackets and duplicated definitions.";

/// How the AI agent is told to resolve conflicts.
#[derive(Clone, Debug, PartialEq)]
pub enum Method {
    /// The rules from Settings.
    Prompt,
    /// The instructions of a skill (its SKILL.md).
    Skill(PathBuf),
}

impl Method {
    pub fn key(&self) -> String {
        match self {
            Method::Prompt => "prompt".into(),
            Method::Skill(p) => format!("skill:{}", p.display()),
        }
    }
    pub fn from_key(k: &str) -> Option<Method> {
        if k == "prompt" {
            Some(Method::Prompt)
        } else {
            k.strip_prefix("skill:").map(|p| Method::Skill(PathBuf::from(p)))
        }
    }
}

#[derive(Clone, Debug)]
pub struct Chunk {
    pub ours: String,
    /// The common ancestor part (diff3 style markers), kept for reference.
    #[allow(dead_code)]
    pub base: Option<String>,
    pub theirs: String,
    /// None while unresolved. Lines joined with '\n'; empty means "no lines".
    pub result: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Seg {
    /// Lines both sides agree on: as git wrote them, and as edited in the result.
    Same { orig: String, text: String },
    Conflict(Chunk),
}

pub struct MergeDoc {
    pub path: String,
    pub ours_label: String,
    pub theirs_label: String,
    pub segs: Vec<Seg>,
    pub final_newline: bool,
    pub crlf: bool,
    /// The file as git left it, with conflict markers.
    pub original: String,
    /// Whole versions of each side (for the AI and for the whole-file view).
    pub ours_file: String,
    pub theirs_file: String,
    pub base_file: String,
    /// The result as one text, when the AI's answer didn't fit the conflict blocks.
    pub whole: Option<String>,
    /// A line under the toolbar: (text, is_error).
    pub note: Option<(String, bool)>,
    /// The file is binary or deleted on one side: only "take yours / theirs" is possible.
    pub no_text: bool,
    /// The conflict (index into `segs`) to bring into view, and the last one gone to.
    pub jump: Option<usize>,
    pub cursor: Option<usize>,
}

/// Split text into lines without line ends.
fn lines_of(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![];
    }
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect()
}

pub fn line_count(s: &str) -> usize {
    if s.is_empty() { 0 } else { s.split('\n').count() }
}

fn same_seg(t: String) -> Seg {
    Seg::Same { orig: t.clone(), text: t }
}

fn marker<'a>(line: &'a str, m: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(m)?;
    if rest.is_empty() || rest.starts_with(' ') { Some(rest.trim()) } else { None }
}

impl MergeDoc {
    pub fn load(dir: &Path, path: &str) -> MergeDoc {
        let bytes = std::fs::read(dir.join(path)).unwrap_or_default();
        let binary = bytes.iter().take(8000).any(|b| *b == 0);
        let original = String::from_utf8_lossy(&bytes).to_string();
        let show = |stage: &str| {
            let o = git::run(dir, &["show", &format!(":{stage}:{path}")]);
            if o.ok { o.stdout } else { String::new() }
        };
        let mut doc = MergeDoc::parse(path, &original);
        doc.ours_file = show("2");
        doc.theirs_file = show("3");
        doc.base_file = show("1");
        doc.no_text = binary || doc.conflicts() == 0;
        if !doc.no_text {
            doc.add_bases(dir);
        }
        doc
    }

    /// Find the common-ancestor text of each conflict: git's three versions of the file
    /// (stages 1-3) are merged again with the base kept, and each conflict there that has the
    /// same two sides gives its base. Needs git 2.43 or newer (merge-file --object-id).
    fn add_bases(&mut self, dir: &Path) {
        let id = |stage: &str| {
            let o = git::run(dir, &["rev-parse", "--verify", "--quiet", &format!(":{stage}:{}", self.path)]);
            o.ok.then(|| o.stdout.trim().to_string())
        };
        let (Some(o), Some(b), Some(t)) = (id("2"), id("1"), id("3")) else { return };
        let mut found: Vec<Chunk> = vec![];
        for style in ["--zdiff3", "--diff3"] {
            let out = git::run(dir, &["merge-file", "-p", style, "--object-id", "-L", "ours", "-L", "base", "-L", "theirs", &o, &b, &t]);
            if out.stdout.is_empty() {
                continue;
            }
            let d = MergeDoc::parse(&self.path, &out.stdout);
            found.extend(d.segs.into_iter().filter_map(|s| match s {
                Seg::Conflict(c) if c.base.is_some() => Some(c),
                _ => None,
            }));
        }
        let norm = |t: &str| t.replace('\r', "");
        for s in &mut self.segs {
            if let Seg::Conflict(c) = s {
                if c.base.is_none() {
                    c.base = found.iter().find(|f| norm(&f.ours) == norm(&c.ours) && norm(&f.theirs) == norm(&c.theirs)).and_then(|f| f.base.clone());
                }
            }
        }
    }

    /// Open conflicts that can be resolved without a person: both sides made the same change,
    /// or only one side changed the lines, or the two sides changed different lines.
    pub fn simple_count(&self) -> usize {
        if self.no_text || self.whole.is_some() {
            return 0;
        }
        self.segs.iter().filter(|s| matches!(s, Seg::Conflict(c) if c.result.is_none() && simple_resolution(c).is_some())).count()
    }

    /// Resolve every simple conflict; returns how many were resolved.
    pub fn resolve_simple(&mut self) -> usize {
        if self.no_text || self.whole.is_some() {
            return 0;
        }
        let mut n = 0;
        for s in &mut self.segs {
            if let Seg::Conflict(c) = s {
                if c.result.is_none() {
                    if let Some(r) = simple_resolution(c) {
                        c.result = Some(r);
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// Index (into `segs`) of the next (or previous) conflict still open after the one last
    /// gone to, wrapping around; all conflicts when none is open.
    pub fn next_open(&self, down: bool) -> Option<usize> {
        let pick = |open_only: bool| -> Vec<usize> {
            self.segs.iter().enumerate().filter(|(_, s)| matches!(s, Seg::Conflict(c) if !open_only || c.result.is_none())).map(|(i, _)| i).collect()
        };
        let mut all = pick(true);
        if all.is_empty() {
            all = pick(false);
        }
        let cur = self.cursor;
        if down {
            all.iter().copied().find(|&i| cur.is_none_or(|c| i > c)).or(all.first().copied())
        } else {
            all.iter().rev().copied().find(|&i| cur.is_none_or(|c| i < c)).or(all.last().copied())
        }
    }

    pub fn parse(path: &str, text: &str) -> MergeDoc {
        let mut doc = MergeDoc {
            path: path.to_string(),
            ours_label: "HEAD".into(),
            theirs_label: String::new(),
            segs: vec![],
            final_newline: text.ends_with('\n'),
            crlf: text.contains("\r\n"),
            original: text.to_string(),
            ours_file: String::new(),
            theirs_file: String::new(),
            base_file: String::new(),
            whole: None,
            note: None,
            no_text: false,
            jump: None,
            cursor: None,
        };
        #[derive(PartialEq)]
        enum St {
            Normal,
            Ours,
            Base,
            Theirs,
        }
        let mut st = St::Normal;
        let (mut same, mut ours, mut base, mut theirs): (Vec<String>, Vec<String>, Vec<String>, Vec<String>) =
            (vec![], vec![], vec![], vec![]);
        let mut has_base = false;
        for l in lines_of(text) {
            match st {
                St::Normal => {
                    if let Some(label) = marker(&l, "<<<<<<<") {
                        if !same.is_empty() {
                            doc.segs.push(same_seg(std::mem::take(&mut same).join("\n")));
                        }
                        if !label.is_empty() {
                            doc.ours_label = label.to_string();
                        }
                        st = St::Ours;
                    } else {
                        same.push(l);
                    }
                }
                St::Ours => {
                    if marker(&l, "|||||||").is_some() {
                        has_base = true;
                        st = St::Base;
                    } else if marker(&l, "=======").is_some() {
                        st = St::Theirs;
                    } else {
                        ours.push(l);
                    }
                }
                St::Base => {
                    if marker(&l, "=======").is_some() {
                        st = St::Theirs;
                    } else {
                        base.push(l);
                    }
                }
                St::Theirs => {
                    if let Some(label) = marker(&l, ">>>>>>>") {
                        if !label.is_empty() {
                            doc.theirs_label = label.to_string();
                        }
                        doc.segs.push(Seg::Conflict(Chunk {
                            ours: std::mem::take(&mut ours).join("\n"),
                            base: if has_base { Some(std::mem::take(&mut base).join("\n")) } else { None },
                            theirs: std::mem::take(&mut theirs).join("\n"),
                            result: None,
                        }));
                        has_base = false;
                        st = St::Normal;
                    } else {
                        theirs.push(l);
                    }
                }
            }
        }
        if st != St::Normal {
            // Broken markers: show the file as it is.
            doc.segs = vec![same_seg(lines_of(text).join("\n"))];
            return doc;
        }
        if !same.is_empty() {
            doc.segs.push(same_seg(same.join("\n")));
        }
        doc
    }

    pub fn conflicts(&self) -> usize {
        self.segs.iter().filter(|s| matches!(s, Seg::Conflict(_))).count()
    }

    pub fn unresolved(&self) -> usize {
        if self.whole.is_some() {
            return 0;
        }
        self.segs.iter().filter(|s| matches!(s, Seg::Conflict(c) if c.result.is_none())).count()
    }

    /// The text that will be saved: conflicts still open keep their markers.
    pub fn result_text(&self) -> String {
        let mut lines: Vec<String> = vec![];
        if let Some(w) = &self.whole {
            lines = lines_of(w);
        } else {
            for s in &self.segs {
                match s {
                    Seg::Same { text, .. } => lines.push(text.clone()),
                    Seg::Conflict(c) => match &c.result {
                        Some(r) if r.is_empty() => {}
                        Some(r) => lines.push(r.clone()),
                        None => {
                            lines.push(format!("<<<<<<< {}", self.ours_label));
                            if !c.ours.is_empty() {
                                lines.push(c.ours.clone());
                            }
                            lines.push("=======".into());
                            if !c.theirs.is_empty() {
                                lines.push(c.theirs.clone());
                            }
                            lines.push(format!(">>>>>>> {}", self.theirs_label));
                        }
                    },
                }
            }
        }
        let nl = if self.crlf { "\r\n" } else { "\n" };
        let mut out = lines.join("\n");
        if self.crlf {
            out = out.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        if self.final_newline && !out.is_empty() {
            out.push_str(nl);
        }
        out
    }

    /// Take one side for every conflict that is still open.
    pub fn take_all(&mut self, ours: bool) {
        for s in &mut self.segs {
            if let Seg::Conflict(c) = s {
                if c.result.is_none() {
                    c.result = Some(if ours { c.ours.clone() } else { c.theirs.clone() });
                }
            }
        }
    }

    /// Put the AI's answer (the whole resolved file) into the conflict blocks. When the
    /// agent changed lines outside the conflicts, the result is shown as one text instead.
    pub fn apply_ai(&mut self, text: &str) -> bool {
        let out = lines_of(text);
        if let Some(found) = fit(&self.segs, &out) {
            for (i, r) in found {
                if let Seg::Conflict(c) = &mut self.segs[i] {
                    c.result = Some(r);
                }
            }
            self.whole = None;
            true
        } else {
            self.whole = Some(out.join("\n"));
            false
        }
    }
}

/// The resolution of a conflict that needs no person, if there is one.
pub fn simple_resolution(c: &Chunk) -> Option<String> {
    if c.ours == c.theirs {
        return Some(c.ours.clone());
    }
    let base = c.base.as_ref()?;
    merge3(base, &c.ours, &c.theirs)
}

/// Line-by-line three-way merge of a small text: Some when the two sides changed different
/// lines of the base (or the same lines in the same way).
pub fn merge3(base: &str, ours: &str, theirs: &str) -> Option<String> {
    let b = lines_of(base);
    let o = lines_of(ours);
    let t = lines_of(theirs);
    if b.len() > 2000 || o.len() > 2000 || t.len() > 2000 {
        return None;
    }
    let ho = hunks(&b, &o);
    let ht = hunks(&b, &t);
    // Two changes clash when they touch the same base lines, or one inserts where the other
    // changes (or inserts) too - unless they are the very same change.
    for x in &ho {
        for y in &ht {
            let same = x == y;
            let overlap = x.0 < y.1 && y.0 < x.1;
            let insert_touch = (x.0 == x.1 || y.0 == y.1) && x.0 <= y.1 && y.0 <= x.1;
            if !same && (overlap || insert_touch) {
                return None;
            }
        }
    }
    let mut all: Vec<&(usize, usize, Vec<String>)> = ho.iter().chain(ht.iter().filter(|y| !ho.contains(y))).collect();
    all.sort_by_key(|h| (h.0, h.1));
    let mut out: Vec<String> = vec![];
    let mut at = 0;
    for (from, to, lines) in all {
        out.extend(b[at..*from].iter().cloned());
        out.extend(lines.iter().cloned());
        at = *to;
    }
    out.extend(b[at..].iter().cloned());
    Some(out.join("\n"))
}

/// The changes from `a` to `b`: (first line of `a`, line after the last, the new lines).
fn hunks(a: &[String], b: &[String]) -> Vec<(usize, usize, Vec<String>)> {
    let (n, m) = (a.len(), b.len());
    // Longest common subsequence table, from the end.
    let mut l = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if a[i] == b[j] { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = vec![];
    let mut cur: Option<(usize, usize, Vec<String>)> = None;
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            if let Some(h) = cur.take() {
                out.push(h);
            }
            i += 1;
            j += 1;
        } else if j < m && (i == n || l[i][j + 1] >= l[i + 1][j]) {
            let h = cur.get_or_insert((i, i, vec![]));
            h.2.push(b[j].clone());
            j += 1;
        } else {
            let h = cur.get_or_insert((i, i, vec![]));
            h.1 = i + 1;
            i += 1;
        }
    }
    if let Some(h) = cur.take() {
        out.push(h);
    }
    out
}

/// Find each agreed block of lines in the answer, in order; the lines between them are the
/// resolutions of the conflicts. Tries other places when a block (say, an empty line) repeats.
fn fit(segs: &[Seg], out: &[String]) -> Option<Vec<(usize, String)>> {
    fn eq(a: &str, b: &str) -> bool {
        a.trim_end() == b.trim_end()
    }
    fn go(segs: &[Seg], out: &[String], i: usize, pos: usize, pending: &mut Vec<usize>, steps: &mut usize, acc: &mut Vec<(usize, String)>) -> bool {
        *steps += 1;
        if *steps > 20_000 {
            return false;
        }
        if i == segs.len() {
            let rest = &out[pos..];
            if pending.is_empty() {
                return rest.iter().all(|l| l.trim().is_empty());
            }
            acc.push((pending[0], rest.join("\n")));
            for &p in pending.iter().skip(1) {
                acc.push((p, String::new()));
            }
            return true;
        }
        match &segs[i] {
            Seg::Conflict(_) => {
                pending.push(i);
                let ok = go(segs, out, i + 1, pos, pending, steps, acc);
                if !ok {
                    pending.pop();
                }
                ok
            }
            Seg::Same { orig, .. } => {
                let want: Vec<&str> = orig.split('\n').collect();
                let n = want.len();
                let mut tries = 0;
                let Some(last_start) = out.len().checked_sub(n) else { return false };
                let starts: Vec<usize> = if pending.is_empty() { vec![pos] } else { (pos..=last_start.max(pos)).collect() };
                for j in starts {
                    if j + n > out.len() {
                        break;
                    }
                    if !(0..n).all(|k| eq(&out[j + k], want[k])) {
                        continue;
                    }
                    tries += 1;
                    if tries > 60 {
                        break;
                    }
                    let mark = acc.len();
                    let saved = pending.clone();
                    if let Some(&first) = pending.first() {
                        acc.push((first, out[pos..j].join("\n")));
                        for &p in pending.iter().skip(1) {
                            acc.push((p, String::new()));
                        }
                    }
                    pending.clear();
                    if go(segs, out, i + 1, j + n, pending, steps, acc) {
                        return true;
                    }
                    acc.truncate(mark);
                    *pending = saved;
                }
                false
            }
        }
    }
    let mut acc = vec![];
    let mut steps = 0;
    if go(segs, out, 0, 0, &mut vec![], &mut steps, &mut acc) { Some(acc) } else { None }
}

/// What the AI agent follows: the rules from Settings or a skill's instructions.
pub enum Rules {
    Prompt(String),
    Skill { name: String, file: PathBuf, body: String },
}

/// The request sent to the agent for one file.
pub fn build_prompt(doc: &MergeDoc, rules: &Rules, op: Option<Operation>, branch: &str) -> String {
    let rules_text = match rules {
        Rules::Prompt(p) => {
            let p = if p.trim().is_empty() { DEFAULT_CONFLICT_PROMPT } else { p.as_str() };
            format!("Rules:\n{p}")
        }
        Rules::Skill { name, file, body } => format!(
            "Follow the \"{name}\" skill. Its instructions (from {}):\n-----\n{body}\n-----\nOther files the skill mentions are in {} and may be read.",
            file.display(),
            file.parent().map(|p| p.display().to_string()).unwrap_or_default()
        ),
    };
    let op = op.map(|o| o.title().to_lowercase()).unwrap_or_else(|| "merge".into());
    let mut text = doc.original.clone();
    const LIMIT: usize = 150_000;
    if text.len() > LIMIT {
        let mut cut = LIMIT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    let base = if doc.base_file.is_empty() || doc.base_file.len() > 60_000 {
        String::new()
    } else {
        format!("\nThe common ancestor version of the file (for reference):\n<<<BASE\n{}\nBASE>>>\n", doc.base_file.trim_end())
    };
    format!(
        "You resolve git merge conflicts.\n\n{rules_text}\n\n\
         A {op} on branch \"{branch}\" stopped with conflicts in the file {path}.\n\
         \"{ours}\" is our side (the current branch), \"{theirs}\" is the incoming side.\n\n\
         Reply with ONLY the complete resolved content of the file: no conflict markers, no explanations, no code fences. \
         Do not change any files yourself.\n{base}\n\
         The file with conflict markers:\n<<<FILE\n{body}\nFILE>>>\n",
        path = doc.path,
        ours = doc.ours_label,
        theirs = if doc.theirs_label.is_empty() { "theirs" } else { &doc.theirs_label },
        body = text.trim_end(),
    )
}

/// Strip what the agent may wrap around the file: code fences, the FILE markers.
pub fn clean_answer(s: &str) -> String {
    let mut lines: Vec<&str> = s.lines().collect();
    while lines.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.remove(0);
    }
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    // A fenced block anywhere: take what's inside the first one that spans most of the answer.
    if let Some(start) = lines.iter().position(|l| l.trim_start().starts_with("```")) {
        if let Some(end) = lines.iter().rposition(|l| l.trim_start().starts_with("```")) {
            if end > start && (end - start) * 2 >= lines.len().saturating_sub(2) {
                lines = lines[start + 1..end].to_vec();
            }
        }
    }
    if lines.first().map(|l| l.trim() == "<<<FILE").unwrap_or(false) {
        lines.remove(0);
    }
    if lines.last().map(|l| l.trim() == "FILE>>>").unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

pub fn has_markers(s: &str) -> bool {
    s.lines().any(|l| marker(l, "<<<<<<<").is_some() || marker(l, ">>>>>>>").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "a\nb\n<<<<<<< HEAD\nours1\n=======\ntheirs1\n>>>>>>> feature\nc\n\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> feature\nend\n";

    #[test]
    fn parses_and_rebuilds() {
        let d = MergeDoc::parse("f", FILE);
        assert_eq!(d.conflicts(), 2);
        assert_eq!(d.theirs_label, "feature");
        assert_eq!(d.result_text(), FILE);
    }

    #[test]
    fn fits_ai_answer_into_blocks() {
        let mut d = MergeDoc::parse("f", FILE);
        assert!(d.apply_ai("a\nb\nours1\ntheirs1\nc\n\nx\ny\nend\n"));
        assert_eq!(d.unresolved(), 0);
        assert_eq!(d.result_text(), "a\nb\nours1\ntheirs1\nc\n\nx\ny\nend\n");
        let mut d = MergeDoc::parse("f", FILE);
        assert!(!d.apply_ai("totally\ndifferent\n"));
        assert!(d.whole.is_some());
    }

    #[test]
    fn resolves_simple_conflicts() {
        // Different lines changed on each side: merged.
        assert_eq!(merge3("a\nb\nc", "A\nb\nc", "a\nb\nC").as_deref(), Some("A\nb\nC"));
        // Next to each other, but not the same line.
        assert_eq!(merge3("a\nb", "A\nb", "a\nB").as_deref(), Some("A\nB"));
        // Only one side changed.
        assert_eq!(merge3("a\nb", "a\nb", "x\ny").as_deref(), Some("x\ny"));
        // The same line changed differently: a real conflict.
        assert_eq!(merge3("a\nb", "A\nb", "Z\nb"), None);
        // Both insert at the same place.
        assert_eq!(merge3("a", "a\nx", "a\ny"), None);
        let c = |o: &str, b: Option<&str>, t: &str| Chunk { ours: o.into(), base: b.map(String::from), theirs: t.into(), result: None };
        assert_eq!(simple_resolution(&c("2.0.7.0", Some("2.0.0.84"), "2.0.4.0")), None);
        assert_eq!(simple_resolution(&c("x", None, "x")).as_deref(), Some("x"));
        assert_eq!(simple_resolution(&c("x", None, "y")), None);
        // A file whose base is known through diff3 markers.
        let mut d = MergeDoc::parse("f", "<<<<<<< HEAD\nA\nb\n||||||| base\na\nb\n=======\na\nB\n>>>>>>> t\n<<<<<<< HEAD\n1\n||||||| base\n0\n=======\n2\n>>>>>>> t\n");
        assert_eq!((d.conflicts(), d.simple_count()), (2, 1));
        assert_eq!(d.resolve_simple(), 1);
        assert_eq!(d.unresolved(), 1);
        assert_eq!(d.next_open(true), Some(1));
    }

    #[test]
    fn finds_bases_with_git() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("ge-merge-test-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let env = [("GIT_AUTHOR_NAME", "A"), ("GIT_AUTHOR_EMAIL", "a@e"), ("GIT_COMMITTER_NAME", "A"), ("GIT_COMMITTER_EMAIL", "a@e")];
        let g = |args: &[&str]| git::run_env(&dir, &env, args);
        assert!(g(&["init", "-q", "-b", "main"]).ok);
        std::fs::write(dir.join("f.txt"), "one\ntwo\nthree\n").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "base"]);
        g(&["checkout", "-qb", "other"]);
        std::fs::write(dir.join("f.txt"), "one\nTWO\nthree\n").unwrap();
        g(&["commit", "-qam", "theirs"]);
        g(&["checkout", "-q", "main"]);
        std::fs::write(dir.join("f.txt"), "ONE\ntwo\nthree\n").unwrap();
        g(&["commit", "-qam", "ours"]);
        assert!(!g(&["merge", "-q", "other"]).ok);
        let mut d = MergeDoc::load(&dir, "f.txt");
        assert_eq!(d.conflicts(), 1);
        // git 2.43+ gives the base; older ones leave it unknown.
        if let Seg::Conflict(c) = &d.segs[0] {
            if c.base.is_some() {
                assert_eq!(c.base.as_deref(), Some("one\ntwo"));
                assert_eq!(d.resolve_simple(), 1);
                assert_eq!(d.result_text(), "ONE\nTWO\nthree\n");
            }
        }
    }

    #[test]
    fn cleans_fences() {
        assert_eq!(clean_answer("```python\nx = 1\n```"), "x = 1");
        assert_eq!(clean_answer("Here it is:\n```\nx\ny\n```\n"), "x\ny");
    }
}
