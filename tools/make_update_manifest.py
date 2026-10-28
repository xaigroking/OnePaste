#!/usr/bin/env python3
"""Writes the update manifest (latest.json) of one release to standard output.

Usage: make-update-manifest.py --tag TAG --released-at SECONDS --kind main|beta|stable --notes-file FILE
                               --sums SHA256SUMS.txt --repo OWNER/NAME

The manifest names one download per platform (taken from the checksums file), the time of the release, a short plain
text taken from the release notes, and the release page. It is signed afterwards; the signature is what makes it
trustworthy, not its content.
"""
import argparse
import json
import re
import sys

PLATFORMS = {
    "windows-x64-portable.zip": "windows-x86_64",
    "macos-universal-portable.zip": "darwin-universal",
}
SECTIONS = ("What's changed", "What's new")
MAX_NOTES = 1000


def summary(notes_text):
    """The bullet lines of the changes section, as plain text, at most MAX_NOTES characters."""
    lines, inside = [], False
    for line in notes_text.splitlines():
        m = re.match(r"^#{1,6}\s+(.*)$", line)
        if m:
            inside = any(m.group(1).strip().startswith(s) for s in SECTIONS)
            continue
        if inside and line.startswith("- "):
            lines.append(line[2:].strip())
    text = "\n".join(lines)
    return text if len(text) <= MAX_NOTES else text[: MAX_NOTES - 1].rstrip() + "…"


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", required=True)
    ap.add_argument("--released-at", type=int, required=True)
    ap.add_argument("--kind", required=True, choices=["main", "beta", "stable"])
    ap.add_argument("--notes-file", required=True)
    ap.add_argument("--sums", required=True)
    ap.add_argument("--repo", required=True)
    a = ap.parse_args(argv)
    base = "https://github.com/%s/releases" % a.repo
    platforms = {}
    for line in open(a.sums).read().splitlines():
        m = re.match(r"^([0-9a-f]{64})\s+\*?(\S+)$", line)
        if not m:
            continue
        digest, name = m.groups()
        for suffix, key in PLATFORMS.items():
            if name == "OnePaste-%s-%s" % (a.tag, suffix):
                platforms[key] = {"url": "%s/download/%s/%s" % (base, a.tag, name), "sha256": digest, "size": 0}
    if not platforms:
        print("no desktop package found in the checksums", file=sys.stderr)
        return 1
    manifest = {
        "schema": 1,
        "version": a.tag.lstrip("v"),
        "tag": a.tag,
        "released_at": a.released_at,
        "kind": a.kind,
        "notes": summary(open(a.notes_file).read()),
        "release_url": "%s/tag/%s" % (base, a.tag),
        "platforms": platforms,
    }
    sys.stdout.write(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
