#!/usr/bin/env bash
# Builds Ursa.app (and Ursa-<version>-macos-<arch>.zip beside it) into apps/macos/dist:
#
#   apps/macos/scripts/bundle.sh [--version X.Y.Z] [arm64|x86_64|universal]   # default: this Mac's arch
#
# Builds the core first (scripts/build-core.sh) unless URSA_SKIP_CORE=1. Signs ad hoc unless
# URSA_SIGN_IDENTITY names a Developer ID identity, which also turns on the hardened runtime.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(cd "$here/.." && pwd)"

version="0.0.0-dev"
if [[ "${1:-}" == "--version" ]]; then
  version="${2:?--version needs X.Y.Z}"
  shift 2
fi
arch="${1:-$(uname -m)}"
case "$arch" in
  arm64 | aarch64) arch=arm64; swift_arch=(--arch arm64) ;;
  x86_64) swift_arch=(--arch x86_64) ;;
  universal) swift_arch=(--arch arm64 --arch x86_64) ;;
  *) echo "unknown arch $arch (arm64, x86_64 or universal)" >&2; exit 2 ;;
esac

if [[ "${URSA_SKIP_CORE:-}" != 1 ]]; then
  bash "$here/build-core.sh" "$arch"
fi

cd "$app"
swift build -c release "${swift_arch[@]}"
bin="$(swift build -c release "${swift_arch[@]}" --show-bin-path)/Ursa"

dist="$app/dist"
bundle="$dist/Ursa.app"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$bin" "$bundle/Contents/MacOS/Ursa"
build="$(git -C "$app" rev-list --count HEAD 2>/dev/null || echo 1)"
sed -e "s/\$(VERSION)/${version}/" -e "s/\$(BUILD)/${build}/" "$app/Resources/Info.plist" > "$bundle/Contents/Info.plist"
printf 'APPL????' > "$bundle/Contents/PkgInfo"

iconset="$(mktemp -d)/Ursa.iconset"
swift "$here/make-icon.swift" "$iconset"
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/Ursa.icns"
rm -rf "$(dirname "$iconset")"

entitlements="$app/Resources/Ursa.entitlements"
if [[ -n "${URSA_SIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --entitlements "$entitlements" --sign "$URSA_SIGN_IDENTITY" "$bundle"
else
  codesign --force --entitlements "$entitlements" --sign - "$bundle"
fi
codesign --verify --strict "$bundle"

zip="$dist/Ursa-${version}-macos-${arch}.zip"
rm -f "$zip"
ditto -c -k --keepParent "$bundle" "$zip"
echo "$bundle"
echo "$zip"
