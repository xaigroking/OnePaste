"""Checks that a Windows executable embeds a manifest that never asks for administrator rights.

Usage: python tools/check_exe_manifest.py <exe>

The manifest must contain requestedExecutionLevel level="asInvoker" and must contain neither
requireAdministrator nor highestAvailable. Only the manifest XML is inspected, not the whole file.
"""
import re
import sys


def manifests(data: bytes):
    """Yields every <assembly>...</assembly> XML fragment embedded in the file."""
    for m in re.finditer(rb"<assembly\b.*?</assembly>", data, re.S):
        yield m.group(0)


def main(path: str) -> int:
    with open(path, "rb") as f:
        data = f.read()
    found = [m for m in manifests(data) if b"requestedExecutionLevel" in m]
    if not found:
        print("FAIL: no manifest with a requestedExecutionLevel is embedded in " + path)
        return 1
    ok = True
    for m in found:
        if b'level="asInvoker"' not in m:
            print('FAIL: the manifest does not contain level="asInvoker"')
            ok = False
        for forbidden in (b"requireAdministrator", b"highestAvailable"):
            if forbidden in m:
                print("FAIL: the manifest contains " + forbidden.decode())
                ok = False
    if ok:
        print('OK: the embedded manifest requests level="asInvoker" only')
    return 0 if ok else 1


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: python tools/check_exe_manifest.py <exe>")
        sys.exit(2)
    sys.exit(main(sys.argv[1]))
