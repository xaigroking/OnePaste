#!/usr/bin/env bash
# Fails if any of the given files contains a test-only fault injection string.
# Usage: scripts/no-fault-strings-in-files.sh <file>...
# Used on the programs that go into release packages, whatever the build cache held.
set -euo pipefail
status=0
for file in "$@"; do
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
