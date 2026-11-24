# Changelog

<!-- last-commit: 1f1c3393d418a5315d32e293887becbae3387294 -->

## 0.5.0 — 2026-11-24

### New
- **Hostings**: GitHub, GitLab and Bitbucket are recognised by the address of the remote. **Open on GitHub** (or GitLab, Bitbucket) in the menu of a commit, a branch and a file opens it on the hosting in the browser.
- **Create Pull Request…** in the Git menu, in the menu of a branch and as a checkbox in the **Push** window (the window opens after the push). With a token from Settings → Hosting the request is created from the program itself; without one the page of the hosting opens.
- The open requests of a branch are shown in its menu and in the commit details, and the result of CI as a mark before the commit in the Log and a line with a link in the details. This needs a token in Settings → Hosting.
- The Settings → **Hosting** page: the hosts of your own servers and their tokens; **Test** checks a token.
- **Checking for updates**: once a day at start the program asks GitHub whether a newer version is out and tells about it. Help → **Check for Updates…** looks at any time, shows what is new and offers the installer for your system; your browser downloads it. Settings → Window turns it off.

### Fixed
- On macOS the shortcuts are shown with its own keys (`⌘K`, `⌥⌘A`), and repository tabs are switched with `⌃Tab`.
- The hint in the diff of a Git LFS file and in the Git → LFS menu promised that **Fetch** downloads the files of other branches. It now says what is true: **Fetch** — the files of the current commit, **Fetch All** — those of earlier commits and other branches too.

## 0.4.4 — 2026-11-24

### New
- **Submodules**: when a repository has submodules, the branch tree gets a **Submodules** section — the path of each, the recorded commit (or "recorded → checked out") and the marks "not initialised", "conflict", "has changes". A double click opens a submodule in a tab of its own; its menu has **Open**, **Init**, **Update**, **Update with Nested Submodules**, **Sync Address**, **Update All Submodules** and copying of the path and the address.
- Git → **Update Submodules** updates all submodules at once, and after a pull, checkout or merge that changed the recorded commit the program offers to update them.
- In Changes a submodule is marked "submodule" with its state, and its diff shows "old commit → new commit" and the list of commits between them — the new ones and the ones left behind. It is shown the same way in the files of a commit and when comparing revisions.
- **Git LFS**: the diff of a file kept in LFS shows its real content in place of the pointer — text as text, a picture in the image view. When the file is not downloaded or is too big, the diff says what happened and what to do.
- Files kept in LFS are marked "LFS" in Changes and in the list of files of a commit.
- The Git → **LFS** menu: **Pull**, **Fetch**, **Fetch All** and the **Tracked Patterns…** window with the list of file patterns kept in LFS, where they can be added and removed. The file menu in Changes has **Track with LFS**. When git-lfs is not installed, the program explains how to install it.
- A first build for **macOS**: a universal `.dmg` for Intel and Apple Silicon (not yet tried on a real Mac). The program finds git, git-lfs and AI agents installed with Homebrew or MacPorts, and uses the system fonts of macOS. File manager integration is not available on macOS yet — open a folder with File → Open.

### Fixed
- Bisect finds the first bad commit with any version of git: with some versions the result was not shown.

## 0.4.3 — 2026-11-23

### New
- **Ignoring white space in a diff**: the **White space** button in the diff toolbar switches between **Show white-space changes**, **Ignore changes in the amount of white space** and **Ignore all white space** — to see what a reformatted file really changes.
- Files in which only the line endings changed are marked "line endings only" in the list of changed files.
- An encoding for files that are not UTF-8: Settings → Window → **Files that are not UTF-8 are read as:** offers **Windows-1251 (Cyrillic)**, **Windows-1252 (Western)** and **CP866 (DOS Cyrillic)**, and such lines are shown in a diff as readable text in place of the � marks.
- The Settings → **Credentials** page: a choice of how git remembers passwords and tokens for HTTPS (**Git Credential Manager**, **Keyring (libsecret)**, **Memory, for 15 minutes**, **A plain file** or **None**), and a list of this computer's SSH keys with a **Copy Public Key** button and a way to make a new key (**Generate**).
- Sign-in errors in plain words: when a fetch, pull, push or clone fails because of credentials or the connection to the server, the program says why — the server did not accept the SSH key, the user name or password was not accepted, the server's certificate is not trusted, the server cannot be reached, the repository was not found or there is no permission — and what to do about it.
- The first SSH connection to a new server brings up a window with the fingerprint of its key and a **Trust and Connect** button; once trusted, the server is remembered.

