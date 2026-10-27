#!/usr/bin/env bash
# Fails if a production binary contains any test-only fault injection string.
# Usage: scripts/no-fault-injection-strings.sh <directory with the built binaries>   (default: target/release)
set -euo pipefail
dir=${1:-target/release}
status=0
for bin in onepaste-server onepaste; do
  file="$dir/$bin"
  if [ ! -f "$file" ]; then
    echo "FAIL: $file does not exist" >&2
    status=1
    continue
  fi
  for needle in ONEPASTE_CRASH_AT drop_response_once ONEPASTE_PAGE_SIZE ONEPASTE_TOMBSTONE_RETENTION_SECS; do
    if grep -aqF -- "$needle" "$file"; then
      echo "FAIL: $file contains the test-only string $needle" >&2
      status=1
    fi
  done
  echo "checked $file"
done
exit $status
