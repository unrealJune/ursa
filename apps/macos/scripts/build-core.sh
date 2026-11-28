#!/usr/bin/env bash
# Builds the Rust core (crate ursa-ffi) for macOS and generates its Swift bindings, where the
# Swift package picks both up:
#
#   Core/lib/libursa_ffi.a                     the core, static (one per arch, or universal)
#   Sources/ursa_ffiFFI/ursa_ffiFFI.h          the C half of the bindings
#   Sources/UrsaFFI/ursa_ffi.swift             the Swift half
#
#   apps/macos/scripts/build-core.sh [--debug] [arm64|x86_64|universal]   # default: this Mac's arch
#
# All three are git-ignored: they are always built from the crate, so the bindings cannot
# disagree with the library (UniFFI checks their checksums when the app starts).
# URSA_DEFAULT_RELAY_TOKEN, when set, is baked into the core (crates/ursa-proto/src/relays.rs).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(cd "$here/.." && pwd)"
root="$(cd "$app/../.." && pwd)"

profile=release
if [[ "${1:-}" == "--debug" ]]; then
  profile=debug
  shift
fi
arch="${1:-$(uname -m)}"
case "$arch" in
  arm64 | aarch64) triples=(aarch64-apple-darwin) ;;
  x86_64) triples=(x86_64-apple-darwin) ;;
  universal) triples=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  *) echo "unknown arch $arch (arm64, x86_64 or universal)" >&2; exit 2 ;;
esac

# The app's minimum, so the linker does not warn about objects built for a newer macOS.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"
cd "$root"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
release_flag=()
[[ "$profile" == release ]] && release_flag=(--release)

libs=()
for triple in "${triples[@]}"; do
  rustup target add "$triple" >/dev/null 2>&1 || true
  # A static library, linked into the app: nothing to ship beside it. The crate itself is a
  # cdylib for Android, so the crate type is chosen here, not in its Cargo.toml.
  cargo rustc "${release_flag[@]}" --locked -p ursa-ffi --lib --target "$triple" --crate-type staticlib
  libs+=("$target_dir/$triple/$profile/libursa_ffi.a")
done

mkdir -p "$app/Core/lib"
rm -f "$app/Core/lib/libursa_ffi.a"
if ((${#libs[@]} > 1)); then
  lipo -create "${libs[@]}" -output "$app/Core/lib/libursa_ffi.a"
else
  cp "${libs[0]}" "$app/Core/lib/libursa_ffi.a"
fi

# The bindings, from the library's UniFFI metadata; every arch has the same interface.
cargo build --locked -p ursa-ffi --features cli --bin uniffi-bindgen
gen="$(mktemp -d)"
trap 'rm -rf "$gen"' EXIT
"$target_dir/debug/uniffi-bindgen" generate --library "${libs[0]}" --language swift --out-dir "$gen"
cp "$gen/ursa_ffiFFI.h" "$app/Sources/ursa_ffiFFI/ursa_ffiFFI.h"
cp "$gen/ursa_ffi.swift" "$app/Sources/UrsaFFI/ursa_ffi.swift"

ls -la "$app/Core/lib" "$app/Sources/ursa_ffiFFI" "$app/Sources/UrsaFFI"
