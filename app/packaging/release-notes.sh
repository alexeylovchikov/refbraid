#!/bin/sh
# Writes the description of a GitHub release: which file is for what, then the section of
# that version from the English changelog.
#   sh packaging/release-notes.sh <version> <folder with the installers>
set -e
version="$1"
dist="$2"
here="$(dirname "$0")"

row() { # <pattern> <what it is for>
  for f in "$dist"/$1; do
    [ -f "$f" ] && echo "| \`$(basename "$f")\` | $2 |"
  done
  return 0
}

echo "## Downloads"
echo
echo "| File | For |"
echo "|---|---|"
row "*.msi" "Windows 10 and 11: the installer"
row "*.zip" "Windows: the program without installing, unpack and run"
row "*.deb" "Debian 12 and newer, Ubuntu 22.04 and newer"
row "*.AppImage" "Any Linux: one file, make it executable and run"
row "*.el10.x86_64.rpm" "RHEL 10 and compatible"
row "*.el9.x86_64.rpm" "RHEL 9 and compatible"
row "*.el8.x86_64.rpm" "RHEL 8 and compatible"
echo
echo "\`SHA256SUMS.txt\` holds the checksums of these files."
echo
echo "## What's new"
echo
notes=$(awk -v v="$version" '$1 == "##" { on = ($2 == v); next } on' "$here/../CHANGELOG.en.md")
if [ -n "$notes" ]; then
  echo "$notes"
else
  echo "See \`app/CHANGELOG.en.md\`."
fi
