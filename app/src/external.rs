//! External diff and merge tools (Beyond Compare, Meld, KDiff3, …): which one a file opens
//! with, the files they are given and starting them.
//!
//! Argument pattern: %1 left (before), %2 right (after), %3 base (common version, three-way),
//! %4 result (merge tools: where the merged file goes). A lone %3 is left out when there is
//! no base.

use crate::settings::{DEFAULT_ASSOC, ExtKind, ExtTool, Settings};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Simple file-name pattern: `*` matches any run of characters; "json" or ".json" mean "*.json".
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let p = pattern.trim();
    if p.is_empty() {
        return false;
    }
    if !p.contains('*') && !p.contains('.') {
        // A bare word: the whole name ("Makefile") or the extension ("json").
        let (p, n) = (p.to_lowercase(), name.to_lowercase());
        return n == p || n.ends_with(&format!(".{p}"));
    }
    let p = if !p.contains('*') && p.starts_with('.') {
        format!("*{p}")
    } else {
        p.to_string()
    };
    let p = p.to_lowercase();
    let n = name.to_lowercase();
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == n;
    }
    let mut rest = n.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// The external tool `path` opens with, by the file-type rules; None = the built-in viewer.
pub fn tool_for<'a>(s: &'a Settings, path: &str, kind: ExtKind) -> Option<&'a ExtTool> {
    if !s.ext_enabled {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let pick = |a: &crate::settings::ExtAssoc| if kind == ExtKind::Diff { a.diff.clone() } else { a.merge.clone() };
    let rule = s
        .ext_assoc
        .iter()
        .filter(|a| a.pattern != DEFAULT_ASSOC)
        .find(|a| glob_match(&a.pattern, name))
        .or_else(|| s.ext_assoc.iter().find(|a| a.pattern == DEFAULT_ASSOC))?;
    let tool = pick(rule);
    if tool.is_empty() {
        return None;
    }
    s.ext_tools.iter().find(|t| t.kind == kind && t.name == tool)
}

/// Split the pattern like a shell would (quotes keep spaces) and fill in the files.
pub fn arguments(pattern: &str, files: [Option<&Path>; 4]) -> Vec<String> {
    let words = split_words(pattern);
    let mut out = vec![];
    for w in words {
        // A placeholder on its own for a file that isn't there (no base) is left out.
        let alone = (1..=4).find(|k| w == format!("%{k}"));
        if let Some(k) = alone {
            if files[k - 1].is_none() {
                continue;
            }
        }
        let mut v = w.clone();
        for k in 1..=4 {
            let f = files[k - 1].map(|p| p.display().to_string()).unwrap_or_default();
            v = v.replace(&format!("%{k}"), &f);
        }
        out.push(v);
    }
    out
}

