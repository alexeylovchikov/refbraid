//! JIRA: the status of the issues a commit mentions (in its message or in its branch names),
//! read over JIRA's REST API with a token. Requests go through the `curl` program, the token is
//! handed to it on stdin so it doesn't show up in the process list.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// Where JIRA is and how to sign in.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Config {
    pub url: String,
    pub token: String,
    /// Jira Cloud: the account e-mail (the token is then an API token, sent with Basic auth).
    pub email: String,
}

impl Config {
    pub fn from(s: &crate::settings::Settings) -> Option<Config> {
        let c = Config { url: s.jira_url.trim().trim_end_matches('/').to_string(), token: s.jira_token.trim().to_string(), email: s.jira_email.trim().to_string() };
        (s.jira_on && !c.url.is_empty() && !c.token.is_empty()).then_some(c)
    }
    pub fn browse(&self, key: &str) -> String {
        format!("{}/browse/{key}", self.url)
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Issue {
    pub key: String,
    pub summary: String,
    pub status: String,
    /// JIRA's status category: "new" (to do), "indeterminate" (in progress) or "done".
    pub category: String,
    pub kind: String,
    pub assignee: String,
}

#[derive(Clone, PartialEq, Debug)]
pub enum State {
    Loading,
    Found(Issue),
    /// JIRA doesn't know this key (or the token may not see it).
    Missing,
    Failed(String),
}

/// Issue keys (ABC-123) in a text, in order, without repeats.
pub fn keys_in(text: &str, out: &mut Vec<String>) {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"(?:^|[^A-Za-z0-9])([A-Z][A-Z0-9_]{0,9}-[1-9][0-9]{0,6})(?:$|[^A-Za-z0-9])").unwrap());
    // Things that look like keys but are names of standards.
    const NOT_KEYS: [&str; 10] = ["UTF", "ISO", "SHA", "RFC", "CP", "WIN", "MD", "TLS", "SSL", "CVE"];
    let mut at = 0;
    while let Some(c) = re.captures_at(text, at) {
        let m = c.get(1).unwrap();
        // Continue right after the key so that "A-1 B-2" finds both.
        at = m.end();
        let key = m.as_str();
        let project = key.split('-').next().unwrap_or("");
        if !NOT_KEYS.contains(&project) && !out.iter().any(|k| k == key) {
            out.push(key.to_string());
        }
    }
}

/// Issue keys of a commit: the ones in its message; when it names none, the ones in the names of
/// its branches — unless they name many (an old commit is in every branch made after it, and
/// those issues are not its own).
pub fn commit_keys(message: &str, branches: &[String]) -> Vec<String> {
    let mut out = vec![];
    keys_in(message, &mut out);
    if out.is_empty() {
        for b in branches {
            keys_in(b, &mut out);
        }
        if out.len() > 3 {
            out.clear();
        }
    }
    out
}

fn quote(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

/// What the server answered.
struct Reply {
    code: u16,
    body: String,
    content_type: String,
}

/// Marks the line curl adds after the body with the status and the content type.
const TRAILER: &str = "\n@@curl@@";

/// GET a path of the REST API.
fn get(cfg: &Config, path: &str) -> Result<Reply, String> {
    let mut c = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    crate::git::no_console(&mut c);
    // --compressed: some proxies in front of JIRA send gzip whether asked or not.
    c.args(["-sS", "--compressed", "--max-time", "20", "-K", "-", "-w"]);
    c.arg(format!("{TRAILER}%{{http_code}} %{{content_type}}"));
    c.arg(format!("{}{path}", cfg.url));
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("could not run curl: {e}"))?;
    let mut conf = String::from("header = \"Accept: application/json\"\n");
    if cfg.email.is_empty() {
        conf.push_str(&format!("header = {}\n", quote(&format!("Authorization: Bearer {}", cfg.token))));
    } else {
        conf.push_str(&format!("user = {}\n", quote(&format!("{}:{}", cfg.email, cfg.token))));
    }
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(conf.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, tail) = text.rsplit_once(TRAILER).unwrap_or(("", text.as_str()));
    let (code, content_type) = tail.trim().split_once(' ').unwrap_or((tail.trim(), ""));
    let code: u16 = code.parse().unwrap_or(0);
    if code == 0 {
        let err = String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("curl: ").to_string();
        // "(6) Could not resolve host: …" → "Could not resolve host: …"
        let err = match err.strip_prefix('(').and_then(|r| r.split_once(") ")) {
            Some((n, rest)) if n.chars().all(|c| c.is_ascii_digit()) => rest.to_string(),
            _ => err,
        };
        return Err(if err.is_empty() { "no answer from the server".into() } else { err });
    }
    Ok(Reply { code, body: body.to_string(), content_type: content_type.trim().to_string() })
}

