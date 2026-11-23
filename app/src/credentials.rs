//! Signing in to servers: what git's sign-in errors mean and what to do about them, the
//! credential helper, the SSH keys of this computer, and the question ssh asks about a
//! server it sees for the first time.

use std::path::{Path, PathBuf};

/// A failed fetch, pull, push or clone that is about signing in or reaching the server: a
/// short name of the trouble and what to do, in plain words. None for any other failure.
pub fn auth_problem(output: &str) -> Option<(&'static str, &'static str)> {
    let has = |words: &[&str]| words.iter().any(|w| output.contains(w));
    if has(&["REMOTE HOST IDENTIFICATION HAS CHANGED"]) {
        return Some(("The server's key has changed", "The key the server shows is not the one remembered for it in the file known_hosts of your .ssh folder. If the server was reinstalled or moved, remove the old line (ssh-keygen -R <server name>) and connect again. If you don't know why it changed, don't connect: someone may be standing between you and the server."));
    }
    if has(&["Host key verification failed"]) {
        return Some(("The server's key was not accepted", "This computer has not connected to the server before, and the question whether to trust its key was not answered with yes. Run the operation again and confirm the key in the window that appears (compare the fingerprint with the one your administrator or the hosting shows)."));
    }
    if has(&["Permission denied (publickey", "Permission denied, please try again"]) {
        return Some(("The server did not accept your SSH key", "Add your public key to your account on the server: Settings → Credentials lists the keys of this computer and copies one. If the key is there already, check that the address of the remote is the right one and that the key's passphrase was typed correctly."));
    }
    if has(&["Authentication failed for", "Invalid username or password", "HTTP Basic: Access denied", "invalid credentials", "Logon failed"]) {
        return Some(("The user name or password was not accepted", "Many servers (GitHub, GitLab, Bitbucket) take a personal access token in place of the account's password — create one on the server and use it as the password. If a wrong password was saved, the credential helper keeps offering it: see Settings → Credentials."));
    }
    if has(&["could not read Username", "could not read Password", "terminal prompts disabled"]) {
        return Some(("Git got no user name or password", "The window that asks for them was closed, or could not be shown. Run the operation again and fill it in. To be asked only once, choose a credential helper in Settings → Credentials."));
    }
    if has(&["SSL certificate problem", "server certificate verification failed", "unable to get local issuer certificate"]) {
        return Some(("The server's certificate is not trusted", "The server's HTTPS certificate is signed by an authority this computer does not know — usual for a company's own server. Ask the administrator for the authority's certificate and point git at it (Git → Git Config: http.sslCAInfo). Turning the check off is unsafe."));
    }
    if has(&["Could not resolve host", "Could not resolve hostname", "Connection timed out", "Network is unreachable", "Connection refused", "No route to host", "Failed to connect to"]) {
        return Some(("The server cannot be reached", "Check the network and the VPN, and the address of the remote (the branch tree's Remote section, or Git → Git Config). The server itself may be down."));
    }
    if has(&["Repository not found", "does not appear to be a git repository", "returned error: 404", "project you were looking for could not be found"]) {
        return Some(("The repository was not found on the server", "Either the address of the remote is wrong, or your account has no access to the repository — servers answer \"not found\" in both cases. Check the address and that you are signed in as the right user."));
    }
    if has(&["returned error: 403", "You are not allowed to push", "protected branch", "Permission to ", "insufficient permission", "not allowed to upload"]) {
        return Some(("The server refused: no permission", "You are signed in, but may not do this: the branch is protected, or your role in the project is read-only. Push to another branch and open a merge request, or ask for the permission."));
    }
    None
}

/// What ssh asks when it meets a server for the first time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostKey {
    pub host: String,
    /// "ED25519", "RSA", "ECDSA"…
    pub kind: String,
    pub fingerprint: String,
}

/// Read ssh's question "The authenticity of host … can't be established. … key fingerprint
/// is …. Are you sure you want to continue connecting (yes/no/[fingerprint])?".
pub fn host_key_question(prompt: &str) -> Option<HostKey> {
    if !prompt.contains("continue connecting") {
        return None;
    }
    let host = prompt.split("authenticity of host '").nth(1).and_then(|s| s.split('\'').next()).unwrap_or("").to_string();
    let line = prompt.lines().find(|l| l.contains("key fingerprint is"))?;
    let kind = line.split_whitespace().next().unwrap_or("").to_string();
    let fingerprint = line.split("fingerprint is").nth(1)?.trim().trim_end_matches('.').to_string();
    Some(HostKey { host, kind, fingerprint })
}

