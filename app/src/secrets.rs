//! Access tokens in the password store of the system — the Secret Service (GNOME Keyring,
//! KWallet) on Linux, the Credential Manager on Windows, the Keychain on macOS — in place of
//! the settings file. The calls may wait (the first one connects to the store, and a locked
//! store asks to be unlocked), so they are made on a thread, never while a frame is drawn.
//!
//! Nothing here runs in tests and screenshot runs: the store belongs to the user and is not
//! set apart by `XDG_CONFIG_HOME`.

use std::sync::atomic::{AtomicBool, Ordering};

/// What the entries are filed under. A development build keeps its own, so that running one
/// can't move or overwrite the tokens of the installed program.
const SERVICE: &str = if cfg!(debug_assertions) { "RefBraid (development)" } else { "RefBraid" };

static OFF: AtomicBool = AtomicBool::new(false);

/// Leave the store alone for the rest of this run (screenshots).
pub fn turn_off() {
    OFF.store(true, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    !cfg!(test) && !OFF.load(Ordering::Relaxed)
}

/// Where a token is kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kept {
    /// In the settings file, unprotected: as before the store was used, or where there is none.
    #[default]
    File,
    /// In the store of the system; the settings file holds only a mark.
    Store,
    /// Entered just now and on its way into the store: not written to the file meanwhile.
    Moving,
}

/// The name of the entry of a hosting's token: by what runs there and the host, so that a
/// token never follows a row to another host.
pub fn hosting_key(kind: crate::hosting::Kind, host: &str) -> String {
    format!("hosting:{}:{}", kind.key(), host.trim().to_lowercase())
}

pub fn jira_key(url: &str) -> String {
    format!("jira:{}", url.trim().trim_end_matches('/').to_lowercase())
}

/// What reading an entry gave.
#[derive(Clone, PartialEq, Debug)]
pub enum Read {
    Found(String),
    /// The store answered and has no such entry.
    Missing,
    /// The store could not be asked (none running, locked and not unlocked, …).
    Failed(String),
}

fn entry(key: &str) -> Result<keyring::Entry, String> {
    if !enabled() {
        return Err("the password store is not used in this run".into());
    }
    keyring::Entry::new(SERVICE, key).map_err(|e| e.to_string())
}

pub fn get(key: &str) -> Read {
    let e = match entry(key) {
        Ok(e) => e,
        Err(e) => return Read::Failed(e),
    };
    match e.get_password() {
        Ok(v) => Read::Found(v),
        // Only this answer means "no token": anything else may be a store that is locked.
        Err(keyring::Error::NoEntry) => Read::Missing,
        Err(e) => Read::Failed(e.to_string()),
    }
}

/// Write an entry and make sure it reads back the same.
pub fn set(key: &str, value: &str) -> Result<(), String> {
    let e = entry(key)?;
    e.set_password(value).map_err(|e| e.to_string())?;
    match e.get_password() {
        Ok(v) if v == value => Ok(()),
        Ok(_) => Err("the password store gave back something else than it was given".into()),
        Err(e) => Err(format!("written, but it can't be read back: {e}")),
    }
}

/// Remove an entry; one that is not there is not an error.
pub fn delete(key: &str) -> Result<(), String> {
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// What a thread did with the store, for the frame that takes it.
#[derive(Clone, PartialEq, Debug)]
pub enum Done {
    /// A token read at start.
    Read(String, Read),
    /// A token put into the store (from the file, or one just entered): how it went.
    Stored(String, Result<(), String>),
    /// An entry removed: only a failure is of interest.
    Deleted(String, Result<(), String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_entries_by_the_host_and_stays_off_in_tests() {
        assert_eq!(hosting_key(crate::hosting::Kind::GitLab, " Code.Example.com "), "hosting:gitlab:code.example.com");
        assert_eq!(jira_key("https://Jira.example.com/"), "jira:https://jira.example.com");
        assert!(!enabled());
        assert!(matches!(get("hosting:github:github.com"), Read::Failed(_)));
        assert!(set("x", "y").is_err());
        assert!(delete("x").is_err());
    }
}
