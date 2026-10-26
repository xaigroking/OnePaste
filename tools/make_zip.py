"""Creates a zip archive of a directory and keeps the directory itself as the top-level entry.

Usage: python tools/make_zip.py <directory> <output.zip>
"""
import os
import sys
import zipfile


def main(src: str, out: str) -> int:
    src = os.path.abspath(src)
    base = os.path.dirname(src)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for root, _dirs, files in os.walk(src):
            for name in sorted(files):
                path = os.path.join(root, name)
                z.write(path, os.path.relpath(path, base))
    print("created " + out)
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("usage: python tools/make_zip.py <directory> <output.zip>")
        sys.exit(2)
    sys.exit(main(sys.argv[1], sys.argv[2]))
