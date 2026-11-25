//! Git hostings — GitHub, GitLab, Bitbucket: which one a remote belongs to, the addresses of its
//! pages (a commit, a branch, a file, a new pull request) and, with a token from the settings,
//! its REST API: the open pull requests, the results of CI and creating a pull request.
//! Nothing here is asked without a token, and a token goes only to the host it was entered for.

use crate::http::{encode, encode_path, json_str};
use crate::jira::Json;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    GitHub,
    GitLab,
    Bitbucket,
}

pub const KINDS: [Kind; 3] = [Kind::GitHub, Kind::GitLab, Kind::Bitbucket];

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::GitHub => "GitHub",
            Kind::GitLab => "GitLab",
            Kind::Bitbucket => "Bitbucket",
        }
    }
    /// The word in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Kind::GitHub => "github",
            Kind::GitLab => "gitlab",
            Kind::Bitbucket => "bitbucket",
        }
    }
    pub fn from_key(k: &str) -> Option<Kind> {
        KINDS.into_iter().find(|x| x.key() == k)
    }
    /// What the hosting calls a request to merge a branch.
    pub fn request(self) -> &'static str {
        if self == Kind::GitLab { "Merge Request" } else { "Pull Request" }
    }
    pub fn requests(self) -> &'static str {
        if self == Kind::GitLab { "Merge Requests" } else { "Pull Requests" }
    }
    /// The sign before the number of a request.
    pub fn sign(self) -> &'static str {
        if self == Kind::GitLab { "!" } else { "#" }
    }
    /// The host everybody uses.
    pub fn public_host(self) -> &'static str {
        match self {
            Kind::GitHub => "github.com",
            Kind::GitLab => "gitlab.com",
            Kind::Bitbucket => "bitbucket.org",
        }
    }
}

/// A host from Settings → Hosting: what runs there and the token to talk to it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Account {
    pub kind: Kind,
    pub host: String,
    pub token: String,
    /// Bitbucket: the Atlassian account e-mail of an API token (empty: an access token).
    pub email: String,
    /// Where the token is kept. In the store of the system, `token` is empty until it is read.
    pub kept: crate::secrets::Kept,
    /// A new token typed into the settings window and not applied yet (never saved).
    pub entered: String,
}

impl Account {
    /// The name of the token's entry in the store of the system.
    pub fn key(&self) -> String {
        crate::secrets::hosting_key(self.kind, &self.host)
    }

    /// A token is saved for the host (it may not be read from the store yet).
    pub fn has_token(&self) -> bool {
        !self.token.trim().is_empty() || self.kept != crate::secrets::Kept::File
    }

    /// The line of the settings file. The token is in it only when it is kept there; one in
    /// the store of the system leaves the word "store" in a fifth field (a build that knows
    /// four fields reads the line as a host without a token).
    pub fn to_line(&self) -> String {
        let f = |s: &str| s.trim().replace(['\t', '\n', '\r'], " ");
        match self.kept {
            crate::secrets::Kept::File => format!("{}\t{}\t{}\t{}", self.kind.key(), f(&self.host), f(&self.email), f(&self.token)),
            _ => format!("{}\t{}\t{}\t\tstore", self.kind.key(), f(&self.host), f(&self.email)),
        }
    }
    pub fn from_line(line: &str) -> Option<Account> {
        let mut f = line.split('\t');
        let kind = Kind::from_key(f.next()?)?;
        let host = f.next()?.trim().to_string();
        let email = f.next().unwrap_or("").trim().to_string();
        let token = f.next().unwrap_or("").trim().to_string();
        let kept = if f.next().map(str::trim) == Some("store") && token.is_empty() { crate::secrets::Kept::Store } else { crate::secrets::Kept::File };
        (!host.is_empty()).then_some(Account { kind, host, token, email, kept, entered: String::new() })
    }
}

/// A repository on a hosting.
#[derive(Clone, PartialEq, Debug)]
pub struct Site {
    pub kind: Kind,
    /// "github.com", "gitlab.example.com:8443"
    pub host: String,
    /// "owner/repo", "group/subgroup/repo"
    pub path: String,
    /// The remote is read over plain http (a server inside a network).
    pub http: bool,
    pub token: String,
    pub email: String,
}

/// The host and the repository path in the URL of a remote; whether it is plain http.
/// A name and a password written into the URL are left out.
pub fn parse_remote(url: &str) -> Option<(String, String, bool)> {
    let url = url.trim();
    let (host, path, http) = if let Some((scheme, rest)) = url.split_once("://") {
        let scheme = scheme.to_lowercase();
        if !matches!(scheme.as_str(), "https" | "http" | "ssh" | "git" | "git+ssh" | "ssh+git") {
            return None;
        }
        let (at, path) = rest.split_once('/')?;
        let host = at.rsplit_once('@').map_or(at, |x| x.1);
        let web = scheme.starts_with("http");
        // The port of ssh is not the port of the web pages.
        let host = if web { host } else { host.split(':').next().unwrap_or(host) };
        (host.to_string(), path.to_string(), scheme == "http")
    } else {
        // git@host:owner/repo.git
        let (at, path) = url.split_once(':')?;
        let host = at.rsplit_once('@').map_or(at, |x| x.1);
        // "C:\work\repo" is a folder, and so is anything with a slash before the colon.
        if host.len() < 2 || host.contains(['/', '\\']) || path.starts_with('\\') {
            return None;
        }
        (host.to_string(), path.to_string(), false)
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path).trim_matches('/');
    if host.is_empty() || !host.contains('.') && !host.contains(':') && host != "localhost" || path.split('/').filter(|p| !p.is_empty()).count() < 2 {
        return None;
    }
    Some((host.to_lowercase(), path.to_string(), http))
}

impl Site {
    /// The hosting behind a remote URL: a host from the settings, one of the public ones, or a
    /// host named after the program that runs there (gitlab.example.com).
    pub fn of(url: &str, accounts: &[Account]) -> Option<Site> {
        let (host, path, http) = parse_remote(url)?;
        let account = accounts.iter().find(|a| a.host.trim().eq_ignore_ascii_case(&host));
        let kind = match account {
            Some(a) => a.kind,
            None => KINDS.into_iter().find(|k| host == k.public_host() || *k != Kind::Bitbucket && host.starts_with(&format!("{}.", k.key())))?,
        };
        // Bitbucket Data Center has other pages and another API: only the cloud one is known.
        if kind == Kind::Bitbucket && host != Kind::Bitbucket.public_host() {
            return None;
        }
        let (token, email) = account.map(|a| (a.token.trim().to_string(), a.email.trim().to_string())).unwrap_or_default();
        Some(Site { kind, host, path, http, token, email })
    }

    /// What the cache knows the repository by.
    pub fn id(&self) -> String {
        format!("{}/{}", self.host, self.path)
    }