## 0.4.2 — 2026-11-23

### New
- **Repository tabs**: several repositories are open at once, each with its own tab in the strip under the title bar. Going back to a tab shows everything as it was left: the Log, the open diffs, the conflicts being resolved. Tabs are switched with `Ctrl+Tab` and `Ctrl+Shift+Tab`, closed with the cross or with **Close Tab** and **Close Other Tabs**, and restored the next time the program starts.
- A repository's tab shows the number of commits to push and to pull and a mark for changed files, as Favorites does.
- Quick view: a repository opened with one click in Favorites takes a temporary tab (its name is in angle brackets), and the next repository opened that way takes its place. A double-click or **Keep Tab** keeps the tab.
- When the tab strip is shown is chosen in Settings → Window → **Repository tabs**: **Always**, **When several repositories are open** or **Never**.
- **Light theme**: Settings → Window → **Theme** — **Dark**, **Light** or **As the system**.
- **Configurable keyboard shortcuts**: on the Settings → **Keymap** page, click a shortcut and press the new keys; **Reset** brings back the default. Menus, hints and the **Keyboard Shortcuts** window show the current shortcuts.
- **Find Action…** (`Ctrl+Shift+A`, also in the Help menu): a command palette — start typing the name of an action and press `Enter`.
- **Checkout Branch…** (`Ctrl+Shift+B`): checks out a branch picked from a list as you type its name.
- Working from the keyboard: `Alt+1`, `Alt+2` and `Alt+3` go to the Log, the branches and the changed files; the arrow keys, `Home` and `End` move through a list, `Enter` opens what is selected, `Space` includes a file in the commit or leaves it out, `Ctrl+W` closes a diff or Annotate tab.
- A window that was maximised when closed opens maximised the next time.

### Fixed
- The splitter between the commit panel and Favorites is no longer wider than the others.
- The "Generating…" label next to the message generation button in the commit panel is gone.

## 0.4.1 — 2026-11-20

### New
- **Bisect**: **Bisect…** in the Git menu, and **Bisect Between These Commits…** and **Bisect: This Commit Is Good…** in a commit's context menu, help find the commit that broke something. A bar above the Log has the **Good**, **Bad** and **Skip** buttons, the number of tests left and a **Stop** button (**Finish** once the commit is found).
- **Worktrees**: the extra working folders of a repository are shown in a section of that name in the branch tree. They can be opened in this window or in a new one (**Open in New Window**) and removed (**Remove Worktree…**), and the records of folders that are gone can be cleared (**Remove Stale Records**). When a branch is already checked out in another working folder, checking it out explains where it is open.
- Commit options: the **Options** button in the commit panel turns on **Sign-off**, signing the commit with a GPG or SSH key, skipping the hooks (**Skip the hooks**), another author and date. Sign-off and signing are remembered for the next commit.
- When a hook of the repository refuses a commit, the **Commit Failed** window shows the hook's whole output.
- The commit details show the state of the commit's signature: good, bad, expired or revoked key, unknown key.
- Comparing revisions: **Compare with Branch…** and **Compare with Revision…** in a file's context menu compare it with its version in another branch or in one of its commits, and **Compare Versions** with two commits selected in the Log shows the files that differ between them.
- Stash contents: **Show Files** lists a stash's files, whose changes can be opened, and **Take This File** puts one file from the stash into the working folder. **Branch…** makes a new branch from a stash.
- When stashing, the changes already added to the index can be kept in place (**Keep the staged changes in place**), and only the selected files can be stashed.

### Fixed
- On Windows, folders and files now open correctly in Explorer, the editor and external tools: before, Explorer could open the Documents folder instead of the repository's folder.

## 0.4.0 — 2026-11-20

