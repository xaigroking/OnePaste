#!/usr/bin/env bash
# Leak probe: plants known strings in a full run of one server and two clients, then scans every place that is written to
# disk, sent over the network or printed, and proves the scanner itself can find a leak.
# Needs: bash, jq, python3, sqlite3, curl and binaries built with the fault-injection features. In CI it also needs
# tcpdump and sudo. Without capture rights (a developer machine) the user-space recorder tools/tcp_tap.py still sees every
# byte the clients exchange with the server, and the tcpdump checks are reported as not run; in CI they must run.
# The desktop application logic (the test harness) takes part in the flow, so its data directory, its history database and
# its traffic are scanned as well. The vault itself is created by the harness the way the first-run screens do it, so the
# passphrase and the recovery kit are planted too (the kit is only known once the vault exists; its patterns are added then).
# Usage: tests/e2e/leak_probe.sh [binary-directory]   (default: target/debug)
set -Eeuo pipefail

exec 3>&2
STEP="setup"
LAST_OUT=""
trap 'echo "FAILED at step: $STEP (line $LINENO)" >&3; [ -n "$LAST_OUT" ] && [ -f "$LAST_OUT" ] && { echo "--- last command output ($LAST_OUT):" >&3; tail -n 8 "$LAST_OUT" >&3; }' ERR
step() { STEP="$1"; echo "== $1"; }
ok() { echo "ok: $1"; }
die() { echo "FAIL: $*" >&3; exit 1; }

for tool in jq python3 sqlite3 curl; do
  command -v "$tool" > /dev/null || die "the required tool '$tool' is not installed"
done

HERE=$(cd "$(dirname "$0")" && pwd)
TAP_TOOL="$HERE/../../tools/tcp_tap.py"
BIN_DIR=${1:-${BIN_DIR:-target/debug}}
SERVER=$(cd "$BIN_DIR" && pwd)/onepaste-server
CLI=$(cd "$BIN_DIR" && pwd)/onepaste
HARNESS=$(cd "$BIN_DIR" && pwd)/onepaste-app-harness
test -x "$HARNESS" || die "the test harness is not built: $HARNESS"
ROOT=$(mktemp -d)
OUT="$ROOT/out"
TAPDIR="$ROOT/tap"
mkdir -p "$OUT" "$TAPDIR" "$ROOT/tmp" "$ROOT/cwd" "$ROOT/cores" "$ROOT/www"
export TMPDIR="$ROOT/tmp"

PIDS=()
TCPDUMP_ON=0
OLD_CORE_PATTERN=""
cleanup() {
  for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2> /dev/null || true; done
  [ -n "${HD_PID:-}" ] && kill "$HD_PID" 2> /dev/null || true
  [ -n "${H0_PID:-}" ] && kill "$H0_PID" 2> /dev/null || true
  [ -n "${HL_PID:-}" ] && kill "$HL_PID" 2> /dev/null || true
  if [ "$TCPDUMP_ON" = 1 ]; then sudo -n pkill -INT -x tcpdump 2> /dev/null || true; fi
  if [ -n "$OLD_CORE_PATTERN" ]; then sudo -n sysctl -q -w "kernel.core_pattern=$OLD_CORE_PATTERN" > /dev/null 2>&1 || true; fi
  rm -rf "$ROOT"
}
trap cleanup EXIT

