# RefBraid — what is missing

The list of tasks was drawn up on 2026-11-17 from the sources of version 0.3.4 (`app/src`) and updated on 2026-11-24 (items 2.4, 3.6, 4.3 and the order of work). It shows which features of a usual Git client the program does not have yet or has only in part.

Tasks that are done carry the note "Implemented after 0.3.4" under the heading, saying what went in and what is left; the description below it is the original statement of the task. What appeared beyond the list is given in the section "Done beyond the list" at the end.

Every task is described in the same way:

- **Why** — which problem of the user it solves.
- **Now** — what is already in the code.
- **To do** — what has to appear.
- **Where** — the modules that will have to change.
- **Done when** — how to tell that the task is closed.

General rules for all tasks (in detail in `CLAUDE.md`):

- git is called only through `git::run` / `git::command`, libgit2 is not used;
- mutating operations go through `Job` and `App::run_job`, reading is done synchronously or over its own channel;
- modal windows are built through `form::show`, colours and icons come from `theme.rs`;
- a screen that is hard to reach gets a `REFBRAID_VIEW` switch;
- the interface text is in English.

Size: **S** — up to a day, **M** — a few days, **L** — a week or more.

---

## 1. High priority

Without these features the client does not replace the command line in daily work.

### 1.1. Interactive rebase (L) — done

> Implemented after 0.3.4: the **Interactive Rebase** window (`rebase.rs`, `rebase_view.rs`) and the Log menu items `Interactively Rebase from Here…`, `Squash Commits…`, `Squash into Parent…`, `Drop Commit…`, `Edit Commit Message…` for any commit. The **Suggest with AI** button in the window asks the AI agent from the settings to suggest a plan (what to squash, reword, drop, reorder); it can be undone with the Undo button and corrected by hand. Not done: dragging rows with the mouse (there are buttons and `Alt+↑/↓`) and ranges with merge commits — the window does not open for them.

- **Why.** Before a branch is pushed its history is put in order: changes are squashed, commits are reworded and dropped. This is the most frequent reason to go to the terminal.
- **Now.** There are `Rebase '<branch>' onto …`, `Edit Commit Message…` (for the last commit only, through `commit --amend`) and continuing/aborting a rebase in progress.
- **To do.**
  - The Log item `Interactively Rebase from Here…` opens a window with the list of commits from the selected one to `HEAD`.
  - An action is chosen for every commit: pick, reword, squash, fixup, drop; the rows can be reordered.
  - Quick items for the selected commits: `Squash Commits…`, `Fixup`, `Drop Commit`, `Edit Commit Message…` for any commit, not only the last one.
  - It is started through `git rebase -i` with a `GIT_SEQUENCE_EDITOR` that puts in the prepared list (the binary itself in the role of the editor, as is done for askpass).
  - On a conflict the existing conflict resolution panel and the Continue / Skip / Abort buttons work.
- **Where.** `log_view.rs` (the menu), a new `rebase_view.rs`, `app.rs` (`Dialog`, a `Job` with `env:`), `main.rs` (the editor mode), `git.rs`.
- **Done when.** Three commits can be squashed into one, swapped and reworded without the terminal; on a conflict the rebase is brought to the end from the program; pushed commits are marked with a warning.

### 1.2. File history (M) — done

> Implemented after 0.3.4: the `Show History` item in the file menu (Changes, the files of a commit, the diff) opens the Log tab `History: <file>` with `git log --follow`; on the left is the list of commits with the details of the selected commit under it, on the right the diff of the file in that commit (with a Git Blame column on request). The filter row of the tab keeps only the text, the user and the date, and the branch labels in the rows of the list are removed. After 0.3.5: `Show History for Selection` in the diff menu opens the tab `History: <file>:<lines>` with the commits that changed the selected lines (`git log -L`); the button with a branch in the filter row adds the commits of all branches (`--all`); the hash is shown in the narrow list too. Not done: in the history of lines the diff of the whole file is shown on the right, not only of the selected lines; for local changes the line numbers are taken from the working file while the history is read from `HEAD`, so with unsaved changes above the selection the range may shift.

- **Why.** "Who changed this file and when" is the second most frequent question after looking at the log.
- **Now.** The log has a filter by path, but it does not follow renames and cannot be called from the list of files.
- **To do.**
  - The `Show History` item in the file menu: in Changes, in the files of a commit, in the diff.
  - A separate Log tab for the file with `git log --follow`; every entry shows the diff of this very file.
  - `Show History for Selection` for the selected lines (`git log -L`).
- **Where.** `git.rs` (`LogQuery` with `follow`), `log_view.rs`, `commit_view.rs`, `details_view.rs`.
- **Done when.** For a renamed file the history before the rename is shown, and a click on an entry opens its diff in that commit.

### 1.3. Blame / Annotate (M) — done

> After 0.3.5 the tab got syntax colouring (see 1.6). Implemented after 0.3.4: the `Annotate` item in the file menu (Changes, the files of a commit, the diff) opens the tab `Annotate: <file>` (`blame.rs`, `blame_view.rs`): the date, the author and the hash at every group of lines, newer commits are tinted more strongly, a click goes to the commit in the log, there is `Annotate Previous Revision`. In the diff view the item `Annotate with Git Blame` turns on a column with the date and the author at every line of the new version — in the "Side-by-side" and "Unified" modes and in the file history tab. `git blame` runs on a background thread. Not done: selecting text in the tab; an open tab is not updated when the file changes.

- **Why.** To find out by which commit and by whom a particular line was written.
- **Now.** There is nothing.
- **To do.**
  - The `Annotate` item in the file menu and in the diff.
  - A column to the left of the text: the date, the author, the short hash; the lines of one commit are filled with one shade.
  - A click on the column selects the commit in the log; `Annotate Previous Revision` goes one step back.
  - The data comes from `git blame --porcelain`, loaded in the background.
- **Where.** `git.rs` (parsing the porcelain output), a new `blame_view.rs`, `split_view.rs`.
- **Done when.** A file of 5000 lines is annotated without the window freezing, going to the commit works.

### 1.4. Clone and Init (M) — done

