//! Help → Changelog: the list of changes by version, from `CHANGELOG.md` built into the program.
//!
//! The file is kept up to date by the `changelog` skill (`.claude/skills/changelog/SKILL.md`),
//! which `make release-all` runs. Only the small Markdown subset it writes is understood:
//! `#`, `##`, `###` headings, `- ` items, `**bold**`, `` `code` `` and blank lines.

use crate::theme;
use crate::i18n::tr;
use eframe::egui::{self, Color32, FontId, RichText, TextFormat, Ui, text::LayoutJob};

/// `CHANGELOG.md` is written in Russian; `CHANGELOG.en.md` holds the same sections in English.
pub const TEXT: &str = include_str!("../CHANGELOG.md");
pub const TEXT_EN: &str = include_str!("../CHANGELOG.en.md");

/// The changelog in the interface language.
pub fn text() -> &'static str {
    match crate::i18n::lang() {
        crate::i18n::Lang::Ru => TEXT,
        crate::i18n::Lang::En => TEXT_EN,
    }
}

#[derive(Debug, PartialEq)]
pub enum Line<'a> {
    /// `## 0.3.2 — 2026-11-16`: the version and the date (may be empty).
    Version(&'a str, &'a str),
    /// `### Новое`
    Group(&'a str),
    Item(&'a str),
    Text(&'a str),
    Blank,
}

pub fn parse(text: &str) -> Vec<Line<'_>> {
    let mut out = vec![];
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with("<!--") || (t.starts_with("# ") && !t.starts_with("## ")) {
            continue;
        }
        let line = if let Some(h) = t.strip_prefix("### ") {
            Line::Group(h.trim())
        } else if let Some(h) = t.strip_prefix("## ") {
            let (v, d) = h.split_once('—').or_else(|| h.split_once(" - ")).unwrap_or((h, ""));
            Line::Version(v.trim(), d.trim())
        } else if let Some(i) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            Line::Item(i.trim())
        } else if t.is_empty() {
            Line::Blank
        } else {
            Line::Text(t)
        };
        // Several blank lines in a row count as one.
        if !(line == Line::Blank && matches!(out.last(), None | Some(Line::Blank))) {
            out.push(line);
        }
    }
    out
}

/// The versions the changelog describes, newest first.
#[cfg(test)]
pub fn versions(text: &str) -> Vec<&str> {
    parse(text).into_iter().filter_map(|l| if let Line::Version(v, _) = l { Some(v) } else { None }).collect()
}

/// `**bold**` and `` `code` `` inside a line.
fn inline(text: &str, size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let (mut bold, mut code) = (false, false);
    let mut rest = text;
    while !rest.is_empty() {
        let next = if code { rest.find('`') } else { [rest.find("**"), rest.find('`')].into_iter().flatten().min() };
        let (part, marker) = match next {
            Some(i) => (&rest[..i], Some(&rest[i..])),
            None => (rest, None),
        };
        if !part.is_empty() {
            let fmt = if code {
                TextFormat { font_id: FontId::monospace(size - 1.0), color: theme::TEXT(), background: theme::BG_INPUT(), ..Default::default() }
            } else {
                TextFormat { font_id: FontId::proportional(size), color: if bold { theme::rgb(255, 255, 255) } else { color }, ..Default::default() }
            };
            job.append(part, 0.0, fmt);
        }
        match marker {
            Some(m) if m.starts_with('`') => {
                code = !code;
                rest = &m[1..];
            }
            Some(m) => {
                bold = !bold;
                rest = &m[2..];
            }
            None => rest = "",
        }
    }
    job
}

pub fn body(ui: &mut Ui) {
    let current = env!("CARGO_PKG_VERSION");
    egui::ScrollArea::vertical().max_height(520.0 + crate::form::grow(ui)).auto_shrink([false, true]).show(ui, |ui| {
        // Keep the text clear of the scroll bar.
        ui.set_max_width(ui.available_width() - 14.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        let mut first = true;
        for line in parse(text()) {
            match line {
                Line::Version(v, date) => {
                    if !first {
                        ui.add_space(10.0);
                        theme::hline(ui, ui.cursor().top(), ui.max_rect().left(), ui.max_rect().right());
                        ui.add_space(10.0);
                    }
                    first = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} {v}", tr("Version"))).size(17.0).strong().color(theme::TEXT()));
                        if !date.is_empty() {
                            ui.label(RichText::new(date).color(theme::TEXT_DIM()));
                        }
                        if v == current {
                            ui.label(RichText::new(tr(" current ")).size(11.0).color(Color32::WHITE).background_color(theme::ACCENT()));
                        }
                    });
                }
                Line::Group(g) => {
                    ui.add_space(4.0);
                    ui.label(RichText::new(g).size(13.0).strong().color(theme::TEXT_MID()));
                }
                Line::Item(t) => {
                    ui.horizontal_top(|ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new("•").color(theme::TEXT_DIM()));
                        ui.add(egui::Label::new(inline(t, 13.0, theme::TEXT())).wrap());
                    });
                }
                Line::Text(t) => {
                    ui.add(egui::Label::new(inline(t, 13.0, theme::TEXT())).wrap());
                }
                Line::Blank => ui.add_space(2.0),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_described() {
        // make release-all writes this section; a build without it would show an outdated list.
        let v = versions(TEXT);
        assert_eq!(v.first().copied(), Some(env!("CARGO_PKG_VERSION")), "CHANGELOG.md has no section for the current version first");
        // The English one describes the same versions.
        assert_eq!(versions(TEXT_EN), v, "CHANGELOG.en.md must have the same version sections as CHANGELOG.md");
    }

    #[test]
    fn parses_the_subset() {
        let lines = parse("# Changelog\n\n<!-- last-commit: abc -->\n\n## 1.2.0 — 2026-01-02\n\n### Новое\n- one **two**\n\n\n- three\n");
        assert_eq!(
            lines,
            vec![Line::Version("1.2.0", "2026-01-02"), Line::Blank, Line::Group("Новое"), Line::Item("one **two**"), Line::Blank, Line::Item("three")]
        );
    }

    #[test]
    fn inline_markup() {
        let job = inline("a **b** `c` d", 13.0, theme::TEXT());
        let parts: Vec<&str> = job.sections.iter().map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0]).collect();
        assert_eq!(parts, vec!["a ", "b", " ", "c", " d"]);
    }
}