/// Split a command line into words like a shell would: quotes keep spaces.
pub fn split_words(pattern: &str) -> Vec<String> {
    let mut words: Vec<String> = vec![];
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in pattern.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Arguments that open `file` at `line` in an editor we know by its program name (VS Code,
/// JetBrains IDEs, Sublime Text, gedit, Kate…); None for other programs.
pub fn editor_line_args(program: &str, file: &str, line: usize) -> Option<Vec<String>> {
    let base = program.rsplit(['/', '\\']).next()?.to_lowercase();
    let base = base.trim_end_matches(".exe").trim_end_matches(".sh").trim_end_matches(".cmd").trim_end_matches(".bat").trim_end_matches("64");
    let l = line.to_string();
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect::<Vec<String>>());
    let jetbrains = ["pycharm", "idea", "webstorm", "clion", "goland", "phpstorm", "rider", "rubymine", "datagrip", "rustrover", "dataspell", "aqua", "writerside"];
    if ["code", "code-insiders", "codium", "code-oss", "vscodium", "cursor", "windsurf"].contains(&base) {
        v(&["-g", &format!("{file}:{l}")])
    } else if jetbrains.iter().any(|j| base == *j || base.starts_with(&format!("{j}-"))) {
        v(&["--line", &l, file])
    } else if ["subl", "sublime_text", "zed", "zeditor"].contains(&base) {
        v(&[&format!("{file}:{l}")])
    } else if ["gedit", "gnome-text-editor", "xed", "pluma", "gvim", "emacs", "emacsclient", "micro"].contains(&base) {
        v(&[&format!("+{l}"), file])
    } else if ["kate", "kwrite"].contains(&base) {
        v(&["-l", &l, file])
    } else if base == "geany" || base == "mousepad" {
        v(&["--line", &l, file])
    } else if base == "notepad++" {
        v(&[&format!("-n{l}"), file])
    } else {
        None
    }
}

/// Linux: the program the desktop opens this file with (from its .desktop entry), with the
/// arguments written before the file. None when unknown or run through flatpak.
#[cfg(all(unix, not(target_os = "macos")))]
fn default_app(file: &Path) -> Option<(String, Vec<String>)> {
    let query = |args: &[&str]| -> Option<String> {
        let o = std::process::Command::new("xdg-mime").args(args).stderr(std::process::Stdio::null()).output().ok()?;
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        (o.status.success() && !s.is_empty()).then_some(s)
    };
    let mime = query(&["query", "filetype", &file.display().to_string()])?;
    let desktop = query(&["query", "default", &mime]).or_else(|| if mime.starts_with("text/") || mime.contains("script") || mime.contains("xml") || mime.contains("json") { query(&["query", "default", "text/plain"]) } else { None })?;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let mut dirs = vec![std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| home.join(".local/share"))];
    let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').filter(|d| !d.is_empty()).map(std::path::PathBuf::from));
    let desktop = desktop.split(';').next()?.trim().to_string();
    let text = dirs.iter().find_map(|d| std::fs::read_to_string(d.join("applications").join(&desktop)).ok())?;
    let exec = text.lines().find_map(|l| l.strip_prefix("Exec="))?;
    if exec.contains("flatpak") {
        return None;
    }
    let words: Vec<String> = split_words(exec).into_iter().filter(|w| !w.starts_with('%')).collect();
    let (prog, rest) = words.split_first()?;
    Some((prog.clone(), rest.to_vec()))
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn default_app(_file: &Path) -> Option<(String, Vec<String>)> {
    None
}

/// Open a file for editing: with the person's command (`%f` file, `%l` line), or with the system's
/// default application, at `line` when that program is an editor we know. Returns what was run.
pub fn open_file(file: &Path, line: Option<usize>, command: &str) -> Result<String, String> {
    let file = &crate::chrome::native_path(file);
    let f = file.display().to_string();
    let words: Vec<String> = if !command.trim().is_empty() {
        let l = line.unwrap_or(1).to_string();
        let mut w: Vec<String> = split_words(command).into_iter().map(|x| x.replace("%f", &f).replace("%l", &l)).collect();
        if !command.contains("%f") {
            w.push(f.clone());
        }
        w
    } else {
        let known = line.and_then(|l| default_app(file).and_then(|(prog, mut rest)| editor_line_args(&prog, &f, l).map(|a| {
            rest.extend(a);
            (prog, rest)
        })));
        match known {
            Some((prog, rest)) => std::iter::once(prog).chain(rest).collect(),
            None => {
                crate::chrome::open_in_file_manager(file);
                return Ok(f);
            }
        }
    };
    let (prog, args) = words.split_first().ok_or("The command is empty.")?;
    let mut c = std::process::Command::new(prog);
    crate::git::no_console(&mut c);
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut child = c.spawn().map_err(|e| format!("Could not start {prog}: {e}"))?;
    std::thread::spawn(move || child.wait());
    Ok(words.join(" "))
}

/// A fresh folder for the copies a tool is given (old versions keep their file names, so the
/// tool can colour the syntax).
pub fn scratch_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    std::env::temp_dir().join("refbraid-compare").join(format!("{}-{t}-{n}", std::process::id()))
}

/// Write `data` as `<dir>/<label>/<file name of path>`.
pub fn write_copy(dir: &Path, label: &str, path: &str, data: &[u8]) -> Result<PathBuf, String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let d = dir.join(label.replace(['/', '\\', ':'], "_"));
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let f = d.join(name);
    std::fs::write(&f, data).map_err(|e| e.to_string())?;
    Ok(f)
}

/// Start the tool; it keeps running on its own. `log`, when given, gets what it printed and
/// how it ended (for "Test Diff" in Settings).
pub fn launch(tool: &ExtTool, files: [Option<&Path>; 4], log: Option<Arc<Mutex<Vec<String>>>>) -> Result<String, String> {
    let program = tool.path.trim();
    if program.is_empty() {
        return Err("The tool has no program path.".into());
    }
    // The repository's files reach the tool with this system's separators.
    let native = files.map(|f| f.map(crate::chrome::native_path));
    let files = [native[0].as_deref(), native[1].as_deref(), native[2].as_deref(), native[3].as_deref()];
    let args = arguments(&tool.args, files);
    let mut c = std::process::Command::new(program);
    crate::git::no_console(&mut c);
    c.args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let shown = format!("{program} {}", args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" "));
    let child = c.spawn().map_err(|e| format!("Could not start {program}: {e}"))?;
    std::thread::spawn(move || {
        let out = child.wait_with_output();
        if let Some(log) = log {
            let mut l = log.lock().unwrap_or_else(|e| e.into_inner());
            match out {
                Ok(o) => {
                    for line in String::from_utf8_lossy(&o.stdout).lines().chain(String::from_utf8_lossy(&o.stderr).lines()).take(40) {
                        l.push(line.to_string());
                    }
                    l.push(match o.status.code() {
                        Some(c) => format!("The tool closed (exit code {c})."),
                        None => "The tool closed.".to_string(),
                    });
                }
                Err(e) => l.push(e.to_string()),
            }
        }
    });
    Ok(shown)
}

