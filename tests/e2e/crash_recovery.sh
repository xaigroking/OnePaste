#!/usr/bin/env bash
# Crash recovery end-to-end test: real processes are killed at named points (or with kill -9) and must come back
# without losing or duplicating items and without leaving half-written files.
# Needs: bash, jq, python3, sqlite3, openssl and binaries built with the fault-injection features of the server and
# the command line client.
# Usage: tests/e2e/crash_recovery.sh [binary-directory]   (default: target/debug)
set -Eeuo pipefail

for tool in jq python3 sqlite3 openssl; do
  command -v "$tool" > /dev/null || { echo "FAIL: the required tool '$tool' is not installed" >&2; exit 1; }
done

BIN_DIR=${1:-${BIN_DIR:-target/debug}}
SERVER=$(cd "$BIN_DIR" && pwd)/onepaste-server
CLI=$(cd "$BIN_DIR" && pwd)/onepaste
ROOT=$(mktemp -d)
SCEN="setup"
STEP=""
SRV_PID=""
SD=""
PORT=""
export ONEPASTE_PASSPHRASE='crash test passphrase'
ulimit -c 0

cleanup() {
  [ -n "$SRV_PID" ] && kill -9 "$SRV_PID" 2> /dev/null || true
  rm -rf "$ROOT"
}
trap cleanup EXIT
# A copy of the real standard error, so the failure report is never swallowed by a "2> /dev/null" on the failing command
exec 3>&2
trap 'echo "FAILED in scenario $SCEN, step: $STEP (line $LINENO)" >&3' ERR

step() { STEP="$1"; echo "-- $SCEN: $1"; }
ok() { echo "ok: scenario $SCEN: $1"; }

begin() {
  SCEN="$1"
  STEP="setup"
  SD="$ROOT/s$(echo "$1" | tr -c 'A-Za-z0-9\n' '_')"
  mkdir -p "$SD"
  PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
  SRV_N=0
  echo "== scenario $SCEN"
}

# srv_start [VAR=value ...]: starts the server on the scenario's data directory and waits until it listens
srv_start() {
  SRV_N=$((SRV_N + 1))
  SRV_OUT="$SD/srv.$SRV_N.out"
  env "$@" "$SERVER" --data-dir "$SD/srv" --listen "127.0.0.1:$PORT" --public-url "https://127.0.0.1:$PORT" > "$SRV_OUT" 2>> "$SD/srv.err" &
  SRV_PID=$!
  for _ in $(seq 1 100); do
    grep -q '"message":"started"' "$SRV_OUT" 2> /dev/null && return 0
    kill -0 "$SRV_PID" 2> /dev/null || { echo "the server exited during start" >&2; return 1; }
    sleep 0.1
  done
  echo "the server did not start" >&2
  return 1
}

# srv_start_expecting_death VAR=value ...: the server must terminate by itself (a crash point)
srv_start_expecting_death() {
  SRV_N=$((SRV_N + 1))
  env "$@" "$SERVER" --data-dir "$SD/srv" --listen "127.0.0.1:$PORT" --public-url "https://127.0.0.1:$PORT" > "$SD/srv.$SRV_N.out" 2>> "$SD/srv.err" &
  local pid=$!
  for _ in $(seq 1 100); do
    kill -0 "$pid" 2> /dev/null || { wait "$pid" 2> /dev/null && return 1 || return 0; }
    sleep 0.1
  done
  kill -9 "$pid" 2> /dev/null || true
  echo "the server did not stop at the crash point" >&2
  return 1
}

srv_wait_dead() {
  for _ in $(seq 1 100); do
    kill -0 "$SRV_PID" 2> /dev/null || { wait "$SRV_PID" 2> /dev/null || true; SRV_PID=""; return 0; }
    sleep 0.1
  done
  echo "the server is still running" >&2
  return 1
}

srv_stop() {
  kill "$SRV_PID"
  wait "$SRV_PID" 2> /dev/null || true
  SRV_PID=""
}

first_code() { grep -m1 '^onepaste1:' "$SRV_OUT"; }
cli() { local who=$1; shift; ONEPASTE_HOME="$SD/$who" "$CLI" "$@"; }
db() { sqlite3 "$SD/srv/onepaste.db" "$1"; }
pending() { cli "$1" status --json | jq '.outbox.pending'; }
# Number of live items whose first line is exactly $2, as seen by client $1
count_text() { cli "$1" list --json | jq --arg t "$2" '[.[] | select(.first_line == $t and .deleted == false)] | length'; }
live_count() { cli "$1" list --json | jq '[.[] | select(.deleted == false)] | length'; }