    fn root(&self) -> String {
        format!("{}://{}", if self.http { "http" } else { "https" }, self.host)
    }

    pub fn repo_url(&self) -> String {
        format!("{}/{}", self.root(), encode_path(&self.path))
    }

    pub fn commit_url(&self, sha: &str) -> String {
        let r = self.repo_url();
        match self.kind {
            Kind::GitHub => format!("{r}/commit/{sha}"),
            Kind::GitLab => format!("{r}/-/commit/{sha}"),
            Kind::Bitbucket => format!("{r}/commits/{sha}"),
        }
    }

    pub fn branch_url(&self, branch: &str) -> String {
        let (r, b) = (self.repo_url(), encode_path(branch));
        match self.kind {
            Kind::GitHub => format!("{r}/tree/{b}"),
            Kind::GitLab => format!("{r}/-/tree/{b}"),
            Kind::Bitbucket => format!("{r}/branch/{b}"),
        }
    }

    /// A file as it is in a commit or at the top of a branch.
    pub fn file_url(&self, sha: &str, path: &str) -> String {
        let (r, sha, p) = (self.repo_url(), encode_path(sha), encode_path(path));
        match self.kind {
            Kind::GitHub => format!("{r}/blob/{sha}/{p}"),
            Kind::GitLab => format!("{r}/-/blob/{sha}/{p}"),
            Kind::Bitbucket => format!("{r}/src/{sha}/{p}"),
        }
    }

    /// The hosting's own page for a new request from `source` into `target`.
    pub fn new_request_url(&self, source: &str, target: &str) -> String {
        let r = self.repo_url();
        match self.kind {
            Kind::GitHub => format!("{r}/compare/{}...{}?quick_pull=1", encode_path(target), encode_path(source)),
            Kind::GitLab => format!("{r}/-/merge_requests/new?merge_request%5Bsource_branch%5D={}&merge_request%5Btarget_branch%5D={}", encode(source), encode(target)),
            Kind::Bitbucket => format!("{r}/pull-requests/new?source={}&dest={}", encode(source), encode(target)),
        }
    }

    /// Where the REST API of this repository's host is; None where there is none we know
    /// (Bitbucket on a host of its own, in a row of the settings being tested).
    fn api(&self) -> Option<String> {
        match self.kind {
            Kind::GitHub if self.host == "github.com" => Some("https://api.github.com".into()),
            Kind::GitHub => Some(format!("{}/api/v3", self.root())),
            Kind::GitLab => Some(format!("{}/api/v4", self.root())),
            Kind::Bitbucket if self.host == "bitbucket.org" => Some("https://api.bitbucket.org/2.0".into()),
            Kind::Bitbucket => None,
        }
    }

    /// The API path of the repository itself.
    fn repo_api(&self) -> Option<String> {
        let api = self.api()?;
        Some(match self.kind {
            Kind::GitHub => format!("{api}/repos/{}", encode_path(&self.path)),
            Kind::GitLab => format!("{api}/projects/{}", encode(&self.path)),
            Kind::Bitbucket => format!("{api}/repositories/{}", encode_path(&self.path)),
        })
    }

    /// Whether the open requests and CI can be read: a token is set and the API is known.
    pub fn connected(&self) -> bool {
        !self.token.is_empty() && self.api().is_some()
    }

    fn request(&self, url: String) -> crate::http::Request {
        let mut req = crate::http::Request { url, ..Default::default() };
        match self.kind {
            Kind::GitHub => {
                req.headers = vec!["Accept: application/vnd.github+json".into(), "X-GitHub-Api-Version: 2022-11-28".into(), "User-Agent: RefBraid".into(), format!("Authorization: Bearer {}", self.token)];
            }
            Kind::GitLab => {
                req.headers = vec!["Accept: application/json".into(), format!("PRIVATE-TOKEN: {}", self.token)];
            }
            Kind::Bitbucket => {
                req.headers = vec!["Accept: application/json".into()];
                if self.email.is_empty() {
                    req.headers.push(format!("Authorization: Bearer {}", self.token));
                } else {
                    req.user = Some(format!("{}:{}", self.email, self.token));
                }
            }
        }
        req
    }

    fn get(&self, url: String) -> Result<Json, String> {
        let r = crate::http::send(&self.request(url))?;
        if r.code != 200 {
            return Err(api_error(self.kind, r.code, &r.body));
        }
        crate::http::json_of(&r, self.kind.name())
    }
}

/// What became of a pull request.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Fate {
    #[default]
    Open,
    /// Closed without merging (declined, superseded).
    Closed,
    Merged,
}

/// A pull request (merge request).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Request {
    /// "12"
    pub number: String,
    pub title: String,
    pub url: String,
    pub source: String,
    pub target: String,
    /// The commit at the top of the source branch, when the hosting gives all of it.
    pub sha: String,
    pub author: String,
    pub draft: bool,
    pub fate: Fate,
    /// When it was merged; of one not merged, when it was last changed (seconds since 1970,
    /// 0 when the hosting did not say).
    pub when: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ci {
    Passed,
    Cancelled,
    Running,
    Failed,
}

/// What CI says about a commit: all its runs taken together and the page of the one that decided.
#[derive(Clone, PartialEq, Debug)]
pub struct CiRun {
    pub state: Ci,
    pub url: String,
    /// The names of the runs: "CI, Release".
    pub name: String,
}

/// What a hosting told about a repository.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Info {
    pub default_branch: String,
    pub requests: Vec<Request>,
    /// By the full hash of the commit.
    pub ci: HashMap<String, CiRun>,
    pub error: Option<String>,
}

impl Info {
    /// The open requests out of a branch.
    pub fn requests_of(&self, branch: &str) -> Vec<&Request> {
        self.requests.iter().filter(|r| r.source == branch).collect()
    }
}

fn text(j: &Json, path: &[&str]) -> String {
    let mut v = Some(j);
    for p in path {
        v = v.and_then(|x| x.get(p));
    }
    match v {
        Some(Json::Num(n)) => format!("{n}"),
        Some(x) => x.str().to_string(),
        None => String::new(),
    }
}

fn flag(j: &Json, key: &str) -> bool {
    matches!(j.get(key), Some(Json::Bool(true)))
}

/// Why the hosting refused, in its own words when it gave any.
fn api_error(kind: Kind, code: u16, body: &str) -> String {
    let what = match code {
        401 => "the token was not accepted (401)".to_string(),
        403 => "access denied (403)".to_string(),
        404 => "not found, or the token may not see this repository (404)".to_string(),
        _ => format!("the server answered {code}"),
    };
    let why = crate::jira::parse_lenient(body).map(|j| message_of(kind, &j)).filter(|s| !s.is_empty());
    match why {
        Some(w) => format!("{what}: {w}"),
        None => what,
    }
}