> Implemented after 0.3.5 (`clone_view.rs`): `File → Clone…` and a button on the welcome screen open a window with the address, the destination folder, the folder name (filled in from the address), the branch, the depth and the submodules checkbox; cloning runs as an ordinary job with progress and cancelling, then the repository is opened. `File → New Repository…` runs `git init` in the chosen folder; if a folder without a repository is opened, the program offers to create one. Not verified: cloning over HTTPS and SSH with a password prompt (a clone over `file://` is verified).

- **Why.** Now the program cannot be used to start work from scratch: the repository has to be fetched in the terminal first.
- **Now.** Only `Open Repository…` for an existing folder.
- **To do.**
  - `File → Clone…`: the URL, the destination folder, an optional branch and depth (`--depth`), the `--recurse-submodules` checkbox.
  - The progress of cloning and a cancel button (see 3.4).
  - `File → New Repository…` (`git init`) and an offer to create a repository if a folder without `.git` is opened.
  - Both items on the welcome screen.
- **Where.** `chrome.rs` (the menu, welcome), `dialogs.rs`, `app.rs`. `Job` is now tied to the open repository — clone needs to be started without one.
- **Done when.** A repository is cloned over HTTPS and SSH with a password prompt through askpass and is opened at once.

### 1.5. Reflog (M) — done

> Implemented after 0.3.4: `Git → Reflog` opens the **Reflog** tab (`reflog.rs`, `reflog_view.rs`) with the entries about the movements of `HEAD`: the time, the commit, the action. Commits that no branch or tag reaches are marked `lost`; there is the filter "Only lost commits". On the right are the details of the commit and the list of its files. In the menu of an entry: `New Branch…`, `Reset Current Branch to Here…`, `Checkout Revision`, `Show in Log`. Not done: showing unreachable commits in the graph of the log, viewing the diff of the files of a lost commit, the reflog of separate branches (only `HEAD` is read).

- **Why.** To get commits back after an unsuccessful reset, rebase or deletion of a branch. For a client where reset is two clicks away this is a safety net.
- **Now.** Nothing.
- **To do.**
  - A mode of the log or a separate `Reflog` tab (`git reflog --format=…`) with the action and the time of every entry.
  - For an entry: `Checkout`, `New Branch from Here…`, `Reset Current Branch to Here…`.
  - Unreachable commits can be shown in the graph.
- **Where.** `git.rs`, `log_view.rs` or a new `reflog_view.rs`.
- **Done when.** After `Reset --hard` the lost commit is found and brought back as a branch.

### 1.6. Syntax colouring in the diff (M) — done

> Implemented after 0.3.5 (`syntax.rs`): a light tokeniser of its own with no external libraries. Keywords, types, strings, numbers, comments and function calls are coloured in Rust, Python, JS/TS, C/C++, Java/Kotlin/C#, Go, Swift, PHP, Ruby, shell, SQL; keys and values in JSON, YAML, TOML; headings, code and links in Markdown; tags and attributes in HTML/XML; CSS. It works in both diff modes, in the file history tab and in Annotate; multi-line comments and strings are tracked separately for the old and the new version. The markup is computed once when the diff is opened; the highlighting of changed words is laid on top. The switch is Settings → Window → Diff. Later the conflict resolution window was coloured too: all three columns, including the editable result; a comment or a multi-line string opened above continues across blocks. Not done: the language is determined only by the file name; files larger than 4 MB and lines longer than 4000 characters are not coloured.

- **Why.** Code without colouring is noticeably harder to read, especially in the Side-by-side mode.
- **Now.** Only the changed words are highlighted (`wordiff.rs`).
- **To do.**
  - Colouring by the file extension; the `syntect` library or a light tokeniser of its own for the main languages.
  - The syntax colours in `theme.rs`; the highlighting of changed words is laid on top.
  - The markup is computed once per file and cached, big files — lazily by the visible lines.
  - A switch in Settings.
- **Where.** `split_view.rs`, `commit_view.rs` (Unified), `merge_view.rs`, `theme.rs`, `Cargo.toml`.
- **Done when.** Rust, Python, JS/TS, JSON, YAML, Markdown are coloured, scrolling a file of 10,000 lines stays smooth.

### 1.7. Text search in the diff (S) — done

> Implemented after 0.3.5 (`find.rs`): `Ctrl+F` in the diff tab and in the diff of the file history (or the `Find…` item in the menu) opens a search bar: highlighting of the matches, the counter "N of M", `F3` / `Shift+F3` / `Enter`, case sensitivity and regular expressions, `Esc` closes it. A match among collapsed lines expands them. It works in the "Side-by-side" and "Unified" modes. Later search was added to the conflict resolution window: `Ctrl+F` searches in the three columns, lines without a conflict are counted once (in the result), the place found is scrolled into view vertically and horizontally, `Esc` closes the search bar, not the window. Not done: replacing what is found.

- **Why.** In a long diff the needed place is looked for by a word.
- **Now.** `Ctrl+F` takes the user to the search of the log.
- **To do.** When the focus is in the diff, `Ctrl+F` opens a search bar above it: highlighting of the matches, a counter, `F3` / `Shift+F3`, case sensitivity and regular expressions (the `search_box` widget already exists).
- **Where.** `chrome.rs` (handling the shortcut), `split_view.rs`, `commit_view.rs`.
- **Done when.** Search works in both diff modes and in the conflict resolution window.

### 1.8. The log of big repositories (L) — done

