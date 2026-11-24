//! The tokens of the settings and the password store of the system: reading them at start,
//! moving there the ones an older version left in the settings file, and what applying the
//! settings window means for them. The store is talked to on threads (`secrets.rs`).

use crate::app::App;
use crate::i18n::trf;
use crate::secrets::{Done, Kept, Read};
use crate::settings::Settings;

/// A thing to do with the password store of the system.
#[derive(Clone, PartialEq, Debug)]
pub enum Todo {
    Read(String),
    Store(String, String),
    Delete(String),
}

/// What applying the settings window means for the tokens: the ones typed in are taken (and go
/// to the store when `store` says it is used), and the entries nothing refers to any more are
/// removed. Nothing else touches a saved token — one not read from the store yet stays as it is.
pub fn token_changes(old: &Settings, new: &mut Settings, jira_entered: &str, store: bool) -> Vec<Todo> {
    let mut todo = vec![];
    for a in new.hosting.iter_mut() {
        let typed = std::mem::take(&mut a.entered).trim().to_string();
        if !typed.is_empty() && !a.host.trim().is_empty() {
            a.kept = if store { Kept::Moving } else { Kept::File };
            if store {
                todo.push(Todo::Store(a.key(), typed.clone()));
            }
            a.token = typed;
        }
    }
    if !jira_entered.is_empty() && !new.jira_url.trim().is_empty() {
        new.jira_kept = if store { Kept::Moving } else { Kept::File };
        if store {
            todo.push(Todo::Store(crate::secrets::jira_key(&new.jira_url), jira_entered.to_string()));
        }
        new.jira_token = jira_entered.to_string();
    }
    // An entry of the store whose host is gone, renamed or cleared.
    let kept = |s: &Settings| {
        let mut keys: Vec<String> = s.hosting.iter().filter(|a| a.kept != Kept::File).map(|a| a.key()).collect();
        if s.jira_kept != Kept::File {
            keys.push(crate::secrets::jira_key(&s.jira_url));
        }
        keys
    };
    let now = kept(new);
    for key in kept(old) {
        if !now.contains(&key) && !todo.contains(&Todo::Delete(key.clone())) {
            todo.push(Todo::Delete(key));
        }
    }
    todo
}

/// What the store's answers mean for a copy of the settings. Returns whether a token arrived.
fn take(s: &mut Settings, done: &Done) -> bool {
    match done {
        Done::Read(key, Read::Found(token)) => {
            for a in s.hosting.iter_mut().filter(|a| a.kept == Kept::Store && a.key() == *key) {
                a.token = token.clone();
            }
            if s.jira_kept == Kept::Store && crate::secrets::jira_key(&s.jira_url) == *key {
                s.jira_token = token.clone();
            }
            true
        }
        Done::Stored(key, result) => {
            // In the store for sure: the file may let go of it. Not there: it stays in (or
            // goes to) the file, as it was before the store was used.
            let kept = if result.is_ok() { Kept::Store } else { Kept::File };
            for a in s.hosting.iter_mut().filter(|a| a.key() == *key && !a.token.is_empty() && a.kept != Kept::Store) {
                a.kept = kept;
            }
            if crate::secrets::jira_key(&s.jira_url) == *key && !s.jira_token.is_empty() && s.jira_kept != Kept::Store {
                s.jira_kept = kept;
            }
            false
        }
        _ => false,
    }
}

impl App {
    /// Do these things with the password store on a thread; `poll_secrets` takes the outcome.
    pub fn run_secrets(&mut self, todo: Vec<Todo>) {
        if todo.is_empty() || !crate::secrets::enabled() {
            return;
        }
        let tx = self.secrets_tx.clone();
        std::thread::spawn(move || {
            for t in todo {
                let done = match t {
                    Todo::Read(key) => {
                        let got = crate::secrets::get(&key);
                        Done::Read(key, got)
                    }
                    Todo::Store(key, value) => {
                        let r = crate::secrets::set(&key, &value);
                        Done::Stored(key, r)
                    }
                    Todo::Delete(key) => {
                        let r = crate::secrets::delete(&key);
                        Done::Deleted(key, r)
                    }
                };
                if tx.send(done).is_err() {
                    break;
                }
                crate::app::repaint();
            }
        });
    }

