#!/usr/bin/env bash
# Real CLI soak traffic and verification. Python stays inline to keep the repository's language boundaries.
set -euo pipefail
SOAK_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
exec python3 - "$SOAK_ROOT" "$@" <<'PY'
import argparse
import collections
import concurrent.futures
import datetime
import json
import math
import os
from pathlib import Path
import queue
import re
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(sys.argv.pop(1))
# The CLI exposes a 60-character first_line. Keep generated single-line markers shorter than that.
LABEL = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,23}\Z")
ITEM_ID = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\Z")
LOG_LOCK = threading.Lock()


class Failure(Exception):
    """Only fixed, non-sensitive diagnostic messages may leave the script."""


def log(message):
    with LOG_LOCK:
        stamp = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
        print(stamp.replace("+00:00", "Z") + " " + message, flush=True)


def label(value):
    if not LABEL.fullmatch(value):
        raise argparse.ArgumentTypeError("use 1-24 ASCII letters, digits, underscores or hyphens")
    return value


def positive(value):
    try:
        number = float(value)
    except ValueError:
        raise argparse.ArgumentTypeError("a positive finite number is required") from None
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("a positive finite number is required")
    return number


class Parser(argparse.ArgumentParser):
    def error(self, message):
        # argparse normally echoes rejected arguments, which could contain a connection code.
        raise Failure("invalid arguments; use --help")


def arguments():
    parser = Parser(prog="soak.sh", description="Write numbered traffic and verify both joined CLI profiles.",
        epilog="Set ONEPASTE_HOME to a dedicated profile and ONEPASTE_PASSPHRASE to the vault passphrase. "
        "ONEPASTE_BIN selects the built client (otherwise PATH, then target/debug/onepaste). "
        "Use a new, distinct device label on each machine for every run. Stop both writers before verifying "
        "--expect labelA,labelB on EACH machine. Sleep/wake and network changes are manual; this script "
        "does not establish that the server is public or the machines use different networks. "
        "Do not enable shell tracing around credentials.")
    parser.add_argument("--self-test", action="store_true", help="real local server and two clients, about 30 seconds of writes")
    parser.add_argument("--command-timeout", type=positive, default=30, help="maximum seconds per CLI command (default: 30)")
    commands = parser.add_subparsers(dest="command")
    join = commands.add_parser("join", help="join with a caller-provided connection code; suppress credential output")
    join.add_argument("code")
    join.add_argument("--name", default="soak-client")
    join.add_argument("--recovery-kit-out", help="optional private output file if this creates a new vault")
    write = commands.add_parser("write", help="add each number exactly once; failures are never retried")
    write.add_argument("--device", type=label, required=True, help="new, unique 1-24 character ASCII label: letters, digits, underscore or hyphen")
    write.add_argument("--duration", type=positive, required=True, help="wall-clock seconds, including time asleep")
    write.add_argument("--interval", type=positive, required=True, help="seconds to wait after each attempted add")
    verify = commands.add_parser("verify", help="sync to an empty queue, then check numbered records and END markers")
    verify.add_argument("--expect", required=True, help="comma-separated, distinct device labels")
    verify.add_argument("--sync-timeout", type=positive, default=180, help="maximum seconds to wait for successful sync (default: 180)")
    args = parser.parse_args()
    if args.self_test == bool(args.command):
        raise Failure("choose exactly one command or --self-test")
    if args.command == "verify":
        args.expect = args.expect.split(",")
        try:
            args.expect = [label(value) for value in args.expect]
        except argparse.ArgumentTypeError:
            raise Failure("invalid expected device labels") from None
        if len(set(args.expect)) != len(args.expect):
            raise Failure("expected device labels must be distinct")
    return args


