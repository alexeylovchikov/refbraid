# RefBraid: version and builds.  Run "make" to see the commands.
#
#   make version              show the current version
#   make bump-patch           0.2.0 -> 0.2.1   (also bump-minor, bump-major)
#   make set-version V=1.0.0  an exact version
#   make bump-release         same version, next RPM release (-1 -> -2)
#
#   make build / run / test   debug build, start it, run the tests
#   make release              optimised build for this computer
#   make rpm-el10 rpm-el9 rpm-el8 windows appimage
#   make dist                 RHEL 10 + RHEL 9 + Windows 64 + AppImage
#   make release-all          new patch version, changelog (Claude Code), commit, tag, then dist
#                             (BUMP=minor|major|x.y.z for another version)
#   make changelog            only update CHANGELOG.md for the current version

VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
RPM_RELEASE := $(shell sed -n 's/^Release: *\([0-9]*\).*/\1/p' packaging/rpm/refbraid.spec)
CARGO ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)

.DEFAULT_GOAL := help
.PHONY: help version bump-patch bump-minor bump-major set-version bump-release \
	build run test release changelog rpm-el10 rpm-el9 rpm-el8 windows appimage notices dist release-all list clean

help:
	@echo "RefBraid $(VERSION) (RPM release $(RPM_RELEASE))"
	@echo ""
	@echo "Version:"
	@echo "  make version               show the version"
	@echo "  make bump-patch            $(VERSION) -> next patch version"
	@echo "  make bump-minor            next minor version"
	@echo "  make bump-major            next major version"
	@echo "  make set-version V=x.y.z   exact version"
	@echo "  make bump-release          same version, next RPM release"
	@echo ""
	@echo "Program:"
	@echo "  make build | run | test | release"
	@echo ""
	@echo "Installers (into dist/):"
	@echo "  make rpm-el10              RHEL 10 package (built on this computer)"
	@echo "  make rpm-el9               RHEL 9 package (container)"
	@echo "  make rpm-el8               RHEL 8 package, also installs on 9 and 10 (container)"
	@echo "  make windows               Windows 64-bit MSI installer and portable zip (container)"
	@echo "  make notices               THIRD-PARTY-NOTICES.md from the dependencies' own licence files"
	@echo "  make appimage              AppImage for most Linux systems (container)"
	@echo "  make dist                  rpm-el10 + rpm-el9 + windows + appimage"
	@echo "  make release-all           version +1, changelog, commit, tag v<version>, then dist"
	@echo "                            (make release-all BUMP=minor | major | x.y.z)"
	@echo "  make changelog             update CHANGELOG.md for the current version (Claude Code)"
	@echo "  make list                  show what is in dist/"

version:
	@echo $(VERSION)

bump-patch:
	@sh packaging/set-version.sh patch

bump-minor:
	@sh packaging/set-version.sh minor

bump-major:
	@sh packaging/set-version.sh major

set-version:
	@test -n "$(V)" || { echo "Usage: make set-version V=x.y.z"; exit 1; }
	@sh packaging/set-version.sh $(V)

bump-release:
	@sh packaging/set-version.sh release

build:
	$(CARGO) build

run:
	$(CARGO) run

test:
	$(CARGO) test

release:
	$(CARGO) build --release

rpm-el10:
	sh packaging/build-rpm.sh --local

rpm-el9:
	sh packaging/build-rpm.sh --el9

rpm-el8:
	sh packaging/build-rpm.sh

windows:
	sh packaging/build-windows.sh

# The licences of the libraries built into the program (after dependencies change).
notices:
	python3 packaging/third-party-notices.py

appimage:
	sh packaging/build-appimage.sh

dist: rpm-el10 rpm-el9 windows appimage
	@$(MAKE) --no-print-directory list

# A new make run, so the builds see the new version.
release-all:
	@sh packaging/release.sh $(or $(BUMP),patch)
	@$(MAKE) --no-print-directory dist

changelog:
	@sh packaging/release.sh --changelog-only

list:
	@ls -l dist

clean:
	$(CARGO) clean
