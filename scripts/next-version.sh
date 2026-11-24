#!/usr/bin/env bash
# The next release of a product, from the Conventional Commits that touched its paths since its
# last tag. Prints `release=true|false`, `version=X.Y.Z`, `tag=<prefix>X.Y.Z`, `level=…`.
#
#   scripts/next-version.sh node|windows [--force auto|patch|minor|major]
#
# feat → minor, any other type that ships (fix, perf, revert, or no type) → patch,
# `type!:` or a BREAKING CHANGE footer → major. docs/chore/ci/test/style/build/refactor ship
# nothing. The first release of a product is 0.1.0 whatever the level.
set -euo pipefail

product="${1:?product: node or windows}"
shift
force=auto
if [[ "${1:-}" == "--force" ]]; then
  force="${2:?--force needs auto|patch|minor|major}"
fi

case "$product" in
  node)
    prefix=node-v
    paths=(crates Cargo.toml Cargo.lock .cargo docker/node.Dockerfile charts/ursa-node)
    ;;
  windows)
    prefix=windows-v
    paths=(crates Cargo.toml Cargo.lock .cargo apps/windows)
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

level=none
if ((${#commits[@]} > 0)); then
  subjects="$(git log --no-walk=unsorted --format='%s' "${commits[@]}")"
  bodies="$(git log --no-walk=unsorted --format='%B' "${commits[@]}")"
  housekeeping='^(docs|chore|ci|test|style|build|refactor)(\([^)]*\))?:'
  if grep -Evq "$housekeeping" <<<"$subjects"; then
    level=patch
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
