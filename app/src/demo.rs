//! Builds a small sample repository so the app has something to show without a real project.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Builder {
    dir: PathBuf,
    t: i64,
}

impl Builder {
    fn git(&self, args: &[&str], author: (&str, &str)) -> bool {
        let date = format!("{} +0000", self.t);
        crate::git::no_console(&mut Command::new(crate::git::program()))
            .current_dir(&self.dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", author.0)
            .env("GIT_AUTHOR_EMAIL", author.1)
            .env("GIT_COMMITTER_NAME", author.0)
            .env("GIT_COMMITTER_EMAIL", author.1)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    fn write(&self, file: &str, text: &str) {
        let p = self.dir.join(file);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut old = std::fs::read_to_string(&p).unwrap_or_default();
        old.push_str(text);
        old.push('\n');
        let _ = std::fs::write(p, old);
    }
    fn commit(&mut self, file: &str, line: &str, msg: &str, who: (&str, &str)) {
        self.t += 7 * 60;
        self.write(file, line);
        self.git(&["add", "-A"], who);
        self.git(&["commit", "-q", "-m", msg], who);
    }
    fn merge(&mut self, into: &str, branch: &str, who: (&str, &str)) {
        self.t += 3 * 60;
        self.git(&["checkout", "-q", into], who);
        let msg = format!("Merge branch '{branch}' into '{into}'");
        self.git(&["merge", "-q", "--no-ff", "-m", &msg, branch], who);
    }
}

const ALEX: (&str, &str) = ("Alex Morgan", "alex@example.com");
const DANA: (&str, &str) = ("Dana Lee", "dana@example.com");
const SAM: (&str, &str) = ("Sam Patel", "sam@example.com");
const BOT: (&str, &str) = ("Review Bot", "bot@example.com");

pub fn demo_dir() -> PathBuf {
    std::env::temp_dir().join("refbraid-demo").join("sample-project")
}

/// Create (or reuse) the sample repository and return its folder.
pub fn ensure_demo() -> Option<PathBuf> {
    let dir = demo_dir();
    if dir.join(".git").exists() {
        return Some(dir);
    }
    let root = dir.parent()?.to_path_buf();
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&dir).ok()?;
    let origin = root.join("origin.git");
    let ok = crate::git::no_console(&mut Command::new(crate::git::program()))
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&origin)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let mut b = Builder {
        dir: dir.clone(),
        t: now - 2 * 86400 - 3600 * 5,
    };
    b.git(&["init", "-q", "-b", "main"], ALEX);
    b.git(&["config", "user.name", ALEX.0], ALEX);
    b.git(&["config", "user.email", ALEX.1], ALEX);
    b.git(&["remote", "add", "origin", origin.to_str()?], ALEX);

    b.commit("README.md", "# Sample project", "Initial project structure", ALEX);
    b.commit("src/app.py", "def main():\n    print('hello')", "[feat] Application entry point", DANA);
    b.commit("src/forms.py", "class Form: pass", "APP-1201 [feat] Base form component", SAM);
    b.git(&["branch", "stage"], ALEX);
    b.git(&["branch", "master"], ALEX);
    b.git(&["tag", "v1.0"], ALEX);
    b.t = now - 86400 - 3600 * 3;

