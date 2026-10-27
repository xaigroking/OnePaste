#!/usr/bin/env bash
# First run: a person with no command line creates a vault from the desktop application logic (the test harness, driven
# from standard input), the command line client joins it and the two sync; a second desktop application joins with the
# recovery kit; a third one runs in local-only mode and never gets a device.
# Needs: bash, jq, python3 and the built server, command line client and harness.
# Usage: tests/e2e/first_run.sh [binary-directory]   (default: target/debug)
set -Eeuo pipefail

exec 3>&2
STEP="setup"
trap 'echo "FAILED at step: $STEP (line $LINENO)" >&3' ERR
step() { STEP="$1"; echo "== $1"; }
ok() { echo "ok: $1"; }
die() { echo "FAIL: $*" >&3; exit 1; }

for tool in jq python3; do
  command -v "$tool" > /dev/null || die "the required tool '$tool' is not installed"
done

BIN_DIR=${1:-${BIN_DIR:-target/debug}}
BIN_DIR=$(cd "$BIN_DIR" && pwd)
SERVER=$BIN_DIR/onepaste-server
CLI=$BIN_DIR/onepaste
HARNESS=$BIN_DIR/onepaste-app-harness
test -x "$HARNESS" || die "the test harness is not built: $HARNESS"
ROOT=$(mktemp -d)
SERVER_PID=""
PASS='first-run-passphrase-for-the-test'   # no spaces: the harness splits its commands on spaces
export ONEPASTE_PASSPHRASE="$PASS"
cleanup() {
  for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2> /dev/null || true; done
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2> /dev/null || true
  rm -rf "$ROOT"
}
trap cleanup EXIT
a() { ONEPASTE_HOME="$ROOT/A" "$CLI" "$@"; }

# Three harness processes, each with its own pair of named pipes (bash allows only one coproc at a time).
# h1, h2 and h3 send one command to a harness and leave its answer in H_OUT.
declare -A WFD RFD
PIDS=()
start_h() {
  local n=$1 w r
  mkfifo "$ROOT/$n.in" "$ROOT/$n.out"
  "$HARNESS" "$ROOT/H$n" < "$ROOT/$n.in" > "$ROOT/$n.out" 2> "$ROOT/h$n.err" &
  PIDS+=($!)
  exec {w}> "$ROOT/$n.in" {r}< "$ROOT/$n.out"
  WFD[$n]=$w
  RFD[$n]=$r
}
start_h 1; start_h 2; start_h 3
ask() {   # ask <harness> <command...>
  local n=$1
  shift
  printf '%s\n' "$*" >&"${WFD[$n]}"
  H_OUT=""
  local line
  while IFS= read -r -t 60 line <&"${RFD[$n]}"; do
    if [ "$line" = "END" ]; then return 0; fi
    H_OUT+="$line"$'\n'
  done
  die "the harness did not answer: $*"
}
h1() { ask 1 "$@"; }
h2() { ask 2 "$@"; }
h3() { ask 3 "$@"; }
expect() { [ "$H_OUT" = "$1"$'\n' ] || die "expected '$1', got '$H_OUT'"; }

devices() {   # the number of devices the server knows, read from its own database
  python3 -c 'import sqlite3, sys; print(sqlite3.connect(sys.argv[1] + "?mode=ro", uri=True).execute("select count(*) from devices").fetchone()[0])' "file:$ROOT/srv/onepaste.db"
}
new_code() { "$SERVER" connect-code --data-dir "$ROOT/srv" --public-url "https://127.0.0.1:$PORT" 2> /dev/null | grep -m1 '^onepaste1:'; }

step "start an empty server"
PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
"$SERVER" --data-dir "$ROOT/srv" --listen "127.0.0.1:$PORT" --public-url "https://127.0.0.1:$PORT" > "$ROOT/server.out" 2> "$ROOT/server.err" &
SERVER_PID=$!
for _ in $(seq 1 100); do grep -q '"message":"started"' "$ROOT/server.out" 2> /dev/null && break; sleep 0.1; done
CODE=$(grep -m1 '^onepaste1:' "$ROOT/server.out")
test -n "$CODE" || die "the server printed no connection code"
test "$(devices)" -eq 0

step "1: a new vault from the screens, then the command line joins it and both sync"
h1 mode; expect '"unset"'
h1 probe "$CODE"; expect "empty"
test "$(devices)" -eq 0 || die "probing registered a device"
h1 create "$CODE" "$PASS" first-desktop
KIT=$(sed -n 's/^kit //p' <<< "$H_OUT")
test "${#KIT}" -eq 69 || die "no recovery kit was shown: $H_OUT"
test "$(devices)" -eq 0 || die "the vault was registered before the kit was confirmed"
h1 confirm; expect "ok"
test "$(devices)" -eq 1 || die "the vault was not registered after the confirmation"
h1 mode; expect '"synced"'
h1 show-kit; test "$H_OUT" = "kit $KIT"$'\n' || die "show-kit differs from the kit that was shown"
a join "$(new_code)" --name cli > /dev/null 2>&1
h1 copy "written on the first desktop"
h1 sync; expect "ok"
a sync 2> /dev/null
test "$(a list --json | jq '[.[] | select(.first_line == "written on the first desktop" and .deleted == false)] | length')" -eq 1
printf 'written on the command line' | a add > /dev/null 2>&1
a sync 2> /dev/null
h1 sync; expect "ok"
h1 search command line
grep -q "written on the command line" <<< "$H_OUT" || die "the desktop does not see the command line item: $H_OUT"
ok "a person with no command line can open a vault, and the two sides sync"

step "2: a second desktop joins with the recovery kit"
h2 join-kit "$(new_code)" "AAAA-AAAA" second-desktop; expect "error wrong_recovery_kit"
test "$(devices)" -eq 2 || die "a bad kit registered a device"
h2 join-kit "$(new_code)" "$KIT" second-desktop; expect "ok"
test "$(devices)" -eq 3
h2 sync; expect "ok"
h2 search first desktop
grep -q "written on the first desktop" <<< "$H_OUT" || die "the second desktop does not see the first desktop's item: $H_OUT"
ok "the recovery kit gives access to the same vault, and a bad kit does nothing"

step "3: local-only mode records and searches, with no device"
h3 local; expect "ok"
h3 mode; expect '"local_only"'
h3 copy "only on this computer"
h3 copy "another local line"
h3 list
test "$(printf '%s' "$H_OUT" | grep -c .)" -eq 2 || die "expected two local items: $H_OUT"
h3 search another local
test "$(printf '%s' "$H_OUT" | grep -c .)" -eq 1 || die "expected one hit: $H_OUT"
h3 sync; expect "ok"
h3 status
test "$(jq -r '.tray' <<< "$H_OUT")" = "local"
test "$(jq -r '.deviceId' <<< "$H_OUT")" = ""
h3 show-kit; expect "error no_recovery_kit"
test "$(devices)" -eq 3 || die "local-only mode registered a device"
test "$(python3 -c 'import sqlite3, sys; print(sqlite3.connect(sys.argv[1]).execute("select count(*) from meta where key in (\"device_id\", \"u\", \"keyparams\")").fetchone()[0])' "$ROOT/H3/onepaste.db")" -eq 0 \
  || die "the local-only database holds a device identity"
if grep -rlaF "only on this computer" "$ROOT/H3" > /dev/null 2>&1; then die "plain text found in the local-only data directory"; fi
ok "local-only: history and search work, there is no device, nothing on the server, no text on disk"

for n in 1 2 3; do printf 'quit\n' >&"${WFD[$n]}"; done
echo "first run passed"