/// Every text in a value: a string, or the strings of a list or of an object's fields.
fn words(j: &Json, out: &mut Vec<String>) {
    match j {
        Json::Str(s) if !s.is_empty() => out.push(s.clone()),
        Json::Arr(v) => v.iter().for_each(|x| words(x, out)),
        Json::Obj(v) => v.iter().for_each(|(_, x)| words(x, out)),
        _ => {}
    }
}

fn message_of(kind: Kind, j: &Json) -> String {
    let mut out = vec![];
    match kind {
        Kind::GitHub => {
            out.push(text(j, &["message"]));
            for e in j.get("errors").map(|e| e.items()).unwrap_or_default() {
                let m = text(e, &["message"]);
                out.push(if m.is_empty() { e.str().to_string() } else { m });
            }
        }
        Kind::GitLab => {
            for key in ["message", "error_description", "error"] {
                if let Some(m) = j.get(key) {
                    words(m, &mut out);
                    break;
                }
            }
        }
        Kind::Bitbucket => out.push(text(j, &["error", "message"])),
    }
    out.retain(|s| !s.is_empty());
    out.join("; ")
}

fn default_branch_from(kind: Kind, j: &Json) -> String {
    match kind {
        Kind::GitHub | Kind::GitLab => text(j, &["default_branch"]),
        Kind::Bitbucket => text(j, &["mainbranch", "name"]),
    }
}

/// The first of these times the hosting gave ("2026-11-20T10:11:12Z"), in seconds since 1970.
fn time_of(texts: &[String]) -> i64 {
    texts.iter().find_map(|t| chrono::DateTime::parse_from_rfc3339(t.trim()).ok()).map_or(0, |t| t.timestamp())
}

/// The order of the list of a branch: the ones not merged first, then the merged ones; in
/// each part the latest first.
pub fn sort_requests(list: &mut [Request]) {
    list.sort_by(|a, b| (a.fate == Fate::Merged).cmp(&(b.fate == Fate::Merged)).then(b.when.cmp(&a.when)).then(b.number.len().cmp(&a.number.len())).then(b.number.cmp(&a.number)));
}

fn request_from(kind: Kind, j: &Json) -> Request {
    match kind {
        Kind::GitHub => {
            let merged = !text(j, &["merged_at"]).is_empty();
            let fate = if merged { Fate::Merged } else if text(j, &["state"]) == "closed" { Fate::Closed } else { Fate::Open };
            let when = time_of(&[text(j, &[if merged { "merged_at" } else { "updated_at" }]), text(j, &["created_at"])]);
            Request {
            fate,
            when,
            number: text(j, &["number"]),
            title: text(j, &["title"]),
            url: text(j, &["html_url"]),
            source: text(j, &["head", "ref"]),
            target: text(j, &["base", "ref"]),
            sha: text(j, &["head", "sha"]),
            author: text(j, &["user", "login"]),
            draft: flag(j, "draft"),
            }
        }
        Kind::GitLab => {
            let fate = match text(j, &["state"]).as_str() {
                "merged" => Fate::Merged,
                "closed" => Fate::Closed,
                _ => Fate::Open,
            };
            let when = time_of(&[text(j, &[if fate == Fate::Merged { "merged_at" } else { "updated_at" }]), text(j, &["updated_at"]), text(j, &["created_at"])]);
            Request {
            fate,
            when,
            number: text(j, &["iid"]),
            title: text(j, &["title"]),
            url: text(j, &["web_url"]),
            source: text(j, &["source_branch"]),
            target: text(j, &["target_branch"]),
            sha: text(j, &["sha"]),
            author: text(j, &["author", "username"]),
            draft: flag(j, "draft") || flag(j, "work_in_progress"),
            }
        }
        Kind::Bitbucket => Request {
            fate: match text(j, &["state"]).as_str() {
                "MERGED" => Fate::Merged,
                "DECLINED" | "SUPERSEDED" => Fate::Closed,
                _ => Fate::Open,
            },
            // Bitbucket has no time of merging of its own: a merged request is not changed after.
            when: time_of(&[text(j, &["updated_on"]), text(j, &["created_on"])]),
            number: text(j, &["id"]),
            title: text(j, &["title"]),
            url: text(j, &["links", "html", "href"]),
            source: text(j, &["source", "branch", "name"]),
            target: text(j, &["destination", "branch", "name"]),
            // Bitbucket gives only the start of the hash here.
            sha: String::new(),
            author: text(j, &["author", "display_name"]),
            draft: flag(j, "draft"),
        },
    }
}

/// The list in an answer: the answer itself, or its field that holds the page.
fn list<'a>(kind: Kind, j: &'a Json, field: &str) -> &'a [Json] {
    match kind {
        Kind::Bitbucket => j.get("values").map(|v| v.items()).unwrap_or_default(),
        _ if field.is_empty() => j.items(),
        _ => j.get(field).map(|v| v.items()).unwrap_or_default(),
    }
}

fn requests_from(kind: Kind, j: &Json) -> Vec<Request> {
    list(kind, j, "").iter().map(|r| request_from(kind, r)).filter(|r| !r.number.is_empty()).collect()
}

/// One run of CI as the hosting lists it: the commit, the name, the state and the page.
fn run_from(site: &Site, j: &Json) -> Option<(String, String, Ci, String)> {
    let (sha, name, state, url) = match site.kind {
        Kind::GitHub => {
            let state = match (text(j, &["status"]).as_str(), text(j, &["conclusion"]).as_str()) {
                ("completed", "success" | "neutral") => Ci::Passed,
                ("completed", "skipped") => return None,
                ("completed", "cancelled" | "stale") => Ci::Cancelled,
                ("completed", _) => Ci::Failed,
                _ => Ci::Running,
            };
            (text(j, &["head_sha"]), text(j, &["name"]), state, text(j, &["html_url"]))
        }
        Kind::GitLab => {
            let state = match text(j, &["status"]).as_str() {
                "success" => Ci::Passed,
                "failed" => Ci::Failed,
                "canceled" | "canceling" => Ci::Cancelled,
                "skipped" => return None,
                _ => Ci::Running,
            };
            (text(j, &["sha"]), text(j, &["name"]), state, text(j, &["web_url"]))
        }
        Kind::Bitbucket => {
            let state = match (text(j, &["state", "name"]).as_str(), text(j, &["state", "result", "name"]).as_str()) {
                ("COMPLETED", "SUCCESSFUL") => Ci::Passed,
                ("COMPLETED", "STOPPED") => Ci::Cancelled,
                ("COMPLETED", _) => Ci::Failed,
                _ => Ci::Running,
            };
            let n = text(j, &["build_number"]);
            let url = if n.is_empty() { format!("{}/pipelines", site.repo_url()) } else { format!("{}/pipelines/results/{n}", site.repo_url()) };
            (text(j, &["target", "commit", "hash"]), String::new(), state, url)
        }
    };
    (!sha.is_empty()).then_some((sha, name, state, url))
}