### New
- **Clone** and **New Repository**: **Clone…** and **New Repository…** in the File menu, and **Clone Repository…** and **New Repository…** buttons on the start screen. When cloning you can choose the folder, the branch, the history depth and cloning of submodules; opening a folder that is not a repository offers to create one.
- **Undo** of the last action: the item at the top of the Git menu and `Ctrl+Z` (outside a text field) put branches, tags and stashes back as they were before a commit, reset, merge, checkout, deleted branch and so on. You see what will be done before undoing; afterwards **Redo** is available.
- The progress of git commands is shown in the status bar, and the **Cancel** button there stops the running command.
- **Search in diffs**: **Find…** (`Ctrl+F`) in the diff view and in the **Merge Revisions** window, with moving between matches (`F3`, `Shift+F3`).
- **Syntax highlighting** of code in diffs, in the **Annotate** tab and in the **Merge Revisions** window. It is turned on and off in Settings → Window → **Syntax highlighting**.
- **Merge with Options…** in a branch's menu: always a merge commit (`--no-ff`), fast-forward only, squash, merging without committing and your own commit message.
- **Cherry-Pick with Options…** and **Revert Commit…** / **Revert Commits…** in a commit's context menu: recording the original commit (`-x`), not committing, choosing the parent of a merge commit, also for several selected commits.
- Tags in the branches panel now have **Push Tag to …** and **Delete Tag on …**, to send a tag to the server and to delete it there.
- The context menu of new files in the list of changes now has **Add to .gitignore** and **Add to .git/info/exclude** (the file, the whole folder or every file of this type) and **Delete…**, and files in Git have **Stop Tracking**.
- **Clean…** in the Git menu deletes the files that are not in Git, showing their list first; ignored files can be deleted too.
- **Show History for Selection** in the diff view shows the commits that changed the selected lines. The file history tab can show the commits of every branch, not only of the current one.
- The Log search can look through the whole history (**Search the whole history**) and find the commits that added or removed a text (**Search in the changes**), not only among the loaded commits.

### Fixed
- The Log of a big repository is loaded in the background: the window no longer freezes while the commits are read, and the previous list stays on screen until the new one is ready.
- The changed files and the branches of the selected commit are read in the background, so the interface no longer stalls on big repositories; while the branches are being looked up, "Looking for the branches with this commit…" is shown.
- The application icon is now smooth and sharp at every display scale on Windows.

## 0.3.5 — 2026-11-19

### New
- **Russian interface language**: the language is chosen in Settings → Window → **Language**. The **Help → Changelog** window shows the changelog in the interface language.
- **Interactive Rebase** window: commits can be reordered (**Move Up** / **Move Down**, `Alt+Up` / `Alt+Down`), squashed, reworded and dropped. It opens from **Interactively Rebase from Here…** in a commit's context menu in the Log.
- A commit's context menu now has **Squash into Parent…**, **Drop Commit…** and **Edit Commit Message…** for any commit of the branch, and, with several commits selected, **Squash Commits** and **Drop Commits**.
- **Suggest with AI** button in the **Interactive Rebase** window: the AI agent chosen in Settings looks at the commits and suggests what to squash, reword, drop or move. The plan can be changed before rebasing, and **Undo** brings back the previous one.
- **Conflicts** window with the list of files with merge conflicts: **Accept Yours**, **Accept Theirs**, **Merge…**, and resolving with AI for one file or for all of them.
- **Merge Revisions** window: your version, the result and their version side by side, **Accept Left** and **Accept Right** buttons, moving between conflicts and opening in an external merge tool.
- The **Resolve Simple Conflicts** button resolves by itself the conflicts where the two sides changed different lines or only one side changed them.
- **Help → Open Merge Conflict Example** opens a sample repository with merge conflicts of every kind, to try resolving them.
- **Patches**: **Create Patch…** and **Apply Patch…** in the Git menu, creating a patch from one or several commits in the Log and from the selected files in the list of changes. A patch can be applied by dropping the file on the window; a 3-way merge is available when applying.
- **File history**: **Show History** in a file's context menu opens a tab with the commits that changed the file, followed through its renames. The details of the selected commit are shown under the commit list.
- **Annotate** tab: who last changed each line of the file and in which commit, with going to the previous revision (**Annotate Previous Revision**) and to the commit in the Log. The diff has an **Annotate with Git Blame** item.
- **Reflog** tab (Git menu → **Reflog**): where HEAD has been, with an **Only lost commits** filter. A lost commit is brought back by creating a branch on it.
- **Checkout Branch** in a commit's context menu checks out a branch that ends at this commit.
- Modal windows can be dragged by their title and resized; a double-click on the title puts the window back.
- In the **Push** and **Split Changes into Atomic Commits…** windows the divider between the panels can be dragged.
- Search fields have a clear button. The hide buttons on the branches and details panels are removed, and the **Git Log** button is removed from the side strip.
- The arrows at the ends of cut commit graph lines are longer and easier to see.

