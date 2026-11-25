//! The files of the checked-out branch: the list git tracks (`git ls-files`), the tree of
//! folders built from it, and the content of one file as it is in the working folder, for
//! reading. Files that are not in git (not added yet, ignored) are not listed.

use crate::git;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// A tracked file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// "src/app/main.rs", with "/" on every system.
    pub path: String,
    pub kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    File,
    /// A symbolic link: its target is named, not followed.
    Link,
    /// A repository of its own inside this one.
    Submodule,
}

/// The entries in the output of `git ls-files -s -z`: "<mode> <hash> <stage>\t<path>\0".
/// A file in a conflict is there once per side; it is listed once.
pub fn parse(out: &str) -> Vec<Entry> {
    let mut list: Vec<Entry> = vec![];
    for item in out.split('\0') {
        let Some((head, path)) = item.split_once('\t') else { continue };
        if path.is_empty() || list.last().is_some_and(|e| e.path == path) {
            continue;
        }
        let kind = match head.split(' ').next() {
            Some("120000") => Kind::Link,
            Some("160000") => Kind::Submodule,
            _ => Kind::File,
        };
        list.push(Entry { path: path.to_string(), kind });
    }
    list
}

/// The tracked files of the repository, as checked out.
pub fn list(dir: &Path) -> Vec<Entry> {
    parse(&git::run(dir, &["ls-files", "-s", "-z"]).stdout)
}

/// A folder of the tree: its folders and the files right in it (indexes into the list).
#[derive(Default, Debug)]
pub struct Dir {
    pub dirs: BTreeMap<String, Dir>,
    pub files: Vec<usize>,
}

pub fn tree(list: &[Entry]) -> Dir {
    let mut root = Dir::default();
    for (i, e) in list.iter().enumerate() {
        let mut at = &mut root;
        let mut parts = e.path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                at = at.dirs.entry(part.to_string()).or_default();
            } else {
                at.files.push(i);
            }
        }
    }
    root
}

/// A line of the tree as it is shown.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Row {
    /// A folder: how deep, its path ("src/app") and whether it is open.
    Dir { depth: usize, path: String, open: bool },
    /// A file: how deep and its index in the list.
    File { depth: usize, index: usize },
}

fn name_key(name: &str) -> (String, &str) {
    (name.to_lowercase(), name)
}

/// The lines to show: folders before files, both by name without regard to case; only the
/// folders in `open` show what is in them.
pub fn rows(list: &[Entry], root: &Dir, open: &HashSet<String>) -> Vec<Row> {
    fn walk(list: &[Entry], dir: &Dir, prefix: &str, depth: usize, open: &HashSet<String>, out: &mut Vec<Row>) {
        let mut names: Vec<&String> = dir.dirs.keys().collect();
        names.sort_by_key(|n| name_key(n));
        for name in names {
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let is_open = open.contains(&path);
            out.push(Row::Dir { depth, path: path.clone(), open: is_open });
            if is_open {
                walk(list, &dir.dirs[name], &path, depth + 1, open, out);
            }
        }
        let mut files = dir.files.clone();
        files.sort_by_key(|&i| name_key(file_name(&list[i].path)));
        out.extend(files.into_iter().map(|index| Row::File { depth, index }));
    }
    let mut out = vec![];
    walk(list, root, "", 0, open, &mut out);
    out
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The files whose path has every word of the filter in it (without regard to case), the ones
/// whose own name has them first; at most `limit`.
pub fn find(list: &[Entry], filter: &str, limit: usize) -> Vec<usize> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return vec![];
    }
    let mut by_name = vec![];
    let mut by_path = vec![];
    for (i, e) in list.iter().enumerate() {
        let path = e.path.to_lowercase();
        if !words.iter().all(|w| path.contains(w.as_str())) {
            continue;
        }
        let name = file_name(&path);
        if words.iter().all(|w| name.contains(w.as_str())) { by_name.push(i) } else { by_path.push(i) }
    }
    by_name.extend(by_path);
    by_name.truncate(limit);
    by_name
}

/// Files bigger than this are not read into a tab.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// What a file gives to read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Content {
    /// Its lines.
    Text(Vec<String>),
    /// A picture, for the image viewer.
    Image(std::sync::Arc<Vec<u8>>),
    /// Not text: how many bytes it has.
    Binary(u64),
    /// Too big to show: how many bytes it has.
    TooBig(u64),
    /// Tracked by git, but not in the working folder (deleted and not committed yet).
    Missing,
    /// A symbolic link, and where it points.
    Link(String),
    Submodule,
    Failed(String),
}

