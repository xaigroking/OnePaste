#!/usr/bin/env bash
# Makes the signed update manifest of a release and puts it, with its signature, next to the packages.
# Usage: scripts/sign-update-manifest.sh <tag> <kind: main|beta|stable> <notes-file> <repo> [out-dir]
# Needs the signing key in TAURI_SIGNING_PRIVATE_KEY (and TAURI_SIGNING_PRIVATE_KEY_PASSWORD if it has one) and the
# frontend dependencies installed (the signer is part of the Tauri command line). Without the key it makes nothing and
# says so: a release is never published with an unsigned manifest, and without a manifest no client offers an update.
set -euo pipefail
cd "$(dirname "$0")/.."
tag=${1:?usage: sign-update-manifest.sh <tag> <kind> <notes-file> <repo> [out-dir]}
kind=$2
notes=$3
repo=$4
out=${5:-out}

if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "No update signing key is set up (the repository secret TAURI_SIGNING_PRIVATE_KEY is missing)."
  echo "No update manifest is made for $tag, so installed programs will not be offered this release. Everything else is published."
  exit 0
fi
python3 tools/make_update_manifest.py --tag "$tag" --released-at "$(date +%s)" --kind "$kind" --notes-file "$notes" --sums "$out/SHA256SUMS.txt" --repo "$repo" > "$out/latest.json"
manifest=$(cd "$out" && pwd)/latest.json
(cd apps/desktop && npx --no-install tauri signer sign "$manifest")
test -s "$out/latest.json.sig" || { echo "the signer made no signature"; exit 1; }
# The manifest and its signature are covered by the checksums file like every other file
(cd "$out" && sha256sum latest.json latest.json.sig >> SHA256SUMS.txt)
python3 tools/check_update_manifest.py --manifest "$out/latest.json" --signature "$out/latest.json.sig" --sums "$out/SHA256SUMS.txt" --tag "$tag" --repo "$repo"
echo "update manifest made and signed for $tag"
