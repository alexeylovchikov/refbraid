//! Syntax colouring for diffs: a small lexer driven by a table per language. It only tells
//! comments, strings, numbers, keywords, types and names of called functions apart — enough to
//! read code comfortably, cheap enough to run over a whole file when a diff opens.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyword,
    /// Built-in types and constants, and names that look like types (`CamelCase`).
    Type,
    String,
    Number,
    Comment,
    /// A name followed by `(`, a macro, a decorator.
    Function,
    /// A key: JSON / YAML / TOML property, HTML attribute, CSS property.
    Key,
    /// Markdown heading, HTML tag, TOML section.
    Heading,
}

/// A coloured piece of a line: bytes `range` of the line are of `kind`.
pub type Span = (Range<usize>, Kind);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Keywords, comments and strings from the tables below.
    Code,
    Json,
    Yaml,
    Toml,
    Markdown,
    Markup,
    Css,
}

pub struct Lang {
    mode: Mode,
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    /// Quotes of strings that end on their line.
    quotes: &'static [char],
    /// Strings that may run over several lines: (start, end).
    long_strings: &'static [(&'static str, &'static str)],
    /// Keywords are written in any letter case (SQL).
    any_case: bool,
    /// `'a` is a lifetime, not the start of a string (Rust).
    lifetimes: bool,
    /// `$name` is a variable (shell, PHP).
    dollar: bool,
}

const fn code(keywords: &'static [&'static str], types: &'static [&'static str], line_comments: &'static [&'static str], block: Option<(&'static str, &'static str)>, quotes: &'static [char], long_strings: &'static [(&'static str, &'static str)]) -> Lang {
    Lang { mode: Mode::Code, keywords, types, line_comments, block, quotes, long_strings, any_case: false, lifetimes: false, dollar: false }
}

const fn special(mode: Mode) -> Lang {
    Lang { mode, keywords: &[], types: &[], line_comments: &[], block: None, quotes: &[], long_strings: &[], any_case: false, lifetimes: false, dollar: false }
}

const C_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));

