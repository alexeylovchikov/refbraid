#!/bin/sh
# Builds the RHEL package: dist/refbraid-<version>-<release>.<el8|el9|el10>.x86_64.rpm
#   sh packaging/build-rpm.sh          RHEL 8 container (UBI 8): installs on RHEL 8, 9 and 10
#   sh packaging/build-rpm.sh --el9    RHEL 9 container (UBI 9): .el9 package for RHEL 9 (and 10)
#   sh packaging/build-rpm.sh --local  this computer (.el10 on RHEL 10)
set -e
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
engine=$(command -v podman || command -v docker || true)
# The licence and the notices of the libraries built into the program are in the repository
# root, which a build container does not see: keep copies where it does.
lic="$PWD/target/licences"
if [ -f ../LICENSE ]; then
  mkdir -p "$lic"
  cp -f ../LICENSE ../THIRD-PARTY-NOTICES.md "$lic/"
fi

el=el8
tag=refbraid-rpm-build
if [ "$1" = "--el9" ]; then
  el=el9
  tag=refbraid-rpm-build-el9
fi

if [ -z "$GE_IN_CONTAINER" ] && [ "$1" != "--local" ] && [ -n "$engine" ]; then
  "$engine" build -q --build-arg BASE="registry.access.redhat.com/ubi${el#el}/ubi" -t "$tag" packaging/rpm
  mkdir -p dist "target/cargo-registry-$el"
  "$engine" run --security-opt label=disable \
    -v "$PWD:/src" \
    -v "$PWD/target/cargo-registry-$el:/root/.cargo/registry" \
    -w /src -e GE_IN_CONTAINER=1 -e CARGO_TARGET_DIR="/src/target/$el" \
    "$tag" sh packaging/build-rpm.sh
  ls -l dist/*.rpm
  exit 0
fi

export PATH="$HOME/.cargo/bin:$PATH"
out="${CARGO_TARGET_DIR:-$PWD/target}"
cargo build --release
stage="$out/rpm-stage"
mkdir -p "$stage" dist
cp -f "$out/release/refbraid" "$stage/"
cp -f packaging/rpm/refbraid.desktop packaging/rpm/refbraid.png packaging/rpm/refbraid-nautilus.py "$stage/"
cp -f "$lic/LICENSE" "$lic/THIRD-PARTY-NOTICES.md" "$stage/"
top="$out/rpmbuild"
rpmbuild -bb packaging/rpm/refbraid.spec \
  --define "_topdir $top" \
  --define "app_version $version" \
  --define "src $stage"
cp -f "$(ls -t "$top"/RPMS/x86_64/refbraid-"$version"-*.rpm | head -n 1)" dist/
