//! Looking for a newer version: the latest release of the program's repository on GitHub, read
//! without signing in, compared with the running version, and the installer among its files
//! that suits this computer. On RHEL and the like the package can be downloaded, checked
//! against the release's SHA256SUMS.txt and installed with dnf through pkexec (the system asks
//! for the password); elsewhere the file is opened in the browser.

use crate::jira::Json;

/// The repository the releases are published in.
pub const REPO: &str = "alexeylovchikov/refbraid";
pub const API: &str = "https://api.github.com";
/// Not more often than this when the program looks by itself.
pub const EVERY_SECS: i64 = 24 * 3600;

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Release {
    /// "0.4.5"
    pub version: String,
    /// The page of the release.
    pub url: String,
    /// The description of the release (Markdown).
    pub body: String,
    /// The files of the release: the name and where to download it from.
    pub assets: Vec<(String, String)>,
}

/// "v0.4.10" or "0.4.10" as numbers.
pub fn version_of(tag: &str) -> Option<[u32; 3]> {
    let t = tag.trim();
    let mut parts = t.strip_prefix('v').unwrap_or(t).split('.');
    let v = [parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?];
    parts.next().is_none().then_some(v)
}

/// Whether `latest` is a later version than `current` (false when either is not a version).
pub fn newer(latest: &str, current: &str) -> bool {
    matches!((version_of(latest), version_of(current)), (Some(a), Some(b)) if a > b)
}

/// Only addresses inside the program's own repository are opened.
fn ours(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{REPO}/"))
}

fn text(j: &Json, key: &str) -> String {
    j.get(key).map(|v| v.str().to_string()).unwrap_or_default()
}

/// The release in GitHub's answer; None when it names no version or leads somewhere else.
pub fn release_from(j: &Json) -> Option<Release> {
    let tag = text(j, "tag_name");
    version_of(&tag)?;
    let url = text(j, "html_url");
    if !ours(&url) || matches!(j.get("draft"), Some(Json::Bool(true))) || matches!(j.get("prerelease"), Some(Json::Bool(true))) {
        return None;
    }
    let assets = j
        .get("assets")
        .map(|a| a.items())
        .unwrap_or_default()
        .iter()
        .map(|a| (text(a, "name"), text(a, "browser_download_url")))
        .filter(|(name, url)| !name.is_empty() && ours(url))
        .collect();
    Some(Release { version: tag.trim().trim_start_matches('v').to_string(), url, body: text(j, "body"), assets })
}

/// Ask GitHub for the latest release. No token is sent: the repository is public.
pub fn fetch(api: &str) -> Result<Release, String> {
    let req = crate::http::Request {
        url: format!("{api}/repos/{REPO}/releases/latest"),
        headers: vec!["Accept: application/vnd.github+json".into(), "X-GitHub-Api-Version: 2022-11-28".into(), "User-Agent: RefBraid".into()],
        ..Default::default()
    };
    let r = crate::http::send(&req)?;
    match r.code {
        200 => {}
        403 | 429 => return Err("GitHub answers too many requests from this network for now; try later".into()),
        404 => return Err("no release is published yet".into()),
        c => return Err(format!("the server answered {c}")),
    }
    release_from(&crate::http::json_of(&r, "GitHub")?).ok_or_else(|| "the answer names no release of RefBraid".to_string())
}

/// How the running program was installed, as far as it can be told.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pack {
    /// An RPM of this major version of RHEL and the like.
    Rpm(u32),
    Deb,
    AppImage,
    Msi,
    Dmg,
    /// Built from the sources, unpacked from the zip, another system: the release page is shown.
    Unknown,
}

/// The Linux package this system takes, from its `/etc/os-release`.
pub fn linux_pack(os_release: &str) -> Pack {
    let field = |key: &str| {
        os_release
            .lines()
            .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
            .map(|v| v.trim().trim_matches('"').to_lowercase())
            .unwrap_or_default()
    };
    let (id, like, version) = (field("ID"), field("ID_LIKE"), field("VERSION_ID"));
    let family: Vec<&str> = std::iter::once(id.as_str()).chain(like.split_whitespace()).collect();
    if family.iter().any(|f| matches!(*f, "debian" | "ubuntu")) {
        return Pack::Deb;
    }
    // Fedora names "rhel" nowhere and has other version numbers; openSUSE has RPMs of its own.
    if family.iter().any(|f| matches!(*f, "rhel" | "centos")) {
        if let Some(major @ (9 | 10)) = version.split('.').next().and_then(|m| m.parse().ok()) {
            return Pack::Rpm(major);
        }
    }
    Pack::Unknown
}

