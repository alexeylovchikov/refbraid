# AGENTS.md

This file provides guidance to Codex (claude.ai/code) when working with code in this repository.

## What this is

RefBraid: a standalone desktop Git client (Log with branch tree and commit graph, commit panel, diffs, push/pull, merge-conflict resolution, AI commit messages) inspired by the Git tool window of JetBrains IDEs. Rust + eframe/egui, targets Linux (RPM, AppImage) and Windows.

All program code lives in `app/`. The repository root holds the licence (`LICENSE`, `THIRD-PARTY-NOTICES.md`), `README.md` and the CI pipeline.

**Clean room:** the program is written from descriptions of behaviour and from what is publicly visible on the screen only. Do not read, decompile, run for inspection, or copy anything from the installation, the sources or the resources of any JetBrains IDE or of any other Git client; do not add third-party images, icons or fonts to the repository (icons are drawn in code).

## Commands

Run from `app/` (`make` with no target lists everything):

```bash
make build            # cargo build (debug; dependencies are still optimised)
make run              # cargo run            — or: cargo run -- /path/to/repo
make test             # cargo test
cargo test graph::    # tests of one module; cargo test <test_name> for a single test
make release          # cargo build --release
```

Tests of git operations use `testrepo.rs` (`TestRepo::new("tag")`: a throw-away repository with its own author, `git`, `write`, `commit`, `run` for a job's commands, `app`; it removes itself) — use it for new tests instead of building a repository by hand. CI is GitHub Actions (`.github/workflows/`): `ci.yml` builds and tests on Linux and Windows on every push and pull request; `release.yml` runs on a `vX.Y.Z` tag, builds the installers through the same `packaging/build-*.sh` scripts (`GE_IN_CONTAINER=1` makes them build where they are instead of starting a container) and publishes them as a GitHub release with the changelog section of that version.

There is no lint/format configuration; the code is not rustfmt-default (long lines), so don't reformat files wholesale.

Versioning and installers:

```bash
make bump-patch | bump-minor | bump-major | set-version V=x.y.z | bump-release
make rpm-el10         # RPM built on this machine
make rpm-el9 rpm-el8 windows appimage   # built in podman/docker containers
make dist             # rpm-el10 + rpm-el9 + windows + appimage, into app/dist/
```

`make release-all` (`packaging/release.sh`, `BUMP=minor|major|x.y.z` for other than patch) requires a clean `app/`, raises the version, runs `Codex -p` with the `changelog` skill (`.Codex/skills/changelog/SKILL.md`) to add the new version's section to `app/CHANGELOG.md`, commits `[chore] Release version X…`, tags `vX`, then runs `make dist`; on failure before the commit it puts the version files back. `make changelog` runs only the changelog step.

`app/CHANGELOG.md` is built into the binary (`include_str!` in `changelog.rs`, shown by Help → Changelog) and a test requires its first section to be the current `Cargo.toml` version — so a version bump without a changelog section fails `cargo test`. The window renders only the Markdown subset the skill describes; the `<!-- last-commit: … -->` marker records the last commit already covered.

The version is written in four places (`Cargo.toml`, `Cargo.lock`, `packaging/windows/refbraid.rc`, and the RPM spec's release number); the Windows MSI (`packaging/windows/refbraid.wxs`, built with `wixl`) gets it on the command line. Never edit them by hand — `packaging/set-version.sh` (behind the `make bump-*` targets) keeps them in sync.

### Checking the UI without a display

```bash
cargo run -- [REPO] --screenshot out.png
```

Opens the window, captures it on frame 120, saves the PNG and exits. With no `REPO` it builds a demo repository (`demo.rs`). If no picture is saved within 20 s (remote session, no compositor), `headless.rs` rasterises the same egui output in software at 1600×820 and saves that instead.

`REFBRAID_VIEW` picks the screen to capture (e.g. `selected`, `diff`, `commit`, `console`, `settings`, `gitconfig`, `mergeai`, `atomicplan`, `irebase`, `filehistory`, `rangelog`, `annotate`, `reflog`, `joinopts`, `pickopts`, `wipefiles`, `getrepo`, `jobrun`, `takeback`, `findtext`, `wholehistory`, `mergelookup`, `shelfopen`, `shelfnew`, `filerevs`, `filebranches`, `twoversions`, `hookfail`, `halving`, `twintabs`, `findaction`, `pickbranch`, `keysetup`, `sshkeys`, `signinfail`, `caretmove`, `branchmenu`, `sortmenu`; grep `view.contains(` for the full list — the handlers are in `App::new` and at the end of `App::draw` in `app.rs`). Values are matched with `contains`, so several can be combined, but names also overlap: `settings` fires inside `aisettings`, `commit` inside `commitsettings`, `merge` inside `mergecommit`. Pick new names that don't contain an existing one. When adding a screen that is hard to reach, add a switch for it the same way.

Other environment switches: `REFBRAID_THEME` (`light` / `dark`, whatever the settings say — for screenshots of either theme), `REFBRAID_SEARCH` (the text for the `wholehistory` / `loadedonly` views), `REFBRAID_WAYLAND` (don't force X11 on Linux), `REFBRAID_ASKPASS` (internal: the binary re-runs itself as git's askpass helper, `askpass.rs`). In the same way `refbraid --rebase-todo <plan> <git's todo>` is the binary acting as git's sequence editor for an interactive rebase (`rebase.rs`).

## Architecture

Single binary crate, flat `app/src/` module list, no workspace.

**Git access (`git.rs`)** — no libgit2: everything shells out to the `git` CLI and parses its text output (`run` / `run_env`, `load_state`, `load_log`, `status`, `parse_diff`, …). The git program path is a process-global (`git::set_program`, from Settings). Spawn git through `git::command` / `git::no_console` so the askpass hookup and the hidden console window on Windows are applied.

**State (`app.rs`)** — several repositories can be open as tabs: the one in front has its state in `App` itself, the others are parked in `App::parked` as `Workspace` values (`swap_workspace` lists the per-repository fields — a new field that belongs to one repository must be added to `Workspace` and to that list). One `App` struct owns everything: `Settings`, the open `Repo` (refs, commits, graph rows, status, per-tab `LogState`), open diff tabs, the current `Dialog`, toasts and the git console. `App::draw` is the single per-frame entry point (called by eframe and by the headless renderer): it polls finished jobs, handles shortcuts, checks for outside changes, then lays out the panels.

**Views are `impl App` blocks split across files**, not separate widgets with their own state: `chrome.rs` (title bar, menus, stripes, status bar, welcome, shortcuts, auto-refresh), `branches_view.rs`, `log_view.rs`, `details_view.rs`, `commit_view.rs`, `split_view.rs` (side-by-side diff), `image_view.rs`, `merge_view.rs`, `rebase_view.rs` (Interactive Rebase window), `blame_view.rs` (Annotate tab), `reflog_view.rs` (Reflog tab), `bisect_view.rs` (Bisect window and banner), `clone_view.rs` (Clone window, New Repository), `push_view.rs`, `config_view.rs`, `settings_view.rs`, `ai_view.rs`, `branch_menu.rs`, `dialogs.rs`. Pure logic sits in view-free modules that carry the unit tests: `graph.rs` (lane layout of the commit graph), `partial.rs` (diff blocks for partial commits / per-change revert), `merge.rs` (conflict document model), `rebase.rs` (interactive rebase plan and the todo list for git), `blame.rs` (reading `git blame --porcelain`), `reflog.rs` (HEAD's reflog and the commits no ref reaches), `bisect.rs` (the state of a bisect in progress), `undo.rs` (ref snapshots around a job and the commands that undo it), `ops.rs` (the git commands behind the Commit, Reset and Delete Branch buttons, built apart from their windows), `credentials.rs` (what sign-in errors mean, SSH keys, the credential helper, ssh's question about a new server), `encoding.rs` (reading lines that are not UTF-8 in a chosen one-byte encoding), `keymap.rs` (the table of actions and their shortcuts: key handling, Settings → Keymap, the command palette and the Keyboard Shortcuts window are built from it; `App::run_action` does what an action stands for), `find.rs` (search in a diff), `syntax.rs` (table-driven syntax colouring for diffs and Annotate), `ignore.rs` (ignore lines, `git clean` preview), `skills.rs` (SKILL.md discovery), `ai.rs` (CLI agent harnesses).

**Mutating git operations go through jobs.** Build a `Job` (`Job::one` / `Job::many`, optional `.then(Then::…)`) and pass it to `App::run_job`: the commands run sequentially on a worker thread and stop at the first failure; `poll_jobs` then shows a toast, runs the follow-up and calls `refresh()`. Each command runs through `git::run_live`: its stderr reaches the console and the status bar (progress, Cancel) while it runs, and fetch / pull / push / clone get `--progress`. A command vector may start with `env:NAME=value` items to set environment variables for that command. `Job::in_dir` runs a job outside the open repository (clone, init). Around every job of the open repository `undo::snapshot` records HEAD, branches, tags and stashes; the difference becomes `App::undo` (Git → Undo), so a new kind of job is undoable without extra code as long as it only moves refs. Read-only loading is done synchronously or via its own channels, not via `Job`. The log is the exception worth knowing: `App::reload_log` reads it on a thread and waits only a moment (`LOG_WAIT_MS`; for ever in screenshot mode and tests) — on a big repository the old list stays until `poll_log` swaps the new one in, so code that needs the fresh list right after `reload_log` must cope with `repo.log_pending`. The changed files and the branches of the selected commit are read the same way: `App::refresh` (after a job) waits for `git status`, `refresh_soon` (auto-refresh, F5, opening a repository) leaves it to `take_pending_status`; `details_of` shows a commit at once and `poll_refs` fills in `containing` / `tags` (`refs_loading` until then).

**Auto-refresh** — `App::watch` compares `git::fingerprint` of the repository every 2 s and on window focus, and reloads when it changed (disabled in screenshot mode and by a setting).

**UI conventions** — all colours, fonts and icons come from `theme.rs`. There are two themes: the palette colours are functions (`theme::BG()`, `theme::TEXT()`…) that answer for the current one, and a tint written as a literal for the dark theme goes through `theme::rgb(r, g, b)` (or `theme::adapt` for a module `const`), which gives its light counterpart — don't use `Color32::from_rgb` directly in views, and don't pass a colour that is already themed through them again; icons are hand-drawn with the painter (`theme::Icon`), there are no image assets (the app icon is also drawn in code, `integrate.rs`, and exported for the installers with `--write-icon`). Every modal window uses `form::show` (title bar, body, right-aligned footer) for a uniform look. UI text is written in English in the code and wrapped in `tr("…")` (`i18n.rs`; `trf` for text with values, `plural` for counted nouns); the Russian version of each string goes into `i18n_ru.rs`, keyed by the exact English text — a string without an entry there is shown in English. The language is chosen in Settings → Window → Language.

**Settings (`settings.rs`)** — a hand-rolled `key=value` line file at `$XDG_CONFIG_HOME/refbraid/settings.txt` (`%APPDATA%` on Windows); no serde. A new setting needs a field, a line in `save` and a match arm in `load`.

**AI features** — `ai.rs` runs an installed coding-agent CLI (Codex, Codex, Gemini, …; table `HARNESSES`) as a subprocess with a prompt on stdin or as an argument. Used for commit messages, splitting changes into atomic commits (AI Tasks menu) and resolving merge conflicts (by prompt or by a discovered skill).

**Platform specifics** — on Linux the app prefers X11 or draws its own title bar (`builtin_title_bar` setting); on Windows it tries wgpu restricted to DX12 first (Vulkan is left out on purpose: an old Intel Vulkan driver crashed the process at startup; `WGPU_BACKEND` overrides) and falls back to glow, startup errors and panics go to a message box and `<config dir>/error.log`, and `build.rs` embeds the icon/version resource. File-manager integration (Nautilus "Open With" / Explorer folder menu) is in `integrate.rs` and `packaging/`.

## Commit messages

History uses `[prefix] Description.` in English: one sentence in the imperative mood, starting with a capital letter and ending with a period, no colon or dash after the prefix — e.g. `[feat] Add the Git Config window.`, `[fix] Open folders in Explorer by a path with backslashes.`, `[chore] …`. The prefixes are `[feat]`, `[fix]`, `[docs]`, `[chore]`, `[refactor]`, `[test]`, `[ci]`.