free_port() { python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'; }
wait_port() {
  for _ in $(seq 1 100); do
    python3 -c 'import socket, sys; socket.create_connection(("127.0.0.1", int(sys.argv[1])), 1).close()' "$1" 2> /dev/null && return 0
    sleep 0.1
  done
  die "nothing listens on port $1"
}

# ---------------------------------------------------------------------------------------------------------------------
# Planted strings. The random part is generated on every run and appears nowhere in this file.
RAND=$(python3 -c 'import secrets; print(secrets.token_hex(8))')
M_TEXT="LEAK-TEXT-$RAND"
M_NOTE="LEAK-NOTE-$RAND"
M_PASS="LEAK-PASS-$RAND"
M_DEV="LEAK-DEV-$RAND"
export ONEPASTE_PASSPHRASE="$M_PASS"

# The forms a planted string may take when it leaks: raw, base64 (three alignments, both alphabets), hex (both cases)
# and URL encoding (both cases). One line per form: name <TAB> form <TAB> pattern.
PATTERNS="$ROOT/patterns.tsv"
python3 - "$PATTERNS" TEXT "$M_TEXT" NOTE "$M_NOTE" PASS "$M_PASS" DEV "$M_DEV" << 'PY'
import base64, sys
out, pairs = sys.argv[1], sys.argv[2:]
HEAD = {0: 0, 1: 2, 2: 3}

def b64_stable(data, k, encode):
    # Keep only the characters that do not depend on the bytes before or after the marker
    padded = b"\xab" * k + data
    s = encode(padded).decode().rstrip("=")
    tail = 0 if len(padded) % 3 == 0 else 1
    return s[HEAD[k]:len(s) - tail]

lines = []
for name, marker in zip(pairs[0::2], pairs[1::2]):
    b = marker.encode()
    forms = {"raw": b.decode(), "hex-lower": b.hex(), "hex-upper": b.hex().upper(),
             "url-upper": "".join("%%%02X" % c for c in b), "url-lower": "".join("%%%02x" % c for c in b)}
    for k in (0, 1, 2):
        forms["base64+%d" % k] = b64_stable(b, k, base64.b64encode)
        url = b64_stable(b, k, base64.urlsafe_b64encode)
        if url != forms["base64+%d" % k]:
            forms["base64url+%d" % k] = url
    for form, pat in forms.items():
        lines.append("%s\t%s\t%s" % (name, form, pat))
open(out, "w").write("\n".join(lines) + "\n")
PY

# scan_for_markers <label> [-x NAME[,NAME]] [-p patterns-file] <path>...
# Scans files and directories recursively for every form of every planted string. -x leaves some strings out of this
# scan; -p uses another pattern file (name <TAB> form <TAB> pattern). Prints what was scanned and fails on a hit.
scan_for_markers() {
  local label=$1 skip="" pat_file="$PATTERNS"
  shift
  while [ "${1:-}" = "-x" ] || [ "${1:-}" = "-p" ]; do
    if [ "$1" = "-x" ]; then skip=$2; else pat_file=$2; fi
    shift 2
  done
  local files nfiles bytes
  files=$(find "$@" -type f 2> /dev/null | sort || true)
  nfiles=$(printf '%s' "$files" | grep -c . || true)
  [ "$nfiles" -gt 0 ] || die "$label: nothing to scan"
  bytes=$(printf '%s\n' "$files" | xargs -d '\n' stat -c %s | awk '{ s += $1 } END { print s + 0 }')
  local hit=0 name form pat
  while IFS=$'\t' read -r name form pat; do
    case ",$skip," in *",$name,"*) continue ;; esac
    local found
    found=$(printf '%s\n' "$files" | xargs -d '\n' grep -laF -e "$pat" -- 2> /dev/null || true)
    if [ -n "$found" ]; then
      while IFS= read -r f; do echo "LEAK: $label: $name found as $form in $f" >&2; done <<< "$found"
      hit=1
    fi
  done < "$pat_file"
  [ "$hit" = 0 ] || return 1
  echo "scanned $label: $nfiles files, $bytes bytes, clean"
}

