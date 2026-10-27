#!/usr/bin/env bash
# Pre-release checks. If any check fails the exit code is 1 and nothing gets published.
# Usage:
#   scripts/release-check.sh <tag>                                    a named release, vX.Y.Z-beta.N; the annotated tag exists
#   scripts/release-check.sh <tag> --commit <sha> --notes-file <file> an automatic pre-release, vX.Y.Z-main.N; the tag
#                                                                     does not exist yet and the notes are generated
#   scripts/release-check.sh --test-tag-names                         asserts how tag names are told apart, and exits
# Needs GH_TOKEN (or a logged-in gh) to query pull requests and releases. The repository comes from
# GITHUB_REPOSITORY, or is derived from the origin URL.
set -uo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=scripts/lib/deny-patterns.sh
. scripts/lib/deny-patterns.sh
# shellcheck source=scripts/lib/cjk-pattern.sh
. scripts/lib/cjk-pattern.sh
# shellcheck source=scripts/lib/release-paths.sh
. scripts/lib/release-paths.sh

say() { printf '%s\n' "$*" >&2; }
fail=0
bad() { say "FAIL $*"; fail=1; }

if [ "${1:-}" = "--test-tag-names" ]; then
  failed=0
  expect() { [ "$(tag_kind "$1")" = "$2" ] || { say "FAIL tag_kind $1: expected $2, got $(tag_kind "$1")"; failed=1; }; }
  expect v0.0.1-beta.4 beta
  expect v0.0.1-beta.12 beta
  expect v0.0.1-main.1 main
  expect v0.0.1-main.123 main
  expect v0.0.1 other
  expect v0.0.1-main other
  expect v0.0.1-main.1x other
  expect v0.0.1-test.1 other
  expect v0.0.1-beta.1-main.2 other
  expect main.1 other
  # The automatic tags are the only ones the clean-up may ever touch
  [ "$(printf 'v0.0.1-beta.2\nv0.0.1-beta.3\nv0.0.1-main.1\n' | prune_list 0)" = "v0.0.1-main.1" ] || { say "FAIL prune_list touched a name that is not an automatic tag"; failed=1; }
  [ "$failed" = 0 ] && echo "release-check.sh: tag name assertions OK"
  exit "$failed"
fi

tag="${1:-}"
[ -n "$tag" ] || { say "usage: scripts/release-check.sh <tag> [--commit <sha> --notes-file <file>]"; exit 2; }
shift
auto_commit=""
auto_notes=""
while [ $# -gt 0 ]; do
  case "$1" in
    --commit) auto_commit="${2:-}"; shift 2 ;;
    --notes-file) auto_notes="${2:-}"; shift 2 ;;
    *) say "unknown argument: $1"; exit 2 ;;
  esac
done
kind=$(tag_kind "$tag")
if [ "$kind" = main ]; then
  { [ -n "$auto_commit" ] && [ -f "$auto_notes" ]; } || { say "an automatic pre-release needs --commit and --notes-file"; exit 2; }
fi

repo="${GITHUB_REPOSITORY:-}"
if [ -z "$repo" ]; then
  repo=$(git remote get-url origin 2>/dev/null | sed -E 's#^.*github\.com[:/]##; s#\.git$##')
fi

