//! Interface language (Settings → Window → Language): English, the language the program is
//! written in, or Russian. `tr("English text")` gives the text in the chosen language; text
//! with no translation is shown in English.
//!
//! The translations are in `i18n_ru.rs`: pairs of the English text, exactly as written in the
//! code, and its Russian version.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ru,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }
    /// The language's own name, as it is shown in the language list.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
        }
    }
    pub fn from_code(code: &str) -> Lang {
        match code.trim() {
            "ru" => Lang::Ru,
            _ => Lang::En,
        }
    }
}

pub const ALL: [Lang; 2] = [Lang::En, Lang::Ru];

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::Ru,
        _ => Lang::En,
    }
}

fn ru() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| crate::i18n_ru::PAIRS.iter().copied().collect())
}

/// `text` in the interface language.
pub fn tr(text: &str) -> &str {
    let t = match lang() {
        Lang::En => text,
        Lang::Ru => ru().get(text).copied().unwrap_or(text),
    };
    if cfg!(target_os = "macos") { mac_keys(t) } else { t }
}

/// macOS: the shortcuts named in a text, written the way a Mac does (`keymap::mac_text`).
/// Each such text is converted once and kept: there are only a few dozen of them.
fn mac_keys(text: &str) -> &str {
    if !text.contains("Ctrl") && !text.contains("Alt") && !text.contains("Shift+") {
        return text;
    }
    static DONE: std::sync::Mutex<Option<HashMap<String, &'static str>>> = std::sync::Mutex::new(None);
    let mut done = DONE.lock().unwrap();
    let map = done.get_or_insert_with(HashMap::new);
    if let Some(t) = map.get(text) {
        return t;
    }
    let mac: &'static str = Box::leak(crate::keymap::mac_text(text).into_boxed_str());
    map.insert(text.to_string(), mac);
    mac
}

/// Like [`tr`] for text with `{}` places, filled in order with `args`.
pub fn trf(text: &str, args: &[&dyn std::fmt::Display]) -> String {
    let t = tr(text);
    let mut out = String::with_capacity(t.len() + 16);
    let mut it = args.iter();
    let mut rest = t;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        if let Some(a) = it.next() {
            out.push_str(&a.to_string());
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// English or Russian plural: `plural(n, "file", "files")` → "files"; in Russian the three
/// forms are looked up as "file|1", "file|2", "file|5" (1 файл, 2 файла, 5 файлов).
pub fn plural(n: usize, one: &'static str, many: &'static str) -> String {
    match lang() {
        Lang::En => (if n == 1 { one } else { many }).to_string(),
        Lang::Ru => {
            let form = if n % 10 == 1 && n % 100 != 11 {
                "|1"
            } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
                "|2"
            } else {
                "|5"
            };
            let key = format!("{one}{form}");
            ru().get(key.as_str()).map(|s| s.to_string()).unwrap_or_else(|| tr(if n == 1 { one } else { many }).to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_and_fills() {
        set(Lang::En);
        assert_eq!(tr("Commit"), "Commit");
        assert_eq!(trf("In {} branches", &[&3]), "In 3 branches");
        set(Lang::Ru);
        assert_eq!(tr("Commit"), "Коммит");
        assert_eq!(tr("No such text anywhere"), "No such text anywhere");
        assert_eq!(plural(1, "file", "files"), "файл");
        assert_eq!(plural(3, "file", "files"), "файла");
        assert_eq!(plural(11, "file", "files"), "файлов");
        assert_eq!(plural(21, "file", "files"), "файл");
        set(Lang::En);
    }

    #[test]
    fn every_key_is_unique() {
        let mut seen = std::collections::HashSet::new();
        for (en, _) in crate::i18n_ru::PAIRS {
            assert!(seen.insert(*en), "translated twice: {en}");
        }
    }
}
