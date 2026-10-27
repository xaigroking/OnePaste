#!/usr/bin/env python3
"""Start, start again, quit: the life cycle of the packaged desktop program, three rounds.

Usage: smoke_lifecycle.py <directory holding the unpacked package> [--mac-open]

The program is found inside the directory (OnePaste.exe on Windows, the executable of OnePaste.app on macOS). Each round:
  1. start it and wait until it has been running for a while;
  2. start it a second time (round 3 uses a copy of the package in another place, which is what a downloaded app on macOS
     looks like): the second start must end by itself, quickly and cleanly, and the first must still be running;
  3. ask the running program to quit (`--quit`): it must end, and nothing may be left running.
Three rounds from the same package and then from a copy show that the data directory is free again after a quit and that no
second copy ever stays alive. With --mac-open the first start of the last round is `open -n` on the .app, as a user does.
"""
import glob
import os
import shutil
import subprocess
import sys
import tempfile
import time

RUN_SECONDS = 8
END_SECONDS = 20


def find_exe(root):
    if sys.platform == "win32":
        hits = glob.glob(os.path.join(root, "**", "OnePaste.exe"), recursive=True)
    else:
        hits = [h for h in glob.glob(os.path.join(root, "*.app", "Contents", "MacOS", "*")) if os.access(h, os.X_OK)]
    if not hits:
        raise SystemExit("FAIL: no program found in %s" % root)
    return hits[0]


def running(name):
    """The number of running processes whose command line holds the name (macOS and Linux)."""
    if sys.platform == "win32":
        out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq OnePaste.exe", "/NH"], capture_output=True, text=True).stdout
        return out.count("OnePaste.exe")
    out = subprocess.run(["pgrep", "-f", name], capture_output=True, text=True).stdout
    return len([l for l in out.splitlines() if l.strip()])


def wait_gone(proc, what):
    try:
        proc.wait(timeout=END_SECONDS)
    except subprocess.TimeoutExpired:
        proc.kill()
        raise SystemExit("FAIL: %s did not end within %d seconds" % (what, END_SECONDS))


def round_trip(exe, label, use_open=None):
    print("== round %s: %s" % (label, exe), flush=True)
    if use_open:
        first = subprocess.Popen(["open", "-n", "-W", use_open])
    else:
        first = subprocess.Popen([exe], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(RUN_SECONDS)
    if first.poll() is not None:
        raise SystemExit("FAIL: the program ended by itself (exit code %s)" % first.returncode)
    started = time.time()
    second = subprocess.Popen([exe], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    wait_gone(second, "the second start")
    print("the second start ended after %.1f seconds with code %s" % (time.time() - started, second.returncode), flush=True)
    if second.returncode != 0:
        raise SystemExit("FAIL: the second start ended with code %s" % second.returncode)
    if first.poll() is not None:
        raise SystemExit("FAIL: the running program ended when it was started a second time")
    if use_open:
        # `open -n` asks for a new copy; there must still be exactly one program
        subprocess.Popen(["open", "-n", use_open])
        time.sleep(4)
        count = running("onepaste-desktop")
        print("programs running after open -n twice: %d" % count, flush=True)
        if count != 1:
            raise SystemExit("FAIL: %d programs are running, expected 1" % count)
    quit_proc = subprocess.Popen([exe, "--quit"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    wait_gone(quit_proc, "the quit request")
    wait_gone(first, "the running program after the quit request")
    time.sleep(1)
    left = running("onepaste-desktop") if sys.platform != "win32" else running("OnePaste")
    print("programs left after quit: %d" % left, flush=True)
    if left != 0:
        raise SystemExit("FAIL: %d programs are still running after quit" % left)


def main(argv):
    if not argv:
        raise SystemExit(__doc__)
    root, mac_open = argv[0], "--mac-open" in argv
    exe = find_exe(root)
    round_trip(exe, "1 (the package)")
    round_trip(exe, "2 (the same package again)")
    # Round 3: a copy in another place, as a second download or the translocated copy of macOS
    copy_root = tempfile.mkdtemp(prefix="onepaste-copy-")
    copy = os.path.join(copy_root, "package")
    shutil.copytree(root, copy, symlinks=True)
    copy_exe = find_exe(copy)
    app = None
    if mac_open and sys.platform == "darwin":
        app = glob.glob(os.path.join(copy, "*.app"))[0]
    round_trip(copy_exe, "3 (a copy in another place)", use_open=app)
    shutil.rmtree(copy_root, ignore_errors=True)
    print("lifecycle smoke test passed")


if __name__ == "__main__":
    main(sys.argv[1:])