/// The JSON in an answer; when there is none, what came instead.
fn json_of(r: &Reply) -> Result<Json, String> {
    if let Some(j) = parse_lenient(&r.body) {
        return Ok(j);
    }
    let body = r.body.trim();
    let what = if body.is_empty() {
        "an empty answer".to_string()
    } else if let Some(t) = html_title(body) {
        format!("a web page \"{t}\" instead of data")
    } else {
        let start: String = body.chars().take(60).map(|c| if c.is_control() { ' ' } else { c }).collect();
        format!("\"{start}…\"")
    };
    let ct = if r.content_type.is_empty() { String::new() } else { format!(", {}", r.content_type) };
    Err(format!("JIRA answered with {what} (HTTP {}{ct}) — check the server URL", r.code))
}

fn html_title(body: &str) -> Option<String> {
    let low = body.get(..body.len().min(400)).unwrap_or(body).to_lowercase();
    if !low.contains("<html") && !low.contains("<!doctype") {
        return None;
    }
    let re = regex::Regex::new(r"(?is)<title[^>]*>(.*?)</title").ok()?;
    let t = re.captures(body).map(|c| c[1].split_whitespace().collect::<Vec<_>>().join(" ")).filter(|t| !t.is_empty());
    Some(t.unwrap_or_else(|| "(HTML)".into()))
}

fn http_error(code: u16, body: &str) -> String {
    let what = match code {
        401 => "the token was not accepted (401)".to_string(),
        403 => "access denied (403)".to_string(),
        _ => format!("the server answered {code}"),
    };
    // JIRA explains itself in "errorMessages".
    let why = parse_lenient(body)
        .and_then(|j| j.get("errorMessages").and_then(|m| m.items().first().map(|s| s.str().to_string())))
        .filter(|s| !s.is_empty());
    match why {
        Some(w) => format!("{what}: {w}"),
        None => what,
    }
}

/// Read one issue.
pub fn fetch(cfg: &Config, key: &str) -> State {
    match get(cfg, &format!("/rest/api/2/issue/{key}?fields=summary,status,issuetype,assignee")) {
        Ok(r) if r.code == 200 => match json_of(&r) {
            Ok(j) => State::Found(issue_from(&j, key)),
            Err(e) => State::Failed(e),
        },
        Ok(r) if r.code == 404 => State::Missing,
        Ok(r) => State::Failed(http_error(r.code, &r.body)),
        Err(e) => State::Failed(e),
    }
}

fn issue_from(j: &Json, key: &str) -> Issue {
    let f = j.get("fields");
    let at = |path: &[&str]| {
        let mut v = f;
        for p in path {
            v = v.and_then(|x| x.get(p));
        }
        v.map(|x| x.str().to_string()).unwrap_or_default()
    };
    Issue {
        key: j.get("key").map(|k| k.str().to_string()).filter(|k| !k.is_empty()).unwrap_or_else(|| key.to_string()),
        summary: at(&["summary"]),
        status: at(&["status", "name"]),
        category: at(&["status", "statusCategory", "key"]),
        kind: at(&["issuetype", "name"]),
        assignee: at(&["assignee", "displayName"]),
    }
}

/// Check the address and the token: the name of the account it signs in as.
pub fn test(cfg: &Config) -> Result<String, String> {
    if cfg.url.is_empty() || cfg.token.is_empty() {
        return Err("enter the server address and the token".into());
    }
    let r = get(cfg, "/rest/api/2/myself")?;
    if r.code != 200 {
        return Err(http_error(r.code, &r.body));
    }
    let j = json_of(&r)?;
    let name = j.get("displayName").map(|n| n.str().to_string()).unwrap_or_default();
    let who = if name.is_empty() { "Connected".to_string() } else { format!("Connected as {name}") };
    // Read one issue the way the commit details do, so a passing test means statuses will show.
    let r = get(cfg, "/rest/api/2/search?maxResults=1&fields=summary,status")?;
    if r.code != 200 {
        return Err(format!("{who}, but issues can't be read: {}", http_error(r.code, &r.body)));
    }
    let j = json_of(&r).map_err(|e| format!("{who}, but issues can't be read: {e}"))?;
    match j.get("issues").and_then(|i| i.items().first()) {
        Some(i) => {
            let i = issue_from(i, "");
            Ok(format!("{who}; issues can be read ({} — {})", i.key, if i.status.is_empty() { "no status" } else { &i.status }))
        }
        None => Ok(format!("{who}; this account sees no issues")),
    }
}

/// Issues already read, kept for a few minutes; the rest are read on a background thread.
pub struct Cache {
    map: HashMap<String, (Instant, State)>,
    cfg: Option<Config>,
    tx: Sender<(String, State)>,
    rx: Receiver<(String, State)>,
}