/// The folder ssh keeps its keys in.
pub fn ssh_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".ssh"))
}

/// An SSH key of this computer: its public half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshKey {
    /// The file of the public key ("…/id_ed25519.pub").
    pub file: PathBuf,
    /// "ssh-ed25519", "ssh-rsa"…
    pub kind: String,
    pub comment: String,
    /// The whole line, as servers want it pasted.
    pub public: String,
}

/// Read a public key file's line: "<kind> <key> [comment]".
pub fn parse_public(file: &Path, text: &str) -> Option<SshKey> {
    let line = text.lines().find(|l| !l.trim().is_empty() && !l.starts_with('#'))?.trim();
    let mut parts = line.splitn(3, ' ');
    let kind = parts.next()?.to_string();
    let key = parts.next()?;
    if !(kind.starts_with("ssh-") || kind.starts_with("ecdsa-") || kind.starts_with("sk-")) || key.len() < 16 {
        return None;
    }
    Some(SshKey { file: file.to_path_buf(), kind, comment: parts.next().unwrap_or("").trim().to_string(), public: line.to_string() })
}

/// The public keys in a folder, by file name.
pub fn list_keys(dir: &Path) -> Vec<SshKey> {
    let mut v: Vec<SshKey> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pub"))
        .filter_map(|p| parse_public(&p, &std::fs::read_to_string(&p).ok()?))
        .collect();
    v.sort_by(|a, b| a.file.cmp(&b.file));
    v
}

/// A name for a new key that is not taken: "id_ed25519", then "id_ed25519_2"…
pub fn free_key_name(dir: &Path) -> String {
    let taken = |n: &str| dir.join(n).exists() || dir.join(format!("{n}.pub")).exists();
    if !taken("id_ed25519") {
        return "id_ed25519".into();
    }
    (2..).map(|k| format!("id_ed25519_{k}")).find(|n| !taken(n)).unwrap_or_default()
}

/// A key's file name is a plain name: nothing that leads out of the folder.
pub fn good_key_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') && !name.starts_with('.') && !name.ends_with(".pub")
}

/// `ssh-keygen` for a new Ed25519 key without a passphrase.
pub fn keygen_args(file: &Path, comment: &str) -> Vec<String> {
    vec!["-q".into(), "-t".into(), "ed25519".into(), "-N".into(), String::new(), "-C".into(), comment.to_string(), "-f".into(), file.display().to_string()]
}