    /// At start: read the tokens kept in the store, and move there the ones an older version
    /// left in the settings file (they stay in the file until the store has them for sure).
    pub fn start_secrets(&mut self) {
        let s = &self.settings;
        let mut todo = vec![];
        for a in &s.hosting {
            match a.kept {
                Kept::File if !a.token.trim().is_empty() => todo.push(Todo::Store(a.key(), a.token.trim().to_string())),
                Kept::Store => todo.push(Todo::Read(a.key())),
                _ => {}
            }
        }
        let jira = crate::secrets::jira_key(&s.jira_url);
        match s.jira_kept {
            Kept::File if !s.jira_token.trim().is_empty() && !s.jira_url.trim().is_empty() => todo.push(Todo::Store(jira, s.jira_token.trim().to_string())),
            Kept::Store => todo.push(Todo::Read(jira)),
            _ => {}
        }
        self.run_secrets(todo);
    }

    /// Every frame: take what the threads did with the password store.
    pub fn poll_secrets(&mut self) {
        let mut save = false;
        let mut fresh = false;
        while let Ok(done) = self.secrets_rx.try_recv() {
            // The settings in use, and the copy the open settings window edits.
            fresh |= take(&mut self.settings, &done);
            if let Some(d) = self.settings_draft.as_mut() {
                take(&mut d.s, &done);
            }
            match done {
                Done::Read(key, Read::Missing) => {
                    let what = key.split_once(':').map_or(key.as_str(), |x| x.1).to_string();
                    self.secrets_note = Some(trf("The password store of the system has no token for {}: enter it again.", &[&what]));
                }
                Done::Read(_, Read::Failed(e)) => {
                    self.secrets_note = Some(trf("The password store of the system can't be read, so the saved tokens are not used: {}", &[&e]));
                }
                Done::Stored(_, result) => {
                    if let Err(e) = result {
                        self.secrets_note = Some(trf("The password store of the system can't be used, so the token is kept unprotected in the settings file: {}", &[&e]));
                    }
                    save = true;
                }
                Done::Deleted(_, Err(e)) => {
                    self.secrets_note = Some(trf("A token could not be removed from the password store of the system: {}", &[&e]));
                }
                _ => {}
            }
        }
        if save {
            self.settings.save();
        }
        if fresh {
            self.jira.forget();
            self.hosting.forget();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::{Account, Kind};

    fn host(host: &str, token: &str, kept: Kept) -> Account {
        Account { kind: Kind::GitLab, host: host.into(), token: token.into(), kept, ..Default::default() }
    }

    #[test]
    fn applying_the_settings_touches_only_the_tokens_that_were_changed() {
        let old = Settings {
            hosting: vec![host("a.example.com", "", Kept::Store), host("b.example.com", "tb", Kept::Store), host("c.example.com", "tc", Kept::File), host("d.example.com", "td", Kept::Store)],
            jira_url: "https://jira.example.com".into(),
            jira_kept: Kept::Store,
            ..Default::default()
        };
        // a: not read from the store yet and left alone; b: a new token typed; c: left in the
        // file; d: its row removed; e: a new host with a token. JIRA untouched.
        let mut new = old.clone();
        new.hosting[1].entered = " new-b ".into();
        new.hosting.remove(3);
        new.hosting.push(Account { entered: "te".into(), ..host("e.example.com", "", Kept::File) });
        let todo = token_changes(&old, &mut new, "", true);
        assert_eq!(todo, vec![Todo::Store("hosting:gitlab:b.example.com".into(), "new-b".into()), Todo::Store("hosting:gitlab:e.example.com".into(), "te".into()), Todo::Delete("hosting:gitlab:d.example.com".into())]);
        let state: Vec<(&str, &str, Kept)> = new.hosting.iter().map(|a| (a.host.as_str(), a.token.as_str(), a.kept)).collect();
        assert_eq!(state, vec![("a.example.com", "", Kept::Store), ("b.example.com", "new-b", Kept::Moving), ("c.example.com", "tc", Kept::File), ("e.example.com", "te", Kept::Moving)]);
        assert!(new.hosting.iter().all(|a| a.entered.is_empty()));
        assert_eq!(new.jira_kept, Kept::Store);
        // A token on its way to the store is not written to the file.
        assert_eq!(new.hosting[1].to_line(), "gitlab\tb.example.com\t\t\tstore");
    }

    #[test]
    fn a_renamed_host_and_a_new_jira_address_let_go_of_their_entries() {
        let old = Settings { hosting: vec![host("a.example.com", "ta", Kept::Store)], jira_url: "https://jira.example.com".into(), jira_token: "tj".into(), jira_kept: Kept::Store, ..Default::default() };
        // The settings page drops the saved token when the host is edited (see the pages).
        let mut new = old.clone();
        new.hosting[0] = host("evil.example.org", "", Kept::File);
        new.jira_url = "https://other.example.com".into();
        new.jira_token.clear();
        new.jira_kept = Kept::File;
        let todo = token_changes(&old, &mut new, "new-jira", true);
        assert_eq!(todo, vec![Todo::Store("jira:https://other.example.com".into(), "new-jira".into()), Todo::Delete("hosting:gitlab:a.example.com".into()), Todo::Delete("jira:https://jira.example.com".into())]);
        assert_eq!((new.jira_token.as_str(), new.jira_kept), ("new-jira", Kept::Moving));
    }

    #[test]
    fn without_a_store_a_typed_token_goes_to_the_file() {
        let old = Settings::default();
        let mut new = Settings { hosting: vec![Account { entered: "t".into(), ..host("a.example.com", "", Kept::File) }], ..Default::default() };
        assert_eq!(token_changes(&old, &mut new, "", false), vec![]);
        assert_eq!((new.hosting[0].token.as_str(), new.hosting[0].kept), ("t", Kept::File));
        assert_eq!(new.hosting[0].to_line(), "gitlab\ta.example.com\t\tt");
    }

    #[test]
    fn the_answers_of_the_store_reach_the_settings() {
        let mut s = Settings { hosting: vec![host("a.example.com", "", Kept::Store), host("b.example.com", "tb", Kept::File), host("c.example.com", "tc", Kept::Moving)], jira_url: "https://jira.example.com".into(), jira_kept: Kept::Store, ..Default::default() };
        assert!(take(&mut s, &Done::Read("hosting:gitlab:a.example.com".into(), Read::Found("ta".into()))));
        assert!(take(&mut s, &Done::Read("jira:https://jira.example.com".into(), Read::Found("tj".into()))));
        assert_eq!((s.hosting[0].token.as_str(), s.jira_token.as_str()), ("ta", "tj"));
        // A token moved from the file leaves it only once the store has it; a failure keeps
        // it in the file, and one just typed falls back to the file too.
        take(&mut s, &Done::Stored("hosting:gitlab:b.example.com".into(), Ok(())));
        take(&mut s, &Done::Stored("hosting:gitlab:c.example.com".into(), Err("no store".into())));
        assert_eq!((s.hosting[1].kept, s.hosting[2].kept), (Kept::Store, Kept::File));
        assert_eq!(s.hosting[1].to_line(), "gitlab\tb.example.com\t\t\tstore");
        assert_eq!(s.hosting[2].to_line(), "gitlab\tc.example.com\t\ttc");
        // A missing entry or a store that can't be asked changes nothing: the mark stays.
        let before = s.clone();
        assert!(!take(&mut s, &Done::Read("hosting:gitlab:a.example.com".into(), Read::Missing)));
        assert!(!take(&mut s, &Done::Read("hosting:gitlab:a.example.com".into(), Read::Failed("locked".into()))));
        assert!(s == before);
    }
}