    let fixes: [(&str, &str, &str, (&str, &str)); 6] = [
        ("fix/APP-1308-session-loader", "src/session.py", "APP-1308 [fix] Session loader handles empty tokens", ALEX),
        ("feature/APP-1314-print-form", "src/print.py", "APP-1314 [feat] Smaller font on the printable form", DANA),
        ("fix/donation-procedure", "src/i18n.py", "[fix] Correct wording on the donation screen", ALEX),
        ("hotfix/APP-1344", "src/forms.py", "APP-1344 [feat] Accreditation badge on report 098", SAM),
        ("fix/APP-1308-waybill", "src/waybill.py", "APP-1308 [fix] Waybill shows the expedition number", ALEX),
        ("fix/APP-1308-stale-data", "src/db.py", "APP-1308 [fix] StaleDataError after SET NOCOUNT", ALEX),
    ];
    for (i, (branch, file, msg, who)) in fixes.iter().enumerate() {
        if i == 3 {
            b.t = now - 3600 * 9;
        }
        b.git(&["checkout", "-q", "main"], *who);
        b.git(&["checkout", "-q", "-b", branch], *who);
        b.commit(file, &format!("# change {i}"), msg, *who);
        if i % 2 == 0 {
            b.commit(file, &format!("# follow-up {i}"), &format!("{msg} (review notes)"), *who);
        }
        b.merge("main", branch, BOT);
        b.merge("stage", branch, BOT);
        if i == 3 {
            b.merge("master", branch, BOT);
        }
    }
    b.git(&["checkout", "-q", "main"], ALEX);
    b.git(&["push", "-q", "origin", "--all"], ALEX);
    b.git(&["push", "-q", "origin", "--tags"], ALEX);
    // Remote-only work so the remote tree has its own branches.
    b.git(&["checkout", "-q", "-b", "bugfix/APP-1220-null-check", "main"], SAM);
    b.commit("src/forms.py", "# null check", "APP-1220 [fix] Null check in the paid form", SAM);
    b.git(&["push", "-q", "origin", "bugfix/APP-1220-null-check"], SAM);
    b.git(&["checkout", "-q", "main"], ALEX);
    b.git(&["branch", "-q", "-D", "bugfix/APP-1220-null-check"], ALEX);
    b.git(&["fetch", "-q", "origin"], ALEX);
    for br in ["main", "stage", "master"] {
        b.git(&["branch", "-q", "--set-upstream-to", &format!("origin/{br}"), br], ALEX);
    }
    // Local commit not pushed yet.
    b.commit("src/app.py", "# logging", "[chore] More detailed logging on start", ALEX);
    // Uncommitted work for the commit panel.
    b.write("src/app.py", "# TODO: handle config file");
    b.write("notes/todo.txt", "Check the release checklist");
    Some(dir)
}

