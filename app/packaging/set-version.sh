#!/bin/sh
# Changes the program version everywhere it is written (Cargo.toml, Cargo.lock, the Windows
# version info) and starts the RPM release number again at 1.
#   sh packaging/set-version.sh patch|minor|major   0.2.0 -> 0.2.1 | 0.3.0 | 1.0.0
#   sh packaging/set-version.sh 1.4.2               an exact version
#   sh packaging/set-version.sh release             same version, next RPM release (-1 -> -2)
set -e
cd "$(dirname "$0")/.."
spec=packaging/rpm/refbraid.spec
cur=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
ma=$(echo "$cur" | cut -d. -f1)
mi=$(echo "$cur" | cut -d. -f2)
pa=$(echo "$cur" | cut -d. -f3)
case "$1" in
  major) new="$((ma + 1)).0.0" ;;
  minor) new="$ma.$((mi + 1)).0" ;;
  patch) new="$ma.$mi.$((pa + 1))" ;;
  release)
    rel=$(sed -n 's/^Release: *\([0-9]*\).*/\1/p' "$spec")
    sed -i "s/^Release:\( *\)[0-9]*/Release:\1$((rel + 1))/" "$spec"
    echo "Version $cur, RPM release $rel -> $((rel + 1))"
    exit 0
    ;;
  *)
    if echo "$1" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
      new="$1"
    else
      echo "Usage: $0 patch|minor|major|release|<x.y.z>" >&2
      exit 1
    fi
    ;;
esac
sed -i "0,/^version = \".*\"/s//version = \"$new\"/" Cargo.toml
sed -i "/^name = \"refbraid\"\$/{n;s/^version = \".*\"/version = \"$new\"/}" Cargo.lock
rc=packaging/windows/refbraid.rc
sed -i "s/^\(FILEVERSION\|PRODUCTVERSION\) .*/\1 $(echo "$new" | tr . ,),0/" "$rc"
sed -i "s/\(\"FileVersion\", \|\"ProductVersion\", \)\"[^\"]*\"/\1\"$new\"/" "$rc"
sed -i "s/^Release:\( *\)[0-9]*/Release:\11/" "$spec"
echo "Version $cur -> $new (RPM release 1)"
