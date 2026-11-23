//! AI agents (coding-assistant command line tools) that can write commit messages.

use crate::git::{self, StatusEntry};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// The prompt is written to the program's standard input.
    Stdin,
    /// The prompt is passed as the last argument.
    Arg,
}

pub struct Harness {
    pub id: &'static str,
    pub name: &'static str,
    pub bin: &'static str,
    pub args: &'static [&'static str],
    pub input: Input,
}

pub const HARNESSES: &[Harness] = &[
    Harness { id: "claude", name: "Claude Code", bin: "claude", args: &["-p", "--output-format", "text"], input: Input::Stdin },
    Harness { id: "codex", name: "Codex CLI", bin: "codex", args: &["exec", "-s", "read-only", "-"], input: Input::Stdin },
    Harness { id: "gemini", name: "Gemini CLI", bin: "gemini", args: &["-p"], input: Input::Arg },
    Harness { id: "opencode", name: "OpenCode", bin: "opencode", args: &["run"], input: Input::Arg },
    Harness { id: "cursor", name: "Cursor Agent", bin: "cursor-agent", args: &["-p", "--output-format", "text"], input: Input::Arg },
    Harness { id: "qwen", name: "Qwen Code", bin: "qwen", args: &["-p"], input: Input::Arg },
    Harness { id: "grok", name: "Grok CLI", bin: "grok", args: &["-p"], input: Input::Arg },
    Harness { id: "crush", name: "Crush", bin: "crush", args: &["run"], input: Input::Arg },
];

pub const DEFAULT_PROMPT: &str = "Avoid overly detailed descriptions and unnecessary details; write in the same language as other commits.
Format: [<prefix>] <Description of the completed work>.
Do not add a colon or dash after the prefix
Use of prefixes:
[feat]: New functionality added.
[fix]: Bug fixed.
[docs]: Documentation updated.
[style]: Changes related to code formatting (e.g., correcting whitespace, indentation, etc.) that do not affect the program logic.
[refactor]: Changes to the code that do not affect the program's behavior but improve its structure (e.g., code optimization).
[test]: Changes related to tests (e.g., adding new tests or updating existing ones).
[chore]: Changes that concern routine tasks, such as updating dependencies, configuration files, Dockerfiles, and other things that are not directly related to the source code.
If the branch name contains a JIRA key, add that key to the very beginning of the message.
Don't add yourself to the comment as a co-author.
Avoid overly verbose descriptions or unnecessary details.";

/// Defaults of earlier versions; a saved copy of one is replaced by the new default.
pub const OLD_DEFAULT_PROMPTS: &[&str] = &[
    "Avoid overly detailed descriptions and unnecessary details; write in Russian.\nFormat: [<prefix>] <Description of the completed work>.\nDo not add a colon or dash after the prefix\nUse of prefixes:\n[feat]: New functionality added.\n[fix]: Bug fixed.\n[docs]: Documentation updated.\n[style]: Changes related to code formatting (e.g., correcting whitespace, indentation, etc.) that do not affect the program logic.\n[refactor]: Changes to the code that do not affect the program's behavior but improve its structure (e.g., code optimization).\n[test]: Changes related to tests (e.g., adding new tests or updating existing ones).\n[chore]: Changes that concern routine tasks, such as updating dependencies, configuration files, Dockerfiles, and other things that are not directly related to the source code.\nIf the branch name contains a JIRA key, add that key to the very beginning of the message.\nDon't add yourself to the comment as a co-author.\nAvoid overly verbose descriptions or unnecessary details.",
    "Write a commit message in the style of this repository: a short subject line (under 72 characters), then an optional short body. Avoid overly verbose descriptions or unnecessary details.",
];

pub fn harness(id: &str) -> Option<&'static Harness> {
    HARNESSES.iter().find(|h| h.id == id)
}

fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = git::home_dir() {
        for d in [
            ".local/bin",
            ".claude/local",
            ".npm-global/bin",
            ".bun/bin",
            ".volta/bin",
            ".cargo/bin",
            ".opencode/bin",
            ".grok/bin",
            ".deno/bin",
            "bin",
        ] {
            dirs.push(home.join(d));
        }
    }
    #[cfg(not(windows))]
    for d in ["/usr/local/bin", "/usr/bin", "/opt/homebrew/bin", "/snap/bin"] {
        dirs.push(PathBuf::from(d));
    }
    #[cfg(windows)]
    {
        if let Some(a) = std::env::var_os("APPDATA").map(PathBuf::from) {
            dirs.push(a.join("npm"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            dirs.push(l.join("Programs").join("claude"));
            dirs.push(l.join("Microsoft").join("WinGet").join("Links"));
            dirs.push(l.join("Volta").join("bin"));
        }
        if let Some(p) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
            dirs.push(p.join("nodejs"));
        }
    }
    dirs
}

/// File names a program may have: plain on Linux/macOS, with .exe/.cmd on Windows.
fn program_names(bin: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{bin}.exe"), format!("{bin}.cmd"), format!("{bin}.bat")]
    } else {
        vec![bin.to_string()]
    }
}

/// Which agents are installed, with the program's location.
pub fn detect() -> Vec<(&'static str, PathBuf)> {
    let dirs = search_dirs();
    HARNESSES
        .iter()
        .filter_map(|h| {
            dirs.iter()
                .flat_map(|d| program_names(h.bin).into_iter().map(move |n| d.join(n)))
                .find(|p| p.is_file())
                .map(|p| (h.id, p))
        })
        .collect()
}