> Implemented after 0.3.5: the log is read on a separate thread; if reading takes longer than a quarter of a second, the window keeps working with the old list and shows "Loading the log…", and the new list is put in when it is ready. On scrolling to the end the next portion is loaded (as many commits again). When a search text is set, a row appears under the filters with the number of what is found and the links "Search the whole history" (`git log --grep` over the messages of all commits) and "Search in the changes" (`git log -S`). The "by branches" sorting is now done by git itself (`--topo-order`) within the limit. Verified on an artificial repository of 300,000 commits: the first portion is read in about a second in the background, an old commit is found by the text of its message. Later `git status` was moved off the UI thread (after an operation of its own the program waits for it; on auto-refresh, `F5` and opening a repository — no longer than a quarter of a second) and so was the search for the branches that contain a commit (`branch --contains`: on 300,000 commits 2.8 s for every click; now the details are shown at once, the branches are filled in when ready, and no more than two searches run at the same time). It was decided not to do the extending of the graph and `--skip` — a measurement on 300,000 commits (release): `git log --date-order` takes 1.3–2.1 s almost regardless of `-n` and `--skip` (the time goes into walking the history), parsing and laying out the graph — 0.06 s for 100,000 commits and 0.22 s for 300,000, and all of this is already on a background thread. Only a commit-graph file in the repository itself (`git commit-graph write --reachable`) can noticeably speed up loading more. Not done: it was not checked on the Linux kernel.

- **Why.** On repositories of hundreds of thousands of commits the window freezes, and the old history is out of reach.
- **Now.** `git::load_log` reads `git log -n <limit>` (5000 by default) synchronously on the UI thread. The text filter and the filter by author are applied only to the commits already loaded.
- **To do.**
  - Loading the log on a worker thread over a channel, the interface is not blocked; the status bar shows the progress of loading.
  - Loading the next portion on scrolling to the end (`--skip` or continuing from the last hashes).
  - If a filter is set and not the whole history is loaded, search through git: `--grep`, `--author`, `-S` / `-G` (search in the content of the changes).
  - The graph layout (`graph.rs`) has to be able to be extended instead of being computed anew.
- **Where.** `git.rs`, `app.rs` (`reload_log`, `LogState`), `graph.rs`, `log_view.rs`.
- **Done when.** The Linux kernel repository opens without freezing, an old commit is found by the text of its message.

### 1.9. Merge options (S) — done

> Implemented after 0.3.5: the `Merge with Options…` item in the menu of a branch, a remote branch and a tag opens a window with the choice "default / `--no-ff` / `--ff-only` / `--squash`", the `--no-commit` checkbox and the message; the choice is remembered. After squash and `--no-commit` the Commit panel opens with the prepared message. The quick item `Merge '<branch>' into …` has stayed.

- **Why.** Teams have different merge rules: always a merge commit, fast-forward only, or squash.
- **Now.** `Merge '<branch>' into '<current>'` always runs `git merge --no-edit`.
- **To do.** A `Merge…` window with the choice `--no-ff` / `--ff-only` / `--squash`, the `--no-commit` checkbox and a message field. The last choice is remembered in the settings. The context menu item stays as the quick way with the default settings.
- **Where.** `branch_menu.rs`, `branches_view.rs`, `dialogs.rs`, `settings.rs`.
- **Done when.** All three kinds of merge are done from the window, after squash the Commit panel opens with the prepared message.

---

## 2. Medium priority

Not needed every day, but their absence is noticeable.

### 2.1. Stash: viewing and partial applying (M) — done

> Implemented after 0.4.0: each stash has a `Show Files` button that expands the list of its files (modified, deleted and untracked ones put away with them); a click opens the diff of a file, the `Take This File` menu item puts one file into the working folder (`git restore --source=<stash> --worktree`), and the stash stays; if the file has changes of its own, a confirmation is asked first. The `Stash Changes` window gets `--keep-index` and "only the selected files" (those selected in Changes). The `Branch…` button creates a branch from the stash (`git stash branch`). Not done: a file that the stash deletes cannot be taken; applying separate changes inside a file.

- **Now.** A list of stashes with Apply / Pop / Drop, creation with a message and `--include-untracked`.
- **To do.**
  - Viewing the files and the diff of the selected stash before applying it (`git stash show -p`).
  - Applying separate files from a stash.
  - A stash of the selected files only (`git stash push -- <paths>`) and `--keep-index`.
  - `New Branch from Stash…` (`git stash branch`).
- **Where.** `git.rs` (`stashes`), `commit_view.rs`, `dialogs.rs`.
- **Done when.** The contents of a stash are visible before applying it, and one file can be taken from it.

### 2.2. Worktrees: management (S) — done

> Implemented after 0.4.0: when the repository has additional working trees, the branch tree gets a `Worktrees` section (`git worktree list --porcelain`): the folder name, the checked-out branch or the hash, the marks "locked" and "folder missing"; the open folder is highlighted. A double-click opens the folder in this window; the menu has `Open`, `Open in New Window`, `Copy Path`, `Remove Worktree…` (before removal the number of uncommitted files is reported, and such a folder is removed with `--force`) and `Remove Stale Records` (`git worktree prune`). A local branch checked out in another folder is marked in the tree with the name of that folder, and an attempt to check it out explains the reason instead of showing git's error. Not done: locking and unlocking (`lock` / `unlock`), moving a folder (`move`).

- **Now.** Only `New Worktree from '<branch>'…`.
- **To do.** A `Worktrees` section in the branch tree (`git worktree list --porcelain`): open, remove (`remove`, with a confirmation when there are changes), `prune`. A branch occupied by another worktree is marked, and an attempt to check it out explains the reason.
- **Where.** `git.rs`, `branches_view.rs`, `branch_menu.rs`.
- **Done when.** All worktrees are visible in the tree, and any of them can be opened and removed from the program.

### 2.3. Submodules (L) — done

> Implemented after 0.4.3 (`submodule.rs`): when the repository has `.gitmodules`, the branch tree gets a `Submodules` section: the path, the recorded commit or "recorded → actual", the marks "not initialised", "conflict", "has changes". A double-click opens the submodule in a separate tab; the menu has `Open`, `Init`, `Update`, `Update with Nested Submodules`, `Sync Address`, `Update All Submodules`, copying of the path and of the address; `Git → Update Submodules` updates all of them. In Changes a submodule is marked with the word "submodule" and its state, and its diff shows "old commit → new" and the list of commits between them (the new ones and those left behind) — the same in the files of a commit and in the comparison of revisions. A submodule in which only the contents are changed does not get into a commit (there is nothing to commit). After a job that changed the recorded commit (pull, checkout, merge) the program offers to update the submodules. The state is read together with `git status` in the background; without `.gitmodules` git is not called. Along the way, auto-refresh in an open submodule is fixed (a relative path in its `.git` file). Not done: adding and removing a submodule (`submodule add` / `deinit`); `Rollback` returns a submodule to the recorded commit but does not touch its internal changes; nested submodules are not shown in the tree; cloning over the network with submodules has not been tried live — local repositories are verified.

