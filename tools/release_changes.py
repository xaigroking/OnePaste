#!/usr/bin/env python3
"""Prints the titles of the pull requests merged into main in a range of commits, one Markdown bullet per line.

Usage: python3 tools/release_changes.py <range>   (for example v0.0.1-main.6..HEAD, or HEAD for everything)

A merge commit made by GitHub has the pull request title as the first line of its body; a squash commit has it as the
subject, followed by the number in parentheses. Only the first parent is followed, so the commits inside a pull request
are not listed.
"""
import subprocess
import sys


def titles(rng):
    out = subprocess.run(
        ["git", "log", "--first-parent", "--format=%s%x1f%b%x1e", rng], check=True, capture_output=True, text=True
    ).stdout
    result = []
    for record in out.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        subject, _, body = record.partition("\x1f")
        if subject.startswith("Merge pull request"):
            lines = [l for l in body.splitlines() if l.strip()]
            number = subject.split("#", 1)[1].split()[0] if "#" in subject else ""
            title = lines[0].strip() if lines else subject
            result.append("%s (#%s)" % (title, number) if number else title)
        else:
            result.append(subject.strip())
    return result


def main():
    if len(sys.argv) != 2:
        print("usage: release_changes.py <range>", file=sys.stderr)
        return 2
    items = titles(sys.argv[1])
    if not items:
        print("- No pull requests since the previous release.")
    for item in items:
        print("- " + item)
    return 0


if __name__ == "__main__":
    sys.exit(main())