static RUST: Lang = Lang {
    lifetimes: true,
    ..code(
        &["as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "type", "unsafe", "use", "where", "while", "macro_rules"],
        &["bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "true", "false", "Some", "None", "Ok", "Err"],
        &["//"],
        C_BLOCK,
        &['"'],
        &[],
    )
};
static PYTHON: Lang = code(
    &["and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "match", "case"],
    &["True", "False", "None", "self", "cls", "int", "str", "float", "bool", "list", "dict", "set", "tuple", "bytes", "object"],
    &["#"],
    None,
    &['"', '\''],
    &[("\"\"\"", "\"\"\""), ("'''", "'''")],
);
static JS: Lang = code(
    &["async", "await", "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "export", "extends", "finally", "for", "from", "function", "if", "import", "in", "instanceof", "let", "new", "of", "return", "static", "super", "switch", "this", "throw", "try", "typeof", "var", "void", "while", "with", "yield", "as", "declare", "enum", "implements", "interface", "namespace", "private", "protected", "public", "readonly", "type", "abstract", "get", "set", "keyof", "satisfies"],
    &["true", "false", "null", "undefined", "NaN", "Infinity", "string", "number", "boolean", "any", "unknown", "never", "object", "symbol", "bigint"],
    &["//"],
    C_BLOCK,
    &['"', '\''],
    &[("`", "`")],
);
static C_LIKE: Lang = code(
    &["auto", "break", "case", "catch", "class", "const", "constexpr", "continue", "default", "delete", "do", "else", "enum", "explicit", "extern", "final", "for", "friend", "goto", "if", "inline", "namespace", "new", "noexcept", "operator", "override", "private", "protected", "public", "register", "return", "sizeof", "static", "struct", "switch", "template", "this", "throw", "try", "typedef", "typename", "union", "using", "virtual", "volatile", "while", "#include", "#define", "#ifdef", "#ifndef", "#endif", "#if", "#else", "#pragma"],
    &["bool", "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void", "size_t", "true", "false", "nullptr", "NULL", "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t"],
    &["//"],
    C_BLOCK,
    &['"', '\''],
    &[],
);
static JAVA: Lang = code(
    &["abstract", "assert", "break", "case", "catch", "class", "const", "continue", "default", "do", "else", "enum", "extends", "final", "finally", "for", "goto", "if", "implements", "import", "instanceof", "interface", "native", "new", "package", "private", "protected", "public", "return", "static", "strictfp", "super", "switch", "synchronized", "this", "throw", "throws", "transient", "try", "volatile", "while", "var", "record", "sealed", "permits", "yield", "fun", "val", "when", "object", "companion", "data", "override", "open", "internal", "suspend", "in", "is", "as", "by", "lateinit", "init", "typealias", "namespace", "using", "async", "await", "get", "set", "readonly", "virtual", "partial", "out", "ref", "params", "foreach", "lock", "event", "delegate", "struct", "where"],
    &["boolean", "byte", "char", "double", "float", "int", "long", "short", "void", "true", "false", "null", "String", "Int", "Long", "Boolean", "Unit", "Any", "Nothing", "bool", "string", "decimal", "object", "uint", "ulong"],
    &["//"],
    C_BLOCK,
    &['"', '\''],
    &[("\"\"\"", "\"\"\"")],
);
static GO: Lang = code(
    &["break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func", "go", "goto", "if", "import", "interface", "map", "package", "range", "return", "select", "struct", "switch", "type", "var"],
    &["bool", "byte", "complex64", "complex128", "error", "float32", "float64", "int", "int8", "int16", "int32", "int64", "rune", "string", "uint", "uint8", "uint16", "uint32", "uint64", "uintptr", "true", "false", "nil", "iota", "any"],
    &["//"],
    C_BLOCK,
    &['"', '\''],
    &[("`", "`")],
);
static SWIFT: Lang = code(
    &["actor", "as", "associatedtype", "async", "await", "break", "case", "catch", "class", "continue", "default", "defer", "deinit", "do", "else", "enum", "extension", "fallthrough", "fileprivate", "final", "for", "func", "guard", "if", "import", "in", "init", "inout", "internal", "is", "let", "mutating", "open", "operator", "override", "private", "protocol", "public", "repeat", "return", "self", "Self", "static", "struct", "subscript", "super", "switch", "throw", "throws", "try", "typealias", "var", "where", "while"],
    &["Bool", "Int", "Double", "Float", "String", "Character", "Void", "Any", "true", "false", "nil"],
    &["//"],
    C_BLOCK,
    &['"'],
    &[("\"\"\"", "\"\"\"")],
);
static PHP: Lang = Lang {
    dollar: true,
    ..code(
        &["abstract", "and", "array", "as", "break", "callable", "case", "catch", "class", "clone", "const", "continue", "declare", "default", "do", "echo", "else", "elseif", "empty", "enum", "extends", "final", "finally", "fn", "for", "foreach", "function", "global", "if", "implements", "include", "include_once", "instanceof", "interface", "isset", "list", "match", "namespace", "new", "or", "print", "private", "protected", "public", "readonly", "require", "require_once", "return", "static", "switch", "throw", "trait", "try", "unset", "use", "var", "while", "yield"],
        &["true", "false", "null", "int", "float", "string", "bool", "void", "mixed", "self", "parent"],
        &["//", "#"],
        C_BLOCK,
        &['"', '\''],
        &[],
    )
};
static RUBY: Lang = code(
    &["alias", "and", "begin", "break", "case", "class", "def", "defined?", "do", "else", "elsif", "end", "ensure", "for", "if", "in", "module", "next", "not", "or", "redo", "rescue", "retry", "return", "self", "super", "then", "undef", "unless", "until", "when", "while", "yield", "require", "include", "attr_accessor", "attr_reader", "attr_writer", "private", "protected", "public"],
    &["true", "false", "nil"],
    &["#"],
    None,
    &['"', '\''],
    &[],
);
static SHELL: Lang = Lang {
    dollar: true,
    ..code(
        &["if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function", "return", "exit", "local", "export", "readonly", "declare", "set", "unset", "shift", "source", "echo", "cd", "test", "break", "continue", "trap", "eval", "exec", "FROM", "RUN", "CMD", "COPY", "ADD", "ENV", "ARG", "WORKDIR", "EXPOSE", "ENTRYPOINT", "USER", "VOLUME", "LABEL", "include", "ifeq", "ifneq", "ifdef", "ifndef", "endif"],
        &["true", "false"],
        &["#"],
        None,
        &['"', '\''],
        &[],
    )
};
static SQL: Lang = Lang {
    any_case: true,
    ..code(
        &["add", "all", "alter", "and", "as", "asc", "begin", "between", "by", "case", "check", "column", "commit", "constraint", "create", "cross", "database", "declare", "default", "delete", "desc", "distinct", "drop", "else", "end", "exec", "execute", "exists", "foreign", "from", "full", "function", "go", "grant", "group", "having", "if", "in", "index", "inner", "insert", "into", "is", "join", "key", "left", "like", "limit", "not", "on", "or", "order", "outer", "primary", "procedure", "references", "return", "returns", "right", "rollback", "select", "set", "table", "then", "top", "transaction", "trigger", "truncate", "union", "unique", "update", "use", "values", "view", "when", "where", "while", "with"],
        &["null", "true", "false", "int", "integer", "bigint", "smallint", "tinyint", "bit", "decimal", "numeric", "float", "real", "money", "char", "varchar", "nvarchar", "nchar", "text", "ntext", "date", "datetime", "datetime2", "time", "timestamp", "boolean", "uuid", "uniqueidentifier", "serial", "json", "jsonb", "varbinary", "blob"],
        &["--"],
        C_BLOCK,
        &['\''],
        &[],
    )
};
static JSON: Lang = special(Mode::Json);
static YAML: Lang = special(Mode::Yaml);
static TOML: Lang = special(Mode::Toml);
static MARKDOWN: Lang = special(Mode::Markdown);
static MARKUP: Lang = special(Mode::Markup);
static CSS: Lang = special(Mode::Css);

/// The language of a file, by its name; None when it is not one of the known ones.
pub fn lang_for(path: &str) -> Option<&'static Lang> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "dockerfile" | "makefile" | "gnumakefile" | ".bashrc" | ".zshrc" | ".profile" | ".gitignore" | ".dockerignore" | ".env" => return Some(&SHELL),
        "cargo.lock" | "pipfile" => return Some(&TOML),
        _ => {}
    }
    let ext = lower.rsplit_once('.').map(|x| x.1)?;
    Some(match ext {
        "rs" => &RUST,
        "py" | "pyi" | "pyw" => &PYTHON,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" => &JS,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "m" | "mm" | "ino" => &C_LIKE,
        "java" | "kt" | "kts" | "cs" | "scala" | "groovy" | "gradle" | "dart" => &JAVA,
        "go" => &GO,
        "swift" => &SWIFT,
        "php" => &PHP,
        "rb" | "rake" | "gemspec" => &RUBY,
        "sh" | "bash" | "zsh" | "fish" | "mk" | "dockerfile" | "spec" | "conf" | "properties" | "ini" | "cfg" | "env" => &SHELL,
        "sql" | "psql" | "ddl" => &SQL,
        "json" | "jsonc" | "json5" | "webmanifest" | "ipynb" => &JSON,
        "yml" | "yaml" => &YAML,
        "toml" => &TOML,
        "md" | "markdown" | "mdx" => &MARKDOWN,
        "html" | "htm" | "xml" | "xhtml" | "svg" | "vue" | "xaml" | "csproj" | "plist" | "xsd" | "xsl" => &MARKUP,
        "css" | "scss" | "sass" | "less" => &CSS,
        _ => return None,
    })
}

/// What carries over from one line to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct State {
    /// Inside a block comment.
    comment: bool,
    /// Inside a long string: its number in `long_strings`, plus one.
    string: u8,
    /// Markdown: inside a ``` fence.
    fence: bool,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The end (byte) of the number that starts at `i`.
fn number_end(s: &str, i: usize) -> usize {
    let b = s.as_bytes();
    let mut j = i;
    let hex = b.len() > i + 1 && b[i] == b'0' && matches!(b[i + 1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O');
    if hex {
        j += 2;
    }
    while j < b.len() {
        let c = b[j];
        let ok = c.is_ascii_alphanumeric() || c == b'_' || (c == b'.' && !hex && b.get(j + 1).is_some_and(|n| n.is_ascii_digit())) || ((c == b'+' || c == b'-') && !hex && matches!(b[j - 1], b'e' | b'E') && b.get(j + 1).is_some_and(|n| n.is_ascii_digit()));
        if !ok {
            break;
        }
        j += 1;
    }
    j
}

/// The end (byte) of the string whose opening quote `q` is at `i`: after the closing quote, or
/// the end of the line.
fn string_end(s: &str, i: usize, q: char) -> usize {
    let mut esc = false;
    for (k, c) in s[i + q.len_utf8()..].char_indices() {
        if esc {
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == q {
            return i + q.len_utf8() + k + c.len_utf8();
        }
    }
    s.len()
}

fn code_line(lang: &Lang, s: &str, st: &mut State, out: &mut Vec<Span>) {
    let mut i = 0;
    // A comment or a long string left open by an earlier line.
    if st.comment {
        let end = lang.block.map(|b| b.1).unwrap_or("*/");
        match s.find(end) {
            Some(k) => {
                out.push((0..k + end.len(), Kind::Comment));
                st.comment = false;
                i = k + end.len();
            }
            None => {
                out.push((0..s.len(), Kind::Comment));
                return;
            }
        }
    } else if st.string > 0 {
        let end = lang.long_strings[st.string as usize - 1].1;
        match s.find(end) {
            Some(k) => {
                out.push((0..k + end.len(), Kind::String));
                st.string = 0;
                i = k + end.len();
            }
            None => {
                out.push((0..s.len(), Kind::String));
                return;
            }
        }
    }
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        if lang.line_comments.iter().any(|p| rest.starts_with(p)) {
            out.push((i..s.len(), Kind::Comment));
            return;
        }
        if let Some((open, close)) = lang.block.filter(|b| rest.starts_with(b.0)) {
            match rest[open.len()..].find(close) {
                Some(k) => {
                    let end = i + open.len() + k + close.len();
                    out.push((i..end, Kind::Comment));
                    i = end;
                    continue;
                }
                None => {
                    out.push((i..s.len(), Kind::Comment));
                    st.comment = true;
                    return;
                }
            }
        }
        if let Some((n, (open, close))) = lang.long_strings.iter().enumerate().find(|(_, l)| rest.starts_with(l.0)) {
            match rest[open.len()..].find(close) {
                Some(k) => {
                    let end = i + open.len() + k + close.len();
                    out.push((i..end, Kind::String));
                    i = end;
                    continue;
                }
                None => {
                    out.push((i..s.len(), Kind::String));
                    st.string = n as u8 + 1;
                    return;
                }
            }
        }
        if lang.quotes.contains(&c) || (lang.lifetimes && c == '\'') {
            if lang.lifetimes && c == '\'' {
                // 'a' and '\n' are characters; 'a alone is a lifetime.
                let mut it = rest.chars().skip(1);
                let (a, b, d) = (it.next(), it.next(), it.next());
                let is_char = matches!((a, b), (Some(x), Some('\'')) if x != '\\') || (a == Some('\\') && (b.is_some() && (d == Some('\'') || rest[1..].find('\'').is_some_and(|k| k <= 10))));
                if !is_char {
                    i += 1;
                    continue;
                }
            }
            let end = string_end(s, i, c);
            out.push((i..end, Kind::String));
            i = end;
            continue;
        }
        if c.is_ascii_digit() {
            let end = number_end(s, i);
            out.push((i..end, Kind::Number));
            i = end;
            continue;
        }
        if lang.dollar && c == '$' {
            let end = i + 1 + rest[1..].find(|x: char| !(is_ident(x) || x == '{' || x == '}')).unwrap_or(rest.len() - 1);
            if end > i + 1 {
                out.push((i..end, Kind::Type));
            }
            i = end.max(i + 1);
            continue;
        }
        if is_ident_start(c) || c == '#' || c == '@' {
            let lead = if c == '#' || c == '@' { 1 } else { 0 };
            let len = lead + rest[lead..].find(|x: char| !is_ident(x)).unwrap_or(rest.len() - lead);
            if len == lead {
                i += lead;
                continue;
            }
            let mut end = i + len;
            let word = &s[i..end];
            let after = s[end..].chars().next();
            let known = |list: &[&str]| if lang.any_case { list.iter().any(|k| k.eq_ignore_ascii_case(word)) } else { list.contains(&word) };
            let kind = if known(lang.keywords) {
                Some(Kind::Keyword)
            } else if known(lang.types) {
                Some(Kind::Type)
            } else if c == '@' {
                // A decorator or an annotation.
                Some(Kind::Function)
            } else if c == '#' {
                None
            } else if after == Some('!') && lang.lifetimes && s[end + 1..].starts_with(['(', '[', '{']) {
                // A Rust macro, with its "!".
                end += 1;
                Some(Kind::Function)
            } else if after == Some('(') {
                Some(Kind::Function)
            } else if word.chars().next().is_some_and(|x| x.is_uppercase()) && word.chars().any(|x| x.is_lowercase()) && !lang.any_case {
                Some(Kind::Type)
            } else {
                None
            };
            if let Some(k) = kind {
                out.push((i..end, k));
            }
            i = end;
            continue;
        }
        i += c.len_utf8();
    }
}

/// A quoted string or a bare word as a value: numbers and true / false / null stand out.
fn value(s: &str, from: usize, out: &mut Vec<Span>) {
    let mut i = from;
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap();
        if c == '"' || c == '\'' {
            let end = string_end(s, i, c);
            out.push((i..end, Kind::String));
            i = end;
        } else if c == '#' && (i == 0 || s[..i].ends_with(char::is_whitespace)) {
            out.push((i..s.len(), Kind::Comment));
            return;
        } else if c.is_ascii_digit() || (c == '-' && rest[1..].starts_with(|x: char| x.is_ascii_digit())) {
            let start = i;
            let end = number_end(s, if c == '-' { i + 1 } else { i });
            // "2024-01-15" or "1.2.3" is not a number; leave such words plain.
            let whole = s[end..].chars().next().is_none_or(|x| !(is_ident(x) || x == '-' || x == '.' || x == ':' || x == '/'));
            let before_ok = start == 0 || !s[..start].ends_with(|x: char| is_ident(x) || x == '-' || x == '.' || x == '/');
            if whole && before_ok {
                out.push((start..end, Kind::Number));
            }
            i = end.max(i + 1);
        } else if is_ident_start(c) {
            let end = i + rest.find(|x: char| !is_ident(x)).unwrap_or(rest.len());
            let word = &s[i..end];
            let alone = (i == 0 || !s[..i].ends_with(|x: char| is_ident(x) || x == '-' || x == '.' || x == '/')) && s[end..].chars().next().is_none_or(|x| !(x == '-' || x == '.' || x == '/'));
            if alone && matches!(word, "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "True" | "False" | "None" | "~") {
                out.push((i..end, Kind::Keyword));
            }
            i = end;
        } else {
            i += c.len_utf8();
        }
    }
}

fn json_line(s: &str, out: &mut Vec<Span>) {
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap();
        if c == '"' {
            let end = string_end(s, i, '"');
            // A string followed by a colon is a key.
            let key = s[end..].trim_start().starts_with(':');
            out.push((i..end, if key { Kind::Key } else { Kind::String }));
            i = end;
        } else if rest.starts_with("//") {
            out.push((i..s.len(), Kind::Comment));
            return;
        } else if c.is_ascii_digit() || (c == '-' && rest[1..].starts_with(|x: char| x.is_ascii_digit())) {
            let end = number_end(s, if c == '-' { i + 1 } else { i });
            out.push((i..end, Kind::Number));
            i = end;
        } else if is_ident_start(c) {
            let end = i + rest.find(|x: char| !is_ident(x)).unwrap_or(rest.len());
            if matches!(&s[i..end], "true" | "false" | "null") {
                out.push((i..end, Kind::Keyword));
            }
            i = end;
        } else {
            i += c.len_utf8();
        }
    }
}

/// "key: value" (YAML) or "key = value" (TOML): the key, then the value.
fn key_value_line(s: &str, sep: char, out: &mut Vec<Span>) {
    let trimmed = s.trim_start();
    let indent = s.len() - trimmed.len();
    if trimmed.starts_with('#') {
        out.push((indent..s.len(), Kind::Comment));
        return;
    }
    if sep == '=' && trimmed.starts_with('[') {
        // A TOML section, maybe with a comment after it.
        let end = trimmed.find(']').map(|k| indent + k + 1).unwrap_or(s.len());
        out.push((indent..end, Kind::Heading));
        value(s, end, out);
        return;
    }
    // YAML list items: "- key: value".
    let mut start = indent;
    while sep == ':' && s[start..].starts_with("- ") {
        start += 2;
        start += s[start..].len() - s[start..].trim_start().len();
    }
    let body = &s[start..];
    let key_end = if body.starts_with(['"', '\'']) {
        let q = body.chars().next().unwrap();
        let e = string_end(s, start, q);
        s[e..].trim_start().starts_with(sep).then_some(e)
    } else {
        body.find(sep)
            .filter(|&k| {
                let key = &body[..k];
                let after = body[k + 1..].chars().next();
                !key.is_empty() && !key.contains(['"', '\'', '#', '{', '[']) && (sep == '=' || after.is_none_or(|c| c.is_whitespace())) && !key.trim().contains(' ') || (sep == '=' && !key.contains(['"', '\'', '#']) && !key.is_empty())
            })
            .map(|k| start + k)
    };
    match key_end {
        Some(e) => {
            let key = s[start..e].trim_end();
            out.push((start..start + key.len(), Kind::Key));
            let after = e + s[e..].find(sep).map(|k| k + 1).unwrap_or(0);
            value(s, after, out);
        }
        None => value(s, start, out),
    }
}

fn markdown_line(s: &str, st: &mut State, out: &mut Vec<Span>) {
    let t = s.trim_start();
    let indent = s.len() - t.len();
    if t.starts_with("```") || t.starts_with("~~~") {
        st.fence = !st.fence;
        out.push((indent..s.len(), Kind::Comment));
        return;
    }
    if st.fence {
        out.push((0..s.len(), Kind::String));
        return;
    }
    if t.starts_with('#') && t.trim_start_matches('#').starts_with(' ') {
        out.push((indent..s.len(), Kind::Heading));
        return;
    }
    if t.starts_with('>') {
        out.push((indent..s.len(), Kind::Comment));
        return;
    }
    let mut i = indent;
    // List bullets and numbers.
    let bullet = if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        1
    } else {
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) { digits + 1 } else { 0 }
    };
    if bullet > 0 {
        out.push((indent..indent + bullet, Kind::Keyword));
        i = indent + bullet;
    }
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap();
        if c == '`' {
            let end = rest[1..].find('`').map(|k| i + k + 2).unwrap_or(s.len());
            out.push((i..end, Kind::String));
            i = end;
        } else if c == '[' {
            // [text](address): the address is dimmed like a comment, the text is a link name.
            if let Some(close) = rest.find("](") {
                if let Some(paren) = rest[close..].find(')') {
                    out.push((i..i + close + 1, Kind::Function));
                    out.push((i + close + 1..i + close + paren + 1, Kind::Comment));
                    i += close + paren + 1;
                    continue;
                }
            }
            i += 1;
        } else if rest.starts_with("**") || rest.starts_with("__") {
            let mark = &rest[..2];
            match rest[2..].find(mark) {
                Some(k) => {
                    out.push((i..i + k + 4, Kind::Type));
                    i += k + 4;
                }
                None => i += 2,
            }
        } else {
            i += c.len_utf8();
        }
    }
}

