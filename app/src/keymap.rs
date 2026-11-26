//! The keyboard: every action that has (or can be given) a shortcut, in one table. The key
//! handling, the Keymap page of the settings, the command palette and the Keyboard Shortcuts
//! window are all built from it.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

/// Something the program does on a key press or from the command palette.
pub struct Action {
    /// The name in the settings file and in `App::run_action`.
    pub id: &'static str,
    /// The name shown to the person (English; translated when shown).
    pub name: &'static str,
    pub group: &'static str,
    /// The shortcut out of the box ("" = none).
    pub default: &'static str,
    /// Works only with a repository open.
    pub repo: bool,
    /// Not while the cursor is in a text field (the key means something there).
    pub not_typing: bool,
}

const fn a(id: &'static str, name: &'static str, group: &'static str, default: &'static str) -> Action {
    Action { id, name, group, default, repo: true, not_typing: false }
}

pub const ACTIONS: &[Action] = &[
    Action { repo: false, ..a("palette", "Find Action…", "Window", "Ctrl+Shift+A") },
    Action { repo: false, ..a("open", "Open Repository…", "Window", "Ctrl+O") },
    Action { repo: false, ..a("clone", "Clone…", "Window", "") },
    Action { repo: false, ..a("settings", "Settings…", "Window", "Ctrl+Alt+S") },
    Action { repo: false, ..a("shortcuts", "Keyboard Shortcuts", "Window", "") },
    a("next_repo", "Next Repository Tab", "Window", "Ctrl+Tab"),
    a("prev_repo", "Previous Repository Tab", "Window", "Ctrl+Shift+Tab"),
    a("close_repo", "Close Repository", "Window", ""),
    a("close_tab", "Close the Diff or Annotate Tab", "Window", "Ctrl+W"),
    a("commit", "Commit…", "Git", "Ctrl+K"),
    a("push", "Push…", "Git", "Ctrl+Shift+K"),
    a("pull", "Pull…", "Git", "Ctrl+T"),
    a("fetch", "Fetch", "Git", ""),
    a("checkout", "Checkout Branch…", "Git", "Ctrl+Shift+B"),
    a("new_branch", "New Branch…", "Git", ""),
    a("stash", "Stash Changes…", "Git", ""),
    Action { not_typing: true, ..a("undo", "Undo the Last Action", "Git", "Ctrl+Z") },
    Action { not_typing: true, ..a("add_to_git", "Add the Selected New Files to Git", "Git", "Ctrl+Alt+A") },
    Action { not_typing: true, ..a("rename_branch", "Rename the Selected Branch", "Git", "F2") },
    a("reflog", "Reflog", "Git", ""),
    a("bisect", "Bisect…", "Git", ""),
    a("refresh", "Refresh", "View", "F5"),
    a("find", "Find", "View", "Ctrl+F"),
    a("focus_log", "Go to the Log", "View", "Alt+1"),
    a("focus_branches", "Go to the Branches", "View", "Alt+2"),
    a("focus_changes", "Go to the Changed Files", "View", "Alt+3"),
    a("console", "Go to the Console", "View", ""),
];