- **Now.** Not supported; in Favorites they are skipped explicitly (`--ignore-submodules`).
- **To do.**
  - A submodules section: the path, the URL, the recorded and the actual commit, the state (not initialised, modified, diverged).
  - Actions: `Init`, `Update` (including recursively), `Sync`, `Open` as a separate repository.
  - In Changes a change of the pointer is shown as "old commit → new" with the list of commits between them, not as a binary file.
  - After pull and checkout — an offer to update the submodules.
- **Where.** `git.rs` (`status`, new parsing of `submodule status`), `branches_view.rs`, `commit_view.rs`.
- **Done when.** A repository with submodules is cloned, updated and commits a change of the pointer without a terminal.

### 2.4. Git LFS (M) — partly done

> Implemented after 0.4.3 (`lfs.rs`, `lfs_view.rs`): the diff of a file under LFS shows the real contents instead of the pointer — text is compared as text, a picture opens in the image viewer, anything else is marked as binary. The contents are taken from the objects already downloaded (`.git/lfs/objects`), the git-lfs program is not needed for this; if the object is not downloaded or is larger than 64 MB, the diff is replaced by a note on what happened and what to do. Files under LFS are marked "LFS" in Changes and in the list of files of a commit (by `git check-attr`). `Git → LFS`: `Pull`, `Fetch`, `Fetch All` and the `Tracked Patterns…` window with the list of patterns from `.gitattributes`, adding and removing; the file menu has `Track with LFS` (by extension or the file itself). If git-lfs is not installed, the commands explain how to install it. The commands `git lfs track` / `untrack` / `fetch` / `fetch --all` / `pull` are verified with the real git-lfs 3.7.1 by the test `works_with_the_real_git_lfs` (a push to a "server" in a neighbouring folder, a clone without the contents, a download); where there is no git-lfs, the test is skipped. The `Git → LFS` menu and the patterns window have not been gone through with the mouse. Not done: the LFS mark follows the attributes of the working folder, not of the commit that is shown; patterns come only from the root `.gitattributes`; file locks (`git lfs lock`).

- **Now.** None; an LFS file in a diff looks like the text of the pointer.
- **To do.** Recognise LFS pointers and show the contents themselves (a picture — through `image_view.rs`), an LFS mark in the file lists, the commands `lfs pull` / `lfs fetch`, a list of tracked patterns and `Track with LFS` in the file menu. If `git-lfs` is not installed — a clear message.
- **Where.** `git.rs`, `commit_view.rs`, `image_view.rs`, `config_view.rs`.
- **Done when.** An image under LFS is compared as a picture, not as three lines of a pointer.

### 2.5. Commit options (M) — done

> Implemented after 0.4.0: the `Options` link next to `Amend` opens a menu: `Sign-off` and `Sign the commit` (remembered in the settings), and for the next commit only — `Skip the hooks`, the author and the date. The options go into every kind of commit, including a partial one and amend. The commit details show the state of the signature (`%G?`) and the name of the signer; it is read in the background together with the list of branches. If a commit is rejected (a hook, a missing key, a wrong author), the output is shown in full in the `Commit Failed` window with a `Copy` button. Not done: signed commits are not marked in the log itself (verifying the signature of every commit is too slow); creating a signed commit is verified only up to passing `-S` to git — there is no signing key on the development machine.

- **Now.** A message, amend, commit and push, a partial commit.
- **To do.**
  - An options menu in the Commit panel: `Sign-off` (`--signoff`), a custom author (`--author`), the date, a signature (`-S`), skipping the hooks (`--no-verify`).
  - In the commit details — the state of the signature (`%G?`, the name of the signer).
  - The output of a failed hook is shown in full in a clear window, not only in the console.
- **Where.** `commit_view.rs`, `details_view.rs`, `git.rs` (`commit_details`), `settings.rs`.
- **Done when.** A signed commit is created and marked in the log; a refusal of the `pre-commit` hook is shown with its text.

### 2.6. .gitignore and cleaning (S) — done

> Implemented after 0.3.5 (`ignore.rs`): the menu of an untracked file has `Add to .gitignore` and `Add to .git/info/exclude` (the file itself, its folder, the extension) and `Delete…`; the menu of a modified file has `Stop Tracking` (`git rm --cached`); `Git → Clean…` shows what `git clean` will delete, with a checkbox for ignored files, and deletes after a confirmation.

- **Now.** Ignored files can be shown in Changes.
- **To do.**
  - `Add to .gitignore` in the menu of an untracked file: the file itself, its folder, the extension; a choice between `.gitignore` and `.git/info/exclude`.
  - `Delete` for untracked files and a general `Clean…` (`git clean`) with a preliminary list (`-n`) and a confirmation.
  - `Stop Tracking` for an already tracked file that falls under an ignore rule (`rm --cached` is already used in the code).
- **Where.** `commit_view.rs`, `dialogs.rs`.
- **Done when.** A file is added to the ignore list from the menu and disappears from the list at once; cleaning shows what will be deleted before deleting.

### 2.7. Tags on the remote (S) — done

> Implemented after 0.3.5: the tag menu has `Push Tag to <remote>` and `Delete Tag on <remote>…` for every remote; the commit details show the author, the date and the message of the annotated tags on it.

- **Now.** Creation, checkout, local deletion; pushing all tags or the tags of the commits being pushed from the Push window.
- **To do.** `Push Tag to <remote>` for one tag, `Delete Tag on Remote` (`push --delete`), viewing the message of an annotated tag in the details.
- **Where.** `branches_view.rs`, `details_view.rs`.
- **Done when.** One tag is pushed and deleted on the server from the tag menu.

### 2.8. Comparing revisions (M) — done

