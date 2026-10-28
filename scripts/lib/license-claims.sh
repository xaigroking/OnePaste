#!/usr/bin/env bash
# Words that must not appear in anything a user reads. The license of the project is not decided, and until it is nothing
# may say or hint what it is. Source this file, then: grep -nEi "$LICENSE_CLAIM_REGEX" <files> (with a UTF-8 locale).
#
# The Chinese words are written as code points so that this file holds no Chinese text. They are: source public, open
# source, non-commercial, commercial use, license, and the compounds that mean a license agreement. The Chinese word for
# "protocol" alone is not a license word (it is in "protocol version"), so only the compounds are listed.
_license_claims_cjk=$'\u6e90\u7801\u516c\u5f00|\u5f00\u6e90|\u975e\u5546\u4e1a|\u5546\u4e1a\u4f7f\u7528|\u8bb8\u53ef\u8bc1|(\u5f00\u6e90|\u6388\u6743|\u8bb8\u53ef|\u4f7f\u7528)\u534f\u8bae'
LICENSE_CLAIM_REGEX='source-available|open[- ]?source|non-?commercial|commercial use|polyform|\bA?GPL\b|MIT licen[sc]e|Apache-2\.0|licen[sc]e|'"$_license_claims_cjk"

# The files a user reads: the translation files, the README, the packaging texts (READ-ME-FIRST, release notes templates),
# the script that writes the release notes and the app description. The list of third-party licenses that the dependency
# check makes is not among them, and neither are the scripts that check dependency licenses.
license_claim_files() {
  git ls-files -- 'apps/*/src/locales/*.json' README.md 'packaging/*' scripts/make-release-notes.sh 'apps/desktop/src-tauri/tauri.conf.json'
}