/// The runs (newest first) put together by commit: of the runs of one name the newest counts,
/// and the worst of those names the commit — failed, then still running, cancelled, passed.
fn ci_from(site: &Site, j: &Json) -> HashMap<String, CiRun> {
    let weight = |c: Ci| match c {
        Ci::Passed => 0,
        Ci::Cancelled => 1,
        Ci::Running => 2,
        Ci::Failed => 3,
    };
    let mut seen: HashMap<String, Vec<String>> = HashMap::new();
    let mut out: HashMap<String, CiRun> = HashMap::new();
    for (sha, name, state, url) in list(site.kind, j, if site.kind == Kind::GitHub { "workflow_runs" } else { "" }).iter().filter_map(|r| run_from(site, r)) {
        let names = seen.entry(sha.clone()).or_default();
        if names.contains(&name) {
            continue;
        }
        names.push(name.clone());
        match out.get_mut(&sha) {
            None => {
                out.insert(sha, CiRun { state, url, name });
            }
            Some(run) => {
                if !name.is_empty() {
                    run.name = if run.name.is_empty() { name } else { format!("{}, {name}", run.name) };
                }
                if weight(state) > weight(run.state) {
                    run.state = state;
                    run.url = url;
                }
            }
        }
    }
    out
}

/// Everything shown about a repository, in three requests.
pub fn load(site: &Site) -> Info {
    let Some(api) = site.repo_api() else { return Info::default() };
    let mut info = Info::default();
    match site.get(api.clone()) {
        Ok(j) => info.default_branch = default_branch_from(site.kind, &j),
        Err(e) => {
            // A refused token fails the rest the same way.
            info.error = Some(e);
            return info;
        }
    }
    let requests = match site.kind {
        Kind::GitHub => format!("{api}/pulls?state=open&per_page=100"),
        Kind::GitLab => format!("{api}/merge_requests?state=opened&per_page=100"),
        Kind::Bitbucket => format!("{api}/pullrequests?state=OPEN&pagelen=50"),
    };
    match site.get(requests) {
        Ok(j) => info.requests = requests_from(site.kind, &j),
        Err(e) => info.error = Some(e),
    }
    let runs = match site.kind {
        Kind::GitHub => format!("{api}/actions/runs?per_page=100"),
        Kind::GitLab => format!("{api}/pipelines?per_page=100"),
        Kind::Bitbucket => format!("{api}/pipelines?sort=-created_on&pagelen=100"),
    };
    // No CI set up, or a token that may not read it, is not a reason to hide the requests.
    if let Ok(j) = site.get(runs) {
        info.ci = ci_from(site, &j);
    }
    info
}

/// Every request there was out of a branch — open, closed and merged — in the order of
/// [`sort_requests`]. The last hundred the hosting has (fifty at Bitbucket).
pub fn branch_requests(site: &Site, branch: &str) -> Result<Vec<Request>, String> {
    let api = site.repo_api().filter(|_| !site.token.is_empty()).ok_or("no token for this host in Settings → Hosting")?;
    let mut list = requests_from(site.kind, &site.get(branch_requests_url(site, &api, branch))?);
    // The hosting was asked for this branch; a request from a fork's branch of the same name is not ours.
    list.retain(|r| r.source == branch);
    sort_requests(&mut list);
    Ok(list)
}

/// The requests a commit came in with, or is waiting in: whatever became of them, in the
/// order of [`sort_requests`].
pub fn commit_requests(site: &Site, sha: &str) -> Result<Vec<Request>, String> {
    let api = site.repo_api().filter(|_| !site.token.is_empty()).ok_or("no token for this host in Settings → Hosting")?;
    let mut list = requests_from(site.kind, &site.get(commit_requests_url(site, &api, sha))?);
    sort_requests(&mut list);
    Ok(list)
}

fn commit_requests_url(site: &Site, api: &str, sha: &str) -> String {
    match site.kind {
        Kind::GitHub => format!("{api}/commits/{}/pulls?per_page=30", encode(sha)),
        Kind::GitLab => format!("{api}/repository/commits/{}/merge_requests", encode(sha)),
        Kind::Bitbucket => format!("{api}/commit/{}/pullrequests?pagelen=30", encode(sha)),
    }
}

fn branch_requests_url(site: &Site, api: &str, branch: &str) -> String {
    match site.kind {
        // "owner:branch": the branch in this repository, not in a fork.
        Kind::GitHub => format!("{api}/pulls?state=all&head={}&sort=updated&direction=desc&per_page=100", encode(&format!("{}:{branch}", site.path.split('/').next().unwrap_or("")))),
        Kind::GitLab => format!("{api}/merge_requests?state=all&source_branch={}&order_by=updated_at&per_page=100", encode(branch)),
        Kind::Bitbucket => format!("{api}/pullrequests?state=OPEN&state=MERGED&state=DECLINED&state=SUPERSEDED&q={}&sort=-updated_on&pagelen=50", encode(&format!("source.branch.name=\"{}\"", branch.replace('\\', "\\\\").replace('"', "\\\"")))),
    }
}

/// The body of the request that creates a pull request.
fn create_body(kind: Kind, title: &str, description: &str, source: &str, target: &str) -> String {
    let (t, d, s, b) = (json_str(title), json_str(description), json_str(source), json_str(target));
    match kind {
        Kind::GitHub => format!("{{\"title\":{t},\"body\":{d},\"head\":{s},\"base\":{b}}}"),
        Kind::GitLab => format!("{{\"title\":{t},\"description\":{d},\"source_branch\":{s},\"target_branch\":{b}}}"),
        Kind::Bitbucket => format!("{{\"title\":{t},\"description\":{d},\"source\":{{\"branch\":{{\"name\":{s}}}}},\"destination\":{{\"branch\":{{\"name\":{b}}}}}}}"),
    }
}

/// Create a pull request from one branch of the repository into another.
pub fn create(site: &Site, title: &str, description: &str, source: &str, target: &str) -> Result<Request, String> {
    let api = site.repo_api().filter(|_| !site.token.is_empty()).ok_or("no token for this host in Settings → Hosting")?;
    let url = match site.kind {
        Kind::GitHub => format!("{api}/pulls"),
        Kind::GitLab => format!("{api}/merge_requests"),
        Kind::Bitbucket => format!("{api}/pullrequests"),
    };
    let mut req = site.request(url);
    req.post = Some(create_body(site.kind, title, description, source, target));
    let r = crate::http::send(&req)?;
    if r.code != 200 && r.code != 201 {
        return Err(api_error(site.kind, r.code, &r.body));
    }
    let made = request_from(site.kind, &crate::http::json_of(&r, site.kind.name())?);
    if made.number.is_empty() { Err("the server did not say what it created".into()) } else { Ok(made) }
}

