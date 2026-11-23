//! Search in the text of a diff (Ctrl+F): where the words are, so the viewer can mark them
//! and step from one to the next.

use crate::git::DiffText;

/// A place where the searched text was found. Columns count characters of the line as shown
/// (tabs are four spaces).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 0 = left (old) side, 1 = right (new) side, 2 = the unified view.
    pub side: u8,
    /// Row of the diff (side by side) or line of the unified text.
    pub line: usize,
    pub from: usize,
    pub to: usize,
}

/// The search line above a diff.
#[derive(Clone, Default)]
pub struct Find {
    pub query: String,
    pub regex: bool,
    pub case: bool,
    /// The hit the viewer is at.
    pub current: usize,
    /// Put the cursor into the field.
    pub focus: bool,
    /// Bring the current hit into view.
    pub go: bool,
    /// The hits, and what they were found for.
    pub cache: Option<(String, Vec<Hit>)>,
}

impl Find {
    pub fn opened() -> Find {
        Find { focus: true, ..Default::default() }
    }

    /// The hits in `d` (found again only when the question or the view changed).
    pub fn hits(&mut self, d: &DiffText, side_by_side: bool) -> &[Hit] {
        let key = format!("{}\u{1}{}{}{}\u{1}{}\u{1}{}", self.query, self.regex as u8, self.case as u8, side_by_side as u8, d.rows.len(), d.unified.len());
        if self.cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let hits = find(d, side_by_side, &self.query, self.regex, self.case);
            self.cache = Some((key, hits));
        }
        let hits = &self.cache.as_ref().unwrap().1;
        if self.current >= hits.len() {
            self.current = 0;
        }
        hits
    }

    /// Step to the next (or previous) hit, around the ends.
    pub fn step(&mut self, n: usize, forward: bool) {
        if n == 0 {
            return;
        }
        self.current = if forward { (self.current + 1) % n } else { (self.current + n - 1) % n };
        self.go = true;
    }
}

/// What is searched for, ready to be looked up line by line.
pub enum Matcher {
    Plain(Vec<char>, bool),
    Regex(regex::Regex),
}

impl Matcher {
    /// None when there is nothing to search for (no text, or a pattern that is not valid).
    pub fn new(query: &str, regex: bool, case: bool) -> Option<Matcher> {
        if query.is_empty() {
            return None;
        }
        if regex {
            regex::RegexBuilder::new(query).case_insensitive(!case).build().ok().map(Matcher::Regex)
        } else {
            Some(Matcher::Plain(query.chars().map(|c| if case { c } else { lower(c) }).collect(), case))
        }
    }

    /// Character columns of the matches in `text`.
    pub fn in_line(&self, text: &str) -> Vec<(usize, usize)> {
        match self {
            Matcher::Plain(needle, case) => {
                let hay: Vec<char> = text.chars().map(|c| if *case { c } else { lower(c) }).collect();
                let mut out = vec![];
                let mut i = 0;
                while i + needle.len() <= hay.len() {
                    if hay[i..i + needle.len()] == needle[..] {
                        out.push((i, i + needle.len()));
                        i += needle.len();
                    } else {
                        i += 1;
                    }
                }
                out
            }
            Matcher::Regex(re) => re
                .find_iter(text)
                .filter(|m| !m.is_empty())
                .map(|m| (text[..m.start()].chars().count(), text[..m.end()].chars().count()))
                .collect(),
        }
    }
    /// The matches in a text of several lines: (line, from, to), top to bottom.
    pub fn in_text(&self, text: &str) -> Vec<(usize, usize, usize)> {
        text.split('\n').enumerate().flat_map(|(n, l)| self.in_line(l).into_iter().map(move |(a, b)| (n, a, b))).collect()
    }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Every place `query` occurs in the diff, top to bottom (left side before right on a row).
pub fn find(d: &DiffText, side_by_side: bool, query: &str, regex: bool, case: bool) -> Vec<Hit> {
    let Some(m) = Matcher::new(query, regex, case) else { return vec![] };
    let mut out = vec![];
    if side_by_side {
        for (k, row) in d.rows.iter().enumerate() {
            for (side, cell) in [(0u8, &row.left), (1, &row.right)] {
                if let Some((_, text)) = cell {
                    out.extend(m.in_line(&text.replace('\t', "    ")).into_iter().map(|(from, to)| Hit { side, line: k, from, to }));
                }
            }
        }
    } else {
        for (k, (tag, text)) in d.unified.iter().enumerate() {
            if *tag != '@' {
                out.extend(m.in_line(&text.replace('\t', "    ")).into_iter().map(|(from, to)| Hit { side: 2, line: k, from, to }));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{DiffRow, DiffSide};

    fn diff() -> DiffText {
        let row = |kind, l: Option<&str>, r: Option<&str>| DiffRow { kind, left: l.map(|t| (1, t.to_string())), right: r.map(|t| (1, t.to_string())) };
        DiffText {
            rows: vec![
                row(DiffSide::Context, Some("fn total(items: &[Item]) {"), Some("fn total(items: &[Item]) {")),
                row(DiffSide::Changed, Some("\tlet Total = 0;"), Some("\tlet sum = 0; // total")),
                row(DiffSide::Added, None, Some("    итого += 1;")),
            ],
            binary: false,
            unified: vec![('@', "@@ -1,2 +1,3 @@ total".into()), (' ', "fn total(items: &[Item]) {".into()), ('-', "\tlet Total = 0;".into()), ('+', "\tlet sum = 0; // total".into())],
            images: None,
        }
    }

    #[test]
    fn finds_text_on_both_sides() {
        let d = diff();
        let h = find(&d, true, "total", false, false);
        let at: Vec<(u8, usize, usize, usize)> = h.iter().map(|h| (h.side, h.line, h.from, h.to)).collect();
        // The tab counts as four columns.
        assert_eq!(at, [(0, 0, 3, 8), (1, 0, 3, 8), (0, 1, 8, 13), (1, 1, 20, 25)]);
        assert_eq!(find(&d, true, "Total", false, true).len(), 1);
        assert_eq!(find(&d, true, "ИТОГО", false, false), [Hit { side: 1, line: 2, from: 4, to: 9 }]);
        assert!(find(&d, true, "", false, false).is_empty());
    }

    #[test]
    fn finds_with_a_pattern_and_in_the_unified_view() {
        let d = diff();
        let h = find(&d, false, r"let \w+", true, false);
        assert_eq!(h, [Hit { side: 2, line: 2, from: 4, to: 13 }, Hit { side: 2, line: 3, from: 4, to: 11 }]);
        // The hunk header is not searched; a broken pattern finds nothing.
        assert_eq!(find(&d, false, "total", false, false).len(), 3);
        assert!(find(&d, false, "(", true, false).is_empty());
    }

    #[test]
    fn steps_around() {
        let mut f = Find { query: "total".into(), ..Default::default() };
        let n = f.hits(&diff(), true).len();
        assert_eq!(n, 4);
        f.step(n, false);
        assert_eq!(f.current, 3);
        f.step(n, true);
        assert_eq!(f.current, 0);
    }
}