/// Make a new key in `dir` (created when missing) and give back its public half.
pub fn generate_key(dir: &Path, name: &str, comment: &str) -> Result<SshKey, String> {
    if !good_key_name(name) {
        return Err("The name may have only letters, digits, '-', '_' and '.'.".into());
    }
    let file = dir.join(name);
    if file.exists() || dir.join(format!("{name}.pub")).exists() {
        return Err(format!("{} already exists.", file.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let mut c = std::process::Command::new("ssh-keygen");
    crate::git::no_console(&mut c);
    let out = c.args(keygen_args(&file, comment)).stdin(std::process::Stdio::null()).output().map_err(|e| format!("Could not start ssh-keygen: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let public = dir.join(format!("{name}.pub"));
    std::fs::read_to_string(&public).ok().and_then(|t| parse_public(&public, &t)).ok_or_else(|| "The key was made, but its public half could not be read.".to_string())
}

/// The credential helpers to choose from: (what git is told, how it is called here, what it does).
pub const HELPERS: [(&str, &str, &str); 5] = [
    ("", "None", "Git asks for the password every time"),
    ("manager", "Git Credential Manager", "Keeps passwords and tokens in the system's protected store; comes with Git for Windows"),
    ("libsecret", "Keyring (libsecret)", "The desktop's keyring on Linux; needs git's libsecret helper installed"),
    ("cache", "Memory, for 15 minutes", "Nothing is written to disk; after a while git asks again"),
    ("store", "A plain file", "Passwords are written unprotected to .git-credentials in your home folder"),
];

/// The helper set for all repositories of this user ("" when none).
pub fn helper(dir: &Path) -> String {
    crate::git::run(dir, &["config", "--global", "--get", "credential.helper"]).stdout.trim().to_string()
}

/// The command that sets the helper for all repositories of this user ("" removes it).
pub fn set_helper_args(value: &str) -> Vec<String> {
    if value.is_empty() {
        vec!["config".into(), "--global".into(), "--unset-all".into(), "credential.helper".into()]
    } else {
        vec!["config".into(), "--global".into(), "credential.helper".into(), value.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_sign_in_troubles_apart() {
        let name = |out: &str| auth_problem(out).map(|p| p.0);
        assert_eq!(name("git@gitlab.example.com: Permission denied (publickey).\nfatal: Could not read from remote repository."), Some("The server did not accept your SSH key"));
        assert_eq!(name("Host key verification failed.\nfatal: Could not read from remote repository."), Some("The server's key was not accepted"));
        assert_eq!(name("@@@@@\n@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n@@@@@\nHost key verification failed."), Some("The server's key has changed"));
        assert_eq!(name("remote: HTTP Basic: Access denied\nfatal: Authentication failed for 'https://gitlab.example.com/a/b.git/'"), Some("The user name or password was not accepted"));
        assert_eq!(name("fatal: could not read Username for 'https://github.com': terminal prompts disabled"), Some("Git got no user name or password"));
        assert_eq!(name("fatal: unable to access 'https://x/': Could not resolve host: x"), Some("The server cannot be reached"));
        assert_eq!(name("ssh: connect to host example.com port 22: Connection timed out"), Some("The server cannot be reached"));
        assert_eq!(name("remote: Repository not found.\nfatal: repository 'https://github.com/a/b.git/' not found"), Some("The repository was not found on the server"));
        assert_eq!(name("remote: GitLab: You are not allowed to push code to protected branches on this project."), Some("The server refused: no permission"));
        assert_eq!(name("fatal: unable to access 'https://x/': SSL certificate problem: unable to get local issuer certificate"), Some("The server's certificate is not trusted"));
        // Other failures are not about signing in.
        assert_eq!(name("error: failed to push some refs to 'origin'\nhint: Updates were rejected because the remote contains work that you do not have"), None);
        assert_eq!(name("CONFLICT (content): Merge conflict in a.txt"), None);
    }

    #[test]
    fn reads_the_question_about_a_new_server() {
        let q = "The authenticity of host 'gitlab.example.com (203.0.113.7)' can't be established.\nED25519 key fingerprint is SHA256:abcDEF123+/xyz.\nThis key is not known by any other names.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ";
        assert_eq!(host_key_question(q), Some(HostKey { host: "gitlab.example.com (203.0.113.7)".into(), kind: "ED25519".into(), fingerprint: "SHA256:abcDEF123+/xyz".into() }));
        assert_eq!(host_key_question("Enter passphrase for key '/home/me/.ssh/id_ed25519': "), None);
        assert_eq!(host_key_question("Password for 'https://me@example.com': "), None);
    }

    #[test]
    fn lists_and_makes_keys() {
        let dir = std::env::temp_dir().join(format!("ge-ssh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("id_rsa.pub"), "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC me@old laptop\n").unwrap();
        std::fs::write(dir.join("notes.pub"), "this is not a key\n").unwrap();
        std::fs::write(dir.join("known_hosts"), "example.com ssh-ed25519 AAAA\n").unwrap();
        let keys = list_keys(&dir);
        assert_eq!(keys.len(), 1);
        assert_eq!((keys[0].kind.as_str(), keys[0].comment.as_str(), keys[0].public.as_str()), ("ssh-rsa", "me@old laptop", "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC me@old laptop"));
        assert_eq!(free_key_name(&dir), "id_ed25519");
        assert!(good_key_name("id_ed25519_work") && !good_key_name("../x") && !good_key_name("") && !good_key_name("a b") && !good_key_name("key.pub") && !good_key_name(".hidden"));
        assert_eq!(keygen_args(Path::new("/h/.ssh/id_ed25519"), "me@example.com"), ["-q", "-t", "ed25519", "-N", "", "-C", "me@example.com", "-f", "/h/.ssh/id_ed25519"]);
        assert!(generate_key(&dir, "../outside", "x").is_err());
        // With ssh-keygen on this machine a key is really made, and a second one gets another name.
        if std::process::Command::new("ssh-keygen").arg("-?").output().is_ok() {
            let k = generate_key(&dir, "id_ed25519", "tester@example.com").unwrap();
            assert_eq!((k.kind.as_str(), k.comment.as_str()), ("ssh-ed25519", "tester@example.com"));
            assert!(dir.join("id_ed25519").is_file() && generate_key(&dir, "id_ed25519", "again").is_err());
            assert_eq!((list_keys(&dir).len(), free_key_name(&dir)), (2, "id_ed25519_2".to_string()));
        }
        assert_eq!(set_helper_args("store"), ["config", "--global", "credential.helper", "store"]);
        assert_eq!(set_helper_args(""), ["config", "--global", "--unset-all", "credential.helper"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