# ---------------------------------------------------------------------------------------------------------------------
step "start the server behind the recorder, the plain HTTP control server and the packet capture"
S_PORT=$(free_port); T1=$(free_port); H_PORT=$(free_port); T2=$(free_port)
python3 "$TAP_TOOL" "$T1" "$S_PORT" "$TAPDIR/server" > "$OUT/tap1.out" 2>&1 &
PIDS+=($!)
python3 "$TAP_TOOL" "$T2" "$H_PORT" "$TAPDIR/control" > "$OUT/tap2.out" 2>&1 &
PIDS+=($!)
(cd "$ROOT/www" && exec python3 -m http.server "$H_PORT" --bind 127.0.0.1 > "$OUT/http.out" 2>&1) &
PIDS+=($!)
wait_port "$T1"; wait_port "$T2"; wait_port "$H_PORT"

if command -v tcpdump > /dev/null && sudo -n true 2> /dev/null; then
  sudo -n tcpdump -Z root -i lo -U -w "$ROOT/cap.pcap" "port $S_PORT or port $T1 or port $H_PORT or port $T2" 2> "$OUT/tcpdump.err" &
  for _ in $(seq 1 50); do grep -q listening "$OUT/tcpdump.err" 2> /dev/null && break; sleep 0.1; done
  grep -q listening "$OUT/tcpdump.err" || die "tcpdump did not start capturing"
  TCPDUMP_ON=1
  if sudo -n sysctl -q -w "kernel.core_pattern=$ROOT/cores/core.%e.%p" > /dev/null 2>&1; then
    OLD_CORE_PATTERN=$(cat /proc/sys/kernel/core_pattern)
  fi
else
  [ "${CI:-}" != "true" ] || die "packet capture is required in CI: tcpdump and passwordless sudo must be available"
  echo "NOTE: no packet capture rights here; the tcpdump checks are not run (CI runs them)"
fi

RUST_LOG=trace "$SERVER" --data-dir "$ROOT/srv" --listen "127.0.0.1:$S_PORT" --public-url "https://127.0.0.1:$T1" > "$OUT/server.out" 2> "$OUT/server.err" &
SERVER_PID=$!
PIDS+=($SERVER_PID)
for _ in $(seq 1 100); do grep -q '"message":"started"' "$OUT/server.out" 2> /dev/null && break; sleep 0.1; done
grep -q '"message":"started"' "$OUT/server.out" || die "the server did not start"
CODE=$(grep -m1 '^onepaste1:' "$OUT/server.out")

# The desktop application logic creates the vault, as the first-run screens do: probe, create (the kit is shown), confirm
step "create the vault with the desktop application logic (new vault, recovery kit, confirmation)"
coproc H0 { "$HARNESS" "$ROOT/H0" 2> "$OUT/harness0.err"; }
hd0() {
  local name=$1
  shift
  printf '%s\n' "$*" >&"${H0[1]}"
  : > "$OUT/$name.out"
  local line
  while IFS= read -r -t 60 line <&"${H0[0]}"; do
    if [ "$line" = "END" ]; then return 0; fi
    printf '%s\n' "$line" >> "$OUT/$name.out"
  done
  die "the harness did not answer: $name"
}
hd0 harness0_probe probe "$CODE"
test "$(cat "$OUT/harness0_probe.out")" = "empty" || die "the server should have no vault yet"
hd0 harness0_create create "$CODE" "$M_PASS" desktop0
M_KIT=$(sed -n 's/^kit //p' "$OUT/harness0_create.out")
test "${#M_KIT}" -eq 69 || die "no recovery kit came back from the create command"
hd0 harness0_confirm confirm
test "$(cat "$OUT/harness0_confirm.out")" = "ok" || die "the confirmation failed"
printf 'quit\n' >&"${H0[1]}"
wait "$H0_PID" 2> /dev/null || true
H0_PID=""
# From here on the recovery kit is a planted string as well, with and without the dashes
printf 'KIT\traw\t%s\nKIT\tno-dashes\t%s\n' "$M_KIT" "${M_KIT//-/}" >> "$PATTERNS"
INVITE0=$("$SERVER" connect-code --data-dir "$ROOT/srv" --public-url "https://127.0.0.1:$T1" 2> /dev/null | grep -m1 '^onepaste1:')
test -n "$INVITE0" || die "the server printed no connection code"

