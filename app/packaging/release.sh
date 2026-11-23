#!/bin/sh
# A release (make release-all): raise the version, write the changelog with Claude Code,
# commit, tag v<version>. The installers are built afterwards by "make dist".
#   sh packaging/release.sh [patch|minor|major|x.y.z]      (default: patch)
#   sh packaging/release.sh --changelog-only               only update CHANGELOG.md and CHANGELOG.en.md (no commit)
set -e
cd "$(dirname "$0")/.."
root=$(git rev-parse --show-toplevel)

die() {
  echo "release: $*" >&2
  exit 1
}

# Claude Code follows .claude/skills/changelog/SKILL.md and edits app/CHANGELOG.md (Russian),
# then puts the same section in English into app/CHANGELOG.en.md (shown when the program's
# interface language is English).
write_changelog() {
  command -v claude >/dev/null 2>&1 || die "Claude Code (claude) is not installed; it writes the changelog"
  echo "Writing the changelog for $1 with Claude Code..."
  (
    cd "$root"
    claude -p "Update the changelog with the changelog skill (.claude/skills/changelog/SKILL.md) for version $1; the date of the section is $(date +%F). Write the section in app/CHANGELOG.md (Russian) and the same section in app/CHANGELOG.en.md (English), with the same last-commit mark in both. Change only these two files and commit nothing. At the end print the resulting section and the commits left out." \
      --permission-mode acceptEdits \
      --allowedTools "Skill" "Read" "Edit" "Write" "Grep" "Glob" "Bash(git log:*)" "Bash(git show:*)" "Bash(git diff:*)"
  ) || die "Claude Code could not update the changelog"
  grep -q "^## $1 " CHANGELOG.md || die "CHANGELOG.md has no section \"## $1 ...\""
  grep -q "^## $1 " CHANGELOG.en.md || die "CHANGELOG.en.md has no section \"## $1 ...\""
}

version() {
  sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1
}

if [ "$1" = "--changelog-only" ]; then
  write_changelog "$(version)"
  exit 0
fi

# The tag must describe exactly what gets built: nothing uncommitted in the program folder.
[ -z "$(git status --porcelain -- .)" ] || die "app/ has uncommitted changes; commit or stash them first"
git symbolic-ref -q HEAD >/dev/null || die "not on a branch (detached HEAD)"

files="Cargo.toml Cargo.lock packaging/windows/refbraid.rc packaging/rpm/refbraid.spec CHANGELOG.md CHANGELOG.en.md"
# If anything below fails, put the version files and the changelog back as they were.
trap 'git checkout -- $files 2>/dev/null; echo "release: stopped, the version and CHANGELOG.md were put back" >&2' EXIT

sh packaging/set-version.sh "${1:-patch}"
new=$(version)
! git rev-parse -q --verify "refs/tags/v$new" >/dev/null || die "tag v$new already exists"

write_changelog "$new"

git add -- $files
git commit -q -m "[chore] Выпущена версия $new, обновлён лог изменений."
trap - EXIT
git tag -a "v$new" -m "RefBraid $new"
echo "Committed and tagged v$new. Push with: git push && git push origin v$new"