pub fn action(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// Keys that work in their place and are not in the table (shown in Keyboard Shortcuts).
pub const FIXED: &[(&str, &str)] = &[
    ("Ctrl+Enter", "Commit (in the message box), Push (in the Push window)"),
    ("↑ ↓ Home End", "Move through the list in front: the Log, the branches, the changed files"),
    ("Enter", "Open what is selected: the diff of a file, the commits of a branch"),
    ("Space", "Include a changed file in the commit or leave it out"),
    ("← →", "Fold and unfold a group of branches"),
    ("← → ↑ ↓ Home End PgUp PgDn", "In the text of a diff (click it first): move the text cursor"),
    ("Shift + those keys", "Select text in a diff from the cursor; Ctrl+C copies it"),
    ("Ctrl+← Ctrl+→ Ctrl+Home Ctrl+End", "In a diff: by words, and to the start or the end of the text"),
    ("F3 / Shift+F3", "Next and previous match of a search in a diff"),
    ("Ctrl+S", "Save a file edited in its tab"),
    ("Esc", "Close a window or a search line"),
];

/// A shortcut written as text: "Ctrl+Shift+K", "F5", "Alt+1". None when it is not one.
pub fn parse(text: &str) -> Option<KeyboardShortcut> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "command" => mods = mods | Modifiers::COMMAND,
            "shift" => mods = mods | Modifiers::SHIFT,
            "alt" => mods = mods | Modifiers::ALT,
            _ => {
                // Key names start with a capital ("Enter", "K"), however they were typed.
                let mut c = part.chars();
                let cap: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
                key = Some(Key::from_name(part).or_else(|| Key::from_name(&cap))?);
            }
        }
    }
    let key = key?;
    // macOS keeps Command+Tab for switching applications: there "Ctrl+Tab" is the Control key.
    if cfg!(target_os = "macos") && key == Key::Tab && mods.command {
        mods = Modifiers { command: false, mac_cmd: false, ctrl: true, ..mods };
    }
    Some(KeyboardShortcut::new(mods, key))
}

/// A key press as shortcut text, modifiers in the order Ctrl, Alt, Shift.
pub fn format(mods: Modifiers, key: Key) -> String {
    let mut s = String::new();
    // On a Mac the shortcuts are made with Command; its Control key is not one of ours.
    // (Except with Tab, see `parse`.)
    let main = if cfg!(target_os = "macos") { mods.command || mods.mac_cmd || (mods.ctrl && key == Key::Tab) } else { mods.command || mods.ctrl || mods.mac_cmd };
    if main {
        s.push_str("Ctrl+");
    }
    if mods.alt {
        s.push_str("Alt+");
    }
    if mods.shift {
        s.push_str("Shift+");
    }
    s.push_str(key.name());
    s
}

