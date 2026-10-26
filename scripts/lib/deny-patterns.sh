# 内网信息、许可证声明和 AI 名称的共用正则。check.sh 第 12 项和 release-check.sh 都读这一份。
# 用法：. scripts/lib/deny-patterns.sh，然后用 grep -E -i "$DENY_REGEX"。
DENY_REGEX='\bINV-[0-9]|\bADR-[0-9]|docs/|dev\.forgejo\.lan|nebula-devsering|doc-onepaste|SPDX-License-Identifier|\b(claude|anthropic|openai|chatgpt|gpt-?[0-9]|codex|copilot|gemini)\b'