class Client:
    def __init__(self, binary, env, timeout=30):
        self.binary, self.env, self.timeout = binary, env.copy(), timeout

    def run(self, *args, text=None, timeout=None):
        try:
            result = subprocess.run([self.binary, *args], input=text or "", text=True,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.env,
                timeout=self.timeout if timeout is None else min(self.timeout, timeout))
            return result.returncode, result.stdout, "ok" if result.returncode == 0 else "exit"
        except subprocess.TimeoutExpired as error:
            output = error.stdout or b""
            if isinstance(output, bytes):
                output = output.decode("utf-8", errors="replace")
            return 124, output, "timeout"
        except OSError:
            return 126, "", "unavailable"

    def data(self, *args, timeout=None):
        code, output, _ = self.run(*args, timeout=timeout)
        if code != 0:
            raise Failure("CLI data command failed (exit={})".format(code))
        try:
            return json.loads(output)
        except (ValueError, TypeError):
            raise Failure("CLI returned invalid JSON") from None

    def items(self):
        items = self.data("list", "--json")
        if not isinstance(items, list) or any(not isinstance(item, dict) for item in items):
            raise Failure("CLI returned an invalid item list")
        return items


def add(client, text):
    code, output, outcome = client.run("add", text=text)
    # The CLI prints the item ID after durable enqueue, before attempting network sync.
    queued = bool(ITEM_ID.fullmatch(output.strip()))
    return code, outcome, queued


def write(client, device, duration, interval):
    prefix = "soak:" + device + ":"
    if any(item.get("first_line", "").startswith(prefix) for item in client.items()):
        raise Failure("device label already exists locally; use a new label for this run")
    deadline = time.time() + duration
    count = queued = failures = 0
    log("write start device={} duration={} interval={} failed_adds_are_not_retried".format(device, duration, interval))
    while count == 0 or time.time() < deadline:
        count += 1
        code, outcome, persisted = add(client, prefix + str(count))
        queued += int(persisted)
        failures += int(code != 0 or not persisted)
        log("add device={} seq={} exit={} outcome={} queued={}".format(device, count, code, outcome, int(persisted)))
        # Wall-clock duration includes suspension. Never catch up with a burst or reuse a number after waking.
        time.sleep(max(0, min(interval, deadline - time.time())))
    code, outcome, ended = add(client, prefix + "END:" + str(count))
    log("end device={} last_seq={} exit={} outcome={} queued={}".format(device, count, code, outcome, int(ended)))
    log("write total device={} attempted={} queued={} failures={} unconfirmed={}".format(device, count, queued, failures, count - queued))
    return {"attempted": count, "queued": queued, "failures": failures, "complete": queued == count and ended}


def drain(client, timeout):
    deadline = time.monotonic() + timeout
    attempts = 0
    while time.monotonic() < deadline:
        attempts += 1
        code, _, outcome = client.run("sync", timeout=max(0.001, deadline - time.monotonic()))
        if code == 0 and time.monotonic() < deadline:
            state = client.data("status", "--json", timeout=max(0.001, deadline - time.monotonic()))
            box = state.get("outbox", {}) if isinstance(state, dict) else {}
            counters = [box.get(key) for key in ("pending", "conflict", "failed")]
            if (all(type(n) is int and n == 0 for n in counters) and state.get("paused") == ""
                    and state.get("refetching") is False):
                log("sync ready attempts={} pending=0 conflict=0 failed=0".format(attempts))
                return
        log("sync wait attempt={} exit={} outcome={}".format(attempts, code, outcome))
        time.sleep(max(0, min(1, deadline - time.monotonic())))
    raise Failure("sync did not finish with an empty queue before the timeout")


def analyze(items, expected):
    records = {name: {"counts": collections.Counter(), "ends": [], "malformed": 0} for name in expected}
    unreadable = unsettled = 0
    for item in items:
        # Catch local changes appearing between the status read and the item snapshot, including queued deletions.
        if item.get("queued") is not False:
            unsettled += 1
        if item.get("deleted") is True:
            continue
        if item.get("decrypt_failed") is not False:
            unreadable += 1
            continue
        text = item.get("first_line")
        if not isinstance(text, str):
            unreadable += 1
            continue
        parts = text.split(":")
        if len(parts) < 2 or parts[0] != "soak" or parts[1] not in records:
            continue
        row = records[parts[1]]
        if len(parts) == 3 and re.fullmatch(r"[1-9][0-9]*", parts[2]):
            row["counts"][int(parts[2])] += 1
        elif len(parts) == 4 and parts[2] == "END" and re.fullmatch(r"[1-9][0-9]*", parts[3]):
            row["ends"].append(int(parts[3]))
        else:
            row["malformed"] += 1
    for row in records.values():
        ends, counts = row["ends"], row["counts"]
        last = ends[0] if len(ends) == 1 else None
        row["last"] = last
        row["duplicates"] = [(number, count) for number, count in sorted(counts.items()) if count > 1]
        row["unexpected"] = sorted(number for number in counts if last is not None and number > last)
        # Represent large gaps as ranges without allocating 1..END from an untrusted record.
        missing, previous = [], 0
        if last is not None:
            for number in sorted(n for n in counts if n <= last):
                if number > previous + 1:
                    missing.append((previous + 1, number - 1))
                previous = number
            if previous < last:
                missing.append((previous + 1, last))
        row["missing"] = missing
        row["ok"] = last is not None and not (missing or row["duplicates"] or row["unexpected"] or row["malformed"] or unreadable or unsettled)
    return records, unreadable, unsettled


