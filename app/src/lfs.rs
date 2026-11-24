//! Git LFS: big files are kept outside the repository, and git itself holds a small text
//! "pointer" in their place. This module tells a pointer from a file, finds the real content
//! among the downloaded objects (without the git-lfs program) and builds the commands that
//! do need it (pull, fetch, track).

use crate::git::{self, DiffRow, DiffSide, DiffText, ImagePair};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// What git holds in place of a file kept in LFS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pointer {
    /// SHA-256 of the content, 64 hexadecimal digits.
    pub oid: String,
    pub size: u64,
}

/// Pointers are tiny; anything longer is a file.
const POINTER_MAX: usize = 1024;

/// The most that is read from an LFS object for a diff or a picture.
pub const MOST: u64 = 64 * 1024 * 1024;

/// Read a pointer file; None when `bytes` is anything else.
pub fn parse_pointer(bytes: &[u8]) -> Option<Pointer> {
    if bytes.len() > POINTER_MAX {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    let version = lines.next()?.strip_prefix("version ")?;
    if !(version.starts_with("https://git-lfs.github.com/spec/") || version.starts_with("https://hawser.github.com/spec/")) {
        return None;
    }
    let (mut oid, mut size) = (None, None);
    for l in lines {
        if let Some(o) = l.strip_prefix("oid sha256:") {
            oid = Some(o.trim().to_string());
        } else if let Some(s) = l.strip_prefix("size ") {
            size = s.trim().parse::<u64>().ok();
        }
    }
    let oid = oid.filter(|o| o.len() == 64 && o.chars().all(|c| c.is_ascii_hexdigit()))?;
    Some(Pointer { oid, size: size? })
}

/// Where the repository keeps its downloaded LFS objects.
pub fn objects_dir(dir: &Path) -> PathBuf {
    let d = git::run(dir, &["rev-parse", "--git-common-dir"]).stdout.trim().to_string();
    let common = if Path::new(&d).is_absolute() { PathBuf::from(d) } else { dir.join(d) };
    common.join("lfs").join("objects")
}

fn object_path(objects: &Path, oid: &str) -> PathBuf {
    objects.join(&oid[..2]).join(&oid[2..4]).join(oid)
}

/// The content behind a pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Data(Vec<u8>),
    /// The object was never downloaded to this computer.
    Missing,
    /// Bigger than [`MOST`]: not read.
    TooBig,
}

pub fn load(dir: &Path, p: &Pointer) -> Content {
    if p.size > MOST {
        return Content::TooBig;
    }
    match std::fs::read(object_path(&objects_dir(dir), &p.oid)) {
        Ok(data) => Content::Data(data),
        Err(_) => Content::Missing,
    }
}

/// Where one side of a diff comes from.
pub enum Side<'a> {
    /// The file is not there on this side (added or deleted).
    Nothing,
    /// "rev:path" in the repository.
    Rev(&'a str, &'a str),
    /// The file in the working folder.
    File(&'a str),
}

/// One side of a diff of an LFS file, followed to its content.
enum Got {
    Nothing,
    Data(Vec<u8>),
    Missing(Pointer),
    TooBig(Pointer),
}

fn get(dir: &Path, side: &Side) -> Got {
    let bytes = match side {
        Side::Nothing => return Got::Nothing,
        Side::Rev(rev, path) => git::blob(dir, rev, path),
        Side::File(path) => {
            let p = dir.join(path);
            match std::fs::metadata(&p) {
                // Not read whole when it is huge: said in words instead.
                Ok(m) if m.len() > MOST => return Got::TooBig(Pointer { oid: String::new(), size: m.len() }),
                _ => std::fs::read(p).ok(),
            }
        }
    };
    let Some(bytes) = bytes else { return Got::Nothing };
    match parse_pointer(&bytes) {
        None => Got::Data(bytes),
        Some(p) => match load(dir, &p) {
            Content::Data(d) => Got::Data(d),
            Content::Missing => Got::Missing(p),
            Content::TooBig => Got::TooBig(p),
        },
    }
}

/// Whether the text of a diff is that of LFS pointers: every line of one of its sides, put
/// together, reads as a pointer.
pub fn is_pointer_diff(d: &DiffText) -> bool {
    let side = |skip: char| -> Vec<u8> {
        let mut s = String::new();
        for (tag, line) in d.unified.iter().filter(|(t, _)| matches!(t, '+' | '-' | ' ') && *t != skip) {
            let _ = tag;
            s.push_str(line);
            s.push('\n');
        }
        s.into_bytes()
    };
    !d.unified.is_empty() && (parse_pointer(&side('+')).is_some() || parse_pointer(&side('-')).is_some())
}

