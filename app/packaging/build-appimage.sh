#!/bin/sh
# Builds dist/refbraid-<version>-x86_64.AppImage: one file that runs on most Linux systems.
# The program is compiled on RHEL 8 (UBI container, glibc 2.28) so it also starts on older systems.
# Needs podman or docker, and the internet once to fetch appimagetool.
set -e
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
engine=$(command -v podman || command -v docker || true)

mkdir -p dist
# The licence and the notices of the libraries built into the program are in the repository
# root, which a build container does not see: keep copies where it does.
lic="$PWD/target/licences"
if [ -f ../LICENSE ]; then
  mkdir -p "$lic"
  cp -f ../LICENSE ../THIRD-PARTY-NOTICES.md "$lic/"
fi
if [ -n "$GE_IN_CONTAINER" ]; then
  # CI: this already is the RHEL 8 build environment.
  CARGO_TARGET_DIR="$PWD/target/el8" cargo build --release
else
  "$engine" build -q -t refbraid-rpm-build packaging/rpm
  mkdir -p target/cargo-registry-el8
  "$engine" run --security-opt label=disable \
    -v "$PWD:/src" \
    -v "$PWD/target/cargo-registry-el8:/root/.cargo/registry" \
    -w /src -e CARGO_TARGET_DIR=/src/target/el8 \
    refbraid-rpm-build cargo build --release
fi

app="$PWD/target/appimage/RefBraid.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"
cp -f target/el8/release/refbraid "$app/usr/bin/refbraid"
sed 's|^Exec=.*|Exec=refbraid %f|' packaging/rpm/refbraid.desktop > "$app/refbraid.desktop"
cp -f "$app/refbraid.desktop" "$app/usr/share/applications/refbraid.desktop"
cp -f packaging/rpm/refbraid.png "$app/refbraid.png"
cp -f packaging/rpm/refbraid.png "$app/usr/share/icons/hicolor/256x256/apps/refbraid.png"
ln -sf refbraid.png "$app/.DirIcon"
mkdir -p "$app/usr/share/licenses/refbraid"
cp -f "$lic/LICENSE" "$lic/THIRD-PARTY-NOTICES.md" "$app/usr/share/licenses/refbraid/"
cat > "$app/AppRun" <<'RUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/refbraid" "$@"
RUN
chmod +x "$app/AppRun"

tool="$PWD/target/tools/appimagetool-x86_64.AppImage"
if [ ! -x "$tool" ]; then
  mkdir -p "$PWD/target/tools"
  curl -sSfL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$tool"
fi
out="dist/refbraid-$version-x86_64.AppImage"
ARCH=x86_64 "$tool" --appimage-extract-and-run --no-appstream "$app" "$out"
chmod +x "$out"
ls -l "$out"