# setup_pair [VAR=value ...]: a server with two joined devices, A and B
setup_pair() {
  srv_start "$@"
  local code
  code=$(first_code)
  cli A join "$code" --name A > /dev/null
  cli B join "$(cli A invite)" --name B > /dev/null
}

# ---------------------------------------------------------------------------------------------------------------------
begin 1
setup_pair
step "add is killed right after the operation is queued"
rc=0
printf 'one' | ONEPASTE_CRASH_AT=after_enqueue cli A add > /dev/null 2>&1 || rc=$?
test "$rc" -ne 0
step "the operation is still queued"
test "$(pending A)" -eq 1
step "sync after the restart"
cli A sync 2> /dev/null
test "$(pending A)" -eq 0
cli B sync 2> /dev/null
test "$(count_text B one)" -eq 1
ok "nothing lost, nothing duplicated, the queue is empty"
srv_stop

# ---------------------------------------------------------------------------------------------------------------------
begin 2
setup_pair
step "add is killed after the server answered but before the result was saved"
rc=0
printf 'two' | ONEPASTE_CRASH_AT=after_response_before_apply cli A add > /dev/null 2>&1 || rc=$?
test "$rc" -ne 0
test "$(pending A)" -eq 1
step "sync after the restart replays the same request"
cli A sync 2> /dev/null
cli B sync 2> /dev/null
test "$(count_text B two)" -eq 1
test "$(pending A)" -eq 0
id=$(cli B list --json | jq -r '.[] | select(.first_line == "two") | .id')
seq_a=$(cli A list --json | jq -r --arg i "$id" '.[] | select(.id == $i) | .seq')
seq_b=$(cli B list --json | jq -r --arg i "$id" '.[] | select(.id == $i) | .seq')
test "$seq_a" = "$seq_b" && test "$seq_a" -ge 1
test "$(db 'select count(*) from items')" -eq 1
ok "one item, the same seq on both devices"
srv_stop

# ---------------------------------------------------------------------------------------------------------------------
begin 3
setup_pair
for n in 1 2 3; do printf 'first %s' "$n" | cli B add > /dev/null 2>&1; done
cli A sync 2> /dev/null
for n in 4 5 6 7; do printf 'later %s' "$n" | cli B add > /dev/null 2>&1; done
before=$(cli A status --json | jq '.cursor')
step "a pull is killed with a page written but not committed"
rc=0
ONEPASTE_PAGE_SIZE=2 ONEPASTE_CRASH_AT=before_page_commit cli A sync > /dev/null 2>&1 || rc=$?
test "$rc" -ne 0
after=$(cli A status --json | jq '.cursor')
test "$before" = "$after"
step "the next sync completes the pull"
ONEPASTE_PAGE_SIZE=2 cli A sync 2> /dev/null
test "$(live_count A)" -eq 7
test "$(cli A list --json | jq '[.[].id] | unique | length')" -eq 7
test "$(cli A status --json | jq '.cursor')" -eq "$(db 'select seq from vault')"
ok "the cursor did not move on the failed page, then 7 items without duplicates"
srv_stop

# ---------------------------------------------------------------------------------------------------------------------
begin 4
setup_pair ONEPASTE_TOMBSTONE_RETENTION_SECS=1
for n in 1 2 3 4 5; do printf 'item %s' "$n" | cli B add > /dev/null 2>&1; done
cli A sync 2> /dev/null
test "$(live_count A)" -eq 5
ids=$(cli B list --json | jq -r '.[0:2][].id')
for id in $ids; do cli B rm "$id" > /dev/null 2>&1; done
sleep 2.5
srv_stop
srv_start ONEPASTE_TOMBSTONE_RETENTION_SECS=1
step "a full pull is killed before its last transaction commits"
rc=0
ONEPASTE_PAGE_SIZE=2 ONEPASTE_CRASH_AT=before_refetch_commit cli A sync > /dev/null 2>&1 || rc=$?
test "$rc" -ne 0
test "$(cli A status --json | jq -r '.refetching')" = "true"
step "the old cache is still there"
test "$(live_count A)" -eq 5
step "the next sync finishes the full pull"
ONEPASTE_PAGE_SIZE=2 cli A sync 2> /dev/null
test "$(cli A list --json | jq 'length')" -eq 3
test "$(live_count A)" -eq 3
test "$(cli A status --json | jq -r '.refetching')" = "false"
ok "5 items kept until the full pull completed, then 3"
srv_stop