# cli <A|B> <name> <args...>: runs the client and keeps everything it prints in $OUT/<name>.out
cli() {
  local who=$1 name=$2
  shift 2
  LAST_OUT="$OUT/$name.out"
  ONEPASTE_HOME="$ROOT/$who" "$CLI" "$@" > "$OUT/$name.out" 2>&1
}
cli_stdin() {
  local who=$1 name=$2 text=$3
  shift 3
  LAST_OUT="$OUT/$name.out"
  printf '%s' "$text" | ONEPASTE_HOME="$ROOT/$who" "$CLI" "$@" > "$OUT/$name.out" 2>&1
}

step "run the whole flow with the planted strings"
cli A join_a join "$INVITE0" --name "$M_DEV"
cli A invite invite
INVITE=$(grep -m1 '^onepaste1:' "$OUT/invite.out")
cli B join_b join "$INVITE" --name "device-b"
cli_stdin A add "body $M_TEXT end" add
ID=$(grep -m1 -E '^[0-9a-f-]{36}$' "$OUT/add.out")
cli A set set "$ID" --note "$M_NOTE"
cli B sync_b sync
cli A sync_a sync
cli_stdin A add2 "second $M_TEXT" add
ID2=$(grep -m1 -E '^[0-9a-f-]{36}$' "$OUT/add2.out")
cli B sync_b2 sync
cli B rm rm "$ID2"
cli A sync_a2 sync
cli A status status
cli A devices devices
BID=$(awk -F'\t' '$2 == "device-b" { print $1 }' "$OUT/devices.out")
test -n "$BID"
cli A revoke revoke "$BID"
rc=0
cli B sync_b3 sync || rc=$?
test "$rc" -eq 5
# list and get print the content to the user on purpose
cli A list list
cli A get get "$ID"
ok "flow finished"

step "run the desktop application logic through the same server with the planted strings"
# Searching builds the history database of A (an encrypted copy of what A holds)
cli A search_a search "$M_TEXT"
# The server allows ten registration-type requests a minute from one address; the flow above has used most of them
sleep 20
cli A invite_h invite
INVITE_H=$(grep -m1 '^onepaste1:' "$OUT/invite_h.out")
coproc HD { "$HARNESS" "$ROOT/H" 2> "$OUT/harness.err"; }
# hd <name> <command...>: sends one command to the harness and keeps its answer in $OUT/<name>.out
hd() {
  local name=$1
  shift
  printf '%s\n' "$*" >&"${HD[1]}"
  : > "$OUT/$name.out"
  local line
  while IFS= read -r -t 60 line <&"${HD[0]}"; do
    if [ "$line" = "END" ]; then return 0; fi
    printf '%s\n' "$line" >> "$OUT/$name.out"
  done
  die "the harness did not answer: $name"
}
hd harness_join join "$INVITE_H" "$M_PASS" desktop
test "$(cat "$OUT/harness_join.out")" = "ok" || die "the desktop join answered: $(cat "$OUT/harness_join.out")"
hd harness_sync1 sync
hd harness_copy copy "desktop copy $M_TEXT"
hd harness_sync2 sync
hd harness_image copy-image
hd harness_sync3 sync
# search, clipboard and list print content to the user on purpose
hd harness_list search "$M_TEXT"
grep -q "$M_TEXT" "$OUT/harness_list.out" || die "the desktop search should find the planted text"
hd harness_clip clipboard
hd harness_status status
printf 'quit\n' >&"${HD[1]}"
wait "$HD_PID" 2> /dev/null || true
HD_PID=""
cli A sync_a3 sync
# A computer used in local-only mode: it records and searches, and has no device and no network use
coproc HL { "$HARNESS" "$ROOT/HL" 2> "$OUT/harnessl.err"; }
hl() {
  local name=$1
  shift
  printf '%s\n' "$*" >&"${HL[1]}"
  : > "$OUT/$name.out"
  local line
  while IFS= read -r -t 60 line <&"${HL[0]}"; do
    if [ "$line" = "END" ]; then return 0; fi
    printf '%s\n' "$line" >> "$OUT/$name.out"
  done
  die "the harness did not answer: $name"
}
hl harnessl_local local
hl harnessl_copy copy "local only copy $M_TEXT"
hl harnessl_search search "$M_TEXT"
grep -q "$M_TEXT" "$OUT/harnessl_search.out" || die "the local-only search should find the planted text"
hl harnessl_status status
printf 'quit\n' >&"${HL[1]}"
wait "$HL_PID" 2> /dev/null || true
HL_PID=""
test -f "$ROOT/H/history.db" || die "the desktop application did not build a history database"
ok "desktop application flow finished"