/// Sample files for "Test Diff" / "Test Three-Side Diff".
pub fn sample_files(three: bool) -> Result<[Option<PathBuf>; 4], String> {
    let dir = scratch_dir();
    let base = "fn greet() {\n    println!(\"Hello\");\n}\n\nfn main() {\n    greet();\n}\n";
    let left = "fn greet() {\n    println!(\"Hello, world\");\n}\n\nfn main() {\n    greet();\n}\n";
    let right = "fn greet(name: &str) {\n    println!(\"Hello\");\n}\n\nfn main() {\n    greet(\"you\");\n}\n";
    let l = write_copy(&dir, "left", "sample.rs", left.as_bytes())?;
    let r = write_copy(&dir, "right", "sample.rs", right.as_bytes())?;
    if !three {
        return Ok([Some(l), Some(r), None, None]);
    }
    let b = write_copy(&dir, "base", "sample.rs", base.as_bytes())?;
    let o = write_copy(&dir, "result", "sample.rs", base.as_bytes())?;
    Ok([Some(l), Some(r), Some(b), Some(o)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ExtAssoc;

    #[test]
    fn patterns() {
        assert!(glob_match("*.json", "request.JSON"));
        assert!(glob_match("json", "a.json"));
        assert!(glob_match(".json", "a.json"));
        assert!(glob_match("Makefile", "makefile"));
        assert!(glob_match("*.tpl.*", "content.tpl.html"));
        assert!(!glob_match("*.json", "a.jsonl"));
        assert!(!glob_match("", "a"));
    }

    #[test]
    fn fills_arguments() {
        let (a, b) = (Path::new("/t/left a.py"), Path::new("/t/right.py"));
        assert_eq!(arguments("%1 %2 %3", [Some(a), Some(b), None, None]), vec!["/t/left a.py", "/t/right.py"]);
        assert_eq!(arguments("-title='x y' --left=%1 \"%2\"", [Some(a), Some(b), None, None]), vec!["-title=x y", "--left=/t/left a.py", "/t/right.py"]);
    }

    #[test]
    fn opens_known_editors_at_a_line() {
        let s = |v: Option<Vec<String>>| v.map(|x| x.join(" "));
        assert_eq!(s(editor_line_args("/usr/share/code/code", "/r/a.py", 12)), Some("-g /r/a.py:12".into()));
        assert_eq!(s(editor_line_args("/opt/pycharm/bin/pycharm.sh", "/r/a.py", 3)), Some("--line 3 /r/a.py".into()));
        assert_eq!(s(editor_line_args("C:\\JetBrains\\bin\\pycharm64.exe", "a.py", 3)), Some("--line 3 a.py".into()));
        assert_eq!(s(editor_line_args("gnome-text-editor", "a.txt", 7)), Some("+7 a.txt".into()));
        assert_eq!(s(editor_line_args("kate", "a.txt", 7)), Some("-l 7 a.txt".into()));
        assert!(editor_line_args("libreoffice", "a.txt", 7).is_none());
        assert_eq!(split_words("code -g \"%f:%l\""), vec!["code", "-g", "%f:%l"]);
    }

    #[test]
    fn picks_tool_by_file_type() {
        let mut s = Settings { ext_enabled: true, ..Settings::default() };
        s.ext_tools = vec![
            ExtTool { kind: ExtKind::Diff, name: "BC".into(), path: "/bin/bc".into(), args: "%1 %2".into() },
            ExtTool { kind: ExtKind::Merge, name: "KD".into(), path: "/bin/kd".into(), args: "%1 %2 %3 %4".into() },
        ];
        s.ext_assoc = vec![
            ExtAssoc { pattern: DEFAULT_ASSOC.into(), diff: String::new(), merge: "KD".into() },
            ExtAssoc { pattern: "*.json".into(), diff: "BC".into(), merge: String::new() },
        ];
        assert_eq!(tool_for(&s, "api/a.json", ExtKind::Diff).map(|t| t.name.as_str()), Some("BC"));
        assert!(tool_for(&s, "src/a.rs", ExtKind::Diff).is_none());
        assert_eq!(tool_for(&s, "src/a.rs", ExtKind::Merge).map(|t| t.name.as_str()), Some("KD"));
        assert!(tool_for(&s, "api/a.json", ExtKind::Merge).is_none());
        s.ext_enabled = false;
        assert!(tool_for(&s, "api/a.json", ExtKind::Diff).is_none());
    }
}