def numbers(values):
    return "[" + ",".join(str(value) for value in values) + "]"


def verify(client, expected, timeout):
    drain(client, timeout)
    records, unreadable, unsettled = analyze(client.items(), expected)
    for name, row in records.items():
        gaps = [str(a) if a == b else "{}-{}".format(a, b) for a, b in row["missing"]]
        duplicates = ["{}:{}".format(number, count) for number, count in row["duplicates"]]
        log("verify device={} expected={} observed={} missing={} duplicates={} unexpected={} end_count={} malformed={} unreadable={} unsettled={} result={}".format(
            name, row["last"] if row["last"] is not None else "unknown", sum(row["counts"].values()),
            numbers(gaps), numbers(duplicates), numbers(row["unexpected"]), len(row["ends"]), row["malformed"],
            unreadable, unsettled, "PASS" if row["ok"] else "FAIL"))
    return all(row["ok"] for row in records.values()), records


class LocalServer:
    def __init__(self, binary, directory):
        self.binary, self.directory, self.process, self.reader = binary, directory, None, None
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            self.port = sock.getsockname()[1]

    def start(self):
        ready, codes = threading.Event(), queue.Queue()
        env = os.environ.copy()
        env.pop("ONEPASTE_FAULT", None)
        self.process = subprocess.Popen([self.binary, "--data-dir", str(self.directory), "--listen",
            "127.0.0.1:{}".format(self.port), "--public-url", "https://127.0.0.1:{}".format(self.port)],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)

        def read():
            for line in self.process.stdout:
                if line.startswith("onepaste1:"):
                    codes.put(line.strip())
                elif '"message":"started"' in line:
                    ready.set()
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        if not ready.wait(10):
            raise Failure("self-test server did not become ready")
        return codes

    def stop(self):
        if self.process is not None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
            self.reader.join(timeout=2)
            self.process.stdout.close()
            self.process = None


def parser_checks():
    def item(text, deleted=False):
        return {"first_line": text, "deleted": deleted, "decrypt_failed": False, "queued": False}
    base = [item("soak:A:1"), item("soak:A:2"), item("soak:A:END:2")]
    assert analyze(base, ["A"])[0]["A"]["ok"]
    assert analyze(base + [item("soak:A:1")], ["A"])[0]["A"]["duplicates"] == [(1, 2)]
    assert analyze([base[0], item("soak:A:2", True), base[2]], ["A"])[0]["A"]["missing"] == [(2, 2)]
    for broken in [base[:-1], base + [base[-1]], base + [item("soak:A:01")], base + [item("soak:A:3")]]:
        assert not analyze(broken, ["A"])[0]["A"]["ok"]
    assert not analyze(base, ["A", "B"])[0]["B"]["ok"]
    huge = analyze([base[0], item("soak:A:END:999999999999999999")], ["A"])[0]["A"]
    assert huge["missing"] == [(2, 999999999999999999)]
    assert not analyze(base + [{"decrypt_failed": True}], ["A"])[0]["A"]["ok"]
    assert not analyze(base + [{"deleted": True, "queued": True}], ["A"])[0]["A"]["ok"]