> Implemented after 0.4.0: with two commits selected in the log the menu gets `Compare Versions` — a window with the list of files that differ (a click opens the diff) and a tab of the commits between them. The file menu (Changes and the list of files of a commit) has `Compare with Branch…` and `Compare with Revision…`: a window with the list of branches or of the commits that changed the file (with search, renames are taken into account), the chosen version is compared with the file in the working folder. The two lists of commits in the comparison of branches existed before. Not done: the comparison window closes after the first file is opened; comparing a file between two arbitrary revisions (not with the working folder).

- **Now.** `Compare with '<current>'` for a branch and `Show Diff with Working Tree` for a commit.
- **To do.**
  - Comparing two commits selected in the log (`Compare Versions`).
  - For a file: `Compare with Revision…` and `Compare with Branch…`.
  - In the comparison of branches — two lists of commits (only here / only there) besides the overall diff.
- **Where.** `log_view.rs`, `branch_menu.rs`, `commit_view.rs`, `git.rs`.
- **Done when.** Any two commits are compared file by file, and a file is compared with any branch.

### 2.9. Options of cherry-pick and revert (S) — done

> Implemented after 0.3.5: `Cherry-Pick with Options…` and `Revert Commit…` in the log menu open a window with the checkboxes `-x` and `--no-commit` and a choice of the parent for a merge commit; both also work for several selected commits. After `--no-commit` the Commit panel opens with the message. The quick `Cherry-Pick` still takes the first parent.

- **Now.** Both actions run without options.
- **To do.** `-x` (append the original hash) and `--no-commit` for cherry-pick, `--no-commit` for revert, a choice of the parent (`-m 1|2`) when working with a merge commit — now such an attempt ends with a git error.
- **Where.** `log_view.rs`, `details_view.rs`, `dialogs.rs`.
- **Done when.** A merge commit is reverted and cherry-picked with a choice of the parent.

### 2.10. Bisect (M) — done

> Implemented after 0.4.0 (`bisect.rs`, `bisect_view.rs`): `Git → Bisect…` opens a window with the bad and the good commit; from the log menu — `Bisect Between These Commits…` for two selected commits and `Bisect: This Commit Is Good…` for one. While a bisect is in progress, a banner is shown above the log: which commit is being tested, roughly how many are left, the buttons `Good` / `Bad` / `Skip` / `Stop`; the commit menu has `Bisect: Mark as Good / Bad`. Tested commits are marked in the log (`good`, `bad`, `skipped`, `first bad`). When git finds the first bad commit, the banner names it and gives `Show Commit` and `Finish`. The steps of a bisect do not get into `Undo`. Not done: an automatic run by a command (`git bisect run`); custom names instead of good / bad (`--term-old` / `--term-new`).

- **To do.** `Git → Bisect`: a start with a choice of the good and the bad commit in the log, a state banner with the buttons Good / Bad / Skip / Reset (modelled on the "Merge in progress" banner), marks of the tested commits in the graph, a result with a jump to the commit found.
- **Where.** `git.rs` (`Operation`), `chrome.rs`, `log_view.rs`.
- **Done when.** The culprit commit is found entirely from within the program.

### 2.11. Undo of the last action (M) — done

> Implemented after 0.3.5 (`undo.rs`): before every job and after it `HEAD`, the branches, the tags and the stashes are recorded; `Git → Undo <action>…` (`Ctrl+Z` outside a text field) shows what will be restored and by which commands, and does it: commit and amend (`reset --soft`), reset, merge, pull, cherry-pick, checkout, creating, deleting and moving branches and tags (with the upstream restored), dropping and creating a stash. The undo itself is an action too — after it the menu item becomes `Redo`. After a push the menu item is unavailable, with an explanation; if the repository has changed since then or a merge/rebase is in progress, the undo refuses. Not done: only the one last action is kept; an undone hard reset does not bring back unsaved edits of files; deletion of files (`Clean`, `Delete`, `Rollback`) is not undone.

- **Now.** Only `Undo Commit (keep changes)`.
- **To do.** `Edit → Undo <action>` for the last operation: commit, amend, checkout, reset, merge, deleting a branch, dropping a stash. Before every `Job`, record `HEAD` and the refs affected; the undo restores them and explains what will be done. What has been pushed to the server is not undone — this is reported.
- **Where.** `app.rs` (`run_job`, `poll_jobs`), `chrome.rs`.
- **Done when.** An accidentally deleted branch and a mistaken reset are restored in one action.

---

## 3. Comfort

### 3.1. Several repositories at once (L) — done

> Implemented after 0.4.1: opening a second repository (File → Open, the recent ones, Favorites, a worktree) does not replace the first one but adds a tab; the tab strip under the title bar appears when there is more than one repository. Each tab has its own state — the log with its selection and filters, the diff and Annotate tabs, the Reflog, the conflicts window, the last action for Undo, the commit options; on switching it is put in place as a whole, the repository is not read again, only refreshed in the background. A tab is closed with its cross, with the middle button or from the menu (`Close Tab`, `Close Other Tabs`); `Ctrl+Tab` / `Ctrl+Shift+Tab` switch between them. The open tabs and the active one are remembered and restored on a start without arguments; the other tabs are read only when they are first switched to. While a git operation is running or an AI agent is working, switching away from the active tab and closing it are not allowed. A single click on a repository in Favorites opens it in the preview tab (`<name>`): the next one opened the same way takes its place; a double click in Favorites or on the tab (or `Keep Tab`) pins it. The maximize / normal state of the window is remembered and restored on start. Not done: the console is shared by all tabs; inactive tabs are not refreshed until they are switched to; separate windows.

- **Now.** `App` holds one open `Repo`; the Favorites panel shows the state of the others, but only one is ever open.
- **To do.** Repository tabs in the title bar (or separate windows), each with its own log, diff and console; the open tabs are restored on start; auto-refresh watches the active tab and, less often, the others.
- **Where.** `app.rs` (`repo: Option<Repo>` → a list), `chrome.rs`, `settings.rs`.
- **Done when.** Two repositories are open side by side and are switched between without loading again.

### 3.2. Light theme (M) — done