fn markup_line(s: &str, st: &mut State, out: &mut Vec<Span>) {
    let mut i = 0;
    if st.comment {
        match s.find("-->") {
            Some(k) => {
                out.push((0..k + 3, Kind::Comment));
                st.comment = false;
                i = k + 3;
            }
            None => {
                out.push((0..s.len(), Kind::Comment));
                return;
            }
        }
    }
    while i < s.len() {
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(k) => {
                    out.push((i..i + k + 3, Kind::Comment));
                    i += k + 3;
                }
                None => {
                    out.push((i..s.len(), Kind::Comment));
                    st.comment = true;
                    return;
                }
            }
            continue;
        }
        if rest.starts_with('<') {
            // The tag name, then attributes up to ">".
            let name_len = 1 + rest[1..].find(|c: char| !(is_ident(c) || matches!(c, '/' | '!' | '?' | ':' | '-' | '.'))).unwrap_or(rest.len() - 1);
            out.push((i..i + name_len, Kind::Heading));
            i += name_len;
            while i < s.len() {
                let r = &s[i..];
                let c = r.chars().next().unwrap();
                if c == '>' {
                    out.push((i..i + 1, Kind::Heading));
                    i += 1;
                    break;
                } else if c == '"' || c == '\'' {
                    let end = string_end(s, i, c);
                    out.push((i..end, Kind::String));
                    i = end;
                } else if is_ident_start(c) || c == ':' || c == '@' {
                    let end = i + r.find(|x: char| !(is_ident(x) || matches!(x, '-' | ':' | '.' | '@'))).unwrap_or(r.len());
                    out.push((i..end.max(i + 1), Kind::Key));
                    i = end.max(i + 1);
                } else if r.starts_with("/>") || r.starts_with("?>") {
                    out.push((i..i + 2, Kind::Heading));
                    i += 2;
                    break;
                } else {
                    i += c.len_utf8();
                }
            }
            continue;
        }
        if rest.starts_with('&') {
            if let Some(k) = rest.find(';').filter(|&k| k <= 10) {
                out.push((i..i + k + 1, Kind::Number));
                i += k + 1;
                continue;
            }
        }
        i += rest.chars().next().unwrap().len_utf8();
    }
}