def self_test(binary):
    started = time.monotonic()
    parser_checks()
    server_binary = os.environ.get("ONEPASTE_SERVER_BIN", str(Path(binary).with_name("onepaste-server")))
    with tempfile.TemporaryDirectory(prefix="onepaste-soak-") as temporary:
        root = Path(temporary)
        server = LocalServer(server_binary, root / "server")
        try:
            codes = server.start()
            env = os.environ.copy()
            env.pop("ONEPASTE_FAULT", None)
            env["ONEPASTE_PASSPHRASE"] = secrets.token_urlsafe(24)
            clients = []
            for device in ("A", "B"):
                env["ONEPASTE_HOME"] = str(root / device)
                clients.append(Client(binary, env, 10))
            a, b = clients
            code = codes.get(timeout=5)
            if a.run("join", code, "--name", "soak-A")[0] != 0:
                raise Failure("self-test first join failed")
            status, invitation, _ = a.run("invite")
            if status != 0 or b.run("join", invitation.strip(), "--name", "soak-B")[0] != 0:
                raise Failure("self-test second join failed")
            log("self-test: two real clients writing for 30 seconds")
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                writers = [pool.submit(write, client, device, 30, 1) for client, device in zip(clients, ("A", "B"))]
                time.sleep(8)
                server.stop()
                log("self-test: server offline; queued adds must not be retried")
                time.sleep(5)
                server.start()
                results = [future.result() for future in writers]
            if not all(result["complete"] and result["attempted"] >= 2 and result["failures"] > 0 for result in results):
                raise Failure("self-test did not exercise durable offline adds on both clients")
            for client in clients:
                if not verify(client, ["A", "B"], 15)[0]:
                    raise Failure("self-test positive verification failed")
            log("self-test: PASS both clients have every numbered item exactly once after offline queue recovery")
            status, duplicate_id, _ = b.run("add", text="soak:A:1")
            if status != 0 or not ITEM_ID.fullmatch(duplicate_id.strip()):
                raise Failure("self-test duplicate setup failed")
            good, report = verify(a, ["A", "B"], 15)
            if good or report["A"]["duplicates"] != [(1, 2)]:
                raise Failure("self-test duplicate was not identified")
            log("self-test: PASS intentional duplicate detected device=A seq=1")
            if b.run("rm", duplicate_id.strip())[0] != 0:
                raise Failure("self-test duplicate cleanup failed")
            original = next(item["id"] for item in a.items() if item.get("first_line") == "soak:A:2" and not item.get("deleted"))
            if a.run("rm", original)[0] != 0:
                raise Failure("self-test loss setup failed")
            good, report = verify(b, ["A", "B"], 15)
            if good or report["A"]["missing"] != [(2, 2)]:
                raise Failure("self-test loss was not identified")
            log("self-test: PASS intentional loss detected device=A seq=2")
        finally:
            server.stop()
    elapsed = time.monotonic() - started
    if elapsed >= 120:
        raise Failure("self-test exceeded the two-minute budget")
    log("self-test: PASS elapsed_seconds={:.2f}".format(elapsed))
    return 0


def main():
    args = arguments()
    binary = os.environ.get("ONEPASTE_BIN") or shutil.which("onepaste") or str(ROOT / "target" / "debug" / "onepaste")
    if args.self_test:
        return self_test(binary)
    if not os.environ.get("ONEPASTE_PASSPHRASE") or not os.environ.get("ONEPASTE_HOME"):
        raise Failure("set ONEPASTE_HOME and ONEPASTE_PASSPHRASE; credentials are never printed")
    client = Client(binary, os.environ, args.command_timeout)
    if args.command == "join":
        command = ["join", args.code, "--name", args.name]
        if args.recovery_kit_out:
            command += ["--recovery-kit-out", args.recovery_kit_out]
        code, _, outcome = client.run(*command)
        log("join exit={} outcome={}".format(code, outcome))
        return int(code != 0)
    if args.command == "write":
        return 0 if write(client, args.device, args.duration, args.interval)["complete"] else 1
    return 0 if verify(client, args.expect, args.sync_timeout)[0] else 1


try:
    sys.exit(main())
except Failure as error:
    log("ERROR " + str(error))
    sys.exit(1)
except KeyboardInterrupt:
    log("ERROR interrupted; a complete END marker is not guaranteed")
    sys.exit(130)
except Exception:
    # Never echo child output, connection codes, environment values or exception arguments.
    log("ERROR operation failed; no credential-bearing diagnostics were emitted")
    sys.exit(1)
PY
