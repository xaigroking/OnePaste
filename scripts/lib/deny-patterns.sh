# Shared patterns for internal information, license declarations and AI names.
# Both check.sh (item 12) and release-check.sh read this one file.
# Usage: . scripts/lib/deny-patterns.sh, then grep -E -i "$DENY_REGEX".
DENY_REGEX='\bINV-[0-9]|\bADR-[0-9]|docs/|dev\.forgejo\.lan|nebula-devsering|doc-onepaste|SPDX-License-Identifier|\b(claude|anthropic|openai|chatgpt|gpt-?[0-9]|codex|copilot|gemini)\b'
