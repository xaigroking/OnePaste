#!/usr/bin/env bash
# Real Compose deployment, non-default published port, registration and image replacement with the same volume.
set -Eeuo pipefail
BIN_DIR=${1:-target/debug}
CLI=$(cd "$BIN_DIR" && pwd)/onepaste
test -x "$CLI"
docker compose version > /dev/null
ROOT=$(mktemp -d)
PROJECT="onepaste-compose-$$"
IMAGE=${IMAGE:-onepaste-server:e2e}
UPGRADE="onepaste-server:compose-upgrade-$$"
COMPOSE=$(cd "$(dirname "$0")/../../docker" && pwd)/compose.yaml
STEP=setup
cleanup() { docker compose -p "$PROJECT" --env-file "$ROOT/server.env" -f "$COMPOSE" down -v > /dev/null 2>&1 || true; docker image rm "$UPGRADE" > /dev/null 2>&1 || true; rm -rf "$ROOT"; }
trap cleanup EXIT
trap 'echo "FAIL Compose step: $STEP (line $LINENO)" >&2' ERR
dc() { docker compose -p "$PROJECT" --env-file "$ROOT/server.env" -f "$COMPOSE" -f "$ROOT/cadence.yaml" "$@"; }
a() { ONEPASTE_HOME="$ROOT/a" "$CLI" "$@"; }
b() { ONEPASTE_HOME="$ROOT/b" "$CLI" "$@"; }
c() { ONEPASTE_HOME="$ROOT/c" "$CLI" "$@"; }
export ONEPASTE_PASSPHRASE=compose-regression-passphrase
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')
test "$PORT" != 8443
cat > "$ROOT/server.env" <<ENV
ONEPASTE_PUBLIC_URL=https://127.0.0.1:$PORT
HOST_PORT=$PORT
IMAGE=$IMAGE
RUST_LOG=info
ENV
cat > "$ROOT/cadence.yaml" <<'YAML'
services:
  server:
    healthcheck:
      interval: 2s
      start_period: 1s
YAML
STEP=config
: > "$ROOT/empty.env"
if env -u ONEPASTE_PUBLIC_URL docker compose --env-file "$ROOT/empty.env" -f "$COMPOSE" config > /dev/null 2>&1; then echo 'FAIL public URL must be explicit' >&2; exit 1; fi
dc config --format json > "$ROOT/config.json"
python3 - "$ROOT/config.json" "$PORT" <<'PY'
import json,sys
service=json.load(open(sys.argv[1]))['services']['server']
assert service['user']=='65532:65532'
assert service['healthcheck']['test']==['CMD','/onepaste-server','healthcheck']
assert service['ports'][0]['target']==8443 and str(service['ports'][0]['published'])==sys.argv[2]
assert service['environment']=={'ONEPASTE_DATA_DIR':'/data','ONEPASTE_LISTEN':'0.0.0.0:8443','ONEPASTE_PUBLIC_URL':'https://127.0.0.1:'+sys.argv[2],'RUST_LOG':'info'}
assert service['volumes'][0]['type']=='volume' and service['volumes'][0]['target']=='/data'
PY
if [ "${SKIP_BUILD:-}" != 1 ]; then docker build --build-arg BUILD_TAG=compose-original -t "$IMAGE" .; fi
STEP=start
dc up -d --wait --wait-timeout 90 > /dev/null
CID=$(dc ps -q server)
BEFORE=$(dc exec -T server /onepaste-server --version)
docker cp "$CID:/data/tls/cert.pem" "$ROOT/before.pem" > /dev/null
CODE=$(dc exec -T server /onepaste-server connect-code | grep -m1 '^onepaste1:')
# The opaque code must advertise the mapped host port, not the container's internal one.
printf '%s' "$CODE" | python3 -c 'import sys,base64,json; raw=sys.stdin.read().split(":",1)[1]; d=json.loads(base64.urlsafe_b64decode(raw+"="*(-len(raw)%4))); assert d["u"]=="https://127.0.0.1:"+sys.argv[1]' "$PORT"
a join "$CODE" --name compose-a > /dev/null 2>&1
b join "$(a invite)" --name compose-b > /dev/null 2>&1
printf 'preserved Compose history' | a add > /dev/null 2>&1
a sync > /dev/null 2>&1
b sync > /dev/null 2>&1
b list | grep -q 'preserved Compose history'
echo 'ok: resolved production settings, native health check, host port and two-device sync'
STEP=upgrade
# Build another real server version; replacing a container with the same image would not test the image update path.
docker build --build-arg BUILD_TAG=compose-upgrade -t "$UPGRADE" . > "$ROOT/upgrade-build.log" 2>&1 || { tail -n 35 "$ROOT/upgrade-build.log" >&2; exit 1; }
python3 - "$ROOT/server.env" "$UPGRADE" <<'PY'
from pathlib import Path
import sys
p=Path(sys.argv[1]);p.write_text('\n'.join('IMAGE='+sys.argv[2] if s.startswith('IMAGE=') else s for s in p.read_text().splitlines())+'\n')
PY
dc up -d --force-recreate --wait --wait-timeout 90 > /dev/null
AFTER=$(dc exec -T server /onepaste-server --version)
test "$BEFORE" != "$AFTER"
CID=$(dc ps -q server)
docker cp "$CID:/data/tls/cert.pem" "$ROOT/after.pem" > /dev/null
cmp "$ROOT/before.pem" "$ROOT/after.pem"
a sync > /dev/null 2>&1
b sync > /dev/null 2>&1
b list | grep -q 'preserved Compose history'
printf 'after Compose image upgrade' | b add > /dev/null 2>&1
b sync > /dev/null 2>&1
a sync > /dev/null 2>&1
a list | grep -q 'after Compose image upgrade'
CODE=$(dc exec -T server /onepaste-server connect-code | grep -m1 '^onepaste1:')
c join "$CODE" --name compose-c > /dev/null 2>&1
c sync > /dev/null 2>&1
c list | grep -q 'preserved Compose history'
c list | grep -q 'after Compose image upgrade'
echo 'ok: replacement server version preserved certificate, vault, old device credentials and history'
echo 'Compose regression passed'
