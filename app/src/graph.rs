//! Lays out the commit graph: which column each commit sits in and which lines pass each row.

use crate::git::Commit;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
pub struct Segment {
    /// Positions in lanes (a line between two rows crosses their border half-way, so these can
    /// be fractional).
    pub from_col: f32,
    pub from: End,
    pub to_col: f32,
    pub to: End,
    pub color: u16,
    /// Drawn dashed: this line skips commits that are hidden.
    pub dashed: bool,
}

/// The cut end of a long line: an arrow pointing down (the line goes on far below) or up (it
/// comes from far above).
#[derive(Clone, Copy, Debug)]
pub struct Arrow {
    pub col: f32,
    pub down: bool,
    pub color: u16,
    /// The commit at the other end of the line (index into the commit list): the parent for a
    /// down arrow, the child for an up arrow. None when the parent is not loaded.
    pub target: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub col: u16,
    pub color: u16,
    pub segments: Vec<Segment>,
    pub arrows: Vec<Arrow>,
    pub width: u16,
}

/// Graph style chosen in Settings → Log: "Fixed lanes" instead of "Compact".
static FIXED_LANES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_fixed_lanes(on: bool) {
    FIXED_LANES.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Lay the graph out in the style chosen in the settings.
pub fn layout_chosen(
    commits: &[Commit],
    dashed: &std::collections::HashSet<String>,
    main: &std::collections::HashSet<String>,
) -> Vec<Row> {
    if FIXED_LANES.load(std::sync::atomic::Ordering::Relaxed) { layout_fixed(commits, dashed, main) } else { layout(commits, dashed, main) }
}

/// Lines to a parent further down than this many rows are cut: only a short piece with an
/// arrow is drawn at each end, so they don't take a lane all the way.
const LONG: usize = 30;

/// A line from a commit to one of its parents.
struct Edge {
    from: usize,
    /// Row of the parent; None when it is not in the list.
    to: Option<usize>,
    color: u16,
    dashed: bool,
    /// Drawn as two short pieces with arrows.
    cut: bool,
    first_parent: bool,
}

/// Lays the graph out the way the IDE does: in every row the lines and the commit are packed
/// from the left with no gaps, and each line runs straight from its place in one row to its
/// place in the next — so lines slant where a lane closes or opens.
///
/// `main` holds the current branch's commits (HEAD and its first parents): they always sit in
/// column 0, and lines that are there when HEAD appears move to the right.
pub fn layout(
    commits: &[Commit],
    dashed: &std::collections::HashSet<String>,
    main: &std::collections::HashSet<String>,
) -> Vec<Row> {
    let n = commits.len();
    let present: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, c)| (c.hash.as_str(), i)).collect();
    let mut rows: Vec<Row> = (0..n).map(|_| Row::default()).collect();
    let mut edges: Vec<Edge> = vec![];
    // Lines crossing the border above the current row, left to right, with their place in the
    // row above.
    let mut active: Vec<(usize, f32)> = vec![];
    // Cut lines whose lower piece starts in a row.
    let mut lower_starts: HashMap<usize, Vec<usize>> = HashMap::new();
    // Colour of the line each parent will continue.
    let mut parent_color: HashMap<&str, u16> = HashMap::new();
    let mut next_color: u16 = 0;
    let mut new_color = || {
        let c = next_color;
        next_color = next_color.wrapping_add(1);
        c
    };

    for (r, c) in commits.iter().enumerate() {
        let is_main = main.contains(&c.hash);
        // Lines that end in this row: at the commit, or with an arrow (the upper piece of a
        // cut line).
        let mut through: Vec<(usize, f32, bool)> = vec![]; // (edge, place above, ends with an arrow)
        let mut into_node: Vec<(usize, f32)> = vec![];
        let mut first_in: Option<usize> = None;
        for &(id, px) in &active {
            let e = &edges[id];
            if e.to == Some(r) {
                first_in.get_or_insert(through.len());
                into_node.push((id, px));
            } else {
                let arrow = e.cut && r == e.from + 1;
                through.push((id, px, arrow));
            }
        }
        let node = if is_main { 0 } else { first_in.unwrap_or(through.len()) };
        let place = |i: usize| if i < node { i as f32 } else { (i + 1) as f32 };
        let node_x = node as f32;
        let color = into_node
            .iter()
            .find(|(id, _)| edges[*id].first_parent)
            .or(into_node.first())
            .map(|(id, _)| edges[*id].color)
            .or_else(|| parent_color.get(c.hash.as_str()).copied())
            .unwrap_or_else(&mut new_color);
        rows[r].col = node as u16;
        rows[r].color = color;

        // The upper half of every line crossing the border above, and the lower half of the
        // same line in the row above.
        let halves = |id: usize, px: f32, cx: f32, rows: &mut Vec<Row>| {
            let e = &edges[id];
            let bx = (px + cx) / 2.0;
            rows[r].segments.push(Segment { from_col: bx, from: End::Top, to_col: cx, to: End::Middle, color: e.color, dashed: e.dashed });
            if r > 0 {
                rows[r - 1].segments.push(Segment { from_col: px, from: End::Middle, to_col: bx, to: End::Bottom, color: e.color, dashed: e.dashed });
            }
        };
        for (i, &(id, px, arrow)) in through.iter().enumerate() {
            let cx = place(i);
            halves(id, px, cx, &mut rows);
            if arrow {
                rows[r].arrows.push(Arrow { col: cx, down: true, color: edges[id].color, target: edges[id].to });
            }
        }
        for &(id, px) in &into_node {
            halves(id, px, node_x, &mut rows);
        }

        // Lines to the parents.
        let is_dashed = dashed.contains(&c.hash);
        let mut out: Vec<usize> = vec![];
        for (k, p) in c.parents.iter().enumerate() {
            let to = present.get(p.as_str()).copied().filter(|&q| q > r);
            let colr = if k == 0 {
                parent_color.entry(p.as_str()).or_insert(color);
                color
            } else {
                *parent_color.entry(p.as_str()).or_insert_with(&mut new_color)
            };
            // The current branch's own line is never cut.
            let keep = k == 0 && is_main;
            let cut = !keep && to.map(|q| q - r > LONG).unwrap_or(true);
            let id = edges.len();
            edges.push(Edge { from: r, to, color: colr, dashed: is_dashed, cut, first_parent: k == 0 });
            if let (true, Some(q)) = (cut, to) {
                lower_starts.entry(q - 1).or_default().push(id);
            }
            if r + 1 < n {
                out.push(id);
            }
        }

        // The lines crossing the border below this row, left to right.
        let mut next: Vec<(usize, f32)> = vec![];
        for (i, &(id, _, arrow)) in through.iter().enumerate() {
            if i == node {
                next.extend(out.iter().map(|&id| (id, node_x)));
            }
            if !arrow {
                next.push((id, place(i)));
            }
        }
        if node >= through.len() {
            next.extend(out.iter().map(|&id| (id, node_x)));
        }
        // Lower pieces of cut lines start here, at the right, with an arrow pointing up.
        let mut last = (through.len() + 1) as f32;
        for id in lower_starts.remove(&r).unwrap_or_default() {
            rows[r].arrows.push(Arrow { col: last, down: false, color: edges[id].color, target: Some(edges[id].from) });
            next.push((id, last));
            last += 1.0;
        }
        active = next;
    }
    for row in &mut rows {
        let w = row
            .segments
            .iter()
            .map(|s| s.from_col.max(s.to_col))
            .chain(row.arrows.iter().map(|a| a.col))
            .fold(row.col as f32, f32::max);
        row.width = w.ceil() as u16 + 1;
    }
    rows
}