> Implemented after 0.4.1: Settings → Window → Theme: Dark / Light / As the system. The colours of `theme.rs` have become a palette with two sets (`theme::BG()`, `theme::TEXT()`…), and the tints written in the code as numbers for the dark theme (the backgrounds of diff and conflict lines, the badges, the colours of the graph and of syntax highlighting) are converted to light ones automatically (`theme::rgb` / `theme::adapt`): greys are inverted, dark tints become pale ones of the same hue, bright ones become darker. Verified on screenshots: the log with the graph and the Commit panel, the diff, the conflict resolution window with search, the settings, Annotate, the command palette. Not done: the other screens of the `REFBRAID_VIEW` list have not been looked at in the light theme; the automatically derived tints have not been tuned by hand.

- **Now.** `theme.rs` turns on `egui::Theme::Dark`, the colours are constants.
- **To do.** Turn the constants into a palette with two sets, a choice of Light / Dark / System in Settings → Appearance. Check the icons drawn with the painter and the colours of the graph on a light background.
- **Where.** `theme.rs` and every place where its constants are used.
- **Done when.** All screens of the `REFBRAID_VIEW` list are readable in the light theme.

### 3.3. Keyboard (M) — done

> Implemented after 0.4.1 (`keymap.rs`): all actions with shortcuts are gathered in one table; key handling, the menus, the tooltips and the Keyboard Shortcuts window are built from it. Settings → Keymap: a click on a shortcut and pressing new keys reassigns it, `Backspace` removes it, `Reset` puts it back, a shortcut already in use is marked. The command palette `Find Action…` (`Ctrl+Shift+A`, also in Help) searches all actions; `Checkout Branch…` (`Ctrl+Shift+B`) is the same kind of list of branches with search. The arrow keys work in the active list — the log, the branch tree (`←` `→` collapse a group, `Enter` shows the branch) and the changed files (`Enter` opens the diff, `Space` includes the file in the commit); the list is chosen with a click or with `Alt+1` / `Alt+2` / `Alt+3`. A commit (`Ctrl+K`, the text, `Ctrl+Enter`), a push (`Ctrl+Shift+K`, `Ctrl+Enter`) and switching the branch are done without the mouse. Later the diff text has a text caret: after a click on the text the arrow keys, `Home` / `End`, `PageUp` / `PageDown` move it, with `Shift` they select text, `Ctrl+←` / `Ctrl+→` go by words, `Ctrl+Home` / `Ctrl+End` go to the start and the end; the caret scrolls the diff by itself and expands folded lines, `Ctrl+C` copies the selection. Not done: the shortcuts inside separate windows (Push, Interactive Rebase, the image viewer) are still written in the code; in the list of a commit's files and in the Conflicts window the arrow keys do not work.

- **Now.** About ten shortcuts, written in the code; the list in Help is a ready-made string.
- **To do.** A table of actions with shortcuts instead of scattered checks; a Settings → Keymap page with reassignment; navigation with the arrow keys and `Enter` through the log, the branches and the files; a command palette (`Ctrl+Shift+A`) with search over all actions; the Keyboard Shortcuts window is built from the same table.
- **Where.** `chrome.rs`, `settings.rs`, `settings_view.rs`.
- **Done when.** A commit, a push and switching the branch are done without the mouse.

### 3.4. Progress and cancelling of long operations (M) — done

> Implemented after 0.3.5: the commands of jobs are started through `git::run_live` — stderr is read as it arrives, fetch, pull, push and clone get `--progress`. The status bar shows the name of the job, a bar with the percentage, the last line of progress and a Cancel button, which ends git together with its child processes. In the console the command appears at once and its output grows line by line. Not verified on Windows (there the process is ended through `taskkill /T`).

- **Now.** A `Job` runs to the end, the output appears in the console after it has finished.
- **To do.** Read stderr as it arrives (`--progress`), show the percentage in the status bar, a Cancel button ends the git process. The output goes to the console line by line, not as one block.
- **Where.** `app.rs` (`run_job`, `poll_jobs`), `git.rs`, `chrome.rs` (`status_bar`).
- **Done when.** During a fetch of a big repository the progress is visible, and the operation is stopped with the button.

### 3.5. Credentials (M) — done

> Implemented after 0.4.2 (`credentials.rs`): when a fetch, pull, push or clone fails because of sign-in or the connection, a window with the name of the cause and advice on what to do, and with git's output below, is shown instead of a line in a notification. Nine cases are told apart: the SSH key is not accepted, the server's key is not confirmed, the server's key has changed, a wrong login or password (advice about a token), the password prompt did not take place, an untrusted certificate, the server is unreachable, the repository is not found, no permission. Settings → Credentials: the choice of the credential helper (`git config --global`), the list of this computer's SSH keys with copying of the public key, and the creation of a new Ed25519 key (`ssh-keygen`). ssh's question about an unknown server is shown as a separate window with the server's name, the key fingerprint and the buttons `Trust and Connect` / `Cancel` instead of a field into which "yes" had to be typed. Not done: a new key is created without a passphrase; a saved wrong password is not removed from within the program; the connection to a new server over SSH itself has not been tried live — the parsing of the question and the creation of a key are verified.

- **Now.** The password prompt goes through the program's own askpass (`askpass.rs`).
- **To do.** Parsing of typical authorisation errors with a hint on what to do; a settings page with the chosen credential helper and the SSH keys (list, creation, copying of the public key); the question about an unknown SSH host key is shown as a window instead of hanging.
- **Where.** `askpass.rs`, `app.rs` (parsing of errors after the pattern of `blocked_checkout`), `settings_view.rs`.
- **Done when.** The first connection to a new server over SSH goes through without a terminal.

### 3.6. Integration with hostings (L) — partly done

