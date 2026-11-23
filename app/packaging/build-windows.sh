#!/bin/sh
# Builds the Windows 64-bit installer in a container (Rust + MinGW-w64 + wixl from msitools):
#   dist/refbraid-<version>-windows-x64.msi        (installer)
#   dist/refbraid-<version>-windows-x64.zip        (portable program, no install)
# Both carry LICENSE.txt and THIRD-PARTY-NOTICES.md.
set -e
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
mkdir -p dist
# The licence and the notices of the libraries built into the program are in the repository
# root, which a build container does not see: keep copies where it does.
lic="$PWD/target/licences"
if [ -f ../LICENSE ]; then
  mkdir -p "$lic"
  cp -f ../LICENSE ../THIRD-PARTY-NOTICES.md "$lic/"
fi

# The build itself; runs where Rust, MinGW-w64, wixl and zip are (the container below, or CI).
build() {
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/windows}"
  cargo build --release --target x86_64-pc-windows-gnu
  cp -f "$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/refbraid.exe" packaging/windows/refbraid.exe
  cp -f "$lic/LICENSE" packaging/windows/LICENSE.txt
  cp -f "$lic/THIRD-PARTY-NOTICES.md" packaging/windows/THIRD-PARTY-NOTICES.md
  ( cd packaging/windows && wixl --arch x64 -D VERSION="$version" -D EXE=refbraid.exe -o "$OLDPWD/dist/refbraid-$version-windows-x64.msi" refbraid.wxs )
  mkdir -p target
  mkdir -p target/windows-zip
  mv -f packaging/windows/refbraid.exe packaging/windows/LICENSE.txt packaging/windows/THIRD-PARTY-NOTICES.md target/windows-zip/
}

# GE_IN_CONTAINER=1: the tools are already here (CI) — no container is started.
if [ -n "$GE_IN_CONTAINER" ]; then
  build
else
  engine=$(command -v podman || command -v docker)
  "$engine" build -q -t refbraid-win-build packaging/windows
  mkdir -p "$HOME/.cargo/registry"
  "$engine" run \
    -v "$PWD:/src:Z" \
    -v "$HOME/.cargo/registry:/usr/local/cargo/registry:Z" \
    -w /src \
    -e GE_IN_CONTAINER=1 -e CARGO_TARGET_DIR=/src/target/windows \
    refbraid-win-build sh packaging/build-windows.sh --build-only
fi
[ "$1" = "--build-only" ] && exit 0
rm -f "dist/refbraid-$version-windows-x64.zip"
( cd target/windows-zip && zip -q -j "../../dist/refbraid-$version-windows-x64.zip" refbraid.exe LICENSE.txt THIRD-PARTY-NOTICES.md )
ls -l dist