step "plain HTTP control request through the control recorder"
curl -s -o /dev/null -X POST --data "control $M_TEXT" "http://127.0.0.1:$T2/" || true

step "crash the client right after an operation is queued, with core dumps allowed"
(
  cd "$ROOT/cwd"
  ulimit -c unlimited
  printf '%s' "crash $M_TEXT" | ONEPASTE_CRASH_AT=after_enqueue ONEPASTE_HOME="$ROOT/A" "$CLI" add > "$OUT/crash.out" 2>&1 || true
)

step "stop everything"
kill "$SERVER_PID"
wait "$SERVER_PID" 2> /dev/null || true
if [ "$TCPDUMP_ON" = 1 ]; then
  sleep 1
  sudo -n pkill -INT -x tcpdump || true
  sleep 1
  TCPDUMP_ON=0
  sudo -n chmod a+r "$ROOT/cap.pcap"
fi
for p in "${PIDS[@]}"; do kill "$p" 2> /dev/null || true; done
PIDS=()
sleep 0.5

# ---------------------------------------------------------------------------------------------------------------------
# Secrets the server may hold but must never print: registration credentials and the stored ciphertext
EXTRA="$ROOT/extra.tsv"
{
  for c in "$CODE" "$INVITE"; do
    t=$(python3 -c 'import sys, base64, json; c = sys.argv[1].split(":", 1)[1]; print(json.loads(base64.urlsafe_b64decode(c + "=" * (-len(c) % 4)))["t"])' "$c")
    printf 'REGTOKEN\traw\t%s\n' "$t"
  done
  sqlite3 "$ROOT/srv/onepaste.db" "SELECT json_extract(envelope, '\$.sealed') FROM items WHERE envelope IS NOT NULL" | while read -r s; do
    printf 'SEALED\traw\t%s\n' "${s:0:48}"
  done
  sqlite3 "$ROOT/srv/onepaste.db" "SELECT sealed_name FROM devices" | while read -r s; do
    printf 'SEALEDNAME\traw\t%s\n' "${s:0:32}"
  done
} > "$EXTRA"
test "$(grep -c SEALED "$EXTRA")" -ge 3

step "1.2.1 server data directory"
scan_for_markers "server data directory" "$ROOT/srv"

step "1.2.2 server stdout and stderr at the most detailed level"
{ grep -v '^onepaste1:' "$OUT/server.out"; cat "$OUT/server.err"; } > "$OUT/server.logs"
LOG_LINES=$(wc -l < "$OUT/server.logs")
test "$LOG_LINES" -ge 20 || die "the server log has only $LOG_LINES lines; the log level switch had no effect"
grep -q '"level":"TRACE"' "$OUT/server.logs" || die "no TRACE line in the server log; RUST_LOG=trace had no effect"
echo "server log: $LOG_LINES lines"
scan_for_markers "server logs" "$OUT/server.logs"
scan_for_markers "server logs, server-held secrets" -p "$EXTRA" "$OUT/server.logs"

