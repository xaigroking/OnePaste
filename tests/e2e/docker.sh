#!/usr/bin/env bash
# The server container image, end to end: build it, start a container with a volume, read the connection code from the
# logs, join two command line devices, sync a text both ways, stop and start the container on the same volume, and check
# that the data is still there and that the code can be fetched again with docker exec. Also checks that the container
# does not run as root and that the image holds no test-only strings.
# Needs: bash, docker, python3 and the built command line client.
# Usage: tests/e2e/docker.sh [binary-directory]   (default: target/debug)
set -Eeuo pipefail

exec 3>&2
STEP="setup"
trap 'echo "FAILED at step: $STEP (line $LINENO)" >&3; docker logs "${NAME:-none}" >&3 2>&1 || true' ERR
step() { STEP="$1"; echo "== $1"; }
ok() { echo "ok: $1"; }
die() { echo "FAIL: $*" >&3; exit 1; }

for tool in docker python3; do
  command -v "$tool" > /dev/null || die "the required tool '$tool' is not installed"
done

BIN_DIR=${1:-${BIN_DIR:-target/debug}}
CLI=$(cd "$BIN_DIR" && pwd)/onepaste
test -x "$CLI" || die "the command line client is not built: $CLI"
ROOT=$(mktemp -d)
NAME="onepaste-e2e-$$"
VOLUME="onepaste-e2e-data-$$"
IMAGE=${IMAGE:-onepaste-server:e2e}
PASS='docker-e2e-passphrase'
export ONEPASTE_PASSPHRASE="$PASS"
cleanup() {
  docker rm -f "$NAME" > /dev/null 2>&1 || true
  docker volume rm "$VOLUME" > /dev/null 2>&1 || true
  rm -rf "$ROOT"
}
trap cleanup EXIT
a() { ONEPASTE_HOME="$ROOT/A" "$CLI" "$@"; }
b() { ONEPASTE_HOME="$ROOT/B" "$CLI" "$@"; }

step "build the image"
if [ "${SKIP_BUILD:-}" != 1 ]; then
  docker build --build-arg BUILD_TAG=e2e-test -t "$IMAGE" .
fi
echo "image size: $(docker image ls --format '{{.Repository}}:{{.Tag}} {{.Size}}' "$IMAGE")"
docker image inspect "$IMAGE" --format 'size in bytes: {{.Size}}'

step "the image runs as an unprivileged user and holds no test-only strings"
user=$(docker image inspect "$IMAGE" --format '{{.Config.User}}')
case "$user" in ""|0|0:*|root|root:*) die "the image runs as root (user is '$user')" ;; esac
echo "configured user: $user"
CID=$(docker create "$IMAGE")
docker cp "$CID:/onepaste-server" "$ROOT/onepaste-server" > /dev/null
docker rm "$CID" > /dev/null
for needle in ONEPASTE_CRASH_AT drop_response_once ONEPASTE_PAGE_SIZE ONEPASTE_TOMBSTONE_RETENTION_SECS; do
  if grep -aqF -- "$needle" "$ROOT/onepaste-server"; then die "the server in the image contains the test-only string $needle"; fi
done
ok "user $user, and no fault injection strings in the image"

step "start the container and read the connection code from its logs"
PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
docker run -d --name "$NAME" -p "127.0.0.1:$PORT:8443" -v "$VOLUME:/data" \
  -e "ONEPASTE_PUBLIC_URL=https://127.0.0.1:$PORT" --health-interval=2s --health-start-period=1s "$IMAGE" > /dev/null
CODE=""
for _ in $(seq 1 100); do
  CODE=$(docker logs "$NAME" 2>&1 | grep -m1 '^onepaste1:' || true)
  [ -n "$CODE" ] && break
  sleep 0.3
done
[ -n "$CODE" ] || die "no connection code in the container logs"
echo "connection code found in the logs"
for _ in $(seq 1 60); do
  health=$(docker inspect "$NAME" --format '{{.State.Health.Status}}')
  [ "$health" = healthy ] && break
  sleep 1
