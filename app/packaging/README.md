# Installers

Shortcut: the `Makefile` in the program folder runs all of this. `make` lists the commands, e.g.
`make bump-patch` (raise the version), `make dist` (RHEL 10 + RHEL 9 + Windows 64 + AppImage),
`make release-all` (new version, changelog written by Claude Code from the commits, commit, tag `v<version>`, then `make dist`; see `release.sh`). The version is changed everywhere by `packaging/set-version.sh`.

The version comes from `Cargo.toml`; the RPM release number is in `rpm/refbraid.spec`.
All container builds need podman or docker.

- `sh packaging/build-rpm.sh --local` builds the RHEL 10 package on this computer:
  `dist/refbraid-<version>-<release>.el10.x86_64.rpm`.
- `sh packaging/build-rpm.sh --el9` builds the RHEL 9 package in a RHEL 9 (UBI 9) container:
  `dist/refbraid-<version>-<release>.el9.x86_64.rpm`.
- `sh packaging/build-rpm.sh` builds on RHEL 8 (UBI 8): one `.el8` package that installs on RHEL 8, 9 and 10.

  Every package adds RefBraid to the applications menu, to "Open With" for folders in Files,
  and "Open in RefBraid" to the folder right-click menu when nautilus-python is installed.
- `sh packaging/build-appimage.sh` builds `dist/refbraid-<version>-x86_64.AppImage`, a single file
  that runs on most Linux systems (compiled on RHEL 8, so glibc 2.28 or newer is enough). Make it
  executable and start it; File → Settings → File Manager adds it to the Files app.
- `sh packaging/build-windows.sh` makes the Windows 64-bit installer
  `dist/refbraid-<version>-windows-x64.msi` (described in `windows/refbraid.wxs`, built with
  `wixl` from msitools) and a portable `.zip`.
  The MSI installs for all users into Program Files (administrator rights are asked for): Start
  menu entry, desktop shortcut, "Open in RefBraid" in the folder right-click menu, uninstall
  from Settings → Apps; a newer version replaces the installed one. It has no pages to click
  through. Silent install: `msiexec /i refbraid-<version>-windows-x64.msi /qn`; without the
  desktop shortcut add `ADDLOCAL=Main,ExplorerMenu`, without the folder menu too `ADDLOCAL=Main`.
  The `.zip` is the program alone, for use without installing or without administrator rights.
  Git for Windows must be installed.
