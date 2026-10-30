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
import ctypes
import glob
import plistlib
import re
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



def visible_windows(pid):
    """Count on-screen application windows without requesting accessibility permission."""
    if sys.platform == "win32":
        from ctypes import wintypes
        found = []
        user32 = ctypes.WinDLL("user32", use_last_error=True)
        callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        user32.IsWindowVisible.argtypes = [wintypes.HWND]
        user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
        def visit(hwnd, _):
            owner = wintypes.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value == pid and user32.IsWindowVisible(hwnd):
                found.append(hwnd)
            return True
        callback = callback_type(visit)
        user32.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
        user32.EnumWindows(callback, 0)
        return len(found)
    cg = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
    cf = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
    cg.CGWindowListCopyWindowInfo.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
    cg.CGWindowListCopyWindowInfo.restype = ctypes.c_void_p
    cf.CFPropertyListCreateData.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_long, ctypes.c_ulong, ctypes.c_void_p]
    cf.CFPropertyListCreateData.restype = ctypes.c_void_p
    cf.CFDataGetLength.argtypes = [ctypes.c_void_p]
    cf.CFDataGetLength.restype = ctypes.c_long
    cf.CFDataGetBytePtr.argtypes = [ctypes.c_void_p]
    cf.CFDataGetBytePtr.restype = ctypes.c_void_p
    cf.CFRelease.argtypes = [ctypes.c_void_p]
    windows = cg.CGWindowListCopyWindowInfo(1 | 16, 0)
    if not windows:
        raise SystemExit("FAIL: could not read visible windows")
    data = cf.CFPropertyListCreateData(None, windows, 100, 0, None)
    try:
        if not data:
            raise SystemExit("FAIL: could not inspect visible windows")
        entries = plistlib.loads(ctypes.string_at(cf.CFDataGetBytePtr(data), cf.CFDataGetLength(data)))
        return sum(w.get("kCGWindowOwnerPID") == pid and w.get("kCGWindowLayer") == 0 for w in entries)
    finally:
        if data:
            cf.CFRelease(data)
        cf.CFRelease(windows)


def assert_hidden(pid, seconds=RUN_SECONDS, process=None):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        if process is not None:
            if process.poll() is not None:
                raise SystemExit("FAIL: the hidden program exited early")
        else:
            os.kill(pid, 0)
        if visible_windows(pid):
            raise SystemExit("FAIL: a hidden start showed a window")
        time.sleep(0.25)


def hidden_round_trip(exe):
    print("== hidden start and hidden duplicate", flush=True)
    first = subprocess.Popen([exe, "--hidden"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        assert_hidden(first.pid, process=first)
        second = subprocess.Popen([exe, "--hidden"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_gone(second, "the second hidden start")
        if second.returncode != 0:
            raise SystemExit("FAIL: the second hidden start did not exit cleanly")
        assert_hidden(first.pid, 2, process=first)
        # The same invisible window must remain usable from a normal second launch.
        normal = subprocess.Popen([exe], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_gone(normal, "the normal start after a hidden start")
        until = time.monotonic() + END_SECONDS
        while not visible_windows(first.pid):
            if time.monotonic() >= until:
                raise SystemExit("FAIL: a normal second start did not show the hidden window")
            time.sleep(0.25)
    finally:
        quit_proc = subprocess.Popen([exe, "--quit"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_gone(quit_proc, "the hidden quit request")
        wait_gone(first, "the hidden program after quit")
    print("hidden lifecycle passed", flush=True)


def mac_launch_agent(exe):
    """Load the ad-hoc signed package through launchd, without a login or global registration."""
    app = exe.split(".app/")[0] + ".app"
    signature = subprocess.run(["codesign", "-dv", "--verbose=4", app], capture_output=True, text=True, check=True)
    if "Signature=adhoc" not in signature.stderr:
        raise SystemExit("FAIL: this launch-agent trial requires the ad-hoc signed CI package")
    label = "dev.onepaste.spike.smoke.%d" % os.getpid()
    domain = "gui/%d" % os.getuid()
    with tempfile.TemporaryDirectory(prefix="onepaste-launch-agent-") as directory:
        path = os.path.join(directory, label + ".plist")
        with open(path, "wb") as output:
            plistlib.dump({"Label": label, "ProgramArguments": [exe, "--hidden"], "RunAtLoad": True,
                          "LimitLoadToSessionType": "Aqua", "AssociatedBundleIdentifiers": ["dev.onepaste.spike"]}, output)
        subprocess.run(["launchctl", "bootstrap", domain, path], check=True)
        try:
            until = time.monotonic() + END_SECONDS
            pid = None
            while not pid:
                result = subprocess.run(["launchctl", "print", domain + "/" + label], capture_output=True, text=True, check=True)
                match = re.search(r"\bpid = (\d+)", result.stdout)
                if match:
                    pid = int(match.group(1))
                elif time.monotonic() >= until:
                    raise SystemExit("FAIL: launchd did not start the ad-hoc package")
                else:
                    time.sleep(0.25)
            assert_hidden(pid)
            quit_proc = subprocess.Popen([exe, "--quit"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            wait_gone(quit_proc, "the launch-agent quit request")
            until = time.monotonic() + END_SECONDS
            while running("onepaste-desktop"):
                if time.monotonic() >= until:
                    raise SystemExit("FAIL: the launch-agent program did not quit")
                time.sleep(0.25)
        finally:
            subprocess.run(["launchctl", "bootout", domain + "/" + label], check=True)
    print("ad-hoc launch-agent hidden startup passed; actual login remains a manual trial", flush=True)


def main(argv):
    if not argv:
        raise SystemExit(__doc__)
    root, mac_open = argv[0], "--mac-open" in argv
    exe = find_exe(root)
    hidden_round_trip(exe)
    if mac_open and sys.platform == "darwin":
        mac_launch_agent(exe)
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
