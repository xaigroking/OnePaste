#!/usr/bin/env python3
"""Checks a release's update manifest before it is published.

Usage: check-update-manifest.py --manifest latest.json --signature latest.json.sig --sums SHA256SUMS.txt --tag TAG --repo OWNER/NAME

Fails if a download address does not point at a file of this very release, if a checksum differs from the checksums file,
if the version does not match the tag, or if the signature is missing or is not a signature file.
"""
import argparse
import base64
import json
import re
import sys


def problems(manifest_text, signature_text, sums_text, tag, repo):
    out = []
    try:
        m = json.loads(manifest_text)
    except ValueError:
        return ["the manifest is not JSON"]
    release = "https://github.com/%s/releases" % repo
    if m.get("tag") != tag or m.get("version") != tag.lstrip("v"):
        out.append("the version in the manifest does not match the tag %s" % tag)
    if m.get("release_url") != "%s/tag/%s" % (release, tag):
        out.append("the release page address does not point at this release")
    sums = {}
    for line in sums_text.splitlines():
        mm = re.match(r"^([0-9a-f]{64})\s+\*?(\S+)$", line)
        if mm:
            sums[mm.group(2)] = mm.group(1)
    if not m.get("platforms"):
        out.append("the manifest names no download")
    for key, f in (m.get("platforms") or {}).items():
        prefix = "%s/download/%s/" % (release, tag)
        url = f.get("url", "")
        if not url.startswith(prefix):
            out.append("%s: the download address does not point at a file of this release: %s" % (key, url))
            continue
        name = url[len(prefix):]
        if sums.get(name) != f.get("sha256"):
            out.append("%s: the checksum differs from the checksums file for %s" % (key, name))
    sig = signature_text.strip()
    if not sig:
        out.append("the signature is empty")
    else:
        try:
            if not base64.b64decode(sig, validate=True).decode().startswith("untrusted comment:"):
                out.append("the signature is not a signature file")
        except Exception:
            out.append("the signature is not a signature file")
    return out


def main(argv):
    ap = argparse.ArgumentParser()
    for n in ("manifest", "signature", "sums", "tag", "repo"):
        ap.add_argument("--" + n, required=True)
    a = ap.parse_args(argv)
    found = problems(open(a.manifest).read(), open(a.signature).read(), open(a.sums).read(), a.tag, a.repo)
    for p in found:
        print("FAIL " + p, file=sys.stderr)
    if not found:
        print("update manifest checks passed")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
