# Built by packaging/build-rpm.sh from an already compiled program.
%global debug_package %{nil}
%global __strip /bin/true

Name:           refbraid
Version:        %{app_version}
Release:        1%{?dist}
Summary:        Git history, branches and commits in one window
License:        MIT
Requires:       git
Recommends:     nautilus-python

%description
RefBraid shows the commit graph, branches, changes and diffs of a Git
repository and lets you commit, push, pull, merge, rebase and more.
Open a folder from Files (Nautilus): right-click it, then "Open With" or
"Open in RefBraid".

%install
install -Dm755 %{src}/refbraid %{buildroot}%{_bindir}/refbraid
install -Dm644 %{src}/refbraid.desktop %{buildroot}%{_datadir}/applications/refbraid.desktop
install -Dm644 %{src}/refbraid.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/refbraid.png
install -Dm644 %{src}/refbraid-nautilus.py %{buildroot}%{_datadir}/nautilus-python/extensions/refbraid-nautilus.py
install -Dm644 %{src}/LICENSE %{buildroot}%{_defaultlicensedir}/%{name}/LICENSE
install -Dm644 %{src}/THIRD-PARTY-NOTICES.md %{buildroot}%{_defaultlicensedir}/%{name}/THIRD-PARTY-NOTICES.md

%files
%license %{_defaultlicensedir}/%{name}
%{_bindir}/refbraid
%{_datadir}/applications/refbraid.desktop
%{_datadir}/icons/hicolor/256x256/apps/refbraid.png
%dir %{_datadir}/nautilus-python
%dir %{_datadir}/nautilus-python/extensions
%{_datadir}/nautilus-python/extensions/refbraid-nautilus.py

%post
touch --no-create %{_datadir}/icons/hicolor >/dev/null 2>&1 || :
update-desktop-database %{_datadir}/applications >/dev/null 2>&1 || :

%postun
update-desktop-database %{_datadir}/applications >/dev/null 2>&1 || :

%posttrans
gtk-update-icon-cache %{_datadir}/icons/hicolor >/dev/null 2>&1 || :
