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
# The test harness of the application logic must not be part of a production build
if [ -e "$dir/onepaste-app-harness" ]; then
  echo "FAIL: $dir/onepaste-app-harness exists: the test harness is in a production build" >&2
  status=1
fi
exit $status
