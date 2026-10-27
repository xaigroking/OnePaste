#!/usr/bin/env bash
# Writes the release notes of an automatic pre-release to standard output.
# Usage: scripts/make-release-notes.sh <tag> <commit> [previous-tag]
set -euo pipefail
cd "$(dirname "$0")/.."
tag=${1:?usage: make-release-notes.sh <tag> <commit> [previous-tag]}
commit=${2:?usage: make-release-notes.sh <tag> <commit> [previous-tag]}
prev=${3:-}

cat <<EOT
OnePaste $tag automatic pre-release of the main branch

## ⚠️ Read first
This is an **automatic pre-release**: it was built from the \`main\` branch at commit \`${commit:0:7}\` right after a pull request was merged. It is **not signed** and it is **not a stable release**. Expect rough edges, and do not rely on it as your only clipboard tool yet.
It is **portable**: unzip it and run it. Nothing is installed and no administrator rights are needed.

EOT
cat packaging/release-notes/first-launch.txt
echo
cat packaging/release-notes/try-it.txt
echo
echo "## What's changed"
if [ -n "$prev" ]; then
  echo "Pull requests merged since \`$prev\`:"
  echo
  python3 tools/release_changes.py "$prev..$commit"
else
  echo "Pull requests merged into \`main\`:"
  echo
  python3 tools/release_changes.py "$commit"
fi
cat <<'EOT'

## Verify downloads
- Check where the file came from: `gh attestation verify <file> --repo xaigroking/OnePaste`
- Windows checksum: `Get-FileHash <file> -Algorithm SHA256`
- macOS and Linux checksum: `shasum -a 256 <file>`

Compare the checksum with the matching line in `SHA256SUMS.txt`.
EOT
