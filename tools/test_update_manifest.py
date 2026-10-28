#!/usr/bin/env python3
"""Tests of the update manifest scripts: fixed input gives fixed output, and each pre-release check has a failing case."""
import base64
import importlib.util
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPTS = HERE


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), os.path.join(SCRIPTS, name + ".py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


check = load("check_update_manifest")
TAG = "v0.0.1-main.7"
REPO = "owner/repo"
WIN = "OnePaste-%s-windows-x64-portable.zip" % TAG
MAC = "OnePaste-%s-macos-universal-portable.zip" % TAG
SUMS = "%s  %s\n%s  %s\n%s  OnePaste-%s-tools-linux-x64-portable.zip\n" % ("a" * 64, WIN, "b" * 64, MAC, "c" * 64, TAG)
NOTES = "## Read first\ntext\n\n## What's changed\nPull requests merged since `v0.0.1-beta.4`:\n\n- Add the update check (#30)\n- Fix the devices table (#31)\n\n## Verify downloads\n- not a change\n"
SIG = base64.b64encode(b"untrusted comment: signature\nRWQ...\ntrusted comment: t\nxyz\n").decode()


def make(extra=()):
    with tempfile.TemporaryDirectory() as d:
        n, s = os.path.join(d, "notes.md"), os.path.join(d, "sums.txt")
        open(n, "w").write(NOTES)
        open(s, "w").write(SUMS)
        r = subprocess.run(
            [sys.executable, os.path.join(SCRIPTS, "make_update_manifest.py"), "--tag", TAG, "--released-at", "1790000000", "--kind", "main", "--notes-file", n, "--sums", s, "--repo", REPO, *extra],
            capture_output=True, text=True,
        )
        return r


def test_the_manifest_for_fixed_input_is_fixed():
    r = make()
    assert r.returncode == 0, r.stderr
    got = json.loads(r.stdout)
    assert got == {
        "schema": 1,
        "version": "0.0.1-main.7",
        "tag": TAG,
        "released_at": 1790000000,
        "kind": "main",
        "notes": "Add the update check (#30)\nFix the devices table (#31)",
        "release_url": "https://github.com/owner/repo/releases/tag/v0.0.1-main.7",
        "platforms": {
            "windows-x86_64": {"url": "https://github.com/owner/repo/releases/download/%s/%s" % (TAG, WIN), "sha256": "a" * 64, "size": 0},
            "darwin-universal": {"url": "https://github.com/owner/repo/releases/download/%s/%s" % (TAG, MAC), "sha256": "b" * 64, "size": 0},
        },
    }
    assert r.stdout == json.dumps(got, indent=2, sort_keys=True) + "\n", "the output is stable"


def test_a_release_without_a_desktop_package_gives_no_manifest():
    with tempfile.TemporaryDirectory() as d:
        s, n = os.path.join(d, "s"), os.path.join(d, "n")
        open(s, "w").write("%s  something-else.zip\n" % ("a" * 64))
        open(n, "w").write(NOTES)
        r = subprocess.run([sys.executable, os.path.join(SCRIPTS, "make_update_manifest.py"), "--tag", TAG, "--released-at", "1", "--kind", "main", "--notes-file", n, "--sums", s, "--repo", REPO], capture_output=True, text=True)
        assert r.returncode == 1


def good():
    return make().stdout


def test_a_good_manifest_passes_the_pre_release_check():
    assert check.problems(good(), SIG, SUMS, TAG, REPO) == []


def test_each_pre_release_check_has_a_failing_case():
    m = json.loads(good())
    # 1. a download address that points at another release or another place
    other = json.loads(good())
    other["platforms"]["windows-x86_64"]["url"] = "https://github.com/owner/repo/releases/download/v0.0.1-main.6/%s" % WIN
    assert any("does not point at a file of this release" in p for p in check.problems(json.dumps(other), SIG, SUMS, TAG, REPO))
    evil = json.loads(good())
    evil["platforms"]["windows-x86_64"]["url"] = "https://example.org/download/%s" % WIN
    assert any("does not point at a file of this release" in p for p in check.problems(json.dumps(evil), SIG, SUMS, TAG, REPO))
    # 2. a checksum that is not the one in the checksums file
    bad = json.loads(good())
    bad["platforms"]["darwin-universal"]["sha256"] = "d" * 64
    assert any("checksum differs" in p for p in check.problems(json.dumps(bad), SIG, SUMS, TAG, REPO))
    # 3. an empty signature, and one that is not a signature file
    assert any("signature is empty" in p for p in check.problems(good(), "  \n", SUMS, TAG, REPO))
    assert any("not a signature file" in p for p in check.problems(good(), "garbage!", SUMS, TAG, REPO))
    assert any("not a signature file" in p for p in check.problems(good(), base64.b64encode(b"hello").decode(), SUMS, TAG, REPO))
    # 4. a version that is not the tag's, and a release page of another release
    wrong = json.loads(good())
    wrong["version"] = "0.0.1-main.8"
    assert any("does not match the tag" in p for p in check.problems(json.dumps(wrong), SIG, SUMS, TAG, REPO))
    page = json.loads(good())
    page["release_url"] = "https://github.com/owner/repo/releases/tag/v0.0.1-main.6"
    assert any("release page address" in p for p in check.problems(json.dumps(page), SIG, SUMS, TAG, REPO))
    assert m["tag"] == TAG


if __name__ == "__main__":
    failed = 0
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            try:
                fn()
                print("ok:", name)
            except AssertionError as e:
                failed = 1
                print("FAIL:", name, e, file=sys.stderr)
    sys.exit(failed)