fn css_line(s: &str, st: &mut State, out: &mut Vec<Span>) {
    let mut i = 0;
    if st.comment {
        match s.find("*/") {
            Some(k) => {
                out.push((0..k + 2, Kind::Comment));
                st.comment = false;
                i = k + 2;
            }
            None => {
                out.push((0..s.len(), Kind::Comment));
                return;
            }
        }
    }
    // "property: value;" inside a rule; anything else is a selector.
    let t = s[i..].trim_start();
    let start = s.len() - t.len();
    let declaration = t.find(':').filter(|&k| !t[..k].contains(['{', ' ', '&', '>', '.', '#']) && !t.trim_end().ends_with('{') && k > 0);
    let mut i = start;
    if let Some(k) = declaration {
        out.push((start..start + k, Kind::Key));
        i = start + k + 1;
    }
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap();
        if rest.starts_with("/*") {
            match rest.find("*/") {
                Some(k) => {
                    out.push((i..i + k + 2, Kind::Comment));
                    i += k + 2;
                }
                None => {
                    out.push((i..s.len(), Kind::Comment));
                    st.comment = true;
                    return;
                }
            }
        } else if rest.starts_with("//") && (i == 0 || s[..i].ends_with(char::is_whitespace)) {
            out.push((i..s.len(), Kind::Comment));
            return;
        } else if c == '"' || c == '\'' {
            let end = string_end(s, i, c);
            out.push((i..end, Kind::String));
            i = end;
        } else if c.is_ascii_digit() && (i == 0 || !s[..i].ends_with(|x: char| is_ident(x) || x == '-')) {
            let end = number_end(s, i);
            let end = if s[end..].starts_with('%') { end + 1 } else { end };
            out.push((i..end, Kind::Number));
            i = end;
        } else if c == '#' && declaration.is_some() {
            let end = i + 1 + rest[1..].find(|x: char| !x.is_ascii_hexdigit()).unwrap_or(rest.len() - 1);
            out.push((i..end, Kind::Number));
            i = end.max(i + 1);
        } else if (c == '@' || c == '$') && rest[1..].starts_with(is_ident_start) {
            let end = i + 1 + rest[1..].find(|x: char| !(is_ident(x) || x == '-')).unwrap_or(rest.len() - 1);
            out.push((i..end, if c == '@' { Kind::Keyword } else { Kind::Type }));
            i = end;
        } else if declaration.is_none() && (is_ident_start(c) || c == '.' || c == '#') && !t.trim_end().ends_with(';') {
            let end = i + c.len_utf8() + rest[c.len_utf8()..].find(|x: char| !(is_ident(x) || x == '-')).unwrap_or(rest.len() - c.len_utf8());
            out.push((i..end, Kind::Heading));
            i = end;
        } else if is_ident_start(c) {
            let end = i + rest.find(|x: char| !(is_ident(x) || x == '-')).unwrap_or(rest.len());
            if s[end..].starts_with('(') {
                out.push((i..end, Kind::Function));
            }
            i = end;
        } else {
            i += c.len_utf8();
        }
    }
}

