#!/usr/bin/env bash
# End-to-end test: one real server and several real command line clients, each with its own data directory.
# Needs: bash, jq, python3 and the built binaries (the client built with --features fault-injection).
# Usage: tests/e2e/two_clients.sh [binary-directory]   (default: target/debug)
set -Eeuo pipefail

BIN_DIR=${1:-${BIN_DIR:-target/debug}}
SERVER=$(cd "$BIN_DIR" && pwd)/onepaste-server
CLI=$(cd "$BIN_DIR" && pwd)/onepaste
ROOT=$(mktemp -d)
STEP="setup"
SERVER_PID=""
PASS='correct horse battery staple'
export ONEPASTE_PASSPHRASE="$PASS"
MARKER='ONEPASTE-LEAK-MARKER-7f3a'
# Non-ASCII text is written with escapes so that this file stays ASCII: "hello <CJK> <emoji> end"
TEXT=$(printf 'hello \xe4\xb8\xad\xe6\x96\x87 \xf0\x9f\x98\x80 end')

cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$ROOT"
}
trap cleanup EXIT
# A copy of the real standard error, so the failure report is never swallowed by a "2> /dev/null" on the failing command
exec 3>&2
trap 'echo "FAILED at step: $STEP (line $LINENO)" >&3' ERR

step() { STEP="$1"; echo "== step $1"; }
ok() { echo "ok: $1"; }
a() { ONEPASTE_HOME="$ROOT/A" "$CLI" "$@"; }
b() { ONEPASTE_HOME="$ROOT/B" "$CLI" "$@"; }
c() { ONEPASTE_HOME="$ROOT/C" "$CLI" "$@"; }

PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')

start_server() {
  : > "$ROOT/server.start"
  "$SERVER" --data-dir "$ROOT/srv" --listen "127.0.0.1:$PORT" --public-url "https://127.0.0.1:$PORT" > "$ROOT/server.start" 2>> "$ROOT/server.err" &
  SERVER_PID=$!
  for _ in $(seq 1 100); do
    grep -q '"message":"started"' "$ROOT/server.start" 2>/dev/null && break
    sleep 0.2
  done
  grep -q '"message":"started"' "$ROOT/server.start"
}

stop_server() {
  kill "$SERVER_PID"
  wait "$SERVER_PID" 2>/dev/null || true
  SERVER_PID=""
  # Collect everything this run of the server wrote
  cat "$ROOT/server.start" >> "$ROOT/server.all"
}

# Finds one item in the JSON list of a client by id: prints the item
item() { jq -e --arg id "$2" '.[] | select(.id == $id)' <<< "$1"; }

step "1: start the server and take the connection code"
start_server
CODE1=$(grep -m1 '^onepaste1:' "$ROOT/server.start")
test -n "$CODE1"
ok "the server printed a connection code on first start"

step "2: the first device joins, a second device joins by invitation"
a join "$CODE1" --name A --recovery-kit-out "$ROOT/kit.txt" > /dev/null
test -s "$ROOT/kit.txt"
CODE2=$(a invite)
b join "$CODE2" --name B > /dev/null
test "$(a devices | wc -l)" -eq 2
ok "two devices registered"

step "3: a wrong passphrase is refused and registers nothing"
CODE3=$(a invite)
before=$(a devices | wc -l)
rc=0
ONEPASTE_PASSPHRASE='wrong passphrase' c join "$CODE3" --name C > /dev/null 2>&1 || rc=$?
test "$rc" -eq 3
test "$(a devices | wc -l)" -eq "$before"
ok "exit code 3 and no new device"

step "4: an item added on A arrives on B byte for byte"
ID1=$(printf '%s' "$TEXT" | a add 2>/dev/null)
b sync 2>/dev/null
b get "$ID1" > "$ROOT/got.bin"
printf '%s' "$TEXT" > "$ROOT/want.bin"
cmp "$ROOT/got.bin" "$ROOT/want.bin"
printf '%s' "$MARKER" | a add > /dev/null 2>&1
ok "text with non-ASCII characters and an emoji is identical"

