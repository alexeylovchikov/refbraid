//! Changes inside a line: which words of the old and the new version of a changed line differ,
//! so the diff can colour just those (e.g. "reg.alpha.example" → "reg.bravo.example").

use std::ops::Range;

/// Words (letters, digits, `_`), runs of spaces and single other characters, as byte ranges.
fn tokens(s: &str) -> Vec<Range<usize>> {
    let mut out = vec![];
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut end = i + c.len_utf8();
        if word(c) || c.is_whitespace() {
            let same = |d: char| if word(c) { word(d) } else { d.is_whitespace() };
            while let Some(&(j, d)) = it.peek() {
                if !same(d) {
                    break;
                }
                end = j + d.len_utf8();
                it.next();
            }
        }
        out.push(i..end);
    }
    out
}

/// Byte ranges of `old` and `new` that differ. Empty when the lines have nothing in common
/// (colouring all of both would say nothing) or are too long to compare quickly.
pub fn changed_ranges(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    if old == new {
        return (vec![], vec![]);
    }
    let (a, b) = (tokens(old), tokens(new));
    const MAX: usize = 400;
    if a.len() > MAX || b.len() > MAX {
        return (vec![], vec![]);
    }
    let ta: Vec<&str> = a.iter().map(|r| &old[r.clone()]).collect();
    let tb: Vec<&str> = b.iter().map(|r| &new[r.clone()]).collect();
    // Longest common subsequence of the tokens.
    let (n, m) = (ta.len(), tb.len());
    let mut l = vec![0u16; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[at(i, j)] = if ta[i] == tb[j] { l[at(i + 1, j + 1)] + 1 } else { l[at(i + 1, j)].max(l[at(i, j + 1)]) };
        }
    }
    let (mut keep_a, mut keep_b) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if ta[i] == tb[j] {
            keep_a[i] = true;
            keep_b[j] = true;
            i += 1;
            j += 1;
        } else if l[at(i + 1, j)] >= l[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    // Nothing but spaces in common: the line was rewritten, not edited.
    let shared_text = ta.iter().zip(&keep_a).any(|(t, k)| *k && !t.trim().is_empty());
    if !shared_text {
        return (vec![], vec![]);
    }
    (merge(&a, &keep_a), merge(&b, &keep_b))
}

/// Join neighbouring changed tokens into ranges.
fn merge(tokens: &[Range<usize>], keep: &[bool]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = vec![];
    for (r, k) in tokens.iter().zip(keep) {
        if *k {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.end == r.start => last.end = r.end,
            _ => out.push(r.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts<'a>(s: &'a str, r: &[Range<usize>]) -> Vec<&'a str> {
        r.iter().map(|x| &s[x.clone()]).collect()
    }

    #[test]
    fn finds_changed_words() {
        let a = "IMAGENAME := reg.alpha.example/shop/scanner_ui";
        let b = "IMAGENAME := reg.bravo.example/shop/scanner_ui";
        let (ra, rb) = changed_ranges(a, b);
        assert_eq!(parts(a, &ra), vec!["alpha"]);
        assert_eq!(parts(b, &rb), vec!["bravo"]);
        let (ra, rb) = changed_ranges("let x = 1;", "let x = 10; // ten");
        assert_eq!(parts("let x = 1;", &ra), vec!["1"]);
        assert_eq!(parts("let x = 10; // ten", &rb), vec!["10", " // ten"]);
    }

    #[test]
    fn rewritten_lines_are_not_coloured() {
        assert_eq!(changed_ranges("alpha beta", "gamma delta"), (vec![], vec![]));
        assert_eq!(changed_ranges("same", "same"), (vec![], vec![]));
        let (ra, _) = changed_ranges("привет мир", "привет всем");
        assert_eq!(parts("привет мир", &ra), vec!["мир"]);
    }
}