/// The coloured pieces of one line; `st` is what the lines before it left open and is updated.
pub fn line(lang: &Lang, text: &str, st: &mut State) -> Vec<Span> {
    let mut out = vec![];
    // A very long line (minified code) is not worth the work.
    if text.len() > 4000 {
        return out;
    }
    match lang.mode {
        Mode::Code => code_line(lang, text, st, &mut out),
        Mode::Json => json_line(text, &mut out),
        Mode::Yaml => key_value_line(text, ':', &mut out),
        Mode::Toml => key_value_line(text, '=', &mut out),
        Mode::Markdown => markdown_line(text, st, &mut out),
        Mode::Markup => markup_line(text, st, &mut out),
        Mode::Css => css_line(text, st, &mut out),
    }
    out
}

/// The colours of both sides of a diff: per row of the side-by-side view (left, right) and per
/// line of the unified view.
#[derive(Default)]
pub struct DiffColors {
    pub left: Vec<Vec<Span>>,
    pub right: Vec<Vec<Span>>,
    pub unified: Vec<Vec<Span>>,
}

/// The colours of the diff of the file `path`, or nothing to colour when the language is not
/// known or the file is very big.
pub fn for_diff(path: &str, d: &crate::git::DiffText) -> DiffColors {
    let size: usize = d.unified.iter().map(|(_, t)| t.len()).sum();
    match lang_for(path) {
        Some(lang) if size < 4_000_000 => diff_colors(lang, d),
        _ => DiffColors::default(),
    }
}