> Implemented after 0.4.4 (`hosting.rs`, `hosting_view.rs`, the requests go through `http.rs`): GitHub, GitLab and Bitbucket are recognised by the address of the remote (`git remote -v`, taking `insteadOf` into account; SSH, HTTPS, GitLab subgroups, a name and password in the address are dropped); self-hosted servers are set in Settings → Hosting (what runs on the host and the token, the `Test` button). Without a token, `Open on <hosting>` works for a commit (the log menu), a branch (the branch menu) and a file (Changes and the files of a commit), and so does `Create Pull Request…`, which opens the hosting's page. With a token: the **Create Pull Request** window (Git → Create Pull Request…, the branch menu, a checkbox in the Push window — the window opens after the push) creates the request through the API; the open requests of a branch are shown in its menu and in the commit details; the result of CI is a mark before the commit subject in the log and a line with a link in the details. Later: the branch menu item `Pull Requests of the Branch…` (`Merge Requests of the Branch…` at GitLab) opens a window with every request there was out of that branch — opened, draft, closed and merged — the ones not merged first, then the merged ones, the latest first in each part (the last hundred the hosting has, fifty at Bitbucket); it needs a token. The data is read in the background with three requests per repository (the repository itself, the open requests, the last 100 CI runs) once every two minutes and after a push / fetch / pull; a token is sent only to its own host. Verified by tests: the parsing of addresses and of API responses, sending a POST through curl to a local server, finding the hosting and the values of the window on a real repository. Not done and not verified: nothing has been run against the real GitHub, GitLab and Bitbucket with a token — a pull request has not been created for real; the address of the new request page at Bitbucket and the target branch parameter at GitLab are not taken from the documentation; CI at GitHub is GitHub Actions only (third-party commit checks are not read), commits older than the last 100 runs have no mark; requests only within one repository (not from a fork); the window of a branch lists the requests out of it only, not the ones into it, and there is no list of all the requests of the repository; the state of a request (merged, closed) is shown only in that window, not in the branch tree; the query for all the states at Bitbucket is not taken from the documentation; Bitbucket Data Center is not supported; there are no reviewers, labels or drafts on creation; `Open on …` is missing from the diff menu; tokens are kept in the settings file in plain text (permissions 600), not in the system store.


- **Now.** There is JIRA (`jira.rs`) and links to issues.
- **To do.** After the pattern of `jira.rs`: recognising GitHub / GitLab / Bitbucket by the URL of the remote, a token in the settings, `Create Pull Request…` after a push, the list of open requests for a branch, the CI status of commits in the log, `Open on <hosting>` for a commit, a branch and a file.
- **Where.** new modules after the pattern of `jira.rs`, `push_view.rs`, `details_view.rs`, `settings_view.rs`.
- **Done when.** After a push a pull request is created from the program, and the result of CI is visible in the log.

### 3.7. White space, line endings and encodings (S) — done

> Implemented after 0.4.2: in the diff panel the `White space` item — show all changes, ignore changes in the amount of white space (`-b`) or all white space (`-w`); the choice is remembered and applies to every diff (the files of a commit, local changes, comparison of revisions). While white space is ignored, the partial commit and the revert of separate changes in the diff are switched off: they are computed from the exact diff. Changed files in which only the line endings differ (CRLF / LF) are marked in the Changes list with the words "line endings only". Settings → Window → Diff: the encoding for files that are not UTF-8 (Windows-1251, Windows-1252, CP866, KOI8-R) — only the lines that are not valid UTF-8 are read this way (`encoding.rs`). Not done: the encoding is one for the whole program, not per file; UTF-16 is not recognised; a partial commit of a file that is not UTF-8 is still unreliable.

- **Now.** The diff always takes white space into account; git's output is read as UTF-8 with invalid bytes replaced.
- **To do.** A switch in the diff: do not ignore / ignore changes of white space (`-b`) / all white space (`-w`); a mark on files where only the line endings have changed; a choice of encoding for a file that is not UTF-8 (for example, Windows-1251) instead of garbled characters.
- **Where.** `git.rs` (`parse_diff` and the calls of `diff`), `split_view.rs`, `commit_view.rs`.
- **Done when.** A file with reformatted indentation shows only the meaningful changes.

---

## 4. Project quality

### 4.1. Integration tests of operations (M) — partly done