/// Check a host and its token: the name of the account it signs in as.
pub fn test(account: &Account) -> Result<String, String> {
    let host = account.host.trim().to_lowercase();
    if host.is_empty() || account.token.trim().is_empty() {
        return Err("enter the host and the token".into());
    }
    let site = Site { kind: account.kind, host, path: String::new(), http: false, token: account.token.trim().into(), email: account.email.trim().into() };
    let api = site.api().ok_or("only bitbucket.org is supported for Bitbucket")?;
    let j = site.get(format!("{api}/user"))?;
    let name = match account.kind {
        Kind::GitHub => text(&j, &["login"]),
        Kind::GitLab => text(&j, &["username"]),
        Kind::Bitbucket => text(&j, &["display_name"]),
    };
    Ok(if name.is_empty() { "Connected".to_string() } else { format!("Connected as {name}") })
}

struct Entry {
    at: Instant,
    info: Option<Info>,
    loading: bool,
    /// Asked to be read again whatever its age.
    stale: bool,
}

/// What the hostings told about the repositories, kept for a few minutes and read again on a
/// background thread.
pub struct Cache {
    map: HashMap<String, Entry>,
    tx: Sender<(String, Info)>,
    rx: Receiver<(String, Info)>,
    /// The requests of single commits, by repository and hash: when they were asked for and,
    /// once the answer is here, what it was (nothing when the hosting refused).
    commits: HashMap<(String, String), (Instant, Option<Vec<Request>>)>,
    commit_tx: Sender<((String, String), Vec<Request>)>,
    commit_rx: Receiver<((String, String), Vec<Request>)>,
    /// Nothing is asked over the network (screenshots, tests).
    pub offline: bool,
}

impl Default for Cache {
    fn default() -> Self {
        let (tx, rx) = channel();
        let (commit_tx, commit_rx) = channel();
        Cache { map: HashMap::new(), tx, rx, commits: HashMap::new(), commit_tx, commit_rx, offline: cfg!(test) }
    }
}

const FRESH: Duration = Duration::from_secs(120);

impl Cache {
    /// What is known about a repository; starts reading when nothing is, or it was read a while ago.
    pub fn info(&mut self, site: &Site, ctx: &eframe::egui::Context) -> Option<&Info> {
        while let Ok((id, info)) = self.rx.try_recv() {
            self.map.insert(id, Entry { at: Instant::now(), info: Some(info), loading: false, stale: false });
        }
        let id = site.id();
        let due = match self.map.get(&id) {
            None => true,
            Some(e) => !e.loading && (e.stale || e.at.elapsed() > FRESH),
        };
        if due && site.connected() && !self.offline {
            let e = self.map.entry(id.clone()).or_insert(Entry { at: Instant::now(), info: None, loading: true, stale: false });
            e.loading = true;
            e.stale = false;
            let (tx, site, ctx, id) = (self.tx.clone(), site.clone(), ctx.clone(), id.clone());
            std::thread::spawn(move || {
                let _ = tx.send((id, load(&site)));
                ctx.request_repaint();
            });
        }
        self.map.get(&id).and_then(|e| e.info.as_ref())
    }

    /// The requests of one commit; starts reading them when they are not known, or were read a
    /// while ago. None until the answer is here.
    pub fn commit_requests(&mut self, site: &Site, sha: &str, ctx: &eframe::egui::Context) -> Option<&[Request]> {
        while let Ok((key, list)) = self.commit_rx.try_recv() {
            self.commits.insert(key, (Instant::now(), Some(list)));
        }
        let key = (site.id(), sha.to_string());
        let due = match self.commits.get(&key) {
            None => true,
            // Asked and not answered yet: wait. Answered: again when it is old.
            Some((_, None)) => false,
            Some((at, Some(_))) => at.elapsed() > FRESH,
        };
        if due && site.connected() && !self.offline {
            // What was known stays on show while it is read again.
            let known = self.commits.remove(&key).and_then(|e| e.1);
            self.commits.insert(key.clone(), (Instant::now(), known));
            {
                let (tx, site, ctx, sha) = (self.commit_tx.clone(), site.clone(), ctx.clone(), sha.to_string());
                std::thread::spawn(move || {
                    // A hosting that refuses (no rights, no such commit there) has nothing to show.
                    let _ = tx.send((key, commit_requests(&site, &sha).unwrap_or_default()));
                    ctx.request_repaint();
                });
            }
        }
        self.commits.get(&(site.id(), sha.to_string())).and_then(|e| e.1.as_deref())
    }

    /// Fill in the requests of a commit without asking the hosting (screenshots).
    pub fn put_commit(&mut self, site: &Site, sha: &str, list: Vec<Request>) {
        self.commits.insert((site.id(), sha.to_string()), (Instant::now(), Some(list)));
    }

    /// What is known already, without starting anything.
    pub fn known(&self, site: &Site) -> Option<&Info> {
        self.map.get(&site.id()).and_then(|e| e.info.as_ref())
    }

    /// Read everything again the next time it is shown (after a push, new settings, F5).
    pub fn forget(&mut self) {
        for e in self.map.values_mut() {
            e.stale = true;
        }
        // The requests of commits are read again as well: one may have been merged.
        self.commits.retain(|_, e| e.1.is_none());
    }

    /// Drop what was read with other tokens.
    pub fn clear(&mut self) {
        self.map.clear();
        self.commits.clear();
    }

    /// Fill in a repository without asking the hosting (screenshots, a request just created).
    pub fn put(&mut self, site: &Site, info: Info) {
        self.map.insert(site.id(), Entry { at: Instant::now(), info: Some(info), loading: false, stale: false });
    }

    /// Add a request just created to what is known, until the list is read again.
    pub fn add_request(&mut self, site: &Site, r: Request) {
        let e = self.map.entry(site.id()).or_insert(Entry { at: Instant::now(), info: None, loading: false, stale: false });
        let info = e.info.get_or_insert_with(Info::default);
        info.requests.retain(|x| x.number != r.number);
        info.requests.insert(0, r);
    }
}

