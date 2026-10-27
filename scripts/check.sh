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

# 8b) Direct dependencies of onepaste-core and onepaste-sync must be on their allow-lists (no database, network, UI or platform SDK)
if command -v cargo >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1 && [ -f Cargo.toml ]; then
  cargo metadata --format-version 1 2>/dev/null | python3 -c '
import sys, json
d = json.load(sys.stdin)
allow = {
    "onepaste-core": {"argon2","chacha20poly1305","rand_core","zeroize","serde","serde_json","base64","thiserror","getrandom","sha2"},
    "onepaste-sync": {"onepaste-core","serde","serde_json","base64","thiserror","zeroize"},
}
seen = set()
bad = False
for p in d["packages"]:
    if p["name"] in allow:
        seen.add(p["name"])
        extra = sorted({x["name"] for x in p["dependencies"]} - allow[p["name"]])
        if extra:
            print("FAIL %s depends on something outside its allow-list: %s" % (p["name"], extra), file=sys.stderr); bad = True
if seen != set(allow):
    print("FAIL dependency allow-list: package missing from the workspace: %s" % sorted(set(allow) - seen), file=sys.stderr); bad = True
sys.exit(1 if bad else 0)
' || fail=1
fi

# 8c) Test-only features must never be on by default: fault injection in the server, the client layer and the command line
#     tool, and the test harness in the application logic crate
if command -v cargo >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1 && [ -f Cargo.toml ]; then
  cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c '
import sys, json
d = json.load(sys.stdin)
bad = False
seen = set()
guard = {"onepaste-server": "fault-injection", "onepaste-client": "fault-injection", "onepaste-cli": "fault-injection", "onepaste-app": "test-harness"}
for p in d["packages"]:
    if p["name"] not in guard:
        continue
    seen.add(p["name"])
    feature = guard[p["name"]]
    feats = p["features"]
    todo, on = list(feats.get("default", [])), set()
    while todo:
        f = todo.pop()
        name = f.split("/")[-1] if "/" in f else f
        if name in on:
            continue
        on.add(name)
        todo.extend(feats.get(name, []))
    if feature in on:
        print("FAIL %s enables %s by default" % (p["name"], feature), file=sys.stderr); bad = True
    if feature not in feats:
        print("FAIL %s has no %s feature" % (p["name"], feature), file=sys.stderr); bad = True
if len(seen) != len(guard):
    print("FAIL test-only feature check: crate missing: %s" % sorted(set(guard) - seen), file=sys.stderr); bad = True
sys.exit(1 if bad else 0)
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

# 13) Automatic pre-releases: the rules are functions in scripts/lib/release-paths.sh, and they are tested here
# shellcheck source=scripts/lib/release-paths.sh
. scripts/lib/release-paths.sh
release_fail=0
# 13a) Which merges publish: five examples (documents only, server code, desktop app, end-to-end tests, a mix)
expect_release() {  # expect_release <yes|no> <description> <file>...
  local want=$1 what=$2 got=no
  shift 2
  if printf '%s\n' "$@" | needs_release; then got=yes; fi
  [ "$got" = "$want" ] || { say "FAIL release paths: $what: expected $want, got $got"; release_fail=1; }
}
expect_release no "only documents" README.md docs/notes.txt
expect_release yes "only the server" crates/onepaste-server/src/main.rs
expect_release yes "only the desktop app" apps/desktop/src/App.tsx
expect_release no "only end-to-end tests" tests/e2e/first_run.sh
expect_release yes "a mix" README.md tests/e2e/first_run.sh packaging/READ-ME-FIRST.macos.txt
expect_release no "only scripts" scripts/check.sh scripts/lib/release-paths.sh
expect_release yes "only the lock file" Cargo.lock
expect_release yes "only a workflow" .github/workflows/ci.yml
# 13b) The next number never repeats and never reuses a number of a deleted pre-release
[ "$(printf 'v0.0.1-beta.4\nv0.0.1-main.3\nv0.0.1-main.10\nv0.0.2-main.99\n' | next_main_number 0.0.1)" = 11 ] || { say "FAIL next_main_number: expected 11"; release_fail=1; }
[ "$(printf 'v0.0.1-beta.4\n' | next_main_number 0.0.1)" = 1 ] || { say "FAIL next_main_number: expected 1 when there is none"; release_fail=1; }
# 13c) The clean-up keeps the newest ten automatic pre-releases and never lists anything else
many=$(for i in $(seq 1 14); do echo "v0.0.1-main.$i"; done; echo v0.0.1-beta.2; echo v0.0.1-beta.3; echo v0.0.1-beta.4)
[ "$(printf '%s\n' "$many" | prune_list 10 | sort -V | tr '\n' ' ')" = "v0.0.1-main.1 v0.0.1-main.2 v0.0.1-main.3 v0.0.1-main.4 " ] || { say "FAIL prune_list: expected main.1 to main.4"; release_fail=1; }
printf '%s\n' "$many" | prune_list 10 | grep -q beta && { say "FAIL prune_list listed a beta"; release_fail=1; }
# 13d) Tag names of named and automatic releases are told apart
bash scripts/release-check.sh --test-tag-names > /dev/null || { say "FAIL release-check.sh tag name assertions"; release_fail=1; }
# 13e) Workflows: actionlint where it is installed, and always a YAML syntax check
if command -v actionlint > /dev/null; then
  actionlint || { say "FAIL actionlint found problems"; release_fail=1; }
else
  skip "actionlint is not installed"
fi
if command -v python3 > /dev/null && python3 -c 'import yaml' 2> /dev/null; then
  for wf in .github/workflows/*.yml; do
    python3 -c 'import sys, yaml; yaml.safe_load(open(sys.argv[1]))' "$wf" || { say "FAIL $wf is not valid YAML"; release_fail=1; }
  done
else
  skip "python3 with PyYAML is not available, so the workflows are not syntax checked"
fi
# 13f) The notes generator produces every section the release check wants
notes_tmp=$(mktemp)
bash scripts/make-release-notes.sh v0.0.1-main.1 "$(git rev-parse HEAD 2> /dev/null || echo 0000000)" > "$notes_tmp" 2> /dev/null || { say "FAIL make-release-notes.sh failed"; release_fail=1; }
for title in "Read first" "First launch" "Try it in 3 steps" "What's changed" "Verify downloads"; do
  grep -Eq "^## .*$title" "$notes_tmp" || { say "FAIL generated release notes lack: $title"; release_fail=1; }
done
rm -f "$notes_tmp"
[ "$release_fail" -eq 0 ] || fail=1

# 14) Every translation file has all the keys of English, and the same placeholders
python3 tools/check_locales.py || fail=1

# 15) The update check: the manifest scripts are tested, and nothing can redirect the update source or key
python3 tools/test_update_manifest.py > /dev/null || { say "FAIL tools/test_update_manifest.py"; fail=1; }
# The address the update check asks and the key it trusts are fixed in the program: no environment variable, argument or
# file may change them (tests do it by building their own configuration in code)
if grep -nE 'env::var|env!\(|option_env' crates/onepaste-app/src/update.rs | grep -v 'include_str'; then
  say "FAIL crates/onepaste-app/src/update.rs reads the environment"; fail=1
fi
if grep -nE 'UPDATE_(URL|KEY|SOURCE)' crates/onepaste-app/src apps/desktop/src-tauri/src | grep -E 'env|getenv'; then
  say "FAIL an environment variable can change the update source"; fail=1
fi

[ "$fail" -eq 0 ] && echo "check.sh: OK" || { echo "check.sh: FAILED" >&2; exit 1; }