impl Default for Cache {
    fn default() -> Self {
        let (tx, rx) = channel();
        Cache { map: HashMap::new(), cfg: None, tx, rx }
    }
}

const FRESH: Duration = Duration::from_secs(180);

impl Cache {
    /// The state of these issues; starts reading the ones not known yet or read a while ago.
    pub fn states(&mut self, cfg: &Config, keys: &[String], ctx: &eframe::egui::Context) -> Vec<(String, State)> {
        if self.cfg.as_ref() != Some(cfg) {
            self.map.clear();
            self.cfg = Some(cfg.clone());
        }
        while let Ok((k, s)) = self.rx.try_recv() {
            self.map.insert(k, (Instant::now(), s));
        }
        let todo: Vec<String> = keys
            .iter()
            .filter(|k| match self.map.get(*k) {
                None => true,
                Some((_, State::Loading)) => false,
                Some((t, _)) => t.elapsed() > FRESH,
            })
            .cloned()
            .collect();
        if !todo.is_empty() {
            for k in &todo {
                // Keep showing the old state while it is read again.
                self.map.entry(k.clone()).or_insert((Instant::now(), State::Loading));
            }
            let (tx, cfg, ctx) = (self.tx.clone(), cfg.clone(), ctx.clone());
            std::thread::spawn(move || {
                let mut failed: Option<State> = None;
                for k in todo {
                    // A refused token fails every key the same way: no need to ask for the rest.
                    let s = failed.clone().unwrap_or_else(|| fetch(&cfg, &k));
                    if matches!(&s, State::Failed(_)) {
                        failed = Some(s.clone());
                    }
                    let _ = tx.send((k, s));
                    ctx.request_repaint();
                }
            });
            // Stale entries keep their state but must not start another read next frame.
            for k in keys {
                if let Some(e) = self.map.get_mut(k) {
                    e.0 = Instant::now();
                }
            }
        }
        keys.iter().map(|k| (k.clone(), self.map.get(k).map(|e| e.1.clone()).unwrap_or(State::Loading))).collect()
    }

    /// Read everything again next time it is shown.
    pub fn forget(&mut self) {
        self.map.retain(|_, (_, s)| matches!(s, State::Loading));
    }

    /// Fill in issues without asking JIRA (screenshots).
    pub fn put(&mut self, cfg: &Config, key: &str, s: State) {
        self.cfg = Some(cfg.clone());
        self.map.insert(key.to_string(), (Instant::now() + Duration::from_secs(3600), s));
    }
}

/// Just enough JSON for JIRA's answers.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            _ => "",
        }
    }
    pub fn items(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => &[],
        }
    }
}

#[cfg(test)]
pub fn parse(text: &str) -> Result<Json, ()> {
    let b = text.as_bytes();
    let mut i = 0;
    let v = value(b, &mut i)?;
    skip_ws(b, &mut i);
    if i == b.len() { Ok(v) } else { Err(()) }
}

/// The first JSON object or array in a text: also past a byte-order mark, an anti-hijacking
/// prefix such as `)]}',` or something left after it.
pub fn parse_lenient(text: &str) -> Option<Json> {
    let b = text.as_bytes();
    let mut i = b.iter().position(|&c| c == b'{' || c == b'[')?;
    value(b, &mut i).ok()
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn value(b: &[u8], i: &mut usize) -> Result<Json, ()> {
    skip_ws(b, i);
    match b.get(*i).ok_or(())? {
        b'{' => {
            *i += 1;
            let mut out = vec![];
            skip_ws(b, i);
            if b.get(*i) == Some(&b'}') {
                *i += 1;
                return Ok(Json::Obj(out));
            }
            loop {
                skip_ws(b, i);
                let k = string(b, i)?;
                skip_ws(b, i);
                if b.get(*i) != Some(&b':') {
                    return Err(());
                }
                *i += 1;
                out.push((k, value(b, i)?));
                skip_ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b'}') => {
                        *i += 1;
                        return Ok(Json::Obj(out));
                    }
                    _ => return Err(()),
                }
            }
        }
        b'[' => {
            *i += 1;
            let mut out = vec![];
            skip_ws(b, i);
            if b.get(*i) == Some(&b']') {
                *i += 1;
                return Ok(Json::Arr(out));
            }
            loop {
                out.push(value(b, i)?);
                skip_ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b']') => {
                        *i += 1;
                        return Ok(Json::Arr(out));
                    }
                    _ => return Err(()),
                }
            }
        }
        b'"' => Ok(Json::Str(string(b, i)?)),
        b't' if b[*i..].starts_with(b"true") => {
            *i += 4;
            Ok(Json::Bool(true))
        }
        b'f' if b[*i..].starts_with(b"false") => {
            *i += 5;
            Ok(Json::Bool(false))
        }
        b'n' if b[*i..].starts_with(b"null") => {
            *i += 4;
            Ok(Json::Null)
        }
        _ => {
            let start = *i;
            while *i < b.len() && matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                *i += 1;
            }
            std::str::from_utf8(&b[start..*i]).ok().and_then(|s| s.parse().ok()).map(Json::Num).ok_or(())
        }
    }
}