/// A size in words: "3.4 MB".
pub fn human(size: u64) -> String {
    let s = size as f64;
    if size < 1024 {
        format!("{size} B")
    } else if s < 1024.0 * 1024.0 {
        format!("{:.1} KB", s / 1024.0)
    } else if s < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", s / 1024.0 / 1024.0)
    } else {
        format!("{:.1} GB", s / 1024.0 / 1024.0 / 1024.0)
    }
}

fn is_text(b: &[u8]) -> bool {
    !b.contains(&0) && b.len() <= git_text_limit()
}

fn git_text_limit() -> usize {
    8 * 1024 * 1024
}

/// Rows that say something in place of a diff.
fn notice(lines: &[String]) -> DiffText {
    let mut d = DiffText::default();
    for (i, l) in lines.iter().enumerate() {
        d.unified.push((' ', l.clone()));
        d.rows.push(DiffRow { left: Some((i + 1, l.clone())), right: Some((i + 1, l.clone())), kind: DiffSide::Context });
    }
    d
}

/// If `d` is a diff of LFS pointers, what to show in its place: the diff of the real content
/// (text), the two pictures, or a few words when the content is not on this computer.
pub fn replace_diff(dir: &Path, path: &str, d: &DiffText, old: Side, new: Side) -> Option<DiffText> {
    use crate::i18n::{tr, trf};
    if !is_pointer_diff(d) {
        return None;
    }
    let (a, b) = (get(dir, &old), get(dir, &new));
    let mut says: Vec<String> = vec![];
    for (which, got) in [("before", &a), ("after", &b)] {
        match got {
            Got::Missing(p) => says.push(trf(if which == "before" { "The version before ({}) is kept in Git LFS and is not downloaded to this computer." } else { "The version after ({}) is kept in Git LFS and is not downloaded to this computer." }, &[&human(p.size)])),
            Got::TooBig(p) => says.push(trf(if which == "before" { "The version before is {}: too big to show." } else { "The version after is {}: too big to show." }, &[&human(p.size)])),
            _ => {}
        }
    }
    if !says.is_empty() {
        if says.iter().any(|_| matches!(a, Got::Missing(_)) || matches!(b, Got::Missing(_))) {
            says.push(String::new());
            says.push(tr("Git → LFS → Pull downloads the files of the current commit; Fetch, those of other branches too.").to_string());
        }
        return Some(notice(&says));
    }
    let data = |g: Got| match g {
        Got::Data(d) => Some(d),
        _ => None,
    };
    let (a, b) = (data(a), data(b));
    if git::is_image_path(path) {
        let mut out = DiffText::default();
        out.images = Some(ImagePair { path: path.to_string(), before: a.map(std::sync::Arc::new), after: b.map(std::sync::Arc::new) });
        return Some(out);
    }
    if a.as_deref().is_some_and(|x| !is_text(x)) || b.as_deref().is_some_and(|x| !is_text(x)) {
        return Some(DiffText { binary: true, ..Default::default() });
    }
    // The two texts side by side, compared by git itself.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let tmp = std::env::temp_dir().join(format!("refbraid-lfs-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&tmp).ok()?;
    let _ = std::fs::write(tmp.join("before"), a.unwrap_or_default());
    let _ = std::fs::write(tmp.join("after"), b.unwrap_or_default());
    let mut args = vec!["diff", "--no-index", "--no-ext-diff", "--unified=100000"];
    args.extend(git::ws_flag(git::diff_ws()));
    args.extend(["--", "before", "after"]);
    let out = git::run(&tmp, &args);
    let _ = std::fs::remove_dir_all(&tmp);
    Some(git::parse_diff(&out.stdout))
}

/// Whether the repository uses LFS at all: nothing below is worth running otherwise.
pub fn in_use(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(".gitattributes")).is_ok_and(|t| t.contains("filter=lfs")) || objects_dir(dir).parent().is_some_and(|p| p.is_dir())
}

/// Read `git check-attr -z filter -- <paths>` (path, attribute, value, each ended by a zero
/// byte): the paths whose filter is lfs.
pub fn parse_attr(text: &str) -> HashSet<String> {
    let f: Vec<&str> = text.split('\0').collect();
    f.chunks(3).filter(|c| c.len() == 3 && c[2] == "lfs").map(|c| c[0].to_string()).collect()
}

