#!/usr/bin/env python3
"""Lists the contents of a portable zip and fails if a required entry is missing.

Usage: python tools/check_portable_zip.py <zip> <required>...

A required entry is either a file name (matched against the last part of every path in the zip) or `prefix:<text>`
(matched against the start of the path).
"""
import os
import sys
import zipfile


def main(path, required):
    names = zipfile.ZipFile(path).namelist()
    print("contents of %s:" % os.path.basename(path))
    for n in names:
        print("  " + n)
    missing = []
    for req in required:
        if req.startswith("prefix:"):
            found = any(n.startswith(req[len("prefix:"):]) for n in names)
        else:
            found = any(os.path.basename(n.rstrip("/")) == req for n in names)
        if not found:
            missing.append(req)
    if missing:
        print("FAIL: %s is missing: %s" % (os.path.basename(path), ", ".join(missing)), file=sys.stderr)
        return 1
    print("ok: all required entries are present")
    return 0


if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1], sys.argv[2:]))
