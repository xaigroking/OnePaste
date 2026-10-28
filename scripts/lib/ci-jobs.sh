#!/usr/bin/env bash
# Which CI jobs a change needs. Source this file; it defines functions only. The ci.yml `changes` job and scripts/check.sh
# call the same function, so the rule exists once and is tested.
#
# The jobs that always run (they are quick and on Linux): check, e2e, portability.

# shellcheck source=scripts/lib/release-paths.sh
. "$(dirname "${BASH_SOURCE[0]}")/release-paths.sh"

# jobs_for_paths: reads the names of the changed files on standard input, one per line, and prints one line per
# optional job: `<job>=true` if that job must run, `<job>=false` if the change cannot affect it.
#   windows_test    workspace tests on Windows (Rust code, the desktop program, or what builds them)
#   desktop_windows desktop unit tests and the desktop build on Windows
#   macos           the macOS desktop job: only what can behave differently there
#   docker          the container image test
#   ui_screens      the screenshot and keyboard tests of the front end
#   update_signing  the update signing chain
jobs_for_paths() {
  local files
  files=$(cat)
  has() { printf '%s\n' "$files" | grep -Eq "$1"; }
  local rust='^(crates/|Cargo\.(toml|lock)$|spikes/|tools/build_version\.rs$)'
  local desktop='^(apps/desktop/|packaging/)'
  local ci_self='^\.github/workflows/(ci|build-portable)\.yml$|^scripts/lib/ci-jobs\.sh$'
  local windows_test=false desktop_windows=false macos=false docker=false ui_screens=false update_signing=false
  if has "$rust" || has "$desktop" || has "$ci_self"; then windows_test=true; fi
  if has '^(apps/desktop/|packaging/|crates/onepaste-(app|platform|history|core)/|Cargo\.lock$|tools/build_version\.rs$)' || has "$ci_self"; then desktop_windows=true; fi
  # Only what can behave differently on macOS: the platform code, the history database, the core, the desktop program, the
  # lock file, and the two workflows that define how it is built. Other workflows (release, docker, ...) do not count.
  if has '^(crates/onepaste-(platform|history|core)/|apps/desktop/|Cargo\.lock$|packaging/|tools/build_version\.rs$)' || has '^\.github/workflows/(ci|build-portable)\.yml$'; then macos=true; fi
  if printf '%s\n' "$files" | needs_docker; then docker=true; fi
  if has '^(apps/desktop/|tests/screens/)' || has '^\.github/workflows/ci\.yml$'; then ui_screens=true; fi
  if has '^(apps/desktop/package(-lock)?\.json$|crates/onepaste-app/|scripts/sign-update-manifest\.sh$|tools/[a-z_]*update[a-z_]*\.py$|Cargo\.lock$)' || has '^\.github/workflows/ci\.yml$'; then update_signing=true; fi
  printf 'windows_test=%s\ndesktop_windows=%s\nmacos=%s\ndocker=%s\nui_screens=%s\nupdate_signing=%s\n' "$windows_test" "$desktop_windows" "$macos" "$docker" "$ui_screens" "$update_signing"
}

# jobs_on_main: a push to main runs everything, except the macOS job, which only pull requests start.
jobs_on_main() {
  printf 'windows_test=true\ndesktop_windows=true\nmacos=false\ndocker=true\nui_screens=true\nupdate_signing=true\n'
}