/// Which of `paths` are kept in LFS, by the repository's `.gitattributes`.
pub fn tracked(dir: &Path, paths: &[String]) -> HashSet<String> {
    let mut out = HashSet::new();
    // In pieces: a command line has a limit (short on Windows).
    for chunk in paths.chunks(100) {
        let mut args = vec!["check-attr", "-z", "filter", "--"];
        args.extend(chunk.iter().map(|p| p.as_str()));
        out.extend(parse_attr(&git::run(dir, &args).stdout));
    }
    out
}

/// The patterns `.gitattributes` in the root of the repository sends to LFS.
pub fn parse_patterns(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let mut f = l.split_whitespace();
            let pat = f.next()?;
            f.any(|a| a == "filter=lfs").then(|| pat.replace("[[:space:]]", " "))
        })
        .collect()
}

pub fn patterns(dir: &Path) -> Vec<String> {
    parse_patterns(&std::fs::read_to_string(dir.join(".gitattributes")).unwrap_or_default())
}

/// What `Track with LFS` offers for a file: every file of its type, and the file itself.
pub fn track_choices(path: &str) -> Vec<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut v = vec![];
    if let Some((stem, ext)) = name.rsplit_once('.') {
        if !stem.is_empty() && !ext.is_empty() {
            v.push(format!("*.{ext}"));
        }
    }
    v.push(path.to_string());
    v
}

/// Is the git-lfs program there? (Asked again each time until it is: it may get installed.)
pub fn installed() -> bool {
    static YES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if YES.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    let ok = git::run(&std::env::temp_dir(), &["lfs", "version"]).ok;
    YES.store(ok, std::sync::atomic::Ordering::Relaxed);
    ok
}

/// What to say when git-lfs is needed and is not there.
pub fn how_to_install() -> String {
    crate::i18n::tr("Git LFS is not installed on this computer, so files kept in LFS cannot be downloaded or added.\n\nInstall it (the package is called git-lfs; Git for Windows has it as an option of its installer), then run this once in a terminal:\n\n    git lfs install").to_string()
}

fn cmd(args: &[&str]) -> Vec<Vec<String>> {
    vec![args.iter().map(|s| s.to_string()).collect()]
}

/// Download the LFS files of the current commit and put them in place.
pub fn pull_args() -> Vec<Vec<String>> {
    cmd(&["lfs", "pull"])
}

/// Download LFS files without touching the working folder; `all`: of every branch.
pub fn fetch_args(all: bool) -> Vec<Vec<String>> {
    if all { cmd(&["lfs", "fetch", "--all"]) } else { cmd(&["lfs", "fetch"]) }
}

/// Keep the files matching `pattern` in LFS from now on (writes `.gitattributes`).
pub fn track_args(pattern: &str) -> Vec<Vec<String>> {
    cmd(&["lfs", "track", "--", pattern])
}

