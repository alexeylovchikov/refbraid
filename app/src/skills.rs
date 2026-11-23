//! Agent skills: folders with a SKILL.md that tell an AI agent how to do a job
//! (for example `.claude/skills/conflict_resolver/SKILL.md`).
//!
//! Where they are looked for:
//! - the repository: `.claude/skills`, `.agents/skills`, `.codex/skills`, `.github/skills`,
//!   `.cursor/skills`, `.gemini/skills`, `.qwen/skills`, `.opencode/skills`;
//! - the person's own folders (can be turned off): the same names under the home folder,
//!   plus skills of installed Claude Code plugins (`~/.claude/plugins/cache/**/skills`);
//! - folders (or single SKILL.md files) added in Settings → Merge Conflicts.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// The SKILL.md file.
    pub file: PathBuf,
    /// Where it was found, shown next to the name.
    pub source: String,
    /// Found inside the repository.
    pub project: bool,
    /// Its name or description talks about conflicts or merging.
    pub about_conflicts: bool,
}

pub const PROJECT_DIRS: &[&str] = &[
    ".claude/skills",
    ".agents/skills",
    ".codex/skills",
    ".github/skills",
    ".cursor/skills",
    ".gemini/skills",
    ".qwen/skills",
    ".opencode/skills",
    ".opencode/skill",
];

pub const PERSONAL_DIRS: &[&str] = &[
    ".claude/skills",
    ".agents/skills",
    ".codex/skills",
    ".gemini/skills",
    ".cursor/skills",
    ".config/opencode/skills",
    ".config/opencode/skill",
];

/// All skills for this repository, the ones about conflicts first.
pub fn find(repo: Option<&Path>, personal: bool, extra: &[String]) -> Vec<Skill> {
    let mut out = vec![];
    if let Some(r) = repo {
        for d in PROJECT_DIRS {
            scan(&r.join(d), &format!("project · {d}"), true, &mut out);
        }
    }
    if personal {
        if let Some(home) = crate::git::home_dir() {
            for d in PERSONAL_DIRS {
                scan(&home.join(d), &format!("personal · ~/{d}"), false, &mut out);
            }
            // Installed plugins only (the marketplaces folder is a catalogue of what could be installed).
            plugins(&home.join(".claude").join("plugins").join("cache"), 0, &mut out);
        }
    }
    for e in extra {
        let p = expand(e);
        if p.is_file() {
            if let Some(s) = read(&p, "added", false) {
                out.push(s);
            }
        } else if p.join("SKILL.md").is_file() {
            if let Some(s) = read(&p.join("SKILL.md"), "added", false) {
                out.push(s);
            }
        } else {
            scan(&p, &format!("added · {e}"), false, &mut out);
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(std::fs::canonicalize(&s.file).unwrap_or_else(|_| s.file.clone())));
    out.sort_by(|a, b| {
        b.about_conflicts
            .cmp(&a.about_conflicts)
            .then(b.project.cmp(&a.project))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// The instructions of a skill, without the header with its name and description.
pub fn body(file: &Path) -> String {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let text = text.trim_start_matches('\u{feff}');
    if let Some(rest) = text.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.trim_start_matches(|c| c == '-' || c == '\r' || c == '\n').trim().to_string();
        }
    }
    text.trim().to_string()
}

fn expand(p: &str) -> PathBuf {
    let p = p.trim();
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = crate::git::home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(p)
}

fn skill_file(dir: &Path) -> Option<PathBuf> {
    ["SKILL.md", "skill.md", "Skill.md"].iter().map(|n| dir.join(n)).find(|f| f.is_file())
}

fn scan(dir: &Path, source: &str, project: bool, out: &mut Vec<Skill>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = skill_file(&p) {
                if let Some(s) = read(&f, source, project) {
                    out.push(s);
                }
            }
        }
    }
}

/// Claude Code plugins keep their skills in `<plugin>/skills/<name>/SKILL.md`, a few levels deep.
fn plugins(dir: &Path, depth: usize, out: &mut Vec<Skill>) {
    if depth > 5 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if !p.is_dir() || name == "node_modules" || name == ".git" {
            continue;
        }
        if name == "skills" {
            scan(&p, "plugin", false, out);
        } else {
            plugins(&p, depth + 1, out);
        }
    }
}

fn read(file: &Path, source: &str, project: bool) -> Option<Skill> {
    let text = std::fs::read_to_string(file).ok()?;
    let folder = file
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let (mut name, mut description) = (String::new(), String::new());
    let text = text.trim_start_matches('\u{feff}');
    if let Some(rest) = text.strip_prefix("---") {
        let header = rest.split("\n---").next().unwrap_or("");
        let mut lines = header.lines().peekable();
        while let Some(l) = lines.next() {
            let Some((k, v)) = l.split_once(':') else { continue };
            let v = v.trim();
            let mut value = v.trim_matches(|c| c == '"' || c == '\'').to_string();
            // Folded / literal blocks: "description: >" followed by indented lines.
            if v == ">" || v == "|" || v == ">-" || v == "|-" {
                let mut parts = vec![];
                while let Some(n) = lines.peek() {
                    if n.starts_with(' ') || n.starts_with('\t') || n.trim().is_empty() {
                        parts.push(n.trim().to_string());
                        lines.next();
                    } else {
                        break;
                    }
                }
                value = parts.join(" ").trim().to_string();
            }
            match k.trim() {
                "name" => name = value,
                "description" => description = value,
                _ => {}
            }
        }
    }
    if name.is_empty() {
        name = folder;
    }
    if description.is_empty() {
        description = body(file).lines().find(|l| !l.trim().is_empty() && !l.starts_with('#')).unwrap_or("").trim().to_string();
    }
    let hay = format!("{name} {description}").to_lowercase();
    let about_conflicts = ["conflict", "merge", "конфликт", "слиян"].iter().any(|w| hay.contains(w));
    Some(Skill { name, description, file: file.to_path_buf(), source: source.to_string(), project, about_conflicts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_project_skill() {
        let dir = std::env::temp_dir().join(format!("ge-skills-{}", std::process::id()));
        let s = dir.join(".claude/skills/conflict_resolver");
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("SKILL.md"), "---\nname: conflict-resolver\ndescription: >\n  Resolves git merge\n  conflicts.\n---\n# Rules\nKeep both.\n").unwrap();
        let found = find(Some(&dir), false, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "conflict-resolver");
        assert_eq!(found[0].description, "Resolves git merge conflicts.");
        assert!(found[0].about_conflicts && found[0].project);
        assert_eq!(body(&found[0].file), "# Rules\nKeep both.");
    }
}