/// How this program was installed.
pub fn pack() -> Pack {
    let exe = std::env::current_exe().unwrap_or_default();
    if cfg!(target_os = "macos") {
        // Inside an application bundle; a binary built by cargo is not.
        return if exe.to_string_lossy().contains(".app/Contents/MacOS/") { Pack::Dmg } else { Pack::Unknown };
    }
    if cfg!(windows) {
        // The installer puts it under Program Files; the zip is unpacked anywhere.
        let dirs: Vec<String> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(|v| std::env::var(v).ok()).map(|d| d.to_lowercase()).collect();
        let exe = exe.to_string_lossy().to_lowercase();
        return if cfg!(target_arch = "x86_64") && dirs.iter().any(|d| !d.is_empty() && exe.starts_with(d.as_str())) { Pack::Msi } else { Pack::Unknown };
    }
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Pack::Unknown;
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return Pack::AppImage;
    }
    // A package installs into /usr; anything else was built or copied by hand.
    if !exe.starts_with("/usr/bin") {
        return Pack::Unknown;
    }
    linux_pack(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
}

/// The list of checksums published with every release.
pub const SUMS: &str = "SHA256SUMS.txt";

/// The checksum of `name` in a SHA256SUMS.txt ("<hash>  <name>" or "<hash> *<name>").
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

/// The command that installs the downloaded package with the system's own tool, the password
/// asked by the system (pkexec). Only for the RPMs of RHEL and the like.
pub fn install_command(pack: Pack, file: &std::path::Path) -> Option<Vec<String>> {
    match pack {
        Pack::Rpm(_) => Some(vec!["pkexec".into(), "dnf".into(), "install".into(), "-y".into(), file.display().to_string()]),
        _ => None,
    }
}

/// A program on PATH.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// Whether the program can install an update by itself here.
pub fn can_install(pack: Pack) -> bool {
    matches!(pack, Pack::Rpm(_)) && on_path("pkexec") && on_path("dnf")
}

/// A step of installing an update, as the window shows it.
#[derive(Clone, PartialEq, Debug)]
pub enum Step {
    Downloading,
    Checking,
    /// pkexec is waiting for the password, then dnf works.
    Installing,
    Done,
    Failed(String),
}

/// A file name that is safe to put in a folder: what release files are called.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The folder the update is downloaded to: in the user's own cache, not in the shared /tmp.
fn download_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).filter(|p| p.is_absolute()).or_else(|| crate::git::home_dir().map(|h| h.join(".cache")))?;
    Some(base.join("refbraid").join("update"))
}

