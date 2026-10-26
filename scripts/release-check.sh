#!/usr/bin/env bash
# 发布前检查：用法 scripts/release-check.sh <tag>。任何一项不过，退出码为 1，什么都不发。
# 需要环境变量 GH_TOKEN（或已登录的 gh）以查询 PR 与 Release；仓库取 GITHUB_REPOSITORY，
# 没有就从 origin 的地址推出。
set -uo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=scripts/lib/deny-patterns.sh
. scripts/lib/deny-patterns.sh

say() { printf '%s\n' "$*" >&2; }
fail=0
bad() { say "FAIL $*"; fail=1; }

tag="${1:-}"
[ -n "$tag" ] || { say "用法: scripts/release-check.sh <tag>"; exit 2; }

repo="${GITHUB_REPOSITORY:-}"
if [ -z "$repo" ]; then
  repo=$(git remote get-url origin 2>/dev/null | sed -E 's#^.*github\.com[:/]##; s#\.git$##')
fi

# 1) tag 格式：只允许 vX.Y.Z-beta.N
version=""
if [[ "$tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)-beta\.[0-9]+$ ]]; then
  version="${BASH_REMATCH[1]}"
elif [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  bad "正式版发布尚未开放: $tag"
else
  bad "tag 格式必须是 vX.Y.Z-beta.N: $tag"
fi

# 2) tag 位置：提交在 main 上，并且是经 PR 合入的
commit=$(git rev-parse -q --verify "refs/tags/${tag}^{commit}" 2>/dev/null || true)
if [ -z "$commit" ]; then
  bad "找不到 tag: $tag"
else
  if ! git rev-parse -q --verify refs/remotes/origin/main >/dev/null; then
    bad "找不到 origin/main，无法确认 tag 是否在 main 上"
  elif ! git merge-base --is-ancestor "$commit" refs/remotes/origin/main; then
    bad "tag 指向的提交不在 main 上: $commit"
  fi
  if ! count=$(gh api "repos/${repo}/commits/${commit}/pulls" --jq 'length' 2>/dev/null); then
    bad "无法查询该提交关联的 PR（仓库 ${repo}）"
  elif [ "$count" = "0" ]; then
    bad "tag 指向的提交不是经 PR 合入 main 的: $commit"
  fi
fi

# 3) 版本号一致：tag、package.json、tauri.conf.json、src-tauri/Cargo.toml
if [ -n "$version" ]; then
  pkg=$(python3 -c 'import json,sys;print(json.load(open("apps/desktop/package.json"))["version"])' 2>/dev/null || echo "?")
  tauri=$(python3 -c 'import json,sys;print(json.load(open("apps/desktop/src-tauri/tauri.conf.json"))["version"])' 2>/dev/null || echo "?")
  cargo=$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' apps/desktop/src-tauri/Cargo.toml | head -n1)
  for pair in "apps/desktop/package.json:$pkg" "apps/desktop/src-tauri/tauri.conf.json:$tauri" "apps/desktop/src-tauri/Cargo.toml:${cargo:-?}"; do
    if [ "${pair#*:}" != "$version" ]; then
      bad "版本号不一致: tag 是 $version，${pair%%:*} 是 ${pair#*:}"
    fi
  done
fi

# 4) 发布说明：必须是附注 tag，正文含 6 个小节标题
notes=""
if [ -n "$commit" ]; then
  if [ "$(git cat-file -t "refs/tags/$tag")" != "tag" ]; then
    bad "必须是附注 tag（git tag -a），轻量 tag 没有发布说明: $tag"
  else
    notes=$(git for-each-ref --format='%(contents)' "refs/tags/$tag")
    for title in "升级前必读" "新功能" "修复" "已知问题" "首次安装提示" "校验"; do
      # 标题行：Markdown 标题，可以带一个前导符号（如 ⚠️）
      if ! grep -Eq "^#{1,6}[[:space:]]+([^[:alnum:][:space:]]+[[:space:]]+)?${title}([[:space:]/（(:：].*)?$" <<<"$notes"; then
        bad "发布说明缺少小节: $title"
      fi
    done
  fi
fi

# 5) 内网信息和 AI 名字：与 check.sh 第 12 项同一套正则
if [ -n "$notes" ]; then
  if hits=$(grep -nE -i -- "$DENY_REGEX" <<<"$notes"); then
    bad "发布说明里出现内网信息、许可证声明或 AI 名称:"
    say "$hits"
  fi
fi

# 6) 不能重复发布
if [ -n "$repo" ]; then
  out=$(gh api "repos/${repo}/releases/tags/${tag}" 2>&1)
  rc=$?
  if [ "$rc" -eq 0 ]; then
    bad "同名 Release 已存在: $tag"
  elif ! grep -q 'HTTP 404' <<<"$out"; then
    bad "无法确认 Release 是否已存在: $out"
  fi
else
  bad "无法确定仓库名"
fi

if [ "$fail" -eq 0 ]; then
  echo "release-check.sh: OK ($tag)"
else
  echo "release-check.sh: FAILED ($tag)" >&2
  exit 1
fi
