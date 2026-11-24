#!/bin/sh
# Builds the macOS application: dist/refbraid-<version>-macos-universal.dmg
# One program for both kinds of Mac (Apple silicon and Intel), macOS 11 and newer.
# Runs on a Mac only (Xcode command line tools and rustup); CI does it on a macOS runner.
# The application is signed "ad hoc" (no Apple developer certificate, not notarised): the first
# start has to be allowed in System Settings → Privacy & Security → Open Anyway.
set -e
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != "Darwin" ]; then
  echo "The macOS application can only be built on a Mac (it needs lipo, codesign and hdiutil)." >&2
  echo "On GitHub: Actions → Release → Run workflow builds it without publishing anything." >&2
  exit 1
fi
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
export PATH="$HOME/.cargo/bin:$PATH"
# The oldest macOS the program starts on (11 is the first one for Apple silicon).
export MACOSX_DEPLOYMENT_TARGET=11.0
out="${CARGO_TARGET_DIR:-$PWD/target}"

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

stage="$out/macos"
app="$stage/RefBraid.app"
rm -rf "$stage"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" dist
lipo -create -output "$app/Contents/MacOS/refbraid" \
  "$out/aarch64-apple-darwin/release/refbraid" "$out/x86_64-apple-darwin/release/refbraid"
chmod 755 "$app/Contents/MacOS/refbraid"
sed -e "s/@VERSION@/$version/g" -e "s/@MACOS@/$MACOSX_DEPLOYMENT_TARGET/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
# The icon is drawn by the program itself.
"$app/Contents/MacOS/refbraid" --write-icon "$app/Contents/Resources/RefBraid.icns"
cp -f ../LICENSE ../THIRD-PARTY-NOTICES.md "$app/Contents/Resources/"
# Without any signature macOS calls a program for Apple silicon damaged.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

# The window a .dmg opens: the application and the folder to drag it into.
ln -s /Applications "$stage/Applications"
dmg="dist/refbraid-$version-macos-universal.dmg"
rm -f "$dmg"
n=0
# hdiutil now and then answers "Resource busy" while the system still indexes the folder.
until hdiutil create -quiet -volname "RefBraid $version" -srcfolder "$stage" -format UDZO -ov "$dmg"; do
  n=$((n + 1))
  [ "$n" -lt 4 ] || exit 1
  sleep 5
done
lipo -archs "$app/Contents/MacOS/refbraid"
ls -l "$dmg"