> Implemented after 0.4.2: the shared helper `testrepo.rs` (`TestRepo`: a temporary repository with its own author name, `git`, `write`, `commit`, `run` for a job's commands, `app`, removal when finished). The commands of the buttons are moved out of the windows into `ops.rs` (`commit_commands`, `reset_args`, `delete_branch_args`) and are checked on real repositories: a commit of selected files and amend, finishing a merge after a conflict, reset in all four modes, deleting a local and a remote branch, Smart Checkout and Smart Update, cherry-pick and revert with options, unusual names (spaces, Cyrillic, quotes, a rename, a branch and a tag with non-Latin letters), white space and line endings. 112 tests in total. Not done: the old tests have not been moved to `TestRepo` and still build a repository themselves; push / pull through the Push window, interactive rebase through the interface and the creation of tags are not covered by separate tests.

- **Now.** About 35 unit tests for 20 thousand lines, mostly pure logic (`graph`, `partial`, `merge`, `wordiff`).
- **To do.** A shared helper for a temporary repository (at present `g(&[...])` is repeated in several files) and tests for every command the interface builds: commit and amend, partial commit, stash, Smart Checkout / Smart Update, reset, cherry-pick, deleting branches, patch. Separately — the parsing of git's output (`status`, `parse_diff`, `load_state`) on unusual names: spaces, Cyrillic, renames.
- **Done when.** Every operation of the menus is covered by at least one test on a real repository.

### 4.2. CI (S) — done

> Implemented: GitHub Actions in `.github/workflows/`. `ci.yml` on every push to `main` and on every pull request: build and tests on Linux and on Windows (a real Windows runner), a build for Windows in the same way as the release (MinGW), `cargo clippy` (errors and suspicious code stop the job, style and complexity are not checked; not blocking for now), screenshots of eight screens in both themes as artifacts. `release.yml` on a `vX.Y.Z` tag: RPMs for RHEL 9 and 10, AppImage, MSI and zip with the same scripts as `make dist` (the `GE_IN_CONTAINER=1` mode), SHA-256 checksums and a release on GitHub with this version's section of the changelog. Later: the pipeline is verified — CI is green on Linux and Windows (about 3.5 minutes with the cache), the release `v0.4.3` is published with six installers; a `.deb` package for Debian 12+ and Ubuntu 22.04+ is added (`packaging/build-deb.sh`), the release description starts with a table of the files (`packaging/release-notes.sh`), `release.yml` can be started by hand without publishing. Not done: the installers are not signed; the set of clippy rules is preliminary.

- **Now.** The build and the release are started by hand through `make`.
- **To do.** A pipeline: `cargo build` and `cargo test` on Linux and Windows on every push, `cargo clippy` with an agreed set of rules, on a `vX` tag — `make dist` and publication of the installers. Screenshots through `--screenshot` as build artifacts.
- **Done when.** A failed test is visible before the merge, the installers are built without a person taking part.

### 4.3. macOS (M) — partly done

> Implemented after 0.4.3: the program compiles for `aarch64-apple-darwin` and `x86_64-apple-darwin`; `packaging/build-macos.sh` (`make macos`, on a Mac only) builds `RefBraid.app` for Apple silicon and Intel at once and packs it into `dist/refbraid-<version>-macos-universal.dmg` (macOS 11 and newer); the program draws the `.icns` icon itself (`--write-icon`). The `macOS application` job in `release.yml` builds the `.dmg` on a GitHub runner, checks both architectures, the signature and a start with a screenshot of the window, and attaches the file to the release; the tests on macOS are run in `ci.yml` only by hand (while the repository was private, a minute of a macOS runner cost about ten times a minute of Linux). On a start from Finder the Homebrew and MacPorts folders are added to `PATH`. The shortcuts on a Mac are labelled with its own signs (`⌘K`, `⌥⌘A`, `⇧⌘K`; `keymap::mac_text`), and switching repository tabs is `⌃Tab`, because `⌘Tab` is taken by the system; in the settings the shortcuts are stored in the former form `Ctrl+…`. Not done: the program has not been run on a real Mac — the window, the fonts, the shortcuts and the folder selection dialogs are checked only by a screenshot in CI; there is no integration with Finder (the settings page says so), opening a folder by dragging it onto the icon does not work; the application is signed ad hoc only, without an Apple certificate and notarisation — the first start has to be allowed in System Settings → Privacy & Security; `make dist` and `make release-all` on Linux do not build the `.dmg`.

- **Now.** The targets are Linux (RPM, AppImage) and Windows.
- **To do.** Check the build and the operation on macOS, replace `Ctrl` with `Cmd` in the shortcuts and labels, build an `.app` / `.dmg` in `packaging/`, add integration with Finder after the pattern of `integrate.rs`.
- **Done when.** `make macos` gives a `.dmg` that can be run.

### 4.4. Interface localisation (M) — done

> Implemented after 0.3.4: the Russian language of the interface, chosen in Settings → Window → Language (`i18n.rs`, the translations in `i18n_ru.rs`). The strings in the code stay English and are wrapped in `tr("…")`; a string without a translation is shown in English. The changelog also exists in two languages (`CHANGELOG.md`, `CHANGELOG.en.md`). Not done: some of the strings are not yet wrapped in `tr` and not translated.

- **Now.** The interface strings are English literals in the code; the changelog and the commits are written in Russian.
- **To do.** Move the strings into a table with keys, add a Russian translation and a choice of language in Settings. Take the decision deliberately: the rule "UI text is English" is written in `CLAUDE.md`, it will have to be changed.
- **Done when.** The language is switched in the settings without a rebuild.

---

## Done beyond the list

Features that were not among the tasks but appeared after 0.3.4:

- **Merge conflicts.** The **Conflicts** window with the list of conflicting files and the **Merge Revisions** window for resolving file by file; simple conflicts are resolved automatically from the common ancestor (`merge.rs`). The Help menu has an example repository with conflicts.
- **Patches.** `Create Patch…` and `Apply Patch…` in the Git menu, in the log and in the list of changes (`patch.rs`).
- **Checkout Branch** in the context menu of a commit: switching to a branch that ends at this commit.
- **Modal windows** can be moved and resized; in the Push window and the window for splitting changes into commits the divider between the panels can be dragged.
- **Search fields** have a clear button.
- **Updates** (after 0.4.4, `update.rs`, `update_view.rs`). Once a day at start the program asks GitHub for the latest release of its repository, without signing in, and shows a notice when it is newer than the running version; `Help → Check for Updates…` looks at any time. The window shows what is new and offers the installer for this computer (RPM for EL9 / EL10, deb, AppImage, MSI, `.dmg`, told from how the program was installed), which the browser downloads; where that cannot be told (built from the sources, unpacked from the zip, another distribution) it opens the release page. "Skip This Version" stops the notice for that version; Settings → Window turns the look at start off. Not done: the program does not download or install anything itself and does not check the checksums; the notice has not been seen for real, because no release newer than the running build exists yet.

---

## Recommended order

Items 1–5 of the former order (1.9, 1.7, 2.6, 2.7, 2.9; 3.4, 1.4, 1.8; 2.11; 1.6; the rest of 1.2) are done after 0.3.5. Next:

1. ~~The remains of what is done~~ — done: highlighting and search in the conflict resolution window (1.6, 1.7), `git status` and the search for a commit's branches off the UI thread (1.8); completing the graph was given up after a measurement (see 1.8).
2. ~~2.1, 2.8, 2.5~~ — done after 0.4.0: stash with a preview, comparison of revisions, commit options.
3. ~~2.2, 2.10~~ — done after 0.4.0: worktrees and bisect.
4. ~~4.1, 4.2~~ — done after 0.4.2: the shared helper for test repositories (4.1 partly) and CI on GitHub Actions.
5. ~~3.1, 3.2, 3.3~~ — done after 0.4.1: repository tabs, the light theme, the keyboard.
6. ~~3.7, 4.1, 4.2, 3.5~~ — done after 0.4.2 (4.1 partly).
7. ~~2.3, 2.4, 4.3, 3.6~~ — done after 0.4.3 and 0.4.4; 2.4, 4.3 and 3.6 — partly, see "Not done" in each item.
8. The rest — as users ask.