/// The remote a branch name starts with: "origin/feature/x" → ("origin", "feature/x").
pub fn split_remote<'a>(name: &'a str, remotes: &[String]) -> Option<(&'a str, &'a str)> {
    remotes
        .iter()
        .filter_map(|r| name.strip_prefix(r.as_str()).and_then(|rest| rest.strip_prefix('/')).map(|b| (&name[..r.len()], b)))
        .max_by_key(|(r, _)| r.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(url: &str) -> Option<(String, String, bool)> {
        parse_remote(url)
    }

    #[test]
    fn reads_the_host_and_the_path_of_a_remote() {
        let gh = Some(("github.com".to_string(), "octo/site".to_string(), false));
        assert_eq!(parsed("git@github.com:octo/site.git"), gh);
        assert_eq!(parsed("https://github.com/octo/site.git"), gh);
        assert_eq!(parsed("https://GitHub.com/octo/site/"), gh);
        assert_eq!(parsed("ssh://git@github.com:22/octo/site.git"), gh);
        assert_eq!(parsed("git://github.com/octo/site"), gh);
        // A name and a password in the URL are not part of the host.
        assert_eq!(parsed("https://alex:p%40ss@github.com/octo/site.git"), gh);
        assert_eq!(parsed("git@gitlab.example.com:group/sub/deep/repo.git"), Some(("gitlab.example.com".into(), "group/sub/deep/repo".into(), false)));
        assert_eq!(parsed("http://gitlab.example.com:8080/group/repo.git"), Some(("gitlab.example.com:8080".into(), "group/repo".into(), true)));
        for local in ["/srv/git/repo.git", "../other", "C:\\work\\repo", "C:/work/repo", "file:///srv/git/a/repo.git", "server:repo.git", "https://github.com/onlyowner"] {
            assert_eq!(parsed(local), None, "{local}");
        }
    }

    fn site(url: &str) -> Site {
        Site::of(url, &[]).unwrap()
    }

    #[test]
    fn knows_the_hosting_and_gives_a_token_only_to_its_host() {
        let accounts = vec![
            Account { kind: Kind::GitLab, host: "code.example.com".into(), token: " glpat-x ".into(), ..Default::default() },
            Account { kind: Kind::GitHub, host: "GitHub.com".into(), token: "ghp-y".into(), ..Default::default() },
        ];
        let s = Site::of("git@code.example.com:team/app.git", &accounts).unwrap();
        assert_eq!((s.kind, s.token.as_str(), s.connected()), (Kind::GitLab, "glpat-x", true));
        let s = Site::of("https://github.com/octo/site", &accounts).unwrap();
        assert_eq!((s.kind, s.token.as_str()), (Kind::GitHub, "ghp-y"));
        // Known by its name, but nothing is asked of it without a token of its own.
        let s = Site::of("git@gitlab.example.com:team/app.git", &accounts).unwrap();
        assert_eq!((s.kind, s.token.as_str(), s.connected()), (Kind::GitLab, "", false));
        assert_eq!(Site::of("git@code.example.org:team/app.git", &accounts), None);
        assert_eq!(site("git@bitbucket.org:team/app.git").kind, Kind::Bitbucket);
        // Bitbucket on a server of one's own is another program: no links made for the cloud one.
        assert_eq!(Site::of("ssh://git@bitbucket.example.com:7999/proj/app.git", &accounts), None);
        let own = vec![Account { kind: Kind::Bitbucket, host: "git.example.com".into(), ..Default::default() }];
        assert_eq!(Site::of("git@git.example.com:proj/app.git", &own), None);
    }

    #[test]
    fn builds_the_addresses_of_pages() {
        let gh = site("git@github.com:octo/site.git");
        assert_eq!(gh.commit_url("abc123"), "https://github.com/octo/site/commit/abc123");
        assert_eq!(gh.branch_url("feature/новая ветка"), "https://github.com/octo/site/tree/feature/%D0%BD%D0%BE%D0%B2%D0%B0%D1%8F%20%D0%B2%D0%B5%D1%82%D0%BA%D0%B0");
        assert_eq!(gh.file_url("abc123", "docs/my notes #1.md"), "https://github.com/octo/site/blob/abc123/docs/my%20notes%20%231.md");
        assert_eq!(gh.file_url("fix #12/ветка", "a.rs"), "https://github.com/octo/site/blob/fix%20%2312/%D0%B2%D0%B5%D1%82%D0%BA%D0%B0/a.rs");
        assert_eq!(gh.new_request_url("feature/x", "main"), "https://github.com/octo/site/compare/main...feature/x?quick_pull=1");
        let gl = site("http://gitlab.example.com:8080/group/sub/repo.git");
        assert_eq!(gl.commit_url("abc"), "http://gitlab.example.com:8080/group/sub/repo/-/commit/abc");
        assert_eq!(gl.branch_url("dev"), "http://gitlab.example.com:8080/group/sub/repo/-/tree/dev");
        assert_eq!(gl.file_url("abc", "a/b.rs"), "http://gitlab.example.com:8080/group/sub/repo/-/blob/abc/a/b.rs");
        assert_eq!(gl.new_request_url("feature/x", "main"), "http://gitlab.example.com:8080/group/sub/repo/-/merge_requests/new?merge_request%5Bsource_branch%5D=feature%2Fx&merge_request%5Btarget_branch%5D=main");
        assert_eq!(gl.repo_api().unwrap(), "http://gitlab.example.com:8080/api/v4/projects/group%2Fsub%2Frepo");
        let bb = site("git@bitbucket.org:team/app.git");
        assert_eq!(bb.commit_url("abc"), "https://bitbucket.org/team/app/commits/abc");
        assert_eq!(bb.branch_url("dev"), "https://bitbucket.org/team/app/branch/dev");
        assert_eq!(bb.file_url("abc", "a/b.rs"), "https://bitbucket.org/team/app/src/abc/a/b.rs");
        assert_eq!(bb.new_request_url("feature/x", "main"), "https://bitbucket.org/team/app/pull-requests/new?source=feature%2Fx&dest=main");
        assert_eq!(bb.repo_api().unwrap(), "https://api.bitbucket.org/2.0/repositories/team/app");
        assert_eq!(gh.repo_api().unwrap(), "https://api.github.com/repos/octo/site");
        let ghe = Site::of("git@git.example.com:octo/site.git", &[Account { kind: Kind::GitHub, host: "git.example.com".into(), ..Default::default() }]).unwrap();
        assert_eq!(ghe.repo_api().unwrap(), "https://git.example.com/api/v3/repos/octo/site");
    }

    fn json(s: &str) -> Json {
        crate::jira::parse(s).unwrap()
    }

    #[test]
    fn reads_the_open_requests() {
        let gh = json(r#"[{"number":12,"title":"Add the search","html_url":"https://github.com/octo/site/pull/12","draft":true,"user":{"login":"alex"},
            "head":{"ref":"feature/search","sha":"1111111111111111111111111111111111111111"},"base":{"ref":"main"}}]"#);
        let r = requests_from(Kind::GitHub, &gh);
        assert_eq!(r, vec![Request { number: "12".into(), title: "Add the search".into(), url: "https://github.com/octo/site/pull/12".into(), source: "feature/search".into(), target: "main".into(), sha: "1".repeat(40), author: "alex".into(), draft: true, fate: Fate::Open, when: 0 }]);
        let gl = json(r#"[{"iid":7,"id":900,"title":"Fix","web_url":"https://gitlab.example.com/g/r/-/merge_requests/7","draft":false,"source_branch":"fix","target_branch":"dev","sha":"abc","author":{"username":"dana"}}]"#);
        let r = requests_from(Kind::GitLab, &gl);
        assert_eq!((r[0].number.as_str(), r[0].source.as_str(), r[0].target.as_str(), r[0].author.as_str(), r[0].draft), ("7", "fix", "dev", "dana", false));
        let bb = json(r#"{"pagelen":50,"values":[{"id":3,"title":"Docs","links":{"html":{"href":"https://bitbucket.org/team/app/pull-requests/3"}},
            "source":{"branch":{"name":"docs"},"commit":{"hash":"abcdef123456"}},"destination":{"branch":{"name":"main"}},"author":{"display_name":"Sam Lee"}}]}"#);
        let r = requests_from(Kind::Bitbucket, &bb);
        assert_eq!((r[0].number.as_str(), r[0].url.as_str(), r[0].source.as_str(), r[0].target.as_str(), r[0].author.as_str()), ("3", "https://bitbucket.org/team/app/pull-requests/3", "docs", "main", "Sam Lee"));
        let info = Info { requests: r, ..Default::default() };
        assert_eq!(info.requests_of("docs").len(), 1);
        assert!(info.requests_of("main").is_empty());
    }

    #[test]
    fn reads_what_became_of_a_request_and_orders_the_list() {
        let gh = json(r#"[
            {"number":3,"title":"Merged long ago","state":"closed","merged_at":"2026-10-01T08:00:00Z","updated_at":"2026-11-23T08:00:00Z","created_at":"2026-09-30T08:00:00Z","head":{"ref":"f"},"base":{"ref":"main"}},
            {"number":9,"title":"Open","state":"open","merged_at":null,"updated_at":"2026-11-20T10:00:00Z","created_at":"2026-11-19T08:00:00Z","head":{"ref":"f"},"base":{"ref":"main"}},
            {"number":7,"title":"Merged lately","state":"closed","merged_at":"2026-11-10T08:00:00Z","updated_at":"2026-11-10T08:00:00Z","head":{"ref":"f"},"base":{"ref":"main"}},
            {"number":8,"title":"Closed without merging","state":"closed","merged_at":null,"updated_at":"2026-11-22T10:00:00Z","head":{"ref":"f"},"base":{"ref":"main"}}]"#);
        let mut list = requests_from(Kind::GitHub, &gh);
        sort_requests(&mut list);
        let seen: Vec<(&str, Fate)> = list.iter().map(|r| (r.number.as_str(), r.fate)).collect();
        // Not merged first (the latest change first), then the merged ones by the day of merging.
        assert_eq!(seen, vec![("8", Fate::Closed), ("9", Fate::Open), ("7", Fate::Merged), ("3", Fate::Merged)]);
        assert_eq!(list[3].when, chrono::DateTime::parse_from_rfc3339("2026-10-01T08:00:00Z").unwrap().timestamp());
        let gl = requests_from(Kind::GitLab, &json(r#"[{"iid":1,"state":"merged","merged_at":"2026-11-10T08:00:00.123Z","updated_at":"2026-11-12T08:00:00.000Z","source_branch":"f","target_branch":"main"},
            {"iid":2,"state":"opened","merged_at":null,"updated_at":"2026-11-12T08:00:00.000+03:00","source_branch":"f","target_branch":"main"},
            {"iid":3,"state":"closed","updated_at":"bad","created_at":"2026-11-01T00:00:00Z","source_branch":"f","target_branch":"main"},
            {"iid":4,"state":"locked","source_branch":"f","target_branch":"main"}]"#));
        assert_eq!(gl.iter().map(|r| r.fate).collect::<Vec<_>>(), vec![Fate::Merged, Fate::Open, Fate::Closed, Fate::Open]);
        assert_eq!(gl[0].when, chrono::DateTime::parse_from_rfc3339("2026-11-10T08:00:00Z").unwrap().timestamp());
        assert_eq!(gl[1].when, chrono::DateTime::parse_from_rfc3339("2026-11-12T05:00:00Z").unwrap().timestamp());
        assert!(gl[2].when > 0 && gl[3].when == 0);
        let bb = requests_from(Kind::Bitbucket, &json(r#"{"values":[{"id":1,"state":"MERGED","updated_on":"2026-11-10T08:00:00.123456+00:00","source":{"branch":{"name":"f"}},"destination":{"branch":{"name":"main"}}},
            {"id":2,"state":"DECLINED","source":{"branch":{"name":"f"}},"destination":{"branch":{"name":"main"}}},{"id":3,"state":"SUPERSEDED"},{"id":4,"state":"OPEN"}]}"#));
        assert_eq!(bb.iter().map(|r| r.fate).collect::<Vec<_>>(), vec![Fate::Merged, Fate::Closed, Fate::Closed, Fate::Open]);
        assert!(bb[0].when > 0);
    }

    #[test]
    fn asks_for_all_the_requests_of_a_branch() {
        let gh = site("git@github.com:octo/site.git");
        assert_eq!(branch_requests_url(&gh, "A", "feature/x y"), "A/pulls?state=all&head=octo%3Afeature%2Fx%20y&sort=updated&direction=desc&per_page=100");
        let gl = site("git@gitlab.com:g/sub/r.git");
        assert_eq!(branch_requests_url(&gl, "A", "feature/x"), "A/merge_requests?state=all&source_branch=feature%2Fx&order_by=updated_at&per_page=100");
        let bb = site("git@bitbucket.org:team/app.git");
        assert_eq!(branch_requests_url(&bb, "A", "fix\"1"), "A/pullrequests?state=OPEN&state=MERGED&state=DECLINED&state=SUPERSEDED&q=source.branch.name%3D%22fix%5C%221%22&sort=-updated_on&pagelen=50");
    }

    #[test]
    fn asks_for_the_requests_of_a_commit() {
        assert_eq!(commit_requests_url(&site("git@github.com:octo/site.git"), "A", "abc123"), "A/commits/abc123/pulls?per_page=30");
        assert_eq!(commit_requests_url(&site("git@gitlab.com:g/r.git"), "A", "abc123"), "A/repository/commits/abc123/merge_requests");
        assert_eq!(commit_requests_url(&site("git@bitbucket.org:team/app.git"), "A", "abc123"), "A/commit/abc123/pullrequests?pagelen=30");
        // Nothing is asked in a test, and a commit filled in by hand is given back.
        let s = Site { token: "t".into(), ..site("git@gitlab.com:g/r.git") };
        let mut cache = Cache::default();
        let ctx = eframe::egui::Context::default();
        assert!(cache.commit_requests(&s, "abc", &ctx).is_none());
        cache.put_commit(&s, "abc", vec![Request { number: "7".into(), fate: Fate::Merged, ..Default::default() }]);
        assert_eq!(cache.commit_requests(&s, "abc", &ctx).map(|l| l.len()), Some(1));
        assert!(cache.commit_requests(&s, "def", &ctx).is_none());
        cache.clear();
        assert!(cache.commit_requests(&s, "abc", &ctx).is_none());
    }

    #[test]
    fn puts_the_runs_of_a_commit_together() {
        let gh = site("git@github.com:octo/site.git");
        let runs = json(r#"{"total_count":5,"workflow_runs":[
            {"name":"CI","head_sha":"aaa","status":"completed","conclusion":"success","html_url":"u1"},
            {"name":"Release","head_sha":"aaa","status":"completed","conclusion":"failure","html_url":"u2"},
            {"name":"CI","head_sha":"aaa","status":"completed","conclusion":"failure","html_url":"old"},
            {"name":"CI","head_sha":"bbb","status":"in_progress","conclusion":null,"html_url":"u3"},
            {"name":"CI","head_sha":"ccc","status":"completed","conclusion":"skipped","html_url":"u4"},
            {"name":"CI","head_sha":"ddd","status":"completed","conclusion":"cancelled","html_url":"u5"}]}"#);
        let ci = ci_from(&gh, &runs);
        // The newer run of "CI" passed (the older one is a run before a retry); "Release" failed.
        assert_eq!(ci["aaa"], CiRun { state: Ci::Failed, url: "u2".into(), name: "CI, Release".into() });
        assert_eq!(ci["bbb"].state, Ci::Running);
        assert!(!ci.contains_key("ccc"));
        assert_eq!(ci["ddd"].state, Ci::Cancelled);
        let gl = site("git@gitlab.com:g/r.git");
        let ci = ci_from(&gl, &json(r#"[{"id":1,"sha":"aaa","status":"success","web_url":"p1","ref":"main"},{"id":2,"sha":"bbb","status":"failed","web_url":"p2"},{"id":3,"sha":"ccc","status":"pending","web_url":"p3"}]"#));
        assert_eq!((ci["aaa"].state, ci["bbb"].state, ci["ccc"].state), (Ci::Passed, Ci::Failed, Ci::Running));
        let bb = site("git@bitbucket.org:team/app.git");
        let ci = ci_from(&bb, &json(r#"{"values":[{"build_number":41,"target":{"commit":{"hash":"aaa"}},"state":{"name":"COMPLETED","result":{"name":"SUCCESSFUL"}}},
            {"build_number":40,"target":{"commit":{"hash":"bbb"}},"state":{"name":"IN_PROGRESS"}}]}"#));
        assert_eq!(ci["aaa"], CiRun { state: Ci::Passed, url: "https://bitbucket.org/team/app/pipelines/results/41".into(), name: String::new() });
        assert_eq!(ci["bbb"].state, Ci::Running);
    }

    #[test]
    fn writes_the_request_to_create_and_reads_refusals() {
        assert_eq!(create_body(Kind::GitHub, "Add \"x\"", "line 1\nline 2", "feature/x", "main"), r#"{"title":"Add \"x\"","body":"line 1\nline 2","head":"feature/x","base":"main"}"#);
        assert_eq!(create_body(Kind::GitLab, "T", "", "a", "b"), r#"{"title":"T","description":"","source_branch":"a","target_branch":"b"}"#);
        let bb = json(&create_body(Kind::Bitbucket, "T", "D", "a", "b"));
        assert_eq!((text(&bb, &["source", "branch", "name"]), text(&bb, &["destination", "branch", "name"]), text(&bb, &["description"])), ("a".into(), "b".into(), "D".into()));
        assert_eq!(api_error(Kind::GitHub, 422, r#"{"message":"Validation Failed","errors":[{"resource":"PullRequest","code":"custom","message":"A pull request already exists for octo:feature/x."}]}"#), "the server answered 422: Validation Failed; A pull request already exists for octo:feature/x.");
        assert_eq!(api_error(Kind::GitLab, 409, r#"{"message":["Another open merge request already exists for this source branch: !7"]}"#), "the server answered 409: Another open merge request already exists for this source branch: !7");
        assert_eq!(api_error(Kind::GitLab, 401, r#"{"message":"401 Unauthorized"}"#), "the token was not accepted (401): 401 Unauthorized");
        assert_eq!(api_error(Kind::Bitbucket, 400, r#"{"type":"error","error":{"message":"There are no changes to be pulled"}}"#), "the server answered 400: There are no changes to be pulled");
        assert_eq!(api_error(Kind::GitHub, 502, "<html>Bad gateway</html>"), "the server answered 502");
    }

    #[test]
    fn keeps_a_host_in_the_settings_line() {
        let a = Account { kind: Kind::Bitbucket, host: " bitbucket.org ".into(), token: "tok\ten".into(), email: "me@example.com".into(), ..Default::default() };
        let back = Account::from_line(&a.to_line()).unwrap();
        assert_eq!(back, Account { kind: Kind::Bitbucket, host: "bitbucket.org".into(), token: "tok en".into(), email: "me@example.com".into(), ..Default::default() });
        // No token and no e-mail: the empty fields at the end may be gone.
        let plain = Account { kind: Kind::GitLab, host: "code.example.com".into(), ..Default::default() };
        assert_eq!(Account::from_line(&plain.to_line()), Some(plain.clone()));
        assert_eq!(Account::from_line("gitlab\tcode.example.com"), Some(plain));
        assert_eq!(Account::from_line("svn\thost\t\t"), None);
        assert_eq!(Account::from_line("github\t\t\ttoken"), None);
        // A token in the store of the system never gets into the line, typed or read.
        use crate::secrets::Kept;
        let kept = Account { kind: Kind::GitHub, host: "github.com".into(), token: "secret".into(), kept: Kept::Store, entered: "typed".into(), ..Default::default() };
        assert_eq!(kept.to_line(), "github\tgithub.com\t\t\tstore");
        let moving = Account { kept: Kept::Moving, ..kept.clone() };
        assert_eq!(moving.to_line(), "github\tgithub.com\t\t\tstore");
        let back = Account::from_line(&kept.to_line()).unwrap();
        assert_eq!((back.kept, back.token.as_str(), back.entered.as_str(), back.has_token()), (Kept::Store, "", "", true));
        assert_eq!(kept.key(), "hosting:github:github.com");
        // An old build reads the same line as a host without a token.
        let line = kept.to_line();
        let old: Vec<&str> = line.split('\t').take(4).collect();
        assert_eq!(old, vec!["github", "github.com", "", ""]);
    }

    #[test]
    fn splits_a_remote_branch_name() {
        let remotes = vec!["origin".to_string(), "origin/fork".to_string(), "up".to_string()];
        assert_eq!(split_remote("origin/feature/x", &remotes), Some(("origin", "feature/x")));
        assert_eq!(split_remote("origin/fork/main", &remotes), Some(("origin/fork", "main")));
        assert_eq!(split_remote("feature/x", &remotes), None);
    }
}