done
[ "$health" = healthy ] || die "the container did not become healthy (status: $health)"
uid=$(docker top "$NAME" -o uid | tail -n +2 | head -n1 | tr -d ' ')
[ -n "$uid" ] && [ "$uid" != 0 ] && [ "$uid" != root ] || die "the server process runs as '$uid'"
ok "the code is in the logs, the container is healthy and the server runs as uid $uid"

step "two devices join and sync a text both ways"
a join "$CODE" --name device-a > /dev/null 2>&1
b join "$(a invite)" --name device-b > /dev/null 2>&1
printf 'text from A before the restart' | a add > /dev/null 2>&1
a sync > /dev/null 2>&1
b sync > /dev/null 2>&1
b list | grep -q 'text from A before the restart' || die "device B did not get the text from device A"
printf 'text from B before the restart' | b add > /dev/null 2>&1
b sync > /dev/null 2>&1
a sync > /dev/null 2>&1
a list | grep -q 'text from B before the restart' || die "device A did not get the text from device B"
ok "texts reached the other device both ways"

step "stop and start the container on the same volume"
docker stop "$NAME" > /dev/null
exit_code=$(docker inspect "$NAME" --format '{{.State.ExitCode}}')
echo "exit code after docker stop: $exit_code"
[ "$exit_code" = 0 ] || die "docker stop did not end the server cleanly (exit code $exit_code)"
docker start "$NAME" > /dev/null
for _ in $(seq 1 60); do
  health=$(docker inspect "$NAME" --format '{{.State.Health.Status}}')
  [ "$health" = healthy ] && break
  sleep 1
done
[ "$health" = healthy ] || die "the container did not become healthy after the restart (status: $health)"
a sync > /dev/null 2>&1
b sync > /dev/null 2>&1
printf 'text from A after the restart' | a add > /dev/null 2>&1
a sync > /dev/null 2>&1
b sync > /dev/null 2>&1
b list | grep -q 'text from A after the restart' || die "device B did not get a text made after the restart"
b list | grep -q 'text from A before the restart' || die "the data from before the restart is gone"
ok "the data survived the restart and both devices keep syncing"

step "the connection code can be fetched again with docker exec"
NEW=$(docker exec "$NAME" /onepaste-server connect-code | grep -m1 '^onepaste1:')
[ -n "$NEW" ] || die "docker exec printed no connection code"
c() { ONEPASTE_HOME="$ROOT/C" "$CLI" "$@"; }
c join "$NEW" --name device-c > /dev/null 2>&1
c sync > /dev/null 2>&1
c list | grep -q 'text from A after the restart' || die "a device joined with the fetched code does not see the data"
ok "a new code from docker exec adds a third device that sees the data"

step "back up the volume, remove everything, restore into a new volume and start a new container (an upgrade works the same way)"
docker stop "$NAME" > /dev/null
docker run --rm -v "$VOLUME:/data:ro" -v "$ROOT:/backup" busybox tar czf /backup/onepaste-backup.tgz -C /data .
test -s "$ROOT/onepaste-backup.tgz" || die "the backup is empty"
docker rm "$NAME" > /dev/null
docker volume rm "$VOLUME" > /dev/null
docker run --rm -v "$VOLUME:/data" -v "$ROOT:/backup" busybox sh -c 'tar xzf /backup/onepaste-backup.tgz -C /data && chown -R 65532:65532 /data'
docker run -d --name "$NAME" -p "127.0.0.1:$PORT:8443" -v "$VOLUME:/data" \
  -e "ONEPASTE_PUBLIC_URL=https://127.0.0.1:$PORT" --health-interval=2s --health-start-period=1s "$IMAGE" > /dev/null
for _ in $(seq 1 60); do
  health=$(docker inspect "$NAME" --format '{{.State.Health.Status}}')
  [ "$health" = healthy ] && break
  sleep 1
done
[ "$health" = healthy ] || die "the container made from the restored volume did not become healthy (status: $health)"
printf 'text from B after the restore' | b add > /dev/null 2>&1
b sync > /dev/null 2>&1
a sync > /dev/null 2>&1
a list | grep -q 'text from B after the restore' || die "device A did not get a text made after the restore"
a list | grep -q 'text from A before the restart' || die "the data from before the backup is gone"
ok "the restored volume holds the data and both devices keep syncing"

echo "docker passed"
