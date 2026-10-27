#!/usr/bin/env bash
# History search end-to-end test: the local encrypted history finds items, follows syncs and deletions, and can be
# rebuilt from nothing.
# Needs: bash, jq, python3 and the built server and command line client.
# Usage: tests/e2e/history_search.sh [binary-directory]   (default: target/debug)
set -Eeuo pipefail

exec 3>&2
STEP="setup"
trap 'echo "FAILED at step: $STEP (line $LINENO)" >&3' ERR
step() { STEP="$1"; echo "== $1"; }
ok() { echo "ok: $1"; }

for tool in jq python3; do
  command -v "$tool" > /dev/null || { echo "FAIL: the required tool '$tool' is not installed" >&2; exit 1; }
done

BIN_DIR=${1:-${BIN_DIR:-target/debug}}
SERVER=$(cd "$BIN_DIR" && pwd)/onepaste-server
CLI=$(cd "$BIN_DIR" && pwd)/onepaste
ROOT=$(mktemp -d)
SERVER_PID=""
export ONEPASTE_PASSPHRASE='history search passphrase'
cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2> /dev/null || true
  rm -rf "$ROOT"
}
trap cleanup EXIT
a() { ONEPASTE_HOME="$ROOT/A" "$CLI" "$@"; }
b() { ONEPASTE_HOME="$ROOT/B" "$CLI" "$@"; }

# Text with non-ASCII characters is written with escapes so that this file stays ASCII.
ZH2=$(printf '\xe4\xb8\xad\xe6\x96\x87')          # a two-character Chinese word
MARKER="LEAK-HIST-$(python3 -c 'import secrets; print(secrets.token_hex(8))')"
# ids_of <client> <query> prints the ids found, sorted
ids_of() { local who=$1; shift; "$who" search "$@" --json 2> /dev/null | jq -r '.[].item_id' | sort; }

step "start the server and join two devices"
PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
"$SERVER" --data-dir "$ROOT/srv" --listen "127.0.0.1:$PORT" --public-url "https://127.0.0.1:$PORT" > "$ROOT/server.out" 2> "$ROOT/server.err" &
SERVER_PID=$!
for _ in $(seq 1 100); do grep -q '"message":"started"' "$ROOT/server.out" 2> /dev/null && break; sleep 0.1; done
a join "$(grep -m1 '^onepaste1:' "$ROOT/server.out")" --name A > /dev/null 2>&1
b join "$(a invite)" --name B > /dev/null 2>&1

step "A adds three items"
ID1=$(printf 'hello search world' | a add 2> /dev/null)
ID2=$(printf 'note about %s today' "$ZH2" | a add 2> /dev/null)
ID3=$(printf 'secret %s inside' "$MARKER" | a add 2> /dev/null)

step "A finds each of them"
test "$(ids_of a world)" = "$ID1"
test "$(ids_of a "$ZH2")" = "$ID2"
test "$(ids_of a "$MARKER")" = "$ID3"
test "$(ids_of a HELLO)" = "$ID1"
test "$(a search "$ZH2" | cut -f1)" = "$ID2"
ok "English word, two-character Chinese word and the marker are found on A"

step "B pulls and finds them too"
b sync 2> /dev/null
test "$(ids_of b world)" = "$ID1"
test "$(ids_of b "$ZH2")" = "$ID2"
test "$(ids_of b "$MARKER")" = "$ID3"
ok "the same searches work on B after a sync"

step "a delete on B disappears from A's search after A syncs"
b rm "$ID1" > /dev/null 2>&1
a sync 2> /dev/null
test -z "$(ids_of a world)"
test "$(ids_of a "$ZH2")" = "$ID2"
ok "the deleted item is no longer found"

step "the history database holds nothing readable and is private"
for f in "$ROOT"/A/history.db*; do
  if grep -qaF -e "$MARKER" -e "$ZH2" -- "$f"; then echo "the history database $f holds readable content" >&3; exit 1; fi
done
test "$(stat -c %a "$ROOT/A/history.db")" = "600"
ok "no readable content, mode 0600"

step "deleting the history database is harmless: it is rebuilt on the next search"
rm -f "$ROOT"/A/history.db*
test ! -e "$ROOT/A/history.db"
test "$(ids_of a "$ZH2")" = "$ID2"
test "$(ids_of a "$MARKER")" = "$ID3"
test -f "$ROOT/A/history.db"
ok "found again after the file was deleted"

step "reindex leaves the results unchanged"
before="$(ids_of a "$ZH2") $(ids_of a "$MARKER") $(a search '' --json | jq -r '.[].item_id' | sort | tr '\n' ' ')"
a reindex > /dev/null 2>&1
after="$(ids_of a "$ZH2") $(ids_of a "$MARKER") $(a search '' --json | jq -r '.[].item_id' | sort | tr '\n' ' ')"
test "$before" = "$after"
test "$(a search '' --json | jq 'length')" -eq 2
ok "the same results after reindex"

echo "history search passed"
