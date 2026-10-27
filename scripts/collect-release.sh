#!/usr/bin/env bash
# Gathers the built packages of a release, checks what is inside them, and writes the checksums and the downloads table.
# Usage: scripts/collect-release.sh <label> <staged-dir> <out-dir> <table-file>
# The label is the tag, for example v0.0.1-main.7. Expected in <staged-dir>: the desktop zips for Windows and macOS, and
# the tools zips (server and command line client) for Windows, macOS and Linux.
set -euo pipefail
cd "$(dirname "$0")/.."
label=${1:?usage: collect-release.sh <label> <staged-dir> <out-dir> <table-file>}
staged=${2:?}
out=${3:?}
table=${4:?}

win="OnePaste-${label}-windows-x64-portable.zip"
mac="OnePaste-${label}-macos-universal-portable.zip"
tools_win="OnePaste-${label}-tools-windows-x64-portable.zip"
tools_mac="OnePaste-${label}-tools-macos-universal-portable.zip"
tools_linux="OnePaste-${label}-tools-linux-x64-portable.zip"

mkdir -p "$out"
for f in "$win" "$mac" "$tools_win" "$tools_mac" "$tools_linux"; do
  test -f "$staged/$f" || { echo "missing build output: $f"; ls -R "$staged"; exit 1; }
  cp "$staged/$f" "$out/"
done
python3 tools/check_portable_zip.py "$out/$win" OnePaste.exe READ-ME-FIRST.txt
python3 tools/check_portable_zip.py "$out/$mac" READ-ME-FIRST.txt prefix:OnePaste.app/
python3 tools/check_portable_zip.py "$out/$tools_win" onepaste-server.exe onepaste.exe READ-ME-FIRST.txt
python3 tools/check_portable_zip.py "$out/$tools_mac" onepaste-server onepaste READ-ME-FIRST.txt
python3 tools/check_portable_zip.py "$out/$tools_linux" onepaste-server onepaste READ-ME-FIRST.txt
(cd "$out" && sha256sum "$win" "$mac" "$tools_win" "$tools_mac" "$tools_linux" > SHA256SUMS.txt && cat SHA256SUMS.txt)
{
  echo "## Downloads"
  echo
  echo "| What | Platform | File |"
  echo "|---|---|---|"
  echo "| OnePaste desktop app | Windows x64 | \`$win\` |"
  echo "| OnePaste desktop app | macOS, Apple silicon and Intel (universal) | \`$mac\` |"
  echo "| Server and command line client | Windows x64 | \`$tools_win\` |"
  echo "| Server and command line client | macOS, Apple silicon and Intel (universal) | \`$tools_mac\` |"
  echo "| Server and command line client | Linux x64 | \`$tools_linux\` |"
} > "$table"
