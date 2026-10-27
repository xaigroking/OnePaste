#!/usr/bin/env bash
# Small functions shared by the automatic release workflow and scripts/check.sh, so the rules are tested.
# Source this file; it defines functions only.

# The tag name patterns. Automatic pre-releases are v<version>-main.<N>; named betas are v<version>-beta.<N>.
MAIN_TAG_REGEX='^v[0-9]+\.[0-9]+\.[0-9]+-main\.[0-9]+$'
BETA_TAG_REGEX='^v[0-9]+\.[0-9]+\.[0-9]+-beta\.[0-9]+$'

# needs_release: reads the names of the changed files on standard input, one per line. Succeeds if a merge that changed
# them should produce a pre-release: only code, apps, packaging, the lock file and the workflows count. Documents,
# tests and scripts alone do not.
needs_release() {
  grep -Eq '^(crates/|apps/|packaging/|Cargo\.lock$|\.github/workflows/)'
}

# next_main_number <version>: reads existing tag names on standard input and prints the next automatic number for
# that version: the highest number in use plus one (1 if there is none). Numbers of deleted tags are never reused,
# as long as the newest one is kept.
next_main_number() {
  local version=$1
  awk -v v="v${version}-main." 'index($0, v) == 1 { n = substr($0, length(v) + 1); if (n ~ /^[0-9]+$/ && n + 0 > max) max = n + 0 } END { print max + 1 }'
}

# prune_list <keep>: reads tag names on standard input and prints the automatic pre-release tags to delete: all but the
# newest <keep> by number. Only names that match the automatic pattern are ever printed, never a beta.
prune_list() {
  local keep=$1
  grep -E "$MAIN_TAG_REGEX" | awk -F'-main.' '{ print $2 "\t" $0 }' | sort -rn | awk -v keep="$keep" 'NR > keep { print $2 }'
}

# tag_kind <tag>: prints main, beta or other
tag_kind() {
  if [[ $1 =~ $MAIN_TAG_REGEX ]]; then echo main
  elif [[ $1 =~ $BETA_TAG_REGEX ]]; then echo beta
  else echo other; fi
}

# needs_docker: reads the names of the changed files on standard input. Succeeds if the container image can behave
# differently afterwards: the Dockerfile, its helper files, the server and what it is built from, the lock file, or the
# workflow that builds it. Documents alone do not count.
needs_docker() {
  grep -Eq '^(Dockerfile$|\.dockerignore$|docker/|crates/onepaste-(server|core|sync)/|Cargo\.(lock|toml)$|tools/build_version\.rs$|tests/e2e/docker\.sh$|\.github/workflows/(ci|docker-image)\.yml$)'
}
