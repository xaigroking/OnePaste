#!/usr/bin/env bash
# Repository discipline checks. A non-zero exit code means failure. With CI=true nothing may be skipped.
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0
in_ci=false; [ "${CI:-}" = "true" ] && in_ci=true
# Skipped items: only a notice locally, always a failure in CI
skip() { if $in_ci; then say "FAIL $* (skipping is not allowed in CI)"; fail=1; else say "SKIP $*"; fi; }
say() { printf '%s\n' "$*" >&2; }

# 1) A single source file may have at most 1500 lines (build output and generated files excluded)
while IFS= read -r f; do
  n=$(wc -l < "$f")
  if [ "$n" -gt 1500 ]; then say "FAIL more than 1500 lines: $f ($n)"; fail=1; fi
done < <(find . -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.py' -o -name '*.dart' -o -name '*.swift' -o -name '*.kt' -o -name '*.cs' -o -name '*.go' -o -name '*.java' \) \
  -not -path '*/target/*' -not -path '*/node_modules/*' -not -path '*/generated/*' -not -name '*.gen.*')

# 2) No temporary or planning files may be committed
for pat in task_plan.md progress.md findings.md 'tmp.*'; do
  if find . -type f -name "$pat" -not -path '*/target/*' -not -path '*/node_modules/*' | grep -q .; then
    say "FAIL temporary file found: $pat"; fail=1; fi
done

# 3) No absolute paths of a local machine (common home directories)
if grep -rIn --exclude-dir=target --exclude-dir=node_modules --exclude=check.sh -E '/(Users|home)/[a-zA-Z0-9_-]+/' . >/dev/null 2>&1; then
  say "FAIL absolute local path found:"; grep -rIn --exclude-dir=target --exclude-dir=node_modules --exclude=check.sh -E '/(Users|home)/[a-zA-Z0-9_-]+/' . >&2 || true; fail=1
fi

# 8) Dependency license allow-list: Rust dependencies
if command -v cargo >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
  for m in $(find . -name Cargo.toml -not -path '*/target/*'); do
    (cd "$(dirname "$m")" && cargo metadata --format-version 1 2>/dev/null) | python3 -c '
import sys, json, re
d = json.load(sys.stdin)
root = set(d.get("workspace_members", []))
bad = re.compile(r"(?<![A-Za-z])(A?GPL|LGPL|SSPL|BUSL|CC-BY-NC)", re.I)
fail = 0
for p in d["packages"]:
    # Only packages that belong to this repository are skipped: workspace members, packages whose name starts with onepaste, and the desktop package
    if p["id"] in root or p["name"].startswith("onepaste") or p["name"] == "onepaste-desktop": continue
    lic = p.get("license") or ""
    # "MIT OR GPL-2.0" style choices are fine as long as one option avoids the forbidden licenses
    options = [o for o in re.split(r"\s+OR\s+|/", lic) if o.strip()]
    ok = bool(lic) and any(not bad.search(o) for o in options)
    if not ok:
        print("FAIL dependency license is not allowed: %s %s (%s)" % (p["name"], p["version"], lic or "undeclared"), file=sys.stderr); fail = 1
sys.exit(fail)
' || fail=1
  done
fi

# 8b) Direct dependencies of onepaste-core must be on the allow-list (no database, network, UI or platform SDK)
if command -v cargo >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1 && [ -f Cargo.toml ]; then
  cargo metadata --format-version 1 2>/dev/null | python3 -c '
import sys, json
d = json.load(sys.stdin)
allow = {"argon2","chacha20poly1305","rand_core","zeroize","serde","serde_json","base64","thiserror","getrandom","sha2"}
for p in d["packages"]:
    if p["name"] == "onepaste-core":
        extra = sorted({x["name"] for x in p["dependencies"]} - allow)
        if extra:
            print("FAIL onepaste-core depends on something outside the allow-list: %s" % extra, file=sys.stderr); sys.exit(1)
' || fail=1
fi

# 9) Rust workspace tests (including known-answer vectors) and the wasm32 build
if command -v cargo >/dev/null 2>&1 && [ -f Cargo.toml ]; then
  cargo test --workspace --quiet >/dev/null 2>&1 || { say "FAIL cargo test --workspace did not pass"; fail=1; }
  if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
    cargo build -p onepaste-core --target wasm32-unknown-unknown --quiet >/dev/null 2>&1 || { say "FAIL onepaste-core does not build for wasm32"; fail=1; }
  else
    skip "the wasm32-unknown-unknown target is not installed, so the wasm build check cannot run"
  fi
else
  skip "cargo is not installed, so cargo test --workspace cannot run"
fi

# 10) Cryptography prototype tests (only with cargo)
if command -v cargo >/dev/null 2>&1; then
  (cd spikes/vaultcore && cargo test --release --quiet >/dev/null 2>&1) || { say "FAIL spikes/vaultcore tests did not pass"; fail=1; }
else
  skip "cargo is not installed, so the spikes/vaultcore tests cannot run"
fi

# 11) Language gate: each layer may only use its agreed language
prune=( -name target -o -name node_modules -o -name dist -o -name .git )
if find . \( "${prune[@]}" \) -prune -o -type f -name '*.py' -not -path './tools/*' -print | grep -q .; then
  say "FAIL .py files may only live under tools/:"; find . \( "${prune[@]}" \) -prune -o -type f -name '*.py' -not -path './tools/*' -print >&2; fail=1
fi
if find . \( "${prune[@]}" \) -prune -o -type f \( -name '*.go' -o -name '*.cs' -o -name '*.java' -o -name '*.dart' -o -name '*.kt' \) -print | grep -q .; then
  say "FAIL .go/.cs/.java/.dart/.kt files are not allowed:"; find . \( "${prune[@]}" \) -prune -o -type f \( -name '*.go' -o -name '*.cs' -o -name '*.java' -o -name '*.dart' -o -name '*.kt' \) -print >&2; fail=1
fi
for d in crates apps tests; do
  [ -d "$d" ] || continue
  if find "$d" \( "${prune[@]}" \) -prune -o -type f \( -name '*.js' -o -name '*.jsx' -o -name '*.mjs' -o -name '*.cjs' \) -not -name '*.config.*' -print | grep -q .; then
    say "FAIL .js/.jsx/.mjs/.cjs files are not allowed under $d/ (toolchain configs named *.config.* are exempt):"
    find "$d" \( "${prune[@]}" \) -prune -o -type f \( -name '*.js' -o -name '*.jsx' -o -name '*.mjs' -o -name '*.cjs' \) -not -name '*.config.*' -print >&2; fail=1
  fi
done

# 12) Internal information gate: the public repository holds code only, no documents, license declarations or internal references
# 12a) Files and directories
extra_md=$(find . \( "${prune[@]}" \) -prune -o -type f -name '*.md' -not -path './README.md' -print)
[ -z "$extra_md" ] || { say "FAIL no .md files are allowed except the root README.md:"; say "$extra_md"; fail=1; }
bad_paths=$(find . \( "${prune[@]}" \) -prune -o \( -type d \( -name docs -o -name design \) -o -type f \( -name 'AGENTS*' -o -name 'COMMERCIAL*' -o -name 'LICENSE*' \) \) -print)
[ -z "$bad_paths" ] || { say "FAIL docs/, design/, AGENTS*, COMMERCIAL* and LICENSE* are not allowed:"; say "$bad_paths"; fail=1; }
# 12b) Content of tracked and not-ignored text files (this script, the shared pattern file and the root README.md are skipped)
# The pattern lives in a shared file that release-check.sh reads too
. scripts/lib/deny-patterns.sh
deny="$DENY_REGEX"
if git rev-parse --git-dir >/dev/null 2>&1; then
  while IFS= read -r -d '' f; do
    [ -f "$f" ] || continue
    case "$f" in scripts/check.sh|scripts/lib/deny-patterns.sh|README.md) continue;; esac
    if hits=$(grep -InE -i -- "$deny" "$f"); then
      say "FAIL internal information, license declaration or AI name found: $f"; say "$hits"; fail=1
    fi
  done < <(git ls-files -z --cached --others --exclude-standard)
  # 12c) No CJK characters in tracked text files (binary files skipped). Exceptions: the root
  #      README.md and the non-English UI translation files apps/*/src/locales/*.json.
  . scripts/lib/cjk-pattern.sh
  if cjk_hits=$(LC_ALL=C.UTF-8 git grep --untracked -I -n -P "$CJK_REGEX" -- . ':(exclude)README.md' \
      | LC_ALL=C.UTF-8 grep -vP '^apps/[^/]+/src/locales/(?!en\.json:)[^/]+\.json:'); then
    say "FAIL CJK characters found in tracked text files (file:line):"
    say "$(printf '%s\n' "$cjk_hits" | cut -d: -f1,2)"; fail=1
  fi
  # 12d) No CJK characters in commit messages after the language rule took effect
  if git cat-file -e "$LANG_RULE_BASE^{commit}" 2>/dev/null; then
    if msg_hits=$(git log "$LANG_RULE_BASE..HEAD" --format='%h %B' | LC_ALL=C.UTF-8 grep -nP "$CJK_REGEX"); then
      say "FAIL CJK characters found in commit messages after $LANG_RULE_BASE:"; say "$msg_hits"; fail=1
    fi
  else
    skip "commit $LANG_RULE_BASE is not available, so commit messages cannot be checked"
  fi
  # 12e) Commit messages and authorship of the whole history
  if hist=$(git log --all --format='%an%n%ae%n%cn%n%ce%n%B' | grep -nE -i -- "$deny|Co-Authored-By"); then
    say "FAIL AI name or Co-Authored-By found in commit history (messages, author, committer):"; say "$hist"; fail=1
  fi
else
  skip "not inside a git repository, so file contents and commit history cannot be checked"
fi

[ "$fail" -eq 0 ] && echo "check.sh: OK" || { echo "check.sh: FAILED" >&2; exit 1; }