/// A sample repository stopped in the middle of a merge, with conflicts of every kind: a real
/// one, simple ones (the two sides changed different lines), several in one file, very long
/// lines, a binary file and a file one side deleted. Each call makes a fresh copy in its own
/// folder, so the example can be tried again after resolving it.
pub fn conflict_example() -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let dir = std::env::temp_dir().join("refbraid-demo").join(format!("merge-conflicts-{stamp}"));
    std::fs::create_dir_all(&dir).ok()?;
    let mut b = Builder { dir: dir.clone(), t: stamp as i64 - 3 * 86400 };
    let put = |b: &Builder, file: &str, text: &str| {
        let p = b.dir.join(file);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, text);
    };
    let save = |b: &mut Builder, msg: &str, who: (&str, &str)| {
        b.t += 3 * 3600;
        b.git(&["add", "-A"], who);
        b.git(&["commit", "-q", "-m", msg], who);
    };
    if !b.git(&["init", "-q", "-b", "main"], ALEX) {
        return None;
    }
    b.git(&["config", "user.name", ALEX.0], ALEX);
    b.git(&["config", "user.email", ALEX.1], ALEX);

    // The common starting point.
    let po = |changed: Option<&str>| {
        let mut s = String::from("# Translations of the donor portal\nmsgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\n");
        for i in 1..=24 {
            s.push_str(&format!("msgid \"Report {i}: every donation made before the end of the day preceding the selected date (23:59:59) is included\"\n"));
            let ru = match (i, changed) {
                (12, Some(t)) => t.to_string(),
                _ => format!("Отчёт {i}: учитываются все донации, сделанные до конца дня, предшествующего выбранной дате (23:59:59)"),
            };
            s.push_str(&format!("msgstr \"{ru}\"\n\n"));
        }
        s
    };
    put(&b, "VERSION", "1.4.0\n");
    put(&b, "config.ini", "[server]\nport = 80\nhost = localhost\nworkers = 2\n\n[report]\nformat = pdf\n");
    put(&b, "src/report.py", "import datetime\n\n\ndef title(period):\n    return \"Report\"\n\n\ndef rows(items):\n    out = []\n    for item in items:\n        out.append(item)\n    return out\n\n\ndef total(items):\n    return sum(i.amount for i in items)\n\n\ndef footer():\n    return \"Generated automatically\"\n");
    put(&b, "src/export.py", "def export(rows):\n    return rows\n");
    put(&b, "translations/ru/messages.po", &po(None));
    put(&b, "docs/old_api.md", "# Old API\n\nDeprecated endpoints.\n");
    let _ = std::fs::write(dir.join("assets/logo.bin"), [0u8, 1, 2, 3, 0, 255, 10, 0]).or_else(|_| {
        let _ = std::fs::create_dir_all(dir.join("assets"));
        std::fs::write(dir.join("assets/logo.bin"), [0u8, 1, 2, 3, 0, 255, 10, 0])
    });
    put(&b, "README.md", "# Donor portal\n\nReports and exports.\n");
    save(&mut b, "Initial project structure", ALEX);

    // The incoming branch.
    b.git(&["checkout", "-q", "-b", "feature/APP-1402-report-export"], DANA);
    put(&b, "VERSION", "1.5.0-rc1\n");
    put(&b, "config.ini", "[server]\nport = 80\nhost = reports.example.com\nworkers = 2\n\n[report]\nformat = pdf\n");
    put(&b, "src/report.py", "import datetime\n\n\ndef title(period):\n    return f\"Report for {period.start:%d.%m.%Y} – {period.end:%d.%m.%Y}\"\n\n\ndef rows(items):\n    out = []\n    for item in items:\n        out.append(item)\n    return out\n\n\ndef total(items):\n    return round(sum(i.amount for i in items), 2)\n\n\ndef footer():\n    return \"Generated automatically\"\n");
    put(&b, "src/export.py", "import csv\n\n\ndef export(rows, path):\n    with open(path, \"w\", newline=\"\") as f:\n        csv.writer(f).writerows(rows)\n");
    put(&b, "translations/ru/messages.po", &po(Some("Отчёт 12: учитываются все донации, сделанные до конца дня, предшествующего выбранной дате (23:59:59), включая отложенные и возвращённые платежи")));
    let _ = std::fs::write(dir.join("assets/logo.bin"), [0u8, 9, 9, 9, 0, 255, 10, 0]);
    put(&b, "docs/old_api.md", "# Old API\n\nDeprecated endpoints. Removed in 2.0.\n");
    save(&mut b, "APP-1402 [feat] Export the report to CSV", DANA);

    // Your branch.
    b.git(&["checkout", "-q", "main"], ALEX);
    put(&b, "VERSION", "1.4.1\n");
    put(&b, "config.ini", "[server]\nport = 8080\nhost = localhost\nworkers = 2\n\n[report]\nformat = pdf\n");
    put(&b, "src/report.py", "import datetime\n\n\ndef title(period):\n    return \"Report for \" + period.name\n\n\ndef rows(items):\n    out = []\n    for item in items:\n        out.append(item)\n    return out\n\n\ndef total(items):\n    return sum(i.amount for i in items if not i.refunded)\n\n\ndef footer():\n    return \"Generated by the donor portal\"\n");
    put(&b, "translations/ru/messages.po", &po(Some("Отчёт 12: все донации до конца предыдущего дня")));
    let _ = std::fs::write(dir.join("assets/logo.bin"), [0u8, 7, 7, 7, 0, 255, 10, 0]);
    b.git(&["rm", "-q", "docs/old_api.md"], ALEX);
    put(&b, "README.md", "# Donor portal\n\nReports and exports.\n\nRun `make report` to build a report.\n");
    save(&mut b, "[fix] Refunds are left out of the report total", ALEX);

    // Merge: stops with conflicts.
    b.t += 3600;
    b.git(&["merge", "--no-ff", "-m", "Merge branch 'feature/APP-1402-report-export'", "feature/APP-1402-report-export"], ALEX);
    Some(dir)
}

