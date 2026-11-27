#!/usr/bin/env bash
# Tests for scripts/next-version.sh against a throwaway repository. Run by CI.
set -euo pipefail

script="$(cd "$(dirname "$0")" && pwd)/next-version.sh"
repo="$(mktemp -d)"
trap 'rm -rf "$repo"' EXIT
cd "$repo"
git init -q -b main
git config user.name test
git config user.email test@example.com
git config commit.gpgsign false
git config core.autocrlf false

failures=0
commit() { # commit <path> <message...>
  local path="$1"
  shift
  mkdir -p "$(dirname "$path")"
  echo "$RANDOM" >>"$path"
  git add "$path"
  git commit -q -m "$1" ${2:+-m "$2"}
}
expect() { # expect <product> <wanted output, one key=value per argument...>
  local product="$1" out want
  shift
  out="$(bash "$script" "$product")"
  for want in "$@"; do
    if ! grep -qxF "$want" <<<"$out"; then
      echo "FAIL ($product): wanted '$want', got: $(tr '\n' ' ' <<<"$out")"
      failures=$((failures + 1))
    fi
  done
}

commit README.md "docs: readme"
expect node release=false level=none

commit crates/ursa-core/src/lib.rs "fix: first"
expect node release=true version=0.1.0 tag=node-v0.1.0
expect windows release=true version=0.1.0
expect android release=true version=0.1.0
git tag node-v0.1.0

commit crates/ursa-node/src/main.rs "chore: tidy"
expect node release=false
commit crates/ursa-node/src/main.rs "fix(node): a bug"
expect node release=true version=0.1.1 level=patch
commit crates/ursa-node/src/main.rs "feat(node): a feature"
expect node release=true version=0.2.0 level=minor

# Paths: the node never ships for an app-only change, and Android ignores the node crate.
git tag node-v0.2.0
commit apps/windows/App.cs "feat: windows only"
expect node release=false
expect windows release=true version=0.1.0
expect android release=true version=0.1.0 level=patch
git tag android-v0.1.0
commit crates/ursa-node/src/main.rs "feat: node only"
expect android release=false

# Breaking changes, by marker and by footer.
git tag node-v0.3.0
commit crates/ursa-node/src/main.rs "refactor!: new wire format"
expect node release=true version=1.0.0 level=major
git tag node-v1.0.0
commit crates/ursa-node/src/main.rs "fix: change" "BREAKING CHANGE: something"
expect node release=true version=2.0.0

# --force overrides, and notes list what ships.
out="$(bash "$script" node --force patch)"
grep -qx "version=1.0.1" <<<"$out" || { echo "FAIL: --force patch: $out"; failures=$((failures + 1)); }
notes="$(bash "$script" node --notes)"
grep -q '^### Breaking changes' <<<"$notes" || { echo "FAIL: notes: $notes"; failures=$((failures + 1)); }
grep -q '^- change (' <<<"$notes" || { echo "FAIL: notes item: $notes"; failures=$((failures + 1)); }

if ((failures > 0)); then
  echo "$failures failure(s)"
  exit 1
fi
echo "next-version.sh: all tests passed"
