---
name: changelog
description: Update the RefBraid changelog (app/CHANGELOG.md in Russian and app/CHANGELOG.en.md in English) from the git commits made since the last commit already covered, in the section of the current version from app/Cargo.toml. Use when asked to update or put together the changelog or "what's new", on the /changelog command, and from make release-all.
---

# The RefBraid changelog

The changelog is shown in the program, in **Help → Changelog**. Two files are built into the
program (`include_str!`) and the one that matches the interface language is shown:

- `app/CHANGELOG.md` — in Russian;
- `app/CHANGELOG.en.md` — the same sections in English.

It says what has changed **for the person using the program**, in plain words. It is not a
list of commits.

## Format

```markdown
# Changelog

<!-- last-commit: <full hash of the last commit covered> -->

## 0.3.3 — 2026-11-20

### New
- ...

### Fixed
- ...

## 0.3.2 — 2026-11-16
...
```

- Sections go by version, newest first. The heading is `## <version> — <YYYY-MM-DD>`; the
  date is the day of the release (today). The dash is an em dash, `—`.
- A section has only two groups, in this order. Leave out a group that would be empty.

  | `app/CHANGELOG.en.md` | `app/CHANGELOG.md` |
  |---|---|
  | `### New` | `### Новое` |
  | `### Fixed` | `### Исправлено` |

- There is one `<!-- last-commit: ... -->` line, right after `# Changelog`: the commit up to
  which the changelog is written. It is the same in both files.
- Both files have the same versions with the same dates, in the same order, and the same
  items in the same order — one is the translation of the other. A test fails the build when
  the first section is not the current version of `app/Cargo.toml`, or when the two files
  list different versions.
- The window in the program understands only this markup: `#`, `##`, `###`, items starting
  with `- `, `**bold**`, `` `code` ``, empty lines. No tables, links, nested lists or HTML
  entities (`&lt;`).

## Steps

1. The version is the one in `app/Cargo.toml` (`version = "…"` under `[package]`), unless
   another one was given.
2. Read the `last-commit` mark in `app/CHANGELOG.md` and list the commits made since:
   ```bash
   git log <last-commit>..HEAD --no-merges --reverse --format='%H|%ad|%s' --date=short
   ```
3. Drop what the person using the program does not see (see below).
4. When a commit message does not make clear what changed, look at `git show --stat <hash>`
   and, if needed, at the diff, to find out what the person sees. Do not make up anything
   that is not in the code.
5. Several commits about the same thing become one item.
6. When the section `## <version>` is already there, add to it (leave its date as it is).
   Otherwise put the new section first, right after the `last-commit` line.
7. When nothing visible has changed at all, still make the section, with one item under
   `### Fixed`: `- Internal improvements with no visible changes.` (in the Russian file,
   under `### Исправлено`: `- Внутренние улучшения без видимых изменений.`).
8. Write the section in both files: first one, then its translation in the other.
9. Set `last-commit` in both files to the full hash of the last commit looked at (`HEAD`),
   even when that commit itself did not make it into the changelog.
10. **Do not commit.** With `make release-all` the commit, the tag and the build are done by
    `app/packaging/release.sh`. When run by hand, show the resulting section and the list of
    commits left out, each with its reason in one line.

## What stays out

- Commits marked `[chore]`, `[docs]`, `[refactor]`, `[test]`, `[ci]`: version changes,
  CLAUDE.md, this skill.
- Building and packaging: the Makefile, the scripts in `packaging/`, names of installer
  files, the Containerfile, dependencies.
- Things that exist only for development: the `--screenshot` mode, the variables
  `REFBRAID_VIEW`, `REFBRAID_POINTER` and the like, tests.
- A `[feat]` or `[fix]` that is internal through and through (changes nothing the person
  sees or does) is left out too.

## How to write

- In the words of the person using the program. English: "Added the … window", "The Log now
  has …", "Fixed: … did not …". Russian: «Добавлено окно …», «В журнале появился …»,
  «Исправлена ошибка, из-за которой …».
- Names of windows, menus, buttons and settings are written exactly as in the interface, in
  English, in both files (the interface text is English in the code): **Push**,
  **Smart Checkout**, Settings → Window. Take them from the code (`app/src/`); do not
  translate them.
- A big new feature is in bold: `**Favorites**`. Keyboard shortcuts are in code:
  `` `Ctrl+Alt+A` ``.
- No commit hashes, no names of source files, no internal names of types and functions.
- One item is one change, in one or two sentences.
