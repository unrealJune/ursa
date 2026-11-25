#!/usr/bin/env bash
# Builds the Rust core (crate ursa-ffi) for Android and generates its Kotlin bindings, into the
# app's native module, where Gradle picks both up:
#
#   modules/ursa-core/android/src/main/jniLibs/<abi>/libursa_ffi.so
#   modules/ursa-core/android/src/main/java/uniffi/ursa_ffi/ursa_ffi.kt
#
#   apps/android/scripts/build-core.sh [abi ...]      # default: arm64-v8a x86_64
#
# Needs the Android NDK (ANDROID_NDK_HOME), cargo-ndk, and the rustup targets for each ABI
# (aarch64-linux-android, x86_64-linux-android, armv7-linux-androideabi). Both outputs are
# git-ignored: they are always built from the crate, so the bindings cannot disagree with the
# library (UniFFI aborts at load when their checksums differ).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
main="$here/../modules/ursa-core/android/src/main"
abis=("$@")
if ((${#abis[@]} == 0)); then
  abis=(arm64-v8a x86_64)
fi

targets=()
for abi in "${abis[@]}"; do
  targets+=(-t "$abi")
done

cd "$root"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
rm -rf "$main/jniLibs"
# Not stripped here: the bindings are read from the library's UniFFI metadata symbols. Gradle
# strips the copy it packages.
cargo ndk "${targets[@]}" --platform 29 -o "$main/jniLibs" build --release --locked -p ursa-ffi

# The bindings come from the first ABI's library; every ABI has the same interface.
first="${abis[0]}"
case "$first" in
  arm64-v8a) triple=aarch64-linux-android ;;
  x86_64) triple=x86_64-linux-android ;;
  armeabi-v7a) triple=armv7-linux-androideabi ;;
  x86) triple=i686-linux-android ;;
  *) echo "unknown ABI $first" >&2; exit 2 ;;
esac
cargo build --locked -p ursa-ffi --features cli --bin uniffi-bindgen
bindgen="$target_dir/debug/uniffi-bindgen"
[[ -x "$bindgen" ]] || bindgen="$bindgen.exe"
rm -rf "$main/java/uniffi"
"$bindgen" generate --library "$target_dir/$triple/release/libursa_ffi.so" --language kotlin --no-format --out-dir "$main/java"

ls -la "$main/jniLibs"/* "$main/java/uniffi/ursa_ffi"
