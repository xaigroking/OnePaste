# Character ranges treated as CJK text. The repository is English only, apart from the
# README.md language section and the non-English UI translation files.
# The ranges are written as escapes so no script has to contain CJK characters itself.
# Usage: . scripts/lib/cjk-pattern.sh, then LC_ALL=C.UTF-8 grep -P "$CJK_REGEX".
CJK_REGEX='[\x{3000}-\x{303F}\x{3040}-\x{30FF}\x{3400}-\x{4DBF}\x{4E00}-\x{9FFF}\x{AC00}-\x{D7AF}\x{F900}-\x{FAFF}\x{FF00}-\x{FFEF}]'
# History before this commit is not rewritten, so commit messages are only checked after it.
LANG_RULE_BASE=72910fb
