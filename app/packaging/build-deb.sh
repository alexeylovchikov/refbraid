#!/bin/sh
# Builds the Debian / Ubuntu package: dist/refbraid_<version>-<release>_amd64.deb
# The program is compiled on Ubuntu 22.04 (container, glibc 2.35), so the package installs on
# Ubuntu 22.04 and newer and on Debian 12 and newer. Needs podman or docker.
set -e
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
release=$(sed -n 's/^Release: *\([0-9]*\).*/\1/p' packaging/rpm/refbraid.spec)
engine=$(command -v podman || command -v docker || true)
# The licence and the notices of the libraries built into the program are in the repository
# root, which a build container does not see: keep copies where it does.
lic="$PWD/target/licences"
if [ -f ../LICENSE ]; then
  mkdir -p "$lic"
  cp -f ../LICENSE ../THIRD-PARTY-NOTICES.md "$lic/"
fi

if [ -z "$GE_IN_CONTAINER" ]; then
  "$engine" build -q -t refbraid-deb-build packaging/deb
  mkdir -p dist target/cargo-registry-deb
  "$engine" run --rm --security-opt label=disable \
    -v "$PWD:/src" \
    -v "$PWD/target/cargo-registry-deb:/root/.cargo/registry" \
    -w /src -e GE_IN_CONTAINER=1 -e CARGO_TARGET_DIR=/src/target/deb \
    refbraid-deb-build sh packaging/build-deb.sh
  ls -l dist/*.deb
  exit 0
fi

export PATH="$HOME/.cargo/bin:$PATH"
out="${CARGO_TARGET_DIR:-$PWD/target}"
cargo build --release

root="$out/deb-stage"
rm -rf "$root"
install -Dm755 "$out/release/refbraid" "$root/usr/bin/refbraid"
install -Dm644 packaging/rpm/refbraid.desktop "$root/usr/share/applications/refbraid.desktop"
install -Dm644 packaging/rpm/refbraid.png "$root/usr/share/icons/hicolor/256x256/apps/refbraid.png"
install -Dm644 packaging/rpm/refbraid-nautilus.py "$root/usr/share/nautilus-python/extensions/refbraid-nautilus.py"
# Where Debian keeps the licence of a package.
install -Dm644 "$lic/LICENSE" "$root/usr/share/doc/refbraid/copyright"
install -Dm644 "$lic/THIRD-PARTY-NOTICES.md" "$root/usr/share/doc/refbraid/THIRD-PARTY-NOTICES.md"

size=$(du -sk "$root/usr" | cut -f1)
mkdir -p "$root/DEBIAN"
sed -e "s/@VERSION@/$version-$release/" -e "s/@SIZE@/$size/" packaging/deb/control > "$root/DEBIAN/control"

mkdir -p dist
deb="dist/refbraid_${version}-${release}_amd64.deb"
# xz, not the zstd Ubuntu uses by default: older Debian cannot unpack that.
dpkg-deb --root-owner-group -Zxz --build "$root" "$deb"
dpkg-deb --info "$deb"