# 1) Tag format: only vX.Y.Z-beta.N (named) and vX.Y.Z-main.N (automatic) are allowed
version=""
if [[ "$tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)-(beta|main)\.[0-9]+$ ]]; then
  version="${BASH_REMATCH[1]}"
elif [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  bad "stable releases are not open yet: $tag"
else
  bad "tag format must be vX.Y.Z-beta.N: $tag"
fi

# 2) Tag position: the commit is on main and was merged through a pull request
if [ "$kind" = main ]; then
  commit=$(git rev-parse -q --verify "${auto_commit}^{commit}" 2>/dev/null || true)
  if git rev-parse -q --verify "refs/tags/$tag" > /dev/null; then bad "the tag already exists: $tag"; fi
else
  commit=$(git rev-parse -q --verify "refs/tags/${tag}^{commit}" 2>/dev/null || true)
fi
if [ -z "$commit" ]; then
  bad "commit or tag not found: $tag"
else
  if ! git rev-parse -q --verify refs/remotes/origin/main >/dev/null; then
    bad "origin/main not found, cannot confirm the tag is on main"
  elif ! git merge-base --is-ancestor "$commit" refs/remotes/origin/main; then
    bad "the commit the tag points to is not on main: $commit"
  fi
  if ! count=$(gh api "repos/${repo}/commits/${commit}/pulls" --jq 'length' 2>/dev/null); then
    bad "cannot query the pull requests of the commit (repository ${repo})"
  elif [ "$count" = "0" ]; then
    bad "the commit the tag points to did not reach main through a pull request: $commit"
  fi
fi

# 3) Versions agree: tag, package.json, tauri.conf.json and src-tauri/Cargo.toml
if [ -n "$version" ]; then
  pkg=$(python3 -c 'import json;print(json.load(open("apps/desktop/package.json"))["version"])' 2>/dev/null || echo "?")
  tauri=$(python3 -c 'import json;print(json.load(open("apps/desktop/src-tauri/tauri.conf.json"))["version"])' 2>/dev/null || echo "?")
  cargo=$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' apps/desktop/src-tauri/Cargo.toml | head -n1)
  for pair in "apps/desktop/package.json:$pkg" "apps/desktop/src-tauri/tauri.conf.json:$tauri" "apps/desktop/src-tauri/Cargo.toml:${cargo:-?}"; do
    if [ "${pair#*:}" != "$version" ]; then
      bad "version mismatch: the tag says $version, ${pair%%:*} says ${pair#*:}"
    fi
  done
fi

# 4) Release notes: the tag must be annotated and its message must contain the 8 required sections
notes=""
whats_new="What's new"
required_sections=("Read first" "What's new" "Fixes" "Known issues" "First launch" "Remove completely" "Verify downloads" "Test checklist")
if [ "$kind" = main ]; then
  # The notes of an automatic pre-release are generated by scripts/make-release-notes.sh
  whats_new="What's changed"
  required_sections=("Read first" "First launch" "Try it in 3 steps" "Run the server with Docker" "What's changed" "Verify downloads")
  notes=$(cat "$auto_notes")
elif [ -n "$commit" ]; then
  if [ "$(git cat-file -t "refs/tags/$tag")" != "tag" ]; then
    bad "the tag must be annotated (git tag -a); a lightweight tag carries no release notes: $tag"
  else
    notes=$(git for-each-ref --format='%(contents)' "refs/tags/$tag")
  fi
fi
if [ -n "$notes" ]; then
  for title in "${required_sections[@]}"; do
    # A Markdown heading, optionally with one leading symbol such as a warning sign
    if ! grep -Eq "^#{1,6}[[:space:]]+([^[:alnum:][:space:]]+[[:space:]]+)?${title}([[:space:]].*)?$" <<<"$notes"; then
      bad "release notes are missing the section: $title"
    fi
  done
  # The first launch steps are the same text in every release: the shared template, word for word
  first_launch_template=$(cat packaging/release-notes/first-launch.txt)
  if [[ "$notes" != *"$first_launch_template"* ]]; then
    bad "the First launch section differs from packaging/release-notes/first-launch.txt"
  fi
fi

# 4b) Order: the first launch steps must come before the list of changes, so nobody has to scroll to find them
if [ -n "$notes" ]; then
  heading_line() {
    grep -nEi "^#{1,6}[[:space:]]+([^[:alnum:][:space:]]+[[:space:]]+)?${1}([[:space:]].*)?$" <<<"$notes" | head -n1 | cut -d: -f1
  }
  read_first=$(heading_line "Read first"); first_launch=$(heading_line "First launch"); whats_new_line=$(heading_line "$whats_new")
  if [ -n "$read_first" ] && [ -n "$first_launch" ] && [ -n "$whats_new_line" ]; then
    if [ "$first_launch" -lt "$read_first" ] || [ "$first_launch" -gt "$whats_new_line" ]; then
      bad "section order: \"First launch\" must come after \"Read first\" and before \"$whats_new\""
    fi
  fi
fi

# 5) Release notes must be English only, and free of internal information and AI names
#    (same pattern as check.sh item 12)
if [ -n "$notes" ]; then
  if hits=$(LC_ALL=C.UTF-8 grep -nP -- "$CJK_REGEX" <<<"$notes"); then
    bad "release notes contain CJK characters:"
    say "$hits"
  fi
  if hits=$(grep -nE -i -- "$DENY_REGEX" <<<"$notes"); then
    bad "release notes contain internal information, license declarations or AI names:"
    say "$hits"
  fi
fi

# 6) No duplicate release
if [ -n "$repo" ]; then
  out=$(gh api "repos/${repo}/releases/tags/${tag}" 2>&1)
  rc=$?
  if [ "$rc" -eq 0 ]; then
    bad "a release with this name already exists: $tag"
  elif ! grep -q 'HTTP 404' <<<"$out"; then
    bad "cannot tell whether the release already exists: $out"
  fi
else
  bad "cannot determine the repository name"
fi

if [ "$fail" -eq 0 ]; then
  echo "release-check.sh: OK ($tag)"
else
  echo "release-check.sh: FAILED ($tag)" >&2
  exit 1
fi