step "1.2.3 client data directories, including the history databases"
scan_for_markers "client A data directory" "$ROOT/A"
scan_for_markers "client B data directory" "$ROOT/B"
scan_for_markers "desktop application data directory" "$ROOT/H"
scan_for_markers "desktop application data directory (vault creation)" "$ROOT/H0"
scan_for_markers "desktop application data directory (local-only)" "$ROOT/HL"
test "$(sqlite3 "$ROOT/HL/onepaste.db" "SELECT count(*) FROM meta WHERE key IN ('device_id','u','keyparams')")" -eq 0 || die "the local-only database holds a device identity"
test -f "$ROOT/A/history.db" || die "client A has no history database to scan"

step "1.2.4 everything the client commands printed"
CLIENT_OUT="$ROOT/client_out"
mkdir -p "$CLIENT_OUT"
for f in join_a invite join_b add set sync_b sync_a add2 sync_b2 rm sync_a2 status revoke sync_b3 invite_h sync_a3 harness0_probe harness0_confirm harness_join harness_sync1 harness_sync2 harness_sync3 harness_status harnessl_status; do cp "$OUT/$f.out" "$CLIENT_OUT/"; done
test "$(find "$CLIENT_OUT" -type f -size +0 | wc -l)" -ge 10 || die "the client printed almost nothing"
scan_for_markers "client output" "$CLIENT_OUT"
scan_for_markers "client output, server-held secrets" -p "$EXTRA" "$CLIENT_OUT"
# devices prints the decrypted device names on purpose, so only the device name is exempt from this scan
scan_for_markers "client output of devices" -x DEV "$OUT/devices.out"
grep -q "$M_DEV" "$OUT/devices.out" || die "devices should show the device name that was planted"

step "1.2.5 network capture"
scan_for_markers "recorded client-server traffic" "$TAPDIR/server"
if [ -f "$ROOT/cap.pcap" ]; then
  tcpdump -nr "$ROOT/cap.pcap" -w "$ROOT/flow.pcap" "port $S_PORT or port $T1" > /dev/null 2>&1
  tcpdump -nr "$ROOT/cap.pcap" -w "$ROOT/control.pcap" "port $H_PORT or port $T2" > /dev/null 2>&1
  scan_for_markers "tcpdump capture of the flow" "$ROOT/flow.pcap"
else
  echo "NOTE: the tcpdump capture was not run on this machine"
fi

step "1.2.6 crash scene"
cores=$(find "$ROOT/cores" "$ROOT/cwd" -maxdepth 1 -name 'core*' -type f | wc -l)
echo "core files found: $cores"
if [ -n "$OLD_CORE_PATTERN" ]; then
  test "$cores" -eq 0 || die "a crashed process left a core dump, which holds the passphrase and the plaintext"
fi
printf 'placeholder\n' > "$ROOT/cwd/.scanned"
scan_for_markers "crash scene" "$ROOT/A" "$ROOT/cwd" "$ROOT/cores" "$ROOT/tmp" "$OUT/crash.out"

step "1.2.7 TMPDIR"
printf 'placeholder\n' > "$ROOT/tmp/.scanned"
scan_for_markers "TMPDIR" "$ROOT/tmp"

step "1.2.8 what the server can see"
EXPECTED_SCHEMA='devices|id
devices|token_hash
devices|sealed_name
devices|created_at
devices|last_seen_at
devices|revoked
idempotency|scope
idempotency|key
idempotency|req_hash
idempotency|status
idempotency|body
idempotency|created_at
items|id
items|seq
items|deleted
items|envelope
items|deleted_at
kv|key
kv|value
pull_state|device_id
pull_state|next_after
pull_state|generation
reg_tokens|hash
reg_tokens|expires_at
reg_tokens|used
vault|id
vault|seq
vault|cursor_floor
vault|keyparams
vault|keyparams_version
vault|proof_hash'
ACTUAL_SCHEMA=$(sqlite3 "$ROOT/srv/onepaste.db" "SELECT m.name || '|' || p.name FROM sqlite_master m, pragma_table_info(m.name) p WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' ORDER BY m.name, p.cid")
if [ "$ACTUAL_SCHEMA" != "$EXPECTED_SCHEMA" ]; then
  echo "the server tables and columns differ from the allowed list (update the list only together with the interface spec):" >&3
  diff <(echo "$EXPECTED_SCHEMA") <(echo "$ACTUAL_SCHEMA") >&3 || true
  die "unexpected server schema"