/// Whether these bytes are text: no zero byte in their start.
pub fn is_text(bytes: &[u8]) -> bool {
    !bytes.iter().take(8000).any(|&b| b == 0)
}

/// The lines of a text: without the line ends, a "\r" before them dropped too.
pub fn lines_of(bytes: &[u8]) -> Vec<String> {
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t.to_string(),
        // Not UTF-8: in the one-byte encoding chosen in the settings.
        Err(_) => crate::encoding::decode(bytes),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    text.lines().map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect()
}

/// Read a tracked file from the working folder.
pub fn read(root: &Path, entry: &Entry) -> Content {
    if entry.kind == Kind::Submodule {
        return Content::Submodule;
    }
    let path = crate::chrome::repo_file(root, &entry.path);
    // The link itself is looked at, never what it points to (that may be outside the repository).
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Content::Missing,
        Err(e) => return Content::Failed(e.to_string()),
    };
    if meta.file_type().is_symlink() {
        return Content::Link(std::fs::read_link(&path).map(|t| t.display().to_string()).unwrap_or_default());
    }
    if !meta.is_file() {
        return Content::Failed("not a file".into());
    }
    if meta.len() > MAX_BYTES {
        return Content::TooBig(meta.len());
    }
    match std::fs::read(&path) {
        Ok(bytes) if git::is_image_path(&entry.path) => Content::Image(std::sync::Arc::new(bytes)),
        Ok(bytes) if is_text(&bytes) => Content::Text(lines_of(&bytes)),
        Ok(bytes) => Content::Binary(bytes.len() as u64),
        Err(e) => Content::Failed(e.to_string()),
    }
}