/// The full request sent to the agent: instructions, branch, style examples and the diff.
pub fn build_prompt(dir: &Path, branch: &str, files: &[StatusEntry], user_prompt: &str, amend: bool) -> String {
    let user_prompt = if user_prompt.trim().is_empty() { DEFAULT_PROMPT } else { user_prompt };
    let user_prompt = user_prompt.replace("$GIT_BRANCH_NAME", branch);
    let recent = git::run(dir, &["log", "-12", "--format=%s"]).stdout;
    let mut diff = String::new();
    let tracked: Vec<String> = files
        .iter()
        .filter(|e| !e.is_untracked())
        .flat_map(|e| std::iter::once(e.path.clone()).chain(e.old_path.clone()))
        .collect();
    let has_head = git::run(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok;
    let base = if amend { "HEAD~1" } else { "HEAD" };
    if !tracked.is_empty() && has_head {
        let mut args = vec!["diff", "--no-ext-diff", "-M", base, "--"];
        args.extend(tracked.iter().map(|s| s.as_str()));
        diff.push_str(&git::run(dir, &args).stdout);
    } else if amend && files.is_empty() && has_head {
        diff.push_str(&git::run(dir, &["show", "--format=", "HEAD"]).stdout);
    }
    for e in files.iter().filter(|e| e.is_untracked()) {
        let out = git::run(dir, &["diff", "--no-index", "--", "/dev/null", &e.path]).stdout;
        diff.push_str(&out);
    }
    const LIMIT: usize = 60_000;
    if diff.len() > LIMIT {
        let mut cut = LIMIT;
        while !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
        diff.push_str("\n… (diff truncated)\n");
    }
    format!(
        "You write git commit messages.\n\n{user_prompt}\n\nReply with ONLY the commit message text: no explanations, no quotes, no code fences. Do not run tools or change any files.\n\nCurrent branch: {branch}\n\nRecent commit subjects in this repository (for style):\n{recent}\nChanges to describe:\n{diff}"
    )
}

pub struct Running {
    pub rx: Receiver<Result<String, String>>,
    pub pid: Option<u32>,
    pub agent: String,
}

pub fn start(h: &Harness, program: &Path, dir: &Path, prompt: String) -> Running {
    start_with(h, program, dir, prompt, clean)
}

/// Run the agent; `tidy` turns its answer into the text the app uses.
pub fn start_with(h: &Harness, program: &Path, dir: &Path, prompt: String, tidy: fn(&str) -> String) -> Running {
    let (tx, rx) = channel();
    let mut cmd = Command::new(program);
    git::no_console(&mut cmd);
    cmd.args(h.args).current_dir(dir);
    if h.input == Input::Arg {
        cmd.arg(&prompt);
    }
    // Make sure helpers the agent needs (node, etc.) are found even when started from the desktop.
    let path = std::env::join_paths(
        program.parent().map(|p| p.to_path_buf()).into_iter().chain(search_dirs()),
    )
    .ok();
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    cmd.env_remove("REFBRAID_ASKPASS")
        .stdin(if h.input == Input::Stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn();
    let mut pid = None;
    match child {
        Err(e) => {
            let _ = tx.send(Err(format!("Could not start {}: {e}", h.name)));
        }
        Ok(mut child) => {
            pid = Some(child.id());
            let stdin_text = if h.input == Input::Stdin { Some(prompt) } else { None };
            std::thread::spawn(move || {
                if let (Some(text), Some(mut sin)) = (stdin_text, child.stdin.take()) {
                    let _ = sin.write_all(text.as_bytes());
                }
                let res = match child.wait_with_output() {
                    Ok(o) if o.status.success() => {
                        let msg = tidy(&String::from_utf8_lossy(&o.stdout));
                        if msg.is_empty() {
                            Err("The agent returned an empty answer.".into())
                        } else {
                            Ok(msg)
                        }
                    }
                    Ok(o) => {
                        let err = String::from_utf8_lossy(&o.stderr);
                        let out = String::from_utf8_lossy(&o.stdout);
                        let text = if err.trim().is_empty() { out } else { err };
                        Err(text.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.send(res);
            });
        }
    }
    Running { rx, pid, agent: h.name.to_string() }
}

pub fn cancel(r: &Running) {
    if let Some(pid) = r.pid {
        #[cfg(windows)]
        let _ = git::no_console(&mut Command::new("taskkill"))
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        #[cfg(not(windows))]
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

/// Strip code fences, quotes and "Commit message:" prefixes the agent may add.
fn clean(s: &str) -> String {
    let mut lines: Vec<&str> = s.lines().collect();
    while lines.first().map(|l| l.trim().is_empty() || l.trim_start().starts_with("```")).unwrap_or(false) {
        lines.remove(0);
    }
    while lines.last().map(|l| l.trim().is_empty() || l.trim_start().starts_with("```")).unwrap_or(false) {
        lines.pop();
    }
    let mut text = lines.join("\n");
    for prefix in ["Commit message:", "commit message:", "Commit Message:"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim_start().to_string();
        }
    }
    let t = text.trim();
    let t = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
    t.to_string()
}