/// Text with shortcuts in it the way a Mac writes them: "Ctrl+Alt+A" becomes "⌥⌘A" (the
/// signs of Option, Shift and Command, in the order of macOS menus, with no plus), and the
/// keys named on their own become "Cmd" and "Option". With Tab it is the Control key, "⌃Tab":
/// Command+Tab belongs to the system. The rest of the text is left alone.
pub fn mac_text(text: &str) -> String {
    const MODS: [&str; 4] = ["Ctrl+", "Alt+", "Shift+", "Cmd+"];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut in_word = false;
    while let Some(c) = rest.chars().next() {
        if !in_word {
            // A run of modifier keys, then the key itself: a capital, a digit or a sign.
            let (mut cmd, mut alt, mut shift, mut after) = (false, false, false, rest);
            while let Some(m) = MODS.iter().find(|m| after.starts_with(**m)) {
                match *m {
                    "Alt+" => alt = true,
                    "Shift+" => shift = true,
                    _ => cmd = true,
                }
                after = &after[m.len()..];
            }
            let key = after.chars().next().is_some_and(|k| !k.is_lowercase() && !k.is_whitespace());
            if after.len() < rest.len() && key {
                let tab = after.starts_with("Tab");
                for (on, sign) in [(cmd && tab, '⌃'), (alt, '⌥'), (shift, '⇧'), (cmd && !tab, '⌘')] {
                    if on {
                        out.push(sign);
                    }
                }
                // A plus that is the key itself ("Ctrl++") is not a word.
                rest = after;
                in_word = false;
                if let Some(k) = rest.chars().next() {
                    out.push(k);
                    rest = &rest[k.len_utf8()..];
                    in_word = k.is_alphanumeric();
                }
                continue;
            }
            // The key named on its own: "with Ctrl", "Ctrl/Shift+click".
            let alone = [("Ctrl", "Cmd"), ("Alt", "Option")].into_iter().find(|(w, _)| rest.starts_with(w) && !rest[w.len()..].chars().next().is_some_and(char::is_alphanumeric));
            if let Some((word, mac)) = alone {
                out.push_str(mac);
                rest = &rest[word.len()..];
                in_word = true;
                continue;
            }
        }
        out.push(c);
        in_word = c.is_alphanumeric();
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Shortcut text as this system shows it: [`mac_text`] on macOS, unchanged elsewhere. What is
/// kept in the settings and in the table of actions is always the "Ctrl+Alt+A" form.
pub fn shown(text: &str) -> std::borrow::Cow<'_, str> {
    if cfg!(target_os = "macos") { std::borrow::Cow::Owned(mac_text(text)) } else { std::borrow::Cow::Borrowed(text) }
}

/// The same shortcut, however it was typed ("ctrl + k" and "Ctrl+K").
pub fn normal(text: &str) -> String {
    parse(text).map(|s| format(s.modifiers, s.logical_key)).unwrap_or_default()
}

/// The shortcut of an action: the person's own (`overrides`: id → shortcut, "" = none) or
/// the default one.
pub fn binding(overrides: &[(String, String)], id: &str) -> String {
    match overrides.iter().find(|(k, _)| k == id) {
        Some((_, v)) => normal(v),
        None => action(id).map(|a| a.default.to_string()).unwrap_or_default(),
    }
}

/// Give an action a shortcut ("" = none); the default one is not written down.
pub fn set(overrides: &mut Vec<(String, String)>, id: &str, shortcut: &str) {
    overrides.retain(|(k, _)| k != id);
    let shortcut = normal(shortcut);
    if action(id).is_some_and(|a| a.default != shortcut) {
        overrides.push((id.to_string(), shortcut));
    }
}

/// The other action that already has this shortcut.
pub fn taken_by(overrides: &[(String, String)], id: &str, shortcut: &str) -> Option<&'static Action> {
    let shortcut = normal(shortcut);
    if shortcut.is_empty() {
        return None;
    }
    ACTIONS.iter().find(|a| a.id != id && binding(overrides, a.id) == shortcut)
}