/// "1.4 MB", "312 KB", "87 bytes".
pub fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        b if b >= 1024 => format!("{} KB", b.div_ceil(1024)),
        b => format!("{b} bytes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testrepo::TestRepo;

    fn e(path: &str) -> Entry {
        Entry { path: path.into(), kind: Kind::File }
    }

    #[test]
    fn reads_the_list_git_gives() {
        let out = "100644 aaa 0\tREADME.md\x00100755 bbb 0\tbin/run me.sh\x00120000 ccc 0\tdocs/latest\x00160000 ddd 0\tvendor/lib\x00100644 e1 1\tsrc/both.rs\x00100644 e2 2\tsrc/both.rs\x00100644 e3 3\tsrc/both.rs\x00100644 fff 0\tзаметки/план.md\x00";
        let list = parse(out);
        let seen: Vec<(&str, Kind)> = list.iter().map(|x| (x.path.as_str(), x.kind)).collect();
        assert_eq!(seen, vec![("README.md", Kind::File), ("bin/run me.sh", Kind::File), ("docs/latest", Kind::Link), ("vendor/lib", Kind::Submodule), ("src/both.rs", Kind::File), ("заметки/план.md", Kind::File)]);
        assert!(parse("").is_empty());
    }

    #[test]
    fn shows_folders_first_and_only_the_open_ones() {
        let list = vec![e("zebra.txt"), e("Makefile"), e("src/main.rs"), e("src/app/view.rs"), e("src/app/Model.rs"), e("docs/a.md"), e("apple.txt")];
        let root = tree(&list);
        let name = |r: &Row| match r {
            Row::Dir { depth, path, open } => format!("{}{}/{}", "  ".repeat(*depth), path, if *open { " -" } else { "" }),
            Row::File { depth, index } => format!("{}{}", "  ".repeat(*depth), file_name(&list[*index].path)),
        };
        let shown = |open: &[&str]| rows(&list, &root, &open.iter().map(|s| s.to_string()).collect()).iter().map(name).collect::<Vec<_>>();
        // Nothing open: the folders of the top, then its files, case not counted.
        assert_eq!(shown(&[]), vec!["docs/", "src/", "apple.txt", "Makefile", "zebra.txt"]);
        assert_eq!(shown(&["src"]), vec!["docs/", "src/ -", "  src/app/", "  main.rs", "apple.txt", "Makefile", "zebra.txt"]);
        // A folder open inside a closed one shows nothing until that one is opened.
        assert_eq!(shown(&["src/app"]), shown(&[]));
        assert_eq!(shown(&["src", "src/app"]), vec!["docs/", "src/ -", "  src/app/ -", "    Model.rs", "    view.rs", "  main.rs", "apple.txt", "Makefile", "zebra.txt"]);
    }

    #[test]
    fn finds_files_by_words_of_the_path() {
        let list = vec![e("src/app/view.rs"), e("docs/view/index.md"), e("src/Review.rs"), e("README.md")];
        let found = |f: &str| find(&list, f, 10).into_iter().map(|i| list[i].path.as_str()).collect::<Vec<_>>();
        // The ones named so first, then the ones that only lie in such a folder.
        assert_eq!(found("view"), vec!["src/app/view.rs", "src/Review.rs", "docs/view/index.md"]);
        assert_eq!(found("SRC view"), vec!["src/app/view.rs", "src/Review.rs"]);
        assert_eq!(found("  "), Vec::<&str>::new());
        assert_eq!(find(&list, "r", 2).len(), 2);
    }

    #[test]
    fn tells_text_from_the_rest() {
        assert_eq!(lines_of(b"one\r\ntwo\n\nlast"), vec!["one", "two", "", "last"]);
        assert_eq!(lines_of("\u{feff}первая\n".as_bytes()), vec!["первая"]);
        assert!(lines_of(b"").is_empty());
        assert!(is_text(b"plain"));
        assert!(!is_text(b"PK\x03\x04\x00\x00"));
        assert_eq!((size_text(87), size_text(2048), size_text(1500), size_text(3 * 1024 * 1024 / 2)), ("87 bytes".into(), "2 KB".into(), "2 KB".into(), "1.5 MB".into()));
    }

    #[test]
    fn lists_and_reads_the_files_of_a_real_repository() {
        let r = TestRepo::new("files");
        // No commit yet: nothing tracked.
        assert!(list(&r.dir).is_empty());
        r.write("README.md", "# Title\n\ntext\n");
        r.write("docs/мои заметки.md", "строка один\nстрока два\n");
        r.write("bin/data.bin", "\u{0}\u{1}\u{2}binary");
        r.commit("Add files");
        r.write("not-added.txt", "untracked\n");
        r.write("staged.txt", "new\n");
        r.git(&["add", "staged.txt"]);
        let files = list(&r.dir);
        let paths: Vec<&str> = files.iter().map(|x| x.path.as_str()).collect();
        // What git tracks, a file just added too; the one never added is not there.
        assert_eq!(paths, vec!["README.md", "bin/data.bin", "docs/мои заметки.md", "staged.txt"]);
        let get = |p: &str| read(&r.dir, files.iter().find(|x| x.path == p).unwrap());
        assert_eq!(get("README.md"), Content::Text(vec!["# Title".into(), "".into(), "text".into()]));
        assert_eq!(get("docs/мои заметки.md"), Content::Text(vec!["строка один".into(), "строка два".into()]));
        assert_eq!(get("bin/data.bin"), Content::Binary(9));
        // A picture is handed over whole, whatever is in it.
        r.write("logo.png", "not really a picture");
        r.git(&["add", "logo.png"]);
        let with_logo = list(&r.dir);
        assert!(matches!(read(&r.dir, with_logo.iter().find(|x| x.path == "logo.png").unwrap()), Content::Image(b) if b.len() == 20));
        // The working folder is what is read: a change not committed is seen, a deleted file is missing.
        r.write("README.md", "# Changed\n");
        assert_eq!(get("README.md"), Content::Text(vec!["# Changed".into()]));
        std::fs::remove_file(r.dir.join("staged.txt")).unwrap();
        assert_eq!(get("staged.txt"), Content::Missing);
        assert_eq!(read(&r.dir, &Entry { path: "vendor/lib".into(), kind: Kind::Submodule }), Content::Submodule);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hostname", r.dir.join("link")).unwrap();
            r.git(&["add", "link"]);
            let files = list(&r.dir);
            let link = files.iter().find(|x| x.path == "link").unwrap();
            assert_eq!(link.kind, Kind::Link);
            // The link is named, what it points to outside the repository is not read.
            assert_eq!(read(&r.dir, link), Content::Link("/etc/hostname".into()));
        }
    }
}