/// The colours of a whole file, line by line.
pub fn for_lines<'a>(path: &str, lines: impl Iterator<Item = &'a str>) -> Vec<Vec<Span>> {
    let Some(lang) = lang_for(path) else { return vec![] };
    let mut st = State::default();
    lines.map(|t| line(lang, &t.replace('\t', "    "), &mut st)).collect()
}

/// Colour a diff. The lines are coloured as shown: a tab is four spaces.
pub fn diff_colors(lang: &Lang, d: &crate::git::DiffText) -> DiffColors {
    let shown = |t: &str| t.replace('\t', "    ");
    let mut c = DiffColors::default();
    let (mut l, mut r) = (State::default(), State::default());
    for row in &d.rows {
        if row.left.is_none() && row.right.is_none() {
            // Lines were left out here: what was open is unknown.
            (l, r) = (State::default(), State::default());
        }
        c.left.push(row.left.as_ref().map(|(_, t)| line(lang, &shown(t), &mut l)).unwrap_or_default());
        c.right.push(row.right.as_ref().map(|(_, t)| line(lang, &shown(t), &mut r)).unwrap_or_default());
    }
    let (mut old, mut new) = (State::default(), State::default());
    for (tag, text) in &d.unified {
        let t = shown(text);
        c.unified.push(match tag {
            '@' => {
                (old, new) = (State::default(), State::default());
                vec![]
            }
            '-' => line(lang, &t, &mut old),
            '+' => line(lang, &t, &mut new),
            _ => {
                // An unchanged line is part of both versions.
                line(lang, &t, &mut old);
                line(lang, &t, &mut new)
            }
        });
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The coloured words of a line as (text, kind).
    fn words<'a>(path: &str, text: &'a str) -> Vec<(&'a str, Kind)> {
        let mut st = State::default();
        line(lang_for(path).unwrap(), text, &mut st).into_iter().map(|(r, k)| (&text[r], k)).collect()
    }

    #[test]
    fn knows_languages_by_file_name() {
        assert!(lang_for("src/main.rs").is_some() && lang_for("a/b/Dockerfile").is_some() && lang_for("x.TSX").is_some());
        assert!(lang_for("notes.txt").is_none() && lang_for("LICENSE").is_none());
    }

    #[test]
    fn colours_rust() {
        use Kind::*;
        // Lifetimes are not strings.
        assert_eq!(words("a.rs", "pub fn total<'a>(items: &'a [Item]) -> u32 { // sum"), [("pub", Keyword), ("fn", Keyword), ("Item", Type), ("u32", Type), ("// sum", Comment)]);
        assert_eq!(words("a.rs", "let c = 'x'; let s = \"a\\\"b\"; println!(\"{}\", 0x1f + 2.5e-3);"), [("let", Keyword), ("'x'", String), ("let", Keyword), ("\"a\\\"b\"", String), ("println!", Function), ("\"{}\"", String), ("0x1f", Number), ("2.5e-3", Number)]);
        assert_eq!(words("a.rs", "Some(x) => foo(x),"), [("Some", Type), ("foo", Function)]);
    }

    #[test]
    fn carries_comments_and_long_strings_over_lines() {
        let lang = lang_for("a.py").unwrap();
        let mut st = State::default();
        let a = line(lang, "x = \"\"\"start", &mut st);
        assert_eq!(a.last().unwrap().1, Kind::String);
        assert_eq!(line(lang, "def not_code(): pass", &mut st), [(0..20, Kind::String)]);
        let c = line(lang, "end\"\"\" + 1  # done", &mut st);
        assert_eq!(c, [(0..6, Kind::String), (9..10, Kind::Number), (12..18, Kind::Comment)]);

        let lang = lang_for("a.c").unwrap();
        let mut st = State::default();
        assert_eq!(line(lang, "int a; /* one", &mut st), [(0..3, Kind::Type), (7..13, Kind::Comment)]);
        assert_eq!(line(lang, "two */ return 0;", &mut st), [(0..6, Kind::Comment), (7..13, Kind::Keyword), (14..15, Kind::Number)]);
    }

    #[test]
    fn colours_data_and_text_formats() {
        use Kind::*;
        assert_eq!(words("a.json", "  \"name\": \"demo\", \"n\": -3.5, \"on\": true"), [("\"name\"", Key), ("\"demo\"", String), ("\"n\"", Key), ("-3.5", Number), ("\"on\"", Key), ("true", Keyword)]);
        assert_eq!(words("a.yml", "  - image: \"nginx:1.25\"  # web"), [("image", Key), ("\"nginx:1.25\"", String), ("# web", Comment)]);
        assert_eq!(words("a.yaml", "replicas: 3"), [("replicas", Key), ("3", Number)]);
        assert_eq!(words("a.yaml", "date: 2024-01-15"), [("date", Key)]);
        assert_eq!(words("Cargo.toml", "[dependencies]"), [("[dependencies]", Heading)]);
        assert_eq!(words("Cargo.toml", "version = \"1.2\" # pinned"), [("version", Key), ("\"1.2\"", String), ("# pinned", Comment)]);
        assert_eq!(words("a.md", "## Title"), [("## Title", Heading)]);
        assert_eq!(words("a.md", "- run `make` or see [docs](http://x)"), [("-", Keyword), ("`make`", String), ("[docs]", Function), ("(http://x)", Comment)]);
        assert_eq!(words("a.html", "<a href=\"x\" class='y'>text</a>"), [("<a", Heading), ("href", Key), ("\"x\"", String), ("class", Key), ("'y'", String), (">", Heading), ("</a", Heading), (">", Heading)]);
        assert_eq!(words("a.css", "  color: #fff;"), [("color", Key), ("#fff", Number)]);
        assert_eq!(words("q.sql", "SELECT id FROM users -- all"), [("SELECT", Keyword), ("FROM", Keyword), ("-- all", Comment)]);
    }

    #[test]
    fn colours_both_sides_of_a_diff() {
        use crate::git::{DiffRow, DiffSide, DiffText};
        let row = |kind, l: Option<&str>, r: Option<&str>| DiffRow { kind, left: l.map(|t| (1, t.to_string())), right: r.map(|t| (1, t.to_string())) };
        let d = DiffText {
            rows: vec![row(DiffSide::Context, Some("/* note"), Some("/* note")), row(DiffSide::Changed, Some("\told */"), Some("\tnew */ x = 1;")), row(DiffSide::Added, None, Some("return x;"))],
            binary: false,
            unified: vec![('@', "@@ -1,2 +1,3 @@".into()), (' ', "/* note".into()), ('-', "\told */".into()), ('+', "\tnew */ x = 1;".into()), ('+', "return x;".into())],
            images: None,
        };
        let c = diff_colors(lang_for("a.c").unwrap(), &d);
        // The comment opened on the first line closes on each side's own second line (the tab
        // is four columns wide).
        assert_eq!(c.left[1], [(0..10, Kind::Comment)]);
        assert_eq!(c.right[1], [(0..10, Kind::Comment), (15..16, Kind::Number)]);
        assert_eq!(c.right[2][0], (0..6, Kind::Keyword));
        assert!(c.left[2].is_empty());
        assert_eq!(c.unified[2], [(0..10, Kind::Comment)]);
        assert_eq!(c.unified[3][0], (0..10, Kind::Comment));
        assert_eq!(c.unified[4][0], (0..6, Kind::Keyword));
    }
}