# ---------------------------------------------------------------------------------------------------------------------
begin 5
setup_pair ONEPASTE_CRASH_AT=after_commit_before_response
step "the server dies after committing, before answering"
rc=0
printf 'five' | cli A add > /dev/null 2>&1 || rc=$?
test "$rc" -ne 0
srv_wait_dead
test "$(pending A)" -eq 1
fp_before=$(grep -m1 -o '"fingerprint":"[^"]*"' "$SRV_OUT")
step "the server restarts on the same data directory"
srv_start
test "$(grep -m1 -o '"fingerprint":"[^"]*"' "$SRV_OUT")" = "$fp_before"
cli A sync 2> /dev/null
cli B sync 2> /dev/null
test "$(count_text B five)" -eq 1
test "$(db 'select seq from vault')" -eq 1
test "$(db 'select count(*) from items')" -eq 1
ok "one item and one sequence number, although the client sent the request twice"
srv_stop

# ---------------------------------------------------------------------------------------------------------------------
for point in tls_before_rename tls_between_files; do
  begin "6 ($point)"
  step "the first start is killed while the certificate files are written"
  srv_start_expecting_death ONEPASTE_CRASH_AT="$point"
  step "the next start works and the files match"
  srv_start
  code=$(first_code)
  pub_cert=$(openssl x509 -in "$SD/srv/tls/cert.pem" -noout -pubkey | openssl md5)
  pub_key=$(openssl pkey -in "$SD/srv/tls/key.pem" -pubout | openssl md5)
  test "$pub_cert" = "$pub_key"
  fp_disk=$(openssl x509 -in "$SD/srv/tls/cert.pem" -noout -pubkey | openssl pkey -pubin -outform DER | openssl dgst -sha256 -binary | base64 -w0 | tr '+/' '-_' | tr -d '=')
  fp_code=$(python3 -c 'import sys, base64, json; c = sys.argv[1].split(":", 1)[1]; print(json.loads(base64.urlsafe_b64decode(c + "=" * (-len(c) % 4)))["fp"])' "$code")
  test "$fp_disk" = "$fp_code"
  step "a new client can join"
  cli A join "$code" --name A > /dev/null
  test "$(cli A devices | wc -l)" -eq 1
  ok "the certificate and the key are a pair, the connection code carries their fingerprint"
  srv_stop
done

# ---------------------------------------------------------------------------------------------------------------------
begin 7
setup_pair
step "kill -9 the server in the middle of a burst of writes"
(
  for i in $(seq 1 30); do
    printf 'burst %s' "$i" | cli A add > /dev/null 2>&1 || true
    sleep 0.15
  done
) &
loop=$!
sleep 2
kill -9 "$SRV_PID"
wait "$SRV_PID" 2> /dev/null || true
SRV_PID=""
wait "$loop"
test "$(pending A)" -gt 0
step "restart on the same data directory and sync until it succeeds"
srv_start
synced=0
for _ in $(seq 1 30); do
  if cli A sync > /dev/null 2>&1; then synced=1; break; fi
  sleep 1
done
test "$synced" -eq 1
cli B sync 2> /dev/null
step "30 distinct items, a healthy database, no repeated sequence numbers"
test "$(cli B list --json | jq '[.[] | select(.first_line | startswith("burst ")) | select(.deleted == false)] | length')" -eq 30
test "$(cli B list --json | jq '[.[].id] | unique | length')" -eq 30
test "$(db 'PRAGMA integrity_check')" = "ok"
test "$(db 'select count(*) - count(distinct seq) from items')" -eq 0
test "$(db 'select count(*) from items')" -eq 30
ok "30 items after kill -9"
srv_stop

# ---------------------------------------------------------------------------------------------------------------------
begin 8
srv_start
code=$(first_code)
step "join is killed while the recovery kit is being written"
rc=0
ONEPASTE_CRASH_AT=during_kit_write cli A join "$code" --name A --recovery-kit-out "$SD/kit.txt" > /dev/null 2>&1 || rc=$?
test "$rc" -ne 0
if [ -e "$SD/kit.txt" ]; then cli A kit-check "$SD/kit.txt" > /dev/null; fi
test "$(db 'select count(*) from devices')" -eq 0
step "joining again works and leaves a complete kit with mode 0600"
cli A join "$code" --name A --recovery-kit-out "$SD/kit.txt" > /dev/null 2>&1
cli A kit-check "$SD/kit.txt" > /dev/null
test "$(stat -c %a "$SD/kit.txt")" = "600"
step "an existing file is never overwritten and the server is not contacted"
printf 'precious' > "$SD/exist.txt"
devices_before=$(db 'select count(*) from devices')
code2=$(cli A invite)
rc=0
cli C join "$code2" --name C --recovery-kit-out "$SD/exist.txt" > /dev/null 2>&1 || rc=$?
test "$rc" -eq 2
test "$(cat "$SD/exist.txt")" = "precious"
test "$(db 'select count(*) from devices')" -eq "$devices_before"
ok "the kit is complete or absent, and an existing file stays untouched"
srv_stop

echo "all scenarios passed"