/// A sample repository with submodules in every state, for looking at the Submodules
/// section: one whose folder has newer commits than the repository records, one with changes
/// of its own, and one that is not initialised. A fresh copy on each call.
pub fn submodule_example() -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let root = std::env::temp_dir().join("refbraid-demo").join(format!("submodules-{stamp}"));
    let _ = std::fs::remove_dir_all(&root);
    let make = |name: &str| -> Option<Builder> {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).ok()?;
        let b = Builder { dir, t: stamp as i64 - 5 * 86400 };
        if !b.git(&["init", "-q", "-b", "main"], ALEX) {
            return None;
        }
        b.git(&["config", "user.name", ALEX.0], ALEX);
        b.git(&["config", "user.email", ALEX.1], ALEX);
        Some(b)
    };
    let save = |b: &mut Builder, file: &str, text: &str, msg: &str, who: (&str, &str)| {
        b.t += 5 * 3600;
        let _ = std::fs::create_dir_all(b.dir.join(file).parent().unwrap_or(&b.dir));
        let _ = std::fs::write(b.dir.join(file), text);
        b.git(&["add", "-A"], who);
        b.git(&["commit", "-q", "-m", msg], who);
    };
    // The libraries the project takes in.
    let mut charts = make("charts-lib")?;
    save(&mut charts, "charts.py", "def bar(values):\n    return values\n", "Draw bar charts", DANA);
    save(&mut charts, "charts.py", "def bar(values):\n    return sorted(values)\n", "Sort the bars", DANA);
    let mut theme = make("theme-kit")?;
    save(&mut theme, "colors.css", ":root { --accent: #2f6fde; }\n", "Add the colours", SAM);
    let mut icons = make("icon-set")?;
    save(&mut icons, "README.md", "# Icons\n", "Start the icon set", SAM);

    let mut b = make("portal")?;
    save(&mut b, "app.py", "import charts\n\nprint(charts.bar([3, 1, 2]))\n", "Start the portal", ALEX);
    for (lib, path) in [("charts-lib", "libs/charts"), ("theme-kit", "libs/theme"), ("icon-set", "assets/icons")] {
        let from = root.join(lib).display().to_string();
        b.git(&["-c", "protocol.file.allow=always", "submodule", "add", "-q", &from, path], ALEX);
    }
    b.t += 5 * 3600;
    b.git(&["commit", "-q", "-m", "Take in the chart, theme and icon libraries"], ALEX);
    for path in ["libs/charts", "libs/theme"] {
        let sub = Builder { dir: b.dir.join(path), t: b.t };
        sub.git(&["config", "user.name", ALEX.0], ALEX);
        sub.git(&["config", "user.email", ALEX.1], ALEX);
    }
    // Newer commits of the chart library, fetched into its folder but not recorded yet.
    save(&mut charts, "charts.py", "def bar(values):\n    return sorted(values)\n\n\ndef pie(values):\n    return values\n", "Add pie charts", DANA);
    save(&mut charts, "charts.py", "def bar(values):\n    return sorted(values)\n\n\ndef pie(values):\n    total = sum(values)\n    return [v / total for v in values]\n", "Give pie charts shares, not amounts", DANA);
    save(&mut charts, "CHANGES.md", "# Changes\n\n- pie charts\n", "Write down what changed", BOT);
    let sub = Builder { dir: b.dir.join("libs/charts"), t: b.t };
    sub.git(&["-c", "protocol.file.allow=always", "pull", "-q", "origin", "main"], ALEX);
    // A change inside the theme library, not committed there.
    let _ = std::fs::write(b.dir.join("libs/theme/colors.css"), ":root { --accent: #d9482b; }\n");
    // The icons were never fetched on this computer.
    b.git(&["submodule", "deinit", "-q", "-f", "assets/icons"], ALEX);
    let _ = std::fs::write(b.dir.join("app.py"), "import charts\n\nprint(charts.pie([3, 1, 2]))\n");
    Some(b.dir)
}

#[allow(dead_code)]
pub fn is_demo(p: &Path) -> bool {
    p.starts_with(std::env::temp_dir().join("refbraid-demo"))
}