fn string(b: &[u8], i: &mut usize) -> Result<String, ()> {
    if b.get(*i) != Some(&b'"') {
        return Err(());
    }
    *i += 1;
    let mut out: Vec<u8> = vec![];
    loop {
        let c = *b.get(*i).ok_or(())?;
        *i += 1;
        match c {
            b'"' => return String::from_utf8(out).map_err(|_| ()),
            b'\\' => {
                let e = *b.get(*i).ok_or(())?;
                *i += 1;
                let ch = match e {
                    b'n' => '\n',
                    b't' => '\t',
                    b'r' => '\r',
                    b'b' => '\u{8}',
                    b'f' => '\u{c}',
                    b'u' => {
                        let mut code = hex4(b, i)?;
                        // A pair of surrogates makes one character.
                        if (0xd800..0xdc00).contains(&code) && b.get(*i) == Some(&b'\\') && b.get(*i + 1) == Some(&b'u') {
                            *i += 2;
                            let low = hex4(b, i)?;
                            code = 0x10000 + ((code - 0xd800) << 10) + (low.wrapping_sub(0xdc00) & 0x3ff);
                        }
                        char::from_u32(code).unwrap_or('\u{fffd}')
                    }
                    other => other as char,
                };
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            _ => out.push(c),
        }
    }
}

fn hex4(b: &[u8], i: &mut usize) -> Result<u32, ()> {
    let s = b.get(*i..*i + 4).ok_or(())?;
    *i += 4;
    u32::from_str_radix(std::str::from_utf8(s).map_err(|_| ())?, 16).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_issue_keys() {
        let k = commit_keys("IDW-13506 [feat] Выбор компонентов, see ABC-7 and UTF-8", &["IDW-13506".into(), "origin/feature/APP-12-fix".into()]);
        assert_eq!(k, vec!["IDW-13506", "ABC-7"]);
        let k = commit_keys("[feat] Выбор компонентов", &["IDW-13506".into(), "origin/IDW-13506".into(), "origin/feature/APP-12-fix".into(), "master".into()]);
        assert_eq!(k, vec!["IDW-13506", "APP-12"]);
        let many: Vec<String> = (1..6).map(|i| format!("feature/APP-{i}")).collect();
        assert!(commit_keys("[chore] Old commit", &many).is_empty());
        let mut v = vec![];
        keys_in("A1-1 B-2,C-3", &mut v);
        assert_eq!(v, vec!["A1-1", "B-2", "C-3"]);
        let mut v = vec![];
        keys_in("xABC-1 ABC-01 abc-1 ABC-1x", &mut v);
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn reads_an_issue() {
        let body = r#"{"expand":"x","id":"10","key":"IDW-13506","fields":{"summary":"Выбор \"компонентов\" и 😀","issuetype":{"name":"Task"},
            "assignee":null,"status":{"name":"In Progress","statusCategory":{"id":4,"key":"indeterminate","colorName":"yellow"}},"labels":[],"n":-1.5e2,"ok":true}}"#;
        let j = parse(body).unwrap();
        let i = issue_from(&j, "IDW-13506");
        assert_eq!(i.summary, "Выбор \"компонентов\" и 😀");
        assert_eq!(i.status, "In Progress");
        assert_eq!(i.category, "indeterminate");
        assert_eq!(i.kind, "Task");
        assert_eq!(i.assignee, "");
        assert!(parse("{\"a\":1,}").is_err());
        assert_eq!(parse_lenient("\u{feff})]}',\n{\"key\":\"A-1\"}\n"), Some(Json::Obj(vec![("key".into(), Json::Str("A-1".into()))])));
        let page = Reply { code: 200, body: "<!DOCTYPE html>\n<html><head><TITLE>\n Log in - Jira </TITLE></head></html>".into(), content_type: "text/html;charset=UTF-8".into() };
        assert_eq!(json_of(&page).unwrap_err(), "JIRA answered with a web page \"Log in - Jira\" instead of data (HTTP 200, text/html;charset=UTF-8) — check the server URL");
        assert_eq!(http_error(401, r#"{"errorMessages":["Bad token"],"errors":{}}"#), "the token was not accepted (401): Bad token");
    }
}

