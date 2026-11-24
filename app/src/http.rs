//! Requests to the REST APIs of JIRA and of the Git hostings. They go through the `curl`
//! program; the headers (with the token) and the body are handed to it on stdin so they don't
//! show up in the process list.

use std::io::Write;
use std::process::{Command, Stdio};

/// One request: GET, or POST when there is a body (JSON).
#[derive(Default)]
pub struct Request {
    pub url: String,
    /// Whole header lines: "Accept: application/json".
    pub headers: Vec<String>,
    /// "name:password" for Basic authorisation.
    pub user: Option<String>,
    pub post: Option<String>,
}

/// What the server answered.
pub struct Reply {
    pub code: u16,
    pub body: String,
    pub content_type: String,
}

/// Marks the line curl adds after the body with the status and the content type.
const TRAILER: &str = "\n@@curl@@";

/// A value of curl's config file: in quotes, with what it reads as an escape written out.
fn quote(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What curl is told on stdin.
fn config(req: &Request) -> String {
    let mut conf = String::new();
    for h in &req.headers {
        conf.push_str(&format!("header = {}\n", quote(h)));
    }
    if let Some(u) = &req.user {
        conf.push_str(&format!("user = {}\n", quote(u)));
    }
    if let Some(body) = &req.post {
        conf.push_str("request = \"POST\"\n");
        conf.push_str("header = \"Content-Type: application/json\"\n");
        conf.push_str(&format!("data-binary = {}\n", quote(body)));
    }
    conf
}

pub fn send(req: &Request) -> Result<Reply, String> {
    let mut c = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    crate::git::no_console(&mut c);
    // --compressed: some proxies in front of a server send gzip whether asked or not.
    c.args(["-sS", "--compressed", "--max-time", "20", "-K", "-", "-w"]);
    c.arg(format!("{TRAILER}%{{http_code}} %{{content_type}}"));
    c.arg(&req.url);
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("could not run curl: {e}"))?;
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(config(req).as_bytes());
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

/// The JSON in an answer of `who` (the name of the service); when there is none, what came instead.
pub fn json_of(r: &Reply, who: &str) -> Result<crate::jira::Json, String> {
    if let Some(j) = crate::jira::parse_lenient(&r.body) {
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
    Err(format!("{who} answered with {what} (HTTP {}{ct}) — check the server URL", r.code))
}

fn html_title(body: &str) -> Option<String> {
    let mut end = body.len().min(400);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    let low = body[..end].to_lowercase();
    if !low.contains("<html") && !low.contains("<!doctype") {
        return None;
    }
    let re = regex::Regex::new(r"(?is)<title[^>]*>(.*?)</title").ok()?;
    let t = re.captures(body).map(|c| c[1].split_whitespace().collect::<Vec<_>>().join(" ")).filter(|t| !t.is_empty());
    Some(t.unwrap_or_else(|| "(HTML)".into()))
}

/// A string as JSON writes it.
pub fn json_str(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A part of a URL path or a query value: everything but letters, digits and `-._~` as %XX.
pub fn encode(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A path in a URL: every part encoded, the slashes kept.
pub fn encode_path(v: &str) -> String {
    v.split('/').map(encode).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn writes_json_strings_and_url_parts() {
        assert_eq!(json_str("a \"b\"\\\n\tя\u{1}"), "\"a \\\"b\\\"\\\\\\n\\tя\\u0001\"");
        assert_eq!(crate::jira::parse(&json_str("строка\n\"x\" \\ 😀")), Ok(crate::jira::Json::Str("строка\n\"x\" \\ 😀".into())));
        assert_eq!(encode("a b/я+c"), "a%20b%2F%D1%8F%2Bc");
        assert_eq!(encode_path("docs/мои заметки.md"), "docs/%D0%BC%D0%BE%D0%B8%20%D0%B7%D0%B0%D0%BC%D0%B5%D1%82%D0%BA%D0%B8.md");
    }

    #[test]
    fn says_what_came_instead_of_json() {
        let page = Reply { code: 200, body: "<!DOCTYPE html>\n<html><head><TITLE>\n Log in - Jira </TITLE></head></html>".into(), content_type: "text/html;charset=UTF-8".into() };
        assert_eq!(json_of(&page, "JIRA").unwrap_err(), "JIRA answered with a web page \"Log in - Jira\" instead of data (HTTP 200, text/html;charset=UTF-8) — check the server URL");
    }

    /// A server for one request on this computer: what it was sent comes back through the thread.
    fn one_answer(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                let Some((head, body)) = text.split_once("\r\n\r\n") else {
                    if n == 0 {
                        break;
                    }
                    continue;
                };
                let len = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                if body.len() >= len || n == 0 {
                    break;
                }
            }
            let _ = s.write_all(answer.as_bytes());
            String::from_utf8_lossy(&got).to_string()
        });
        (url, t)
    }

    #[test]
    fn posts_the_body_and_the_headers_through_curl() {
        if Command::new(if cfg!(windows) { "curl.exe" } else { "curl" }).arg("--version").output().is_err() {
            return;
        }
        let (url, server) = one_answer("HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}");
        let body = format!("{{\"title\":{}}}", json_str("Кавычки \"и\" \\ обратная черта\nвторая строка"));
        let r = send(&Request { url: format!("{url}/repos/a/b/pulls"), headers: vec!["Authorization: Bearer se\"cr\\et".into()], user: None, post: Some(body.clone()) }).unwrap();
        assert_eq!((r.code, r.body.as_str(), r.content_type.as_str()), (201, "{\"ok\":true}", "application/json"));
        let got = server.join().unwrap();
        assert!(got.starts_with("POST /repos/a/b/pulls HTTP/1.1\r\n"), "{got}");
        assert!(got.contains("Authorization: Bearer se\"cr\\et\r\n"), "{got}");
        assert!(got.contains("Content-Type: application/json\r\n"), "{got}");
        assert!(got.ends_with(&body), "{got}");
    }

    #[test]
    fn gets_with_basic_authorisation() {
        if Command::new(if cfg!(windows) { "curl.exe" } else { "curl" }).arg("--version").output().is_err() {
            return;
        }
        let (url, server) = one_answer("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let r = send(&Request { url: format!("{url}/x?a=1"), user: Some("me@example.com:pass".into()), ..Default::default() }).unwrap();
        assert_eq!(r.code, 404);
        let got = server.join().unwrap();
        assert!(got.starts_with("GET /x?a=1 HTTP/1.1\r\n"), "{got}");
        // base64 of "me@example.com:pass"
        assert!(got.contains("Authorization: Basic bWVAZXhhbXBsZS5jb206cGFzcw==\r\n"), "{got}");
    }
}
