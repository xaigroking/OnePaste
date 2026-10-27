#!/usr/bin/env python3
"""Checks every UI translation file against the English one.

HOW TO ADD A LANGUAGE
 1. Copy apps/desktop/src/locales/en.json to apps/desktop/src/locales/<code>.json. The code is a
    lower-case language tag: fr, de, pt-br, zh-tw.
 2. Set meta.nativeName to the language's own name and meta.dir to "ltr" or "rtl".
 3. Translate the values. Keep the keys and the placeholders such as {{count}} unchanged.
 4. Run: python3 tools/check_locales.py
Nothing else needs to change: the Settings picker, "follow the system" and the tray texts pick the file
up on their own. A system language such as pt-BR uses pt-br.json if it exists, then pt.json, then English.

Missing keys are not fatal at run time (the UI shows English for them), but a translation should be
complete, so this check fails on missing keys, unknown keys, changed placeholders and a missing
meta.nativeName.
"""
import json
import re
import sys
from pathlib import Path

LOCALES = Path(__file__).resolve().parent.parent / "apps" / "desktop" / "src" / "locales"
NAME_RE = re.compile(r"^[a-z]{2,3}(-[a-z0-9]{2,8})*\.json$")
PLACEHOLDER_RE = re.compile(r"\{\{\s*(\w+)\s*\}\}|\{(\w+)\}")


def flatten(obj, prefix=""):
    out = {}
    for key, value in obj.items():
        path = f"{prefix}.{key}" if prefix else key
        if isinstance(value, dict):
            out.update(flatten(value, path))
        else:
            out[path] = str(value)
    return out


def placeholders(text):
    return sorted(a or b for a, b in PLACEHOLDER_RE.findall(text))


def main():
    files = sorted(p for p in LOCALES.glob("*.json"))
    reference = flatten(json.loads((LOCALES / "en.json").read_text(encoding="utf-8")))
    problems = []
    if "meta.nativeName" not in reference:
        problems.append("en.json: missing meta.nativeName")
    for path in files:
        if not NAME_RE.match(path.name):
            problems.append(f"{path.name}: file name must be a lower-case language tag such as zh.json or pt-br.json")
        if path.name == "en.json":
            continue
        flat = flatten(json.loads(path.read_text(encoding="utf-8")))
        if not flat.get("meta.nativeName"):
            problems.append(f"{path.name}: missing meta.nativeName (the language's own name, shown in the picker)")
        for key, text in reference.items():
            if key.startswith("meta."):
                continue
            if key not in flat:
                problems.append(f"{path.name}: missing key {key}")
            elif placeholders(flat[key]) != placeholders(text):
                problems.append(f"{path.name}: placeholders differ in {key}")
        for key in flat:
            if key not in reference:
                problems.append(f"{path.name}: unknown key {key}")
    if problems:
        print("\n".join(problems), file=sys.stderr)
        print(f"{len(problems)} locale problem(s)", file=sys.stderr)
        return 1
    print("locales ok: " + ", ".join(p.stem for p in files))
    return 0


if __name__ == "__main__":
    sys.exit(main())