/// The actions with their shortcuts, those with more modifier keys first: Ctrl+Shift+K has to
/// be tried before Ctrl+K, which a press of it also matches.
pub fn bound(overrides: &[(String, String)]) -> Vec<(&'static Action, KeyboardShortcut)> {
    let mut v: Vec<(&'static Action, KeyboardShortcut)> = ACTIONS.iter().filter_map(|a| parse(&binding(overrides, a.id)).map(|s| (a, s))).collect();
    let weight = |s: &KeyboardShortcut| (s.modifiers.command || s.modifiers.ctrl) as u8 + s.modifiers.shift as u8 + s.modifiers.alt as u8;
    v.sort_by_key(|(_, s)| std::cmp::Reverse(weight(s)));
    v
}

/// The actions whose name has every word of `query`, in table order.
pub fn search<'a>(query: &str, names: impl Iterator<Item = (usize, &'a str)>) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    names.filter(|(_, n)| {
        let n = n.to_lowercase();
        words.iter().all(|w| n.contains(w.as_str()))
    })
    .map(|(i, _)| i)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_shortcuts() {
        let s = parse("Ctrl+Shift+K").unwrap();
        assert!(s.modifiers.command && s.modifiers.shift && !s.modifiers.alt && s.logical_key == Key::K);
        assert_eq!(format(s.modifiers, s.logical_key), "Ctrl+Shift+K");
        assert_eq!(normal(" ctrl + alt+s "), "Ctrl+Alt+S");
        assert_eq!(normal("F5"), "F5");
        assert_eq!(normal("Alt+1"), "Alt+1");
        assert!(parse("Ctrl+").is_none() && parse("Ctrl+Nonsense").is_none() && parse("").is_none());
    }

    #[test]
    fn a_mac_shows_its_own_keys() {
        assert_eq!(mac_text("Ctrl+K"), "⌘K");
        assert_eq!(mac_text("Ctrl+Alt+A"), "⌥⌘A");
        assert_eq!(mac_text("Ctrl+Shift+K"), "⇧⌘K");
        assert_eq!(mac_text("Alt+1"), "⌥1");
        assert_eq!(mac_text("F5"), "F5");
        assert_eq!((mac_text("Ctrl+Tab"), mac_text("Ctrl+Shift+Tab")), ("⌃Tab".to_string(), "⌃⇧Tab".to_string()));
        assert_eq!(mac_text("Commit…    Ctrl+Enter"), "Commit…    ⌘Enter");
        assert_eq!(mac_text("Zoom In  Ctrl++"), "Zoom In  ⌘+");
        assert_eq!(mac_text("Ctrl+← Ctrl+→ Ctrl+Home"), "⌘← ⌘→ ⌘Home");
        assert_eq!(mac_text("F3 / Shift+F3"), "F3 / ⇧F3");
        assert_eq!(mac_text("Apply this commit earlier (Alt+Up)"), "Apply this commit earlier (⌥Up)");
        // A key named on its own, and a click with a key held down.
        assert_eq!(mac_text("Ctrl/Shift+click; Ctrl+C copies it"), "Cmd/Shift+click; ⌘C copies it");
        assert_eq!(mac_text("Shift + those keys"), "Shift + those keys");
        assert_eq!(mac_text("Выделение: Ctrl+A, с Alt — иначе"), "Выделение: ⌘A, с Option — иначе");
        // Words that only contain the names are not keys.
        assert_eq!(mac_text("Alternative, Salt+Pepper, Ctrlx"), "Alternative, Salt+Pepper, Ctrlx");
        // Every default shortcut still reads as one.
        for a in ACTIONS {
            assert!(!mac_text(a.default).contains("Ctrl") && !mac_text(a.default).contains("Alt"), "{}", a.id);
        }
    }

    #[test]
    fn every_default_is_a_shortcut_and_none_is_used_twice() {
        let mut seen = std::collections::HashSet::new();
        for a in ACTIONS {
            assert!(seen.insert(a.id), "{} is there twice", a.id);
            if !a.default.is_empty() {
                assert_eq!(normal(a.default), a.default, "{}", a.id);
                assert!(taken_by(&[], a.id, a.default).is_none(), "{} shares {}", a.id, a.default);
            }
        }
    }

    #[test]
    fn a_shortcut_of_ones_own_replaces_the_default() {
        let mut o = vec![];
        assert_eq!(binding(&o, "commit"), "Ctrl+K");
        set(&mut o, "commit", "ctrl+enter");
        set(&mut o, "fetch", "Ctrl+Shift+F");
        set(&mut o, "pull", "");
        assert_eq!((binding(&o, "commit"), binding(&o, "fetch"), binding(&o, "pull")), ("Ctrl+Enter".to_string(), "Ctrl+Shift+F".to_string(), String::new()));
        // Back to the default: nothing is kept for it.
        set(&mut o, "commit", "Ctrl+K");
        assert!(!o.iter().any(|(k, _)| k == "commit"));
        assert_eq!(taken_by(&o, "fetch", "Ctrl+Shift+K").map(|a| a.id), Some("push"));
        assert!(taken_by(&o, "fetch", "Ctrl+T").is_none());
        // More modifier keys are tried first.
        let order: Vec<&str> = bound(&o).iter().map(|(a, _)| a.id).collect();
        let at = |id: &str| order.iter().position(|x| *x == id).unwrap();
        assert!(at("push") < at("commit") && at("prev_repo") < at("next_repo") && !order.contains(&"pull"));
        assert_eq!(search("check bran", ACTIONS.iter().map(|a| a.name).enumerate()).iter().map(|&i| ACTIONS[i].id).collect::<Vec<_>>(), ["checkout"]);
    }
}
