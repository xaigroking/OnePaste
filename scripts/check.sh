#!/usr/bin/env bash
# 仓库纪律检查。退出码非 0 即失败。环境变量 CI=true 时不允许任何跳过。
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0
in_ci=false; [ "${CI:-}" = "true" ] && in_ci=true
# 跳过项：本地只提示，CI 里一律算失败
skip() { if $in_ci; then say "FAIL $*（CI 中不允许跳过）"; fail=1; else say "SKIP $*"; fi; }
say() { printf '%s\n' "$*" >&2; }

# 1) 单个源码文件 <= 1500 行（排除构建产物与生成物）
while IFS= read -r f; do
  n=$(wc -l < "$f")
  if [ "$n" -gt 1500 ]; then say "FAIL 超过 1500 行: $f ($n)"; fail=1; fi
done < <(find . -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.py' -o -name '*.dart' -o -name '*.swift' -o -name '*.kt' -o -name '*.cs' -o -name '*.go' -o -name '*.java' \) \
  -not -path '*/target/*' -not -path '*/node_modules/*' -not -path '*/generated/*' -not -name '*.gen.*')

# 2) 不得提交 AI 临时/规划文件
for pat in task_plan.md progress.md findings.md 'tmp.*'; do
  if find . -type f -name "$pat" -not -path '*/target/*' -not -path '*/node_modules/*' | grep -q .; then
    say "FAIL 发现临时文件: $pat"; fail=1; fi
done

# 3) 不得出现本机绝对路径（常见家目录）
if grep -rIn --exclude-dir=target --exclude-dir=node_modules --exclude=check.sh -E '/(Users|home)/[a-zA-Z0-9_-]+/' . >/dev/null 2>&1; then
  say "FAIL 发现本机绝对路径:"; grep -rIn --exclude-dir=target --exclude-dir=node_modules --exclude=check.sh -E '/(Users|home)/[a-zA-Z0-9_-]+/' . >&2 || true; fail=1
fi

# 8) 依赖许可证白名单：Rust 依赖
if command -v cargo >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
  for m in $(find . -name Cargo.toml -not -path '*/target/*'); do
    (cd "$(dirname "$m")" && cargo metadata --format-version 1 2>/dev/null) | python3 -c '
import sys, json, re
d = json.load(sys.stdin)
root = set(d.get("workspace_members", []))
bad = re.compile(r"(?<![A-Za-z])(A?GPL|LGPL|SSPL|BUSL|CC-BY-NC)", re.I)
fail = 0
for p in d["packages"]:
    if p["id"] in root: continue
    lic = p.get("license") or ""
    # 允许 "MIT OR GPL-2.0" 这类“任选其一”：只要有一个选项不含禁用项即可
    options = [o for o in re.split(r"\s+OR\s+|/", lic) if o.strip()]
    ok = bool(lic) and any(not bad.search(o) for o in options)
    if not ok:
        print("FAIL 依赖许可证不在白名单: %s %s (%s)" % (p["name"], p["version"], lic or "未声明"), file=sys.stderr); fail = 1
sys.exit(fail)
' || fail=1
  done
fi

# 8b) onepaste-core 的直接依赖必须在白名单内（无数据库/网络/UI/平台 SDK）
if command -v cargo >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1 && [ -f Cargo.toml ]; then
  cargo metadata --format-version 1 2>/dev/null | python3 -c '
import sys, json
d = json.load(sys.stdin)
allow = {"argon2","chacha20poly1305","rand_core","zeroize","serde","serde_json","base64","thiserror","getrandom","sha2"}
for p in d["packages"]:
    if p["name"] == "onepaste-core":
        extra = sorted({x["name"] for x in p["dependencies"]} - allow)
        if extra:
            print("FAIL onepaste-core 引入了白名单之外的依赖: %s" % extra, file=sys.stderr); sys.exit(1)
' || fail=1
fi

# 9) Rust 工作区测试（含已知答案向量）与 wasm32 编译
if command -v cargo >/dev/null 2>&1 && [ -f Cargo.toml ]; then
  cargo test --workspace --quiet >/dev/null 2>&1 || { say "FAIL cargo test --workspace 未通过"; fail=1; }
  if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
    cargo build -p onepaste-core --target wasm32-unknown-unknown --quiet >/dev/null 2>&1 || { say "FAIL onepaste-core 无法编译到 wasm32"; fail=1; }
  else
    skip "未安装 wasm32-unknown-unknown 目标，无法做 wasm 编译检查"
  fi
else
  skip "未安装 cargo，无法运行 cargo test --workspace"
fi

# 10) 密码学原型测试（有 cargo 才跑）
if command -v cargo >/dev/null 2>&1; then
  (cd spikes/vaultcore && cargo test --release --quiet >/dev/null 2>&1) || { say "FAIL spikes/vaultcore 测试未通过"; fail=1; }
else
  skip "未安装 cargo，无法运行 spikes/vaultcore 测试"
fi

# 11) 语言卡口：各层只允许使用约定的语言
prune=( -name target -o -name node_modules -o -name .git )
if find . \( "${prune[@]}" \) -prune -o -type f -name '*.py' -not -path './tools/*' -print | grep -q .; then
  say "FAIL .py 文件只能放在 tools/ 下:"; find . \( "${prune[@]}" \) -prune -o -type f -name '*.py' -not -path './tools/*' -print >&2; fail=1
fi
if find . \( "${prune[@]}" \) -prune -o -type f \( -name '*.go' -o -name '*.cs' -o -name '*.java' -o -name '*.dart' -o -name '*.kt' \) -print | grep -q .; then
  say "FAIL 不允许出现 .go/.cs/.java/.dart/.kt 文件:"; find . \( "${prune[@]}" \) -prune -o -type f \( -name '*.go' -o -name '*.cs' -o -name '*.java' -o -name '*.dart' -o -name '*.kt' \) -print >&2; fail=1
fi
for d in crates apps tests; do
  [ -d "$d" ] || continue
  if find "$d" \( "${prune[@]}" \) -prune -o -type f \( -name '*.js' -o -name '*.jsx' -o -name '*.mjs' -o -name '*.cjs' \) -not -name '*.config.*' -print | grep -q .; then
    say "FAIL $d/ 下不允许出现 .js/.jsx/.mjs/.cjs 文件（*.config.* 工具链配置除外）:"
    find "$d" \( "${prune[@]}" \) -prune -o -type f \( -name '*.js' -o -name '*.jsx' -o -name '*.mjs' -o -name '*.cjs' \) -not -name '*.config.*' -print >&2; fail=1
  fi
done

# 12) 内网信息卡口：公开仓库只放代码，不放文档、许可证声明和内部引用
# 12a) 文件与目录
extra_md=$(find . \( "${prune[@]}" \) -prune -o -type f -name '*.md' -not -path './README.md' -print)
[ -z "$extra_md" ] || { say "FAIL 除根目录 README.md 外不允许有 .md 文件:"; say "$extra_md"; fail=1; }
bad_paths=$(find . \( "${prune[@]}" \) -prune -o \( -type d \( -name docs -o -name design \) -o -type f \( -name 'AGENTS*' -o -name 'COMMERCIAL*' -o -name 'LICENSE*' \) \) -print)
[ -z "$bad_paths" ] || { say "FAIL 不允许出现 docs/、design/、AGENTS*、COMMERCIAL*、LICENSE*:"; say "$bad_paths"; fail=1; }
# 12b) 内容：已跟踪与未忽略的文本文件（跳过本脚本与根目录 README.md）
deny='\bINV-[0-9]|\bADR-[0-9]|docs/|dev\.forgejo\.lan|nebula-devsering|doc-onepaste|SPDX-License-Identifier|\b(claude|anthropic|openai|chatgpt|gpt-?[0-9]|codex|copilot|gemini)\b'
if git rev-parse --git-dir >/dev/null 2>&1; then
  while IFS= read -r -d '' f; do
    [ -f "$f" ] || continue
    case "$f" in scripts/check.sh|README.md) continue;; esac
    if hits=$(grep -InE -i -- "$deny" "$f"); then
      say "FAIL 出现内网信息、许可证声明或 AI 名称: $f"; say "$hits"; fail=1
    fi
  done < <(git ls-files -z --cached --others --exclude-standard)
  # 12c) 全部历史的提交信息与署名
  if hist=$(git log --all --format='%an%n%ae%n%cn%n%ce%n%B' | grep -nE -i -- "$deny|Co-Authored-By"); then
    say "FAIL 提交历史（信息、author、committer）中出现 AI 名称或 Co-Authored-By:"; say "$hist"; fail=1
  fi
else
  skip "不在 git 仓库内，无法检查文件内容与提交历史"
fi

[ "$fail" -eq 0 ] && echo "check.sh: OK" || { echo "check.sh: FAILED" >&2; exit 1; }
