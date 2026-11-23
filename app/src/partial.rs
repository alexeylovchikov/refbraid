//! The separate changes ("blocks") of a file's diff: what partial commits include, what the »
//! arrow reverts, and which unchanged lines can be folded away.

use crate::git::{DiffRow, DiffSide};
use std::hash::{Hash, Hasher};

/// One change: consecutive changed rows of a whole-file diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub first_row: usize,
    pub last_row: usize,
    /// Lines (0-based) the change replaces in the old file…
    pub old_start: usize,
    pub old_len: usize,
    /// …and the lines it has in the new file.
    pub new_start: usize,
    pub new_len: usize,
    /// Identifies the change by its text, so a choice survives edits elsewhere in the file.
    pub hash: u64,
}

pub fn blocks(rows: &[DiffRow]) -> Vec<Block> {
    let mut out = vec![];
    let (mut l, mut r) = (0usize, 0usize);
    let mut i = 0;
    while i < rows.len() {
        let row = &rows[i];
        if row.kind == DiffSide::Context {
            if let Some((n, _)) = &row.left {
                l = *n;
            }
            if let Some((n, _)) = &row.right {
                r = *n;
            }
            i += 1;
            continue;
        }
        let (start_l, start_r, first) = (l, r, i);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let (mut ol, mut nl) = (0, 0);
        while i < rows.len() && rows[i].kind != DiffSide::Context {
            if let Some((n, t)) = &rows[i].left {
                l = *n;
                ol += 1;
                ("-", t).hash(&mut h);
            }
            if let Some((n, t)) = &rows[i].right {
                r = *n;
                nl += 1;
                ("+", t).hash(&mut h);
            }
            i += 1;
        }
        out.push(Block { first_row: first, last_row: i - 1, old_start: start_l, old_len: ol, new_start: start_r, new_len: nl, hash: h.finish() });
    }
    out
}

fn lines(b: &[u8]) -> Vec<&[u8]> {
    b.split_inclusive(|c| *c == b'\n').collect()
}

/// The file with the chosen changes applied to the old version: `take_new(i)` decides block i.
/// With every block taken it is the new file; with none, the old one.
pub fn mix(old: &[u8], new: &[u8], blocks: &[Block], take_new: impl Fn(usize) -> bool) -> Vec<u8> {
    let (o, n) = (lines(old), lines(new));
    let mut out = Vec::with_capacity(new.len().max(old.len()));
    let mut at = 0;
    for (i, b) in blocks.iter().enumerate() {
        let os = b.old_start.min(o.len());
        for l in &o[at.min(os)..os] {
            out.extend_from_slice(l);
        }
        let part = if take_new(i) {
            &n[b.new_start.min(n.len())..(b.new_start + b.new_len).min(n.len())]
        } else {
            &o[os..(b.old_start + b.old_len).min(o.len())]
        };
        for l in part {
            out.extend_from_slice(l);
        }
        at = (b.old_start + b.old_len).min(o.len());
    }
    for l in &o[at.min(o.len())..] {
        out.extend_from_slice(l);
    }
    out
}

/// What the diff view shows: rows, or folded runs of unchanged rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disp {
    Row(usize),
    /// Rows `from..to` hidden.
    Fold { from: usize, to: usize },
}

/// Fold unchanged runs, keeping `ctx` rows next to each change; folds whose first row is in
/// `expanded` stay open.
pub fn fold_rows(rows: &[DiffRow], ctx: usize, expanded: &std::collections::HashSet<usize>) -> Vec<Disp> {
    let mut out = vec![];
    let mut i = 0;
    while i < rows.len() {
        if rows[i].kind != DiffSide::Context {
            out.push(Disp::Row(i));
            i += 1;
            continue;
        }
        let start = i;
        while i < rows.len() && rows[i].kind == DiffSide::Context {
            i += 1;
        }
        let end = i;
        let keep_top = if start == 0 { 0 } else { ctx };
        let keep_bottom = if end == rows.len() { 0 } else { ctx };
        if end - start > keep_top + keep_bottom + 1 && !expanded.contains(&(start + keep_top)) {
            out.extend((start..start + keep_top).map(Disp::Row));
            out.push(Disp::Fold { from: start + keep_top, to: end - keep_bottom });
            out.extend((end - keep_bottom..end).map(Disp::Row));
        } else {
            out.extend((start..end).map(Disp::Row));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(old: &str, new: &str) -> Vec<DiffRow> {
        // A tiny line diff good enough for the tests: common prefix/suffix, the rest changed.
        let o: Vec<&str> = old.lines().collect();
        let n: Vec<&str> = new.lines().collect();
        let mut text = String::from("@@ -1 +1 @@\n");
        let mut a = 0;
        while a < o.len() && a < n.len() && o[a] == n[a] {
            text += &format!(" {}\n", o[a]);
            a += 1;
        }
        let mut z = 0;
        while z < o.len() - a && z < n.len() - a && o[o.len() - 1 - z] == n[n.len() - 1 - z] {
            z += 1;
        }
        for l in &o[a..o.len() - z] {
            text += &format!("-{l}\n");
        }
        for l in &n[a..n.len() - z] {
            text += &format!("+{l}\n");
        }
        for l in &o[o.len() - z..] {
            text += &format!(" {l}\n");
        }
        crate::git::parse_diff(&text).rows
    }

    #[test]
    fn partial_and_revert() {
        let old = "a\nb\nc\nd\ne\n";
        let new = "a\nB\nc\nd\ne\nf\n";
        // Two separate changes: b→B and the added f; build the rows by hand from two diffs.
        let mut text = String::from("@@ -1,5 +1,6 @@\n a\n-b\n+B\n c\n d\n e\n+f\n");
        text.push('\n');
        let rows = crate::git::parse_diff(&text).rows;
        let bl = blocks(&rows);
        assert_eq!(bl.len(), 2);
        assert_eq!((bl[1].old_start, bl[1].old_len, bl[1].new_start, bl[1].new_len), (5, 0, 5, 1));
        assert_eq!(mix(old.as_bytes(), new.as_bytes(), &bl, |_| true), new.as_bytes());
        assert_eq!(mix(old.as_bytes(), new.as_bytes(), &bl, |_| false), old.as_bytes());
        assert_eq!(mix(old.as_bytes(), new.as_bytes(), &bl, |i| i == 1), b"a\nb\nc\nd\ne\nf\n");
        // Revert the first change only.
        assert_eq!(mix(old.as_bytes(), new.as_bytes(), &bl, |i| i != 0), b"a\nb\nc\nd\ne\nf\n");
        let _ = diff(old, new);
    }

    #[test]
    fn folds_long_unchanged_runs() {
        let old: String = (0..30).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l15\n", "X\n");
        let rows = diff(&old, &new);
        let d = fold_rows(&rows, 3, &Default::default());
        assert!(matches!(d[0], Disp::Fold { from: 0, to: 12 }));
        assert!(d.iter().filter(|x| matches!(x, Disp::Fold { .. })).count() == 2);
        let mut open = std::collections::HashSet::new();
        open.insert(0);
        assert!(!matches!(fold_rows(&rows, 3, &open)[0], Disp::Fold { .. }));
    }
}
