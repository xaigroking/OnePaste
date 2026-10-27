#!/usr/bin/env bash
# Fails if a frontend dependency of the desktop app has a forbidden license. Run it from apps/desktop after npm ci.
set -euo pipefail
cd "$(dirname "$0")/../apps/desktop"
npx --no-install license-checker-rseidelsohn --summary | tee licenses.txt
npx --no-install license-checker-rseidelsohn --failOn "GPL;GPL-1.0;GPL-2.0;GPL-2.0-only;GPL-2.0-or-later;GPL-3.0;GPL-3.0-only;GPL-3.0-or-later;AGPL;AGPL-1.0;AGPL-3.0;AGPL-3.0-only;AGPL-3.0-or-later;LGPL;LGPL-2.0;LGPL-2.0-only;LGPL-2.0-or-later;LGPL-2.1;LGPL-2.1-only;LGPL-2.1-or-later;LGPL-3.0;LGPL-3.0-only;LGPL-3.0-or-later;SSPL-1.0;BUSL-1.1" > /dev/null
if grep -Ei '(^|[^A-Za-z])(A?GPL|LGPL|SSPL|BUSL)' licenses.txt; then
  echo "FAIL: a forbidden license appears among the frontend dependencies"; exit 1
fi