## 0.3.4 — 2026-11-17

### New
- **Compact commit graph**: branch lines move over when a branch ends, and very long lines are cut, with arrows at their ends. The graph style is chosen in Settings → Log → Graph: **Compact** or **Fixed lanes** (the previous look, where every line has its own column).
- A click on the arrow of a cut graph line goes to the commit at its other end; the tooltip above the arrow shows that commit.
- Settings → **JIRA** page: connects to JIRA Server / Data Center or Jira Cloud with a token, with a **Test Connection** button.
- The commit details show the status of JIRA issues. Issue keys are taken from the commit message, or, when it has none, from the names of the branches that contain the commit.
- The issue link patterns in Settings → **Issue Navigation** are now shared by all repositories (**All Repositories**). A repository can have its own on the **This Repository** tab.
- In the commit details, master and main are shown in green in the list of branches that contain the commit.

## 0.3.3 — 2026-11-16

### New
- **Help → Changelog** window with the changes of each version.
- The diff highlights the changed words inside changed lines.
- Text in the diff can be selected and copied: **Copy** (`Ctrl+C`) and **Select All** (`Ctrl+A`) in the context menu.
- **Open in Editor at Line** in the diff's context menu opens the file in an editor at the line that was clicked (VS Code, JetBrains IDEs, Sublime Text, Kate and others). The changed-file lists have **Open in Default Application**.
- Settings → Tools → **Open Files** page: choose the program that opens files — the system's default application or your own command, with ready-made templates for popular editors.

## 0.3.2 — 2026-11-16

### New
- **Smart Update**: when local changes get in the way of a pull or merge, RefBraid offers to put them aside, update, and bring them back.

## 0.3.1 — 2026-11-16

### New
- **Favorites** panel with favourite repositories, groups and the state of their branches. The star in the panel adds or removes the open repository; the folder button opens a repository.
- **Smart Checkout**: when local changes get in the way of switching branches, they can be put aside for a moment and brought back after the switch.
- Pulling into a branch that has diverged opens a window to choose between merge and rebase.
- Quick-view diff tab: a click on a file shows its changes in it right away.
- The diff window got a draggable divider between its sides and horizontal scrolling.
- When viewing the diff of a file from a commit, the panel with the commit's files and details stays on the right.
- External **Diff** and **Merge** tools, set up by file type, with a button to start them in the diff panel.
- The colour of the window's title bar can be chosen in Settings → Window.
- A tooltip with all branches and tags when the pointer is over a commit's labels in the log.
- `Ctrl+Alt+A` adds the selected new files to Git.

### Fixed
- On Windows the program closed right after starting on computers with an old Intel graphics driver. The window is now drawn with Direct3D 12, and if the program still can't start, it shows a message with the reason and writes it to `error.log`.
- The side panels and the log's columns no longer squeeze the Git window when the program's window is narrow.
- A new or deleted image in a diff is shown as one picture, not side by side.
- The borders and title lines of the Git window line up with the Commit and Favorites panels.

## 0.3.0 — 2026-11-13

### New
- **Help → About** window with the version of the program and of git.
- **AI Tasks** menu: an AI agent splits the changes into atomic commits; the plan can be edited before the commits are made.
- Partial commits: single changes of a file can go into a commit, and unwanted ones can be rolled back. Unchanged lines in the diff are folded.
- **Push** window with the commits to be pushed, their files and details.
- **Git Config** window with the remotes and the main git settings.
- Image viewer in the diff with zoom and rotation.
- Several files can be selected in Changes (`Ctrl`/`Shift`+click) and acted on at once.
- More log tabs with the "+" button, each with its own filters and selected commit.
- **Not Merged into <current branch>** log mode; the changes of a merge commit are shown separately for each parent.
- Branch prefixes can be set up (`feature/`, `hotfix/` …) and chosen when creating a branch.
- Root branches stand out in the branch list and in the commit details; the behind/ahead counters are coloured.
- AI commit messages follow the `[prefix] Description.` form by default and use the language of the repository's other commits.
- All modal windows look the same; the list of recent repositories was updated.

## 0.2.0 — 2026-11-12

### New
- First version: the log with the branch tree and commit graph, the commit's details and changes, the Commit panel, push, pull and fetch, branch and tag operations.
- Opening a repository from the file manager: "Open With" and "Open in RefBraid" in Nautilus, an item in the folder context menu on Windows.
