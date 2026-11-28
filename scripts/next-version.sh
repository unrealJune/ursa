#!/usr/bin/env bash
# The next release of a product, from the Conventional Commits that touched its paths since its
# last tag. Prints `release=true|false`, `version=X.Y.Z`, `tag=<prefix>X.Y.Z`, `level=…`.
#
#   scripts/next-version.sh node|windows|android|macos [--force auto|patch|minor|major]
#   scripts/next-version.sh node|windows|android|macos --notes    # Markdown release notes instead
#
# feat → minor, any other type that ships (fix, perf, revert, or no type) → patch,
# `type!:` or a BREAKING CHANGE footer → major. docs/chore/ci/test/style/build/refactor ship
# nothing. The first release of a product is 0.1.0 whatever the level.
set -euo pipefail

product="${1:?product: node, windows, android or macos}"
shift
force=auto
notes=false
case "${1:-}" in
  --force) force="${2:?--force needs auto|patch|minor|major}" ;;
  --notes) notes=true ;;
esac

case "$product" in
  node)
    prefix=node-v
    paths=(crates ':(exclude)crates/ursa-ffi' Cargo.toml Cargo.lock .cargo docker/node.Dockerfile charts/ursa-node)
    ;;
  windows)
    prefix=windows-v
    paths=(crates ':(exclude)crates/ursa-ffi' Cargo.toml Cargo.lock .cargo apps/windows)
    ;;
  android)
    prefix=android-v
    paths=(crates ':(exclude)crates/ursa-node' ':(exclude)crates/ursa-host' ':(exclude)crates/ursa-cli' Cargo.toml Cargo.lock .cargo apps/android)
    ;;
  macos)
    prefix=macos-v
    paths=(crates ':(exclude)crates/ursa-node' ':(exclude)crates/ursa-host' ':(exclude)crates/ursa-cli' Cargo.toml Cargo.lock .cargo apps/macos)
    ;;
  *)
    echo "unknown product: $product" >&2
    exit 2
    ;;
esac

last="$(git tag --list "${prefix}*" --sort=-v:refname | head -n 1)"
if [[ -n "$last" ]]; then
  base="${last#"$prefix"}"
  range="${last}..HEAD"
else
  base=""
  range="HEAD"
fi

mapfile -t commits < <(git rev-list --no-merges "$range" -- "${paths[@]}")

if [[ "$notes" == true ]]; then
  # The commits that ship, newest first, grouped the way the version bump reads them (a commit
  # without a Conventional Commits type ships as a patch, so it is listed too).
  if ((${#commits[@]} == 0)); then
    echo "No changes to this product since ${last:-the beginning}."
    exit 0
  fi
  scope='(\([^)]*\))?'
  re_breaking="^[a-zA-Z]+${scope}!:" re_feat="^feat${scope}:" re_fix="^(fix|perf|revert)${scope}:" re_typed="^[a-zA-Z]+${scope}:"
  declare -A groups=()
  for c in "${commits[@]}"; do
    subject="$(git log -1 --format=%s "$c")"
    if [[ "$subject" =~ $re_breaking ]] || git log -1 --format=%B "$c" | grep -Eq '^BREAKING[ -]CHANGE:'; then
      group=breaking
    elif [[ "$subject" =~ $re_feat ]]; then
      group=features
    elif [[ "$subject" =~ $re_fix ]]; then
      group=fixes
    elif [[ "$subject" =~ $re_typed ]]; then
      continue # housekeeping ships nothing
    else
      group=other
    fi
    groups[$group]+="- $(sed -E 's/^[a-zA-Z]+(\([^)]*\))?!?: *//' <<<"$subject") (${c:0:7})"$'\n'
  done
  for group in breaking features fixes other; do
    case "$group" in
      breaking) title="Breaking changes" ;; features) title=Features ;; fixes) title=Fixes ;; other) title="Other changes" ;;
    esac
    if [[ -n "${groups[$group]:-}" ]]; then printf '### %s\n\n%s\n' "$title" "${groups[$group]}"; fi
  done
  exit 0
fi

level=none
if ((${#commits[@]} > 0)); then
  subjects="$(git log --no-walk=unsorted --format='%s' "${commits[@]}")"
  bodies="$(git log --no-walk=unsorted --format='%B' "${commits[@]}")"
  housekeeping='^(docs|chore|ci|test|style|build|refactor)(\([^)]*\))?:'
  if grep -Evq "$housekeeping" <<<"$subjects"; then
    level="patch"
  fi
  if [[ "$level" != none ]] && grep -Eq '^feat(\([^)]*\))?:' <<<"$subjects"; then
    level=minor
  fi
  if grep -Eq '^[a-zA-Z]+(\([^)]*\))?!:' <<<"$subjects" || grep -Eq '^BREAKING[ -]CHANGE:' <<<"$bodies"; then
    level=major
  fi
fi
if [[ "$force" != auto ]]; then
  level="$force"
fi

if [[ "$level" == none ]]; then
  echo "release=false"
  echo "version=${base:-0.0.0}"
  echo "tag="
  echo "level=none"
  exit 0
fi

if [[ -z "$base" ]]; then
  version=0.1.0
else
  if [[ ! "$base" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
    echo "last tag $last is not ${prefix}MAJOR.MINOR.PATCH" >&2
    exit 2
  fi
  major="${BASH_REMATCH[1]}" minor="${BASH_REMATCH[2]}" patch="${BASH_REMATCH[3]}"
  case "$level" in
    major) version="$((major + 1)).0.0" ;;
    minor) version="${major}.$((minor + 1)).0" ;;
    patch) version="${major}.${minor}.$((patch + 1))" ;;
  esac
fi

echo "release=true"
echo "version=${version}"
echo "tag=${prefix}${version}"
echo "level=${level}"
