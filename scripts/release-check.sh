#!/usr/bin/env bash
# Pre-release checks. Usage: scripts/release-check.sh <tag>. If any check fails the exit code is 1
# and nothing gets published.
# Needs GH_TOKEN (or a logged-in gh) to query pull requests and releases. The repository comes from
# GITHUB_REPOSITORY, or is derived from the origin URL.
set -uo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=scripts/lib/deny-patterns.sh
. scripts/lib/deny-patterns.sh
# shellcheck source=scripts/lib/cjk-pattern.sh
. scripts/lib/cjk-pattern.sh

say() { printf '%s\n' "$*" >&2; }
fail=0
bad() { say "FAIL $*"; fail=1; }

tag="${1:-}"
[ -n "$tag" ] || { say "usage: scripts/release-check.sh <tag>"; exit 2; }

repo="${GITHUB_REPOSITORY:-}"
if [ -z "$repo" ]; then
  repo=$(git remote get-url origin 2>/dev/null | sed -E 's#^.*github\.com[:/]##; s#\.git$##')
fi

# 1) Tag format: only vX.Y.Z-beta.N is allowed
version=""
if [[ "$tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)-beta\.[0-9]+$ ]]; then
  version="${BASH_REMATCH[1]}"
elif [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  bad "stable releases are not open yet: $tag"
else
  bad "tag format must be vX.Y.Z-beta.N: $tag"
fi

# 2) Tag position: the commit is on main and was merged through a pull request
commit=$(git rev-parse -q --verify "refs/tags/${tag}^{commit}" 2>/dev/null || true)
if [ -z "$commit" ]; then
  bad "tag not found: $tag"
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
if [ -n "$commit" ]; then
  if [ "$(git cat-file -t "refs/tags/$tag")" != "tag" ]; then
    bad "the tag must be annotated (git tag -a); a lightweight tag carries no release notes: $tag"
  else
    notes=$(git for-each-ref --format='%(contents)' "refs/tags/$tag")
    for title in "Read first" "What's new" "Fixes" "Known issues" "First launch" "Remove completely" "Verify downloads" "Test checklist"; do
      # A Markdown heading, optionally with one leading symbol such as a warning sign
      if ! grep -Eq "^#{1,6}[[:space:]]+([^[:alnum:][:space:]]+[[:space:]]+)?${title}([[:space:]].*)?$" <<<"$notes"; then
        bad "release notes are missing the section: $title"
      fi
    done
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
