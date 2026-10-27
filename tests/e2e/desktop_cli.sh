#!/usr/bin/env bash
# Desktop application against the command line client, through a real server: what one writes shows up on the other,
# nothing comes back as a duplicate, a deletion and a revocation take effect. The desktop side is the application logic
# with a fake clipboard (the test harness, built with the test-harness feature), driven from standard input.
# Needs: bash, jq, python3 and the built server, command line client and harness.
# Usage: tests/e2e/desktop_cli.sh [binary-directory]   (default: target/debug)
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
SERVER=$(cd "$BIN_DIR" && pwd)/onepaste-server
CLI=$(cd "$BIN_DIR" && pwd)/onepaste
HARNESS=$(cd "$BIN_DIR" && pwd)/onepaste-app-harness
test -x "$HARNESS" || die "the test harness is not built: $HARNESS"
ROOT=$(mktemp -d)
SERVER_PID=""
PASS='desktop-cli-passphrase-for-the-test'   # no spaces: the harness splits its join command on spaces
export ONEPASTE_PASSPHRASE="$PASS"
cleanup() {
  [ -n "${H_PID:-}" ] && kill "$H_PID" 2> /dev/null || true
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2> /dev/null || true
  rm -rf "$ROOT"
}
trap cleanup EXIT
a() { ONEPASTE_HOME="$ROOT/A" "$CLI" "$@"; }
ZH2=$(printf '\xe4\xb8\xad\xe6\x96\x87')   # a two-character Chinese word

# h <command...>: sends one command to the harness and leaves its answer in H_OUT
h() {
  printf '%s\n' "$*" >&"${H[1]}"
  H_OUT=""
  local line
  while IFS= read -r -t 60 line <&"${H[0]}"; do
    if [ "$line" = "END" ]; then return 0; fi
    H_OUT+="$line"$'\n'
  done
  die "the harness did not answer: $*"
}
h_ok() { h "$@"; [ "$H_OUT" = $'ok\n' ] || die "the harness answered '$H_OUT' to: $*"; }

step "start the server, the first device and the desktop application"
PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
"$SERVER" --data-dir "$ROOT/srv" --listen "127.0.0.1:$PORT" --public-url "https://127.0.0.1:$PORT" > "$ROOT/server.out" 2> "$ROOT/server.err" &
SERVER_PID=$!
for _ in $(seq 1 100); do grep -q '"message":"started"' "$ROOT/server.out" 2> /dev/null && break; sleep 0.1; done
a join "$(grep -m1 '^onepaste1:' "$ROOT/server.out")" --name cli > /dev/null 2>&1
coproc H { "$HARNESS" "$ROOT/H" 2> "$ROOT/harness.err"; }
h_ok join "$(a invite)" "$PASS" harness
h_ok sync
ok "the desktop application joined the existing vault and finished its first sync"

step "1: an item added on the command line shows up on the desktop and is written to its clipboard"
printf 'text from the command line' | a add > /dev/null 2>&1
h_ok sync
h list
grep -q "text from the command line" <<< "$H_OUT" || die "the desktop history does not hold the new item: $H_OUT"
h clipboard
test "$H_OUT" = $'text text from the command line\n' || die "the fake clipboard holds: $H_OUT"
h status
test "$(jq '.pending' <<< "$H_OUT")" -eq 0 || die "the desktop queued something after applying: $H_OUT"
h_ok sync
a sync 2> /dev/null
test "$(a list --json | jq '[.[] | select(.deleted == false)] | length')" -eq 1
ok "the item is in the history and on the clipboard, and nothing came back as a duplicate"

step "2: text copied on the desktop reaches the command line exactly once"
h_ok copy "text from the desktop"
h_ok sync
a sync 2> /dev/null
test "$(a list --json | jq '[.[] | select(.first_line == "text from the desktop" and .deleted == false)] | length')" -eq 1
h_ok sync
a sync 2> /dev/null
test "$(a list --json | jq '[.[] | select(.deleted == false)] | length')" -eq 2
ok "one copy, one item, also after further syncs"

step "3: an image copied on the desktop reaches the command line as an image"
h_ok copy-image
h_ok sync
a sync 2> /dev/null
test "$(a list --json | jq '[.[] | select(.kind == "image" and .deleted == false)] | length')" -eq 1
test "$(a list --json | jq -r '[.[] | select(.kind == "image")][0].first_line')" = ""
ok "the image arrived with kind image"

step "4: a deletion on the command line removes the item from the desktop and its search"
OLD=$(a list --json | jq -r '.[] | select(.first_line == "text from the command line") | .id')
a rm "$OLD" > /dev/null 2>&1
h_ok sync
h list
if grep -q "text from the command line" <<< "$H_OUT"; then die "the deleted item is still listed: $H_OUT"; fi
h search command line
if grep -q "text from the command line" <<< "$H_OUT"; then die "the deleted item is still found: $H_OUT"; fi
ok "gone from the list and from the search"

step "5: a two-character Chinese word from the command line is found on the desktop"
printf 'a note about %s today' "$ZH2" | a add > /dev/null 2>&1
h_ok sync
h search "$ZH2"
test "$(printf '%s' "$H_OUT" | grep -c .)" -eq 1 || die "expected one hit for the two-character word: $H_OUT"
ok "found with the substring search"

step "6: revoking the desktop device stops it"
HID=$(a devices | awk -F'\t' '$2 == "harness" { print $1 }')
test -n "$HID"
a revoke "$HID" > /dev/null 2>&1
h sync
test "$H_OUT" = $'error revoked\n' || die "expected 'error revoked', got: $H_OUT"
h status
test "$(jq -r '.paused' <<< "$H_OUT")" = "revoked"
test "$(jq -r '.tray' <<< "$H_OUT")" = "error"
ok "the desktop application reports revoked and shows the error state"

printf 'quit\n' >&"${H[1]}"
wait "$H_PID" 2> /dev/null || true
H_PID=""
echo "desktop cli passed"