/// Download `url` into `dest` with curl (following GitHub's redirect to its file storage).
fn download(url: &str, dest: &std::path::Path) -> Result<(), String> {
    let mut c = std::process::Command::new("curl");
    c.args(["-fsSL", "--max-time", "900", "-o"]).arg(dest).arg(url);
    let out = c.output().map_err(|e| format!("could not run curl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("curl: ").to_string();
        Err(if err.is_empty() { "the download failed".into() } else { err })
    }
}

fn sha256_of(path: &std::path::Path) -> Result<String, String> {
    let out = std::process::Command::new("sha256sum").arg(path).output().map_err(|e| format!("could not run sha256sum: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().next().filter(|h| h.len() == 64).map(|h| h.to_ascii_lowercase()).ok_or_else(|| "sha256sum gave no checksum".to_string())
}

/// What pkexec's and dnf's way of ending means.
pub fn install_error(code: Option<i32>, stderr: &str) -> String {
    match code {
        // pkexec: the password window was closed, or the user may not do this.
        Some(126) => "the installation was cancelled".into(),
        Some(127) => "the system did not allow the installation".into(),
        _ => {
            let last: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            let tail = last[last.len().saturating_sub(3)..].join(" ");
            if tail.is_empty() { format!("dnf ended with code {}", code.unwrap_or(-1)) } else { tail }
        }
    }
}

/// Download the package of `r` for `pack` and check it against SHA256SUMS.txt: the file ready
/// to install.
pub fn prepare(r: &Release, pack: Pack, step: &dyn Fn(Step)) -> Result<std::path::PathBuf, String> {
    let (Some((name, url)), Some((_, sums_url))) = (r.asset_for(pack), r.assets.iter().find(|(n, _)| n == SUMS)) else {
        return Err("the release has no package or no checksums for this system".into());
    };
    if !plain_name(name) {
        return Err("the package has an unexpected name".into());
    }
    let dir = download_dir().ok_or("no folder to download to")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    step(Step::Downloading);
    let file = dir.join(name);
    let sums_file = dir.join(SUMS);
    let _ = std::fs::remove_file(&file);
    download(url, &file).and_then(|_| download(sums_url, &sums_file))?;
    step(Step::Checking);
    let sums = std::fs::read_to_string(&sums_file).unwrap_or_default();
    let want = sum_for(&sums, name).ok_or("SHA256SUMS.txt does not list the package")?;
    if sha256_of(&file)? != want {
        let _ = std::fs::remove_file(&file);
        return Err("the downloaded package does not match its checksum".into());
    }
    Ok(file)
}

/// Download the package, check it and install it with the system's tool. Every step is told
/// through `step`. Runs on a thread.
pub fn install(r: &Release, pack: Pack, step: &dyn Fn(Step)) {
    let file = match prepare(r, pack, step) {
        Ok(f) => f,
        Err(e) => return step(Step::Failed(e)),
    };
    let Some(cmd) = install_command(pack, &file) else { return step(Step::Failed("this system has no installer for it".into())) };
    step(Step::Installing);
    match std::process::Command::new(&cmd[0]).args(&cmd[1..]).output() {
        Ok(out) if out.status.success() => {
            let _ = std::fs::remove_file(&file);
            step(Step::Done)
        }
        Ok(out) => step(Step::Failed(install_error(out.status.code(), &String::from_utf8_lossy(&out.stderr)))),
        Err(e) => step(Step::Failed(format!("could not run pkexec: {e}"))),
    }
}

impl Release {
    /// The file of the release for a way of installing: found by how its name ends.
    pub fn asset_for(&self, pack: Pack) -> Option<&(String, String)> {
        let ending = match pack {
            Pack::Rpm(el) => format!(".el{el}.x86_64.rpm"),
            Pack::Deb => "_amd64.deb".into(),
            Pack::AppImage => "-x86_64.AppImage".into(),
            Pack::Msi => "-windows-x64.msi".into(),
            Pack::Dmg => "-macos-universal.dmg".into(),
            Pack::Unknown => return None,
        };
        self.assets.iter().find(|(name, _)| name.ends_with(&ending))
    }

    /// What is new, out of the description: the part after its "What's new" heading (the table
    /// of downloads before it is of no use in a window); all of it when there is no such heading.
    pub fn notes(&self) -> &str {
        let mut at = 0;
        for line in self.body.split_inclusive('\n') {
            at += line.len();
            let t = line.trim();
            if t.starts_with("## ") && t.to_lowercase().contains("what") && t.to_lowercase().contains("new") {
                return &self.body[at..];
            }
        }
        &self.body
    }
}

#[cfg(test)]
mod tests {
    /// Against the real releases on GitHub (run by hand: cargo test -- --ignored real_release).
    #[test]
    #[ignore]
    fn downloads_and_checks_a_real_release() {
        let r = fetch(API).unwrap();
        let f = prepare(&r, Pack::Rpm(10), &|s| eprintln!("{s:?}")).unwrap();
        assert!(f.is_file() && std::fs::metadata(&f).unwrap().len() > 1_000_000);
        let _ = std::fs::remove_file(f);
    }

    #[test]
    fn checks_the_package_and_installs_it_with_dnf() {
        let sums = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0  refbraid-0.6.1-1.el10.x86_64.rpm\nABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789 *refbraid_0.6.1-1_amd64.deb\n";
        assert_eq!(sum_for(sums, "refbraid-0.6.1-1.el10.x86_64.rpm").as_deref(), Some("0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0"));
        assert_eq!(sum_for(sums, "refbraid_0.6.1-1_amd64.deb").as_deref(), Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"));
        assert_eq!(sum_for(sums, "refbraid-0.6.1-1.el9.x86_64.rpm"), None);
        assert_eq!(sum_for("short  a.rpm", "a.rpm"), None);
        let f = std::path::Path::new("/home/u/.cache/refbraid/update/refbraid-0.6.1-1.el10.x86_64.rpm");
        assert_eq!(install_command(Pack::Rpm(10), f).unwrap(), vec!["pkexec", "dnf", "install", "-y", "/home/u/.cache/refbraid/update/refbraid-0.6.1-1.el10.x86_64.rpm"]);
        assert_eq!(install_command(Pack::Deb, f), None);
        assert_eq!(install_command(Pack::Msi, f), None);
        assert!(!can_install(Pack::Deb) && !can_install(Pack::Unknown));
        assert!(plain_name("refbraid-0.6.1-1.el10.x86_64.rpm") && !plain_name("../x.rpm") && !plain_name(".rpm") && !plain_name("a b.rpm"));
        assert_eq!(install_error(Some(126), ""), "the installation was cancelled");
        assert_eq!(install_error(Some(1), "Error:\n Problem: conflicting requests\n  - nothing provides libX\n"), "Error: Problem: conflicting requests - nothing provides libX");
        assert_eq!(install_error(Some(1), ""), "dnf ended with code 1");
    }

    use super::*;

    #[test]
    fn compares_versions_as_numbers() {
        assert_eq!(version_of("v0.4.10"), Some([0, 4, 10]));
        assert_eq!(version_of(" 1.2.3\n"), Some([1, 2, 3]));
        for bad in ["", "v1.2", "1.2.3.4", "1.2.x", "nightly", "v1.2.3-rc1"] {
            assert_eq!(version_of(bad), None, "{bad}");
        }
        assert!(newer("v0.4.10", "0.4.9"));
        assert!(newer("0.5.0", "0.4.99"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.4.4", "0.4.4"));
        assert!(!newer("0.4.3", "0.4.4"));
        assert!(!newer("latest", "0.4.4"));
    }

    const ANSWER: &str = r###"{"url":"https://api.github.com/repos/alexeylovchikov/refbraid/releases/1","html_url":"https://github.com/alexeylovchikov/refbraid/releases/tag/v0.4.5",
        "tag_name":"v0.4.5","name":"RefBraid 0.4.5","draft":false,"prerelease":false,
        "assets":[
          {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/alexeylovchikov/refbraid/releases/download/v0.4.5/SHA256SUMS.txt"},
          {"name":"refbraid-0.4.5-1.el10.x86_64.rpm","browser_download_url":"https://github.com/alexeylovchikov/refbraid/releases/download/v0.4.5/refbraid-0.4.5-1.el10.x86_64.rpm"},
          {"name":"refbraid-0.4.5-1.el9.x86_64.rpm","browser_download_url":"https://github.com/alexeylovchikov/refbraid/releases/download/v0.4.5/refbraid-0.4.5-1.el9.x86_64.rpm"},
          {"name":"refbraid-0.4.5-macos-universal.dmg","browser_download_url":"https://github.com/alexeylovchikov/refbraid/releases/download/v0.4.5/refbraid-0.4.5-macos-universal.dmg"},
          {"name":"refbraid-0.4.5-windows-x64.msi","browser_download_url":"https://github.com/alexeylovchikov/refbraid/releases/download/v0.4.5/refbraid-0.4.5-windows-x64.msi"},
          {"name":"refbraid-0.4.5-windows-x64.zip","browser_download_url":"https://github.com/alexeylovchikov/refbraid/releases/download/v0.4.5/refbraid-0.4.5-windows-x64.zip"},
          {"name":"refbraid-0.4.5-x86_64.AppImage","browser_download_url":"https://github.com/alexeylovchikov/refbraid/releases/download/v0.4.5/refbraid-0.4.5-x86_64.AppImage"},
          {"name":"refbraid_0.4.5-1_amd64.deb","browser_download_url":"https://github.com/alexeylovchikov/refbraid/releases/download/v0.4.5/refbraid_0.4.5-1_amd64.deb"},
          {"name":"elsewhere.rpm","browser_download_url":"https://example.com/elsewhere.el9.x86_64.rpm"}],
        "body":"## Downloads\n\n| File | For |\n|---|---|\n| `a.msi` | Windows |\n\n## What's new\n\n### New\n- **Hostings**: pull requests and CI.\n\n### Fixed\n- A thing.\n"}"###;

    #[test]
    fn reads_a_release_and_picks_the_file() {
        let r = release_from(&crate::jira::parse(ANSWER).unwrap()).unwrap();
        assert_eq!((r.version.as_str(), r.url.as_str(), r.assets.len()), ("0.4.5", "https://github.com/alexeylovchikov/refbraid/releases/tag/v0.4.5", 8));
        let name = |p: Pack| r.asset_for(p).map(|a| a.0.as_str());
        assert_eq!(name(Pack::Rpm(10)), Some("refbraid-0.4.5-1.el10.x86_64.rpm"));
        assert_eq!(name(Pack::Rpm(9)), Some("refbraid-0.4.5-1.el9.x86_64.rpm"));
        assert_eq!(name(Pack::Rpm(8)), None);
        assert_eq!(name(Pack::Deb), Some("refbraid_0.4.5-1_amd64.deb"));
        assert_eq!(name(Pack::AppImage), Some("refbraid-0.4.5-x86_64.AppImage"));
        assert_eq!(name(Pack::Msi), Some("refbraid-0.4.5-windows-x64.msi"));
        assert_eq!(name(Pack::Dmg), Some("refbraid-0.4.5-macos-universal.dmg"));
        assert_eq!(name(Pack::Unknown), None);
        assert!(r.asset_for(Pack::Deb).unwrap().1.starts_with("https://github.com/alexeylovchikov/refbraid/releases/download/"));
        assert_eq!(r.notes(), "\n### New\n- **Hostings**: pull requests and CI.\n\n### Fixed\n- A thing.\n");
        assert_eq!(Release { body: "Just text.".into(), ..Default::default() }.notes(), "Just text.");
    }

    #[test]
    fn refuses_what_is_not_a_release_of_the_program() {
        let j = |s: &str| crate::jira::parse(s).unwrap();
        // Somebody else's page, a draft, a pre-release, a tag that is not a version.
        assert_eq!(release_from(&j(r#"{"tag_name":"v9.9.9","html_url":"https://github.com/someone/else/releases/tag/v9.9.9"}"#)), None);
        assert_eq!(release_from(&j(r#"{"tag_name":"v9.9.9","draft":true,"html_url":"https://github.com/alexeylovchikov/refbraid/releases/tag/v9.9.9"}"#)), None);
        assert_eq!(release_from(&j(r#"{"tag_name":"v9.9.9","prerelease":true,"html_url":"https://github.com/alexeylovchikov/refbraid/releases/tag/v9.9.9"}"#)), None);
        assert_eq!(release_from(&j(r#"{"tag_name":"nightly","html_url":"https://github.com/alexeylovchikov/refbraid/releases/tag/nightly"}"#)), None);
        assert_eq!(release_from(&j(r#"{"message":"Not Found"}"#)), None);
    }

    #[test]
    fn knows_the_package_of_a_linux_system() {
        assert_eq!(linux_pack("NAME=\"AlmaLinux\"\nVERSION_ID=\"10.1\"\nID=\"almalinux\"\nID_LIKE=\"rhel centos fedora\"\n"), Pack::Rpm(10));
        assert_eq!(linux_pack("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.5\"\n"), Pack::Rpm(9));
        assert_eq!(linux_pack("ID=rhel\nVERSION_ID=8.10\n"), Pack::Unknown);
        assert_eq!(linux_pack("ID=fedora\nVERSION_ID=43\n"), Pack::Unknown);
        assert_eq!(linux_pack("ID=ubuntu\nID_LIKE=debian\nVERSION_ID=\"24.04\"\n"), Pack::Deb);
        assert_eq!(linux_pack("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"), Pack::Deb);
        assert_eq!(linux_pack("ID=debian\nVERSION_ID=\"13\"\n"), Pack::Deb);
        assert_eq!(linux_pack("ID=\"opensuse-leap\"\nID_LIKE=\"suse opensuse\"\n"), Pack::Unknown);
        assert_eq!(linux_pack(""), Pack::Unknown);
    }

    #[test]
    fn asks_the_server_for_the_latest_release() {
        use std::io::{Read, Write};
        if std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" }).arg("--version").output().is_err() {
            return;
        }
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", l.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            let body = ANSWER.replace('\n', " ");
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
            String::from_utf8_lossy(&got).to_string()
        });
        let r = fetch(&api).unwrap();
        assert_eq!(r.version, "0.4.5");
        let got = server.join().unwrap();
        assert!(got.starts_with("GET /repos/alexeylovchikov/refbraid/releases/latest HTTP/1.1\r\n"), "{got}");
        // Asked without signing in.
        assert!(!got.to_lowercase().contains("authorization:"), "{got}");
    }
}