/// The "Fixed lanes" style: every line keeps its column from the commit where it starts to
/// the one where it ends, and a new branch takes the leftmost free column.
///
/// `main` holds the current branch's commits (HEAD and its first parents): they always sit in
/// column 0, and a line that is there when HEAD appears moves one column to the right.
pub fn layout_fixed(
    commits: &[Commit],
    dashed: &std::collections::HashSet<String>,
    main: &std::collections::HashSet<String>,
) -> Vec<Row> {
    // Each lane holds the hash it is waiting for and its colour.
    let mut lanes: Vec<Option<(String, u16)>> = Vec::new();
    let mut next_color: u16 = 0;
    let mut rows = Vec::with_capacity(commits.len());
    let present: HashMap<&str, usize> =
        commits.iter().enumerate().map(|(i, c)| (c.hash.as_str(), i)).collect();

    for c in commits {
        let mut row = Row::default();
        let is_main = main.contains(&c.hash);
        // Make room in column 0 for the current branch.
        let mut moved: Option<(usize, usize)> = None;
        if is_main {
            if lanes.is_empty() {
                lanes.push(None);
            }
            if lanes[0].as_ref().map(|(h, _)| h != &c.hash).unwrap_or(false) {
                let k = lanes.iter().skip(1).position(|l| l.is_none()).map(|p| p + 1).unwrap_or(lanes.len());
                if k == lanes.len() {
                    lanes.push(None);
                }
                lanes[k] = lanes[0].take();
                moved = Some((0, k));
            }
        }
        let mut before = lanes.clone();
        if let Some((from, to)) = moved {
            // Draw the moved line from its old column at the top of this row.
            before[from] = before[to].take();
        }
        let matching: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_ref().map(|(h, _)| h == &c.hash).unwrap_or(false))
            .map(|(i, _)| i)
            .collect();
        let (col, color) = if is_main {
            let color = match matching.first() {
                Some(&first) => lanes[first].as_ref().unwrap().1,
                None => {
                    let c = next_color;
                    next_color = next_color.wrapping_add(1);
                    c
                }
            };
            (0, color)
        } else if let Some(&first) = matching.first() {
            (first, lanes[first].as_ref().unwrap().1)
        } else {
            let col = lanes.iter().position(|l| l.is_none()).unwrap_or(lanes.len());
            if col == lanes.len() {
                lanes.push(None);
            }
            let color = next_color;
            next_color = next_color.wrapping_add(1);
            (col, color)
        };
        row.col = col as u16;
        row.color = color;

        // Lines coming into the commit from above.
        for &j in &matching {
            let col_color = before[j].as_ref().unwrap().1;
            row.segments.push(Segment {
                from_col: j as f32,
                from: End::Top,
                to_col: col as f32,
                to: End::Middle,
                color: col_color,
                dashed: false,
            });
            if j != col {
                lanes[j] = None;
            }
        }
        // Lines just passing through.
        for (j, l) in before.iter().enumerate() {
            if let Some((h, colr)) = l {
                if h != &c.hash {
                    let to = match moved {
                        Some((from, to)) if from == j => to,
                        _ => j,
                    };
                    row.segments.push(Segment {
                        from_col: j as f32,
                        from: End::Top,
                        to_col: to as f32,
                        to: End::Bottom,
                        color: *colr,
                        dashed: false,
                    });
                }
            }
        }
        // Lines going down to the parents.
        let is_dashed = dashed.contains(&c.hash);
        let parents: Vec<&String> = c.parents.iter().collect();
        if parents.is_empty() {
            lanes[col] = None;
        } else {
            // First parent: continue this lane unless another lane already waits for it.
            let p0 = parents[0];
            let existing = lanes
                .iter()
                .enumerate()
                .position(|(j, l)| j != col && l.as_ref().map(|(h, _)| h == p0).unwrap_or(false));
            match existing {
                Some(k) if present.contains_key(p0.as_str()) && !is_main => {
                    lanes[col] = None;
                    row.segments.push(Segment {
                        from_col: col as f32,
                        from: End::Middle,
                        to_col: k as f32,
                        to: End::Bottom,
                        color: lanes[k].as_ref().unwrap().1,
                        dashed: is_dashed,
                    });
                }
                _ => {
                    lanes[col] = Some((p0.clone(), color));
                    row.segments.push(Segment {
                        from_col: col as f32,
                        from: End::Middle,
                        to_col: col as f32,
                        to: End::Bottom,
                        color,
                        dashed: is_dashed,
                    });
                }
            }
            for p in parents.iter().skip(1) {
                let existing = lanes
                    .iter()
                    .position(|l| l.as_ref().map(|(h, _)| h == *p).unwrap_or(false));
                let (k, colr) = match existing {
                    Some(k) => (k, lanes[k].as_ref().unwrap().1),
                    None => {
                        let k = lanes.iter().position(|l| l.is_none()).unwrap_or(lanes.len());
                        if k == lanes.len() {
                            lanes.push(None);
                        }
                        let colr = next_color;
                        next_color = next_color.wrapping_add(1);
                        lanes[k] = Some(((*p).clone(), colr));
                        (k, colr)
                    }
                };
                row.segments.push(Segment {
                    from_col: col as f32,
                    from: End::Middle,
                    to_col: k as f32,
                    to: End::Bottom,
                    color: colr,
                    dashed: is_dashed,
                });
            }
        }
        while matches!(lanes.last(), Some(None)) {
            lanes.pop();
        }
        let w = row
            .segments
            .iter()
            .map(|s| s.from_col.max(s.to_col).ceil() as u16 + 1)
            .max()
            .unwrap_or(1)
            .max(row.col + 1);
        row.width = w;
        rows.push(row);
    }
    rows
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn c(h: &str, p: &[&str]) -> Commit {
        Commit { hash: h.into(), parents: p.iter().map(|s| s.to_string()).collect(), author: String::new(), email: String::new(), time: 0, subject: String::new() }
    }

    #[test]
    fn current_branch_stays_left() {
        // s2 (stage tip, merge) is newest; m2 is HEAD (main).
        let commits = vec![
            c("s2", &["s1", "f1"]),
            c("m2", &["m1", "f1"]),
            c("s1", &["base"]),
            c("f1", &["base"]),
            c("m1", &["base"]),
            c("base", &[]),
        ];
        let main: HashSet<String> = ["m2", "m1", "base"].iter().map(|s| s.to_string()).collect();
        let rows = layout(&commits, &HashSet::new(), &main);
        assert_eq!(rows[1].col, 0, "HEAD in column 0");
        assert_eq!(rows[4].col, 0, "main's parent in column 0");
        assert_eq!(rows[5].col, 0);
        assert_ne!(rows[2].col, 0, "stage commit moved right");
        // the stage line slants from column 0 to the right in HEAD's row
        assert!(rows[1].segments.iter().any(|s| s.from == End::Top && s.to == End::Middle && s.to_col == 1.0 && s.from_col < 1.0));
    }

    #[test]
    fn lanes_close_up_and_long_lines_are_cut() {
        // b2 merges into m1; after that the line of c (column 2) slants left to column 1.
        let commits = vec![
            c("m2", &["m1", "b2"]),
            c("b2", &["m1"]),
            c("c2", &["c1"]),
            c("m1", &["m0"]),
            c("c1", &["m0"]),
            c("m0", &[]),
        ];
        let main: HashSet<String> = ["m2", "m1", "m0"].iter().map(|s| s.to_string()).collect();
        let rows = layout(&commits, &HashSet::new(), &main);
        assert_eq!(rows[1].col, 1);
        assert_eq!(rows[2].col, 2);
        assert!(rows[3].segments.iter().any(|s| s.from == End::Top && s.from_col == 1.5 && s.to_col == 1.0), "c's line slants left");
        assert_eq!(rows[4].col, 1, "c moved into the closed lane");
        assert!(rows.iter().all(|r| r.arrows.is_empty()));

        // A side commit whose parent is 41 rows down: cut, with an arrow at each end.
        let mut commits = vec![c("x", &["m0"])];
        for i in (0..=40).rev() {
            let p = if i > 0 { vec![format!("m{}", i - 1)] } else { vec![] };
            let p: Vec<&str> = p.iter().map(|s| s.as_str()).collect();
            commits.push(c(&format!("m{i}"), &p));
        }
        let main: HashSet<String> = (0..=40).map(|i| format!("m{i}")).collect();
        let rows = layout(&commits, &HashSet::new(), &main);
        assert!(rows[1].arrows.iter().any(|a| a.down && a.target == Some(41)), "arrow below x leads to m0");
        assert!(rows[40].arrows.iter().any(|a| !a.down && a.target == Some(0)), "arrow above m0 leads back to x");
        assert!(rows[2..40].iter().all(|r| r.width == 1), "no lane kept in between");
        assert_eq!(rows[41].col, 0);
    }
}