step "5: concurrent changes to different fields are both kept"
b sync 2>/dev/null
stop_server
# Both fail to reach the server, and both leave the change queued
a set "$ID1" --pinned true > /dev/null 2>&1 || true
b set "$ID1" --note x > /dev/null 2>&1 || true
test "$(a status --json | jq '.outbox.pending')" -eq 1
start_server
a sync 2>/dev/null
b sync 2>/dev/null
a sync 2>/dev/null
for who in a b; do
  item "$($who list --json)" "$ID1" | jq -e '.pinned == true and .note == "x"' > /dev/null
done
ok "pinned and note both survive on both devices"

step "6: a delete on B shows as a tombstone on A"
b rm "$ID1" 2>/dev/null
a sync 2>/dev/null
item "$(a list --json)" "$ID1" | jq -e '.deleted == true' > /dev/null
ok "A sees the tombstone"

step "7: items queued while the server is down are uploaded later"
stop_server
rc1=0
rc2=0
ID_X=$(printf 'offline one' | a add 2>/dev/null) || rc1=$?
ID_Y=$(printf 'offline two' | a add 2>/dev/null) || rc2=$?
test "$rc1" -ne 0 && test "$rc2" -ne 0
test "$(a status --json | jq '.outbox.pending')" -eq 2
start_server
a sync 2>/dev/null
b sync 2>/dev/null
for id in "$ID_X" "$ID_Y"; do
  item "$(b list --json)" "$id" | jq -e '.deleted == false' > /dev/null
done
ok "both queued items arrived"

step "8: a lost response does not duplicate an item"
rc=0
ID_F=$(printf 'lost response' | ONEPASTE_FAULT=drop_response_once a add 2>/dev/null) || rc=$?
test "$rc" -ne 0
a sync 2>/dev/null
b sync 2>/dev/null
test "$(b list --json | jq '[.[] | select(.first_line == "lost response")] | length')" -eq 1
test "$(a list --json | jq '[.[] | select(.first_line == "lost response")] | length')" -eq 1
item "$(b list --json)" "$ID_F" > /dev/null
ok "the item exists exactly once"

step "9: a revoked device is stopped"
BID=$(a devices | awk -F'\t' '$2 == "B" { print $1 }')
test -n "$BID"
a revoke "$BID" > /dev/null
rc=0
b sync > /dev/null 2>&1 || rc=$?
test "$rc" -eq 5
ok "exit code 5"

step "10: a rolled-back server is detected"
stop_server
cp -a "$ROOT/srv" "$ROOT/srv.backup"
start_server
for n in 1 2 3; do printf 'after backup %s' "$n" | a add > /dev/null 2>&1; done
stop_server
rm -rf "$ROOT/srv"
mv "$ROOT/srv.backup" "$ROOT/srv"
start_server
rc=0
a sync > /dev/null 2>&1 || rc=$?
test "$rc" -eq 4
test "$(a status --json | jq -r '.paused')" = "server_rollback"
ok "exit code 4 and the state is server_rollback"

step "11: the server logs hold no plaintext and no secrets"
stop_server
cat "$ROOT/server.err" >> "$ROOT/server.all" 2>/dev/null || true
grep -v '^onepaste1:' "$ROOT/server.all" > "$ROOT/server.logs" || true
# The access log must really be there, otherwise the search below proves nothing
test "$(grep -c '"request_id"' "$ROOT/server.logs")" -gt 20
for needle in "$MARKER" "$PASS" "$TEXT" "lost response" "offline one"; do
  if grep -qF -- "$needle" "$ROOT/server.logs"; then
    echo "the server log contains a secret or plaintext" >&2
    exit 1
  fi
done
ok "no leak found in $(wc -l < "$ROOT/server.logs") log lines"

echo "all 11 steps passed"