pub fn untrack_args(pattern: &str) -> Vec<Vec<String>> {
    cmd(&["lfs", "untrack", "--", pattern])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testrepo::TestRepo;

    const OID: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

    fn pointer(oid: &str, size: usize) -> String {
        format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n")
    }

    /// Put an object among the downloaded ones, as git-lfs would.
    fn store(r: &TestRepo, oid: &str, data: &[u8]) {
        let p = object_path(&objects_dir(&r.dir), oid);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    #[test]
    fn tells_a_pointer_from_a_file() {
        assert_eq!(parse_pointer(pointer(OID, 12345).as_bytes()), Some(Pointer { oid: OID.into(), size: 12345 }));
        // Extra lines (extensions) are allowed; a short hash, a missing size and plain text are not pointers.
        assert!(parse_pointer(format!("version https://git-lfs.github.com/spec/v1\next-0-x sha256:{OID}\noid sha256:{OID}\nsize 5\n").as_bytes()).is_some());
        assert_eq!(parse_pointer(b"version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 5\n"), None);
        assert_eq!(parse_pointer(format!("version https://git-lfs.github.com/spec/v1\noid sha256:{OID}\n").as_bytes()), None);
        assert_eq!(parse_pointer(b"version 2\nhello\n"), None);
        assert_eq!(parse_pointer(&vec![b'x'; 5000]), None);
        assert_eq!(parse_pointer(&[0xff, 0xfe, 0x00]), None);
    }

    #[test]
    fn reads_attributes_and_patterns() {
        assert_eq!(parse_attr("a.psd\0filter\0lfs\0b.txt\0filter\0unspecified\0dir/c d.bin\0filter\0lfs\0"), ["a.psd".to_string(), "dir/c d.bin".to_string()].into_iter().collect());
        let text = "# pictures\n*.psd filter=lfs diff=lfs merge=lfs -text\n*.txt text\nbig[[:space:]]file.bin filter=lfs diff=lfs merge=lfs -text\n";
        assert_eq!(parse_patterns(text), ["*.psd", "big file.bin"]);
        assert_eq!(track_choices("art/logo.final.psd"), ["*.psd", "art/logo.final.psd"]);
        assert_eq!(track_choices("Makefile"), ["Makefile"]);
        assert_eq!(track_choices(".env"), [".env"]);
        assert_eq!((human(512), human(2048), human(5 * 1024 * 1024)), ("512 B".to_string(), "2.0 KB".to_string(), "5.0 MB".to_string()));
    }

    /// Two commits of a pointer file, with the objects named `have` downloaded.
    fn two_versions(tag: &str, name: &str, old: &[u8], new: &[u8], have: (bool, bool)) -> (TestRepo, String, String) {
        let r = TestRepo::new(tag);
        let (o1, o2) = (OID.to_string(), OID.replace('4', "5"));
        r.write(name, &pointer(&o1, old.len()));
        let first = r.commit("One");
        r.write(name, &pointer(&o2, new.len()));
        let second = r.commit("Two");
        if have.0 {
            store(&r, &o1, old);
        }
        if have.1 {
            store(&r, &o2, new);
        }
        (r, first, second)
    }

    fn file(path: &str) -> git::ChangedFile {
        git::ChangedFile { path: path.into(), old_path: None, status: 'M' }
    }

    #[test]
    fn shows_the_text_behind_the_pointers() {
        let (r, first, second) = two_versions("lfs-text", "data.csv", b"id,name\n1,one\n2,two\n", b"id,name\n1,one\n2,TWO\n3,three\n", (true, true));
        let d = git::commit_file_diff(&r.dir, &second, Some(&first), &file("data.csv"));
        let lines: Vec<(char, &str)> = d.unified.iter().filter(|l| l.0 != '@').map(|l| (l.0, l.1.as_str())).collect();
        assert_eq!(lines, [(' ', "id,name"), (' ', "1,one"), ('-', "2,two"), ('+', "2,TWO"), ('+', "3,three")]);
        assert!(!d.binary && d.images.is_none());
    }

    #[test]
    fn says_when_the_content_is_not_here() {
        let (r, first, second) = two_versions("lfs-missing", "data.csv", b"old\n", b"new\n", (true, false));
        let d = git::commit_file_diff(&r.dir, &second, Some(&first), &file("data.csv"));
        let text: String = d.unified.iter().map(|l| l.1.as_str()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("not downloaded") && text.contains("4 B") && !text.contains("sha256"), "{text}");
    }

    #[test]
    fn shows_pictures_and_other_files_kept_in_lfs() {
        let (r, first, second) = two_versions("lfs-image", "logo.png", &[0x89, b'P', b'N', b'G', 0, 1], &[0x89, b'P', b'N', b'G', 0, 2, 3], (true, true));
        let d = git::commit_file_diff(&r.dir, &second, Some(&first), &file("logo.png"));
        let img = d.images.expect("the two pictures");
        assert_eq!((img.before.unwrap().len(), img.after.unwrap().len()), (6, 7));
        let (r, first, second) = two_versions("lfs-binary", "model.bin", &[1, 0, 2], &[1, 0, 3], (true, true));
        assert!(git::commit_file_diff(&r.dir, &second, Some(&first), &file("model.bin")).binary);
    }

    #[test]
    fn marks_the_changed_files_that_go_to_lfs() {
        let r = TestRepo::new("lfs-marks");
        r.write(".gitattributes", "*.psd filter=lfs diff=lfs merge=lfs -text\n");
        r.commit("Keep pictures in LFS");
        assert!(in_use(&r.dir));
        assert_eq!(patterns(&r.dir), ["*.psd"]);
        // New files, not added: nothing here runs the LFS program itself.
        r.write("art/logo.psd", "pretend picture\n");
        r.write("notes.txt", "x\n");
        let app = r.app();
        let repo = app.repo.as_ref().unwrap();
        assert!(repo.lfs_in_use);
        assert_eq!(repo.lfs_paths, ["art/logo.psd".to_string()].into_iter().collect());
    }

    #[test]
    fn an_ordinary_diff_is_left_alone() {
        let r = TestRepo::new("lfs-plain");
        r.write("notes.txt", "version one\n");
        let first = r.commit("One");
        r.write("notes.txt", "version two\noid sha256:not really\n");
        let second = r.commit("Two");
        let d = git::commit_file_diff(&r.dir, &second, Some(&first), &file("notes.txt"));
        assert!(d.unified.iter().any(|l| l.1 == "version two"));
        assert!(!in_use(&r.dir));
    }
}