fi
ENVELOPE=$(sqlite3 "$ROOT/srv/onepaste.db" "SELECT envelope FROM items WHERE envelope IS NOT NULL LIMIT 1")
test -n "$ENVELOPE"
case "$ENVELOPE" in *LEAK-*) die "the stored envelope contains a planted string" ;; esac
printf '%s' "$ENVELOPE" | ONEPASTE_HOME="$ROOT/A" "$CLI" probe-envelope | grep -q 'decrypt failed with a wrong key'
ok "server schema matches the allowed list; the stored content is ciphertext that a wrong key cannot open"

step "1.2.9 permissions"
mode() { stat -c %a "$1"; }
check_mode() { [ "$(mode "$1")" = "$2" ] || die "$1 has mode $(mode "$1"), expected $2"; }
check_mode "$ROOT/A" 700
check_mode "$ROOT/B" 700
check_mode "$ROOT/srv" 700
check_mode "$ROOT/H" 700
check_mode "$ROOT/H0" 700
check_mode "$ROOT/HL" 700
check_mode "$ROOT/srv/tls/key.pem" 600
for f in "$ROOT"/A/onepaste.db* "$ROOT"/B/onepaste.db* "$ROOT"/srv/onepaste.db* "$ROOT"/A/history.db* "$ROOT"/H/client.db* "$ROOT"/H/onepaste.db* "$ROOT"/H/history.db* "$ROOT"/H0/onepaste.db* "$ROOT"/HL/onepaste.db* "$ROOT"/HL/history.db* "$ROOT"/H0/history.db*; do
  [ -e "$f" ] && check_mode "$f" 600
done
ok "directories 0700, database files and the private key 0600"

# ---------------------------------------------------------------------------------------------------------------------
step "1.3.1 control: the scanner finds every form of a planted string"
CTRL="$ROOT/ctrl"
mkdir -p "$CTRL"
forms_found=0
while IFS=$'\t' read -r name form pat; do
  [ "$name" = TEXT ] || [ "$name" = KIT ] || continue
  f="$CTRL/form-$form"
  printf 'noise %s noise' "$pat" > "$f"
  report=$(scan_for_markers "control $form" "$f" 2>&1 || true)
  case "$report" in *"$name found as $form in"*) forms_found=$((forms_found + 1)) ;; *) die "the scanner missed the form $form" ;; esac
done < "$PATTERNS"
test "$forms_found" -ge 10
ok "the scanner reported all $forms_found forms of the planted text"

step "1.3.2 control: a plaintext HTTP request is visible in the recorded traffic"
report=$(scan_for_markers "control traffic" "$TAPDIR/control" 2>&1 || true)
case "$report" in *"TEXT found as raw in"*) ;; *) die "the recorder did not show the planted text sent over plain HTTP" ;; esac
if [ -f "$ROOT/control.pcap" ]; then
  report=$(scan_for_markers "control tcpdump" "$ROOT/control.pcap" 2>&1 || true)
  case "$report" in *"TEXT found as raw in"*) ;; *) die "the tcpdump capture did not show the planted text sent over plain HTTP" ;; esac
fi
ok "plain HTTP is caught, so the clean scans of the encrypted traffic mean something"

step "1.3.3 control: the logs and the outputs are not empty"
test "$LOG_LINES" -ge 20
test "$(find "$CLIENT_OUT" -type f -size +0 | wc -l)" -ge 10
ok "server log $LOG_LINES lines, client outputs present"

echo "leak probe passed"
