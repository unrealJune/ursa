# Ursa for macOS

A SwiftUI app (macOS 14 or later) over the Rust core: crate `ursa-ffi`
([`crates/ursa-ffi`](../../crates/ursa-ffi)), the same `ursa-client` the Windows host, the
Android app and the CLI run, behind UniFFI's Swift bindings and linked in statically. The app keeps
the account root and the relay token in the login keychain and hands the root to the core, which
holds it in memory only.

## Build

You need Xcode (or its command line tools with the macOS SDK) and stable Rust. From the
repository root:

```sh
bash apps/macos/scripts/build-core.sh          # the core and its Swift bindings (git-ignored)
cd apps/macos
swift build && swift run                       # a debug build, run from the terminal
swift test                                     # unit tests
bash scripts/bundle.sh                         # dist/Ursa.app and a zip of it
```

`build-core.sh` and `bundle.sh` take `arm64`, `x86_64` or `universal` (default: this Mac's).
With only the command line tools selected, prefix commands with
`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`. Set `URSA_DEFAULT_RELAY_TOKEN` when
building the core to bake in the token for Ursa's own relays, as release builds do.

`URSA_DATA_DIR` runs a separate copy (its own device identity, settings and keychain entries),
for development beside the real one. Data lives in `~/Library/Application Support/Ursa`
otherwise.

### End-to-end test

`Tests/UrsaTests/EndToEndTests.swift` drives the app's model against a live node: account,
claim, uploads (files and folders), thumbnails, downloads, edits, deletion, sign-out. Start a
node on n0's public relays, which need no token, and pass its claim link:

```sh
URSA_STORE=disk URSA_DATA_DIR=/tmp/node URSA_CLAIM_CODE=a-claim-code-of-16+ cargo run -p ursa-node
# it prints ursa-claim:<URSA_CLAIM_CODE>@endpoint…; put the code in
URSA_DATA_DIR=$(mktemp -d) URSA_TEST_CLAIM='ursa-claim:a-claim-code-of-16+@endpoint…' swift test --filter EndToEnd
```

Debug builds also take `URSA_ACCOUNT_KEY` (open an account without its keychain entry) and
`URSA_SNAPSHOT_DIR` (render each screen to a PNG, then quit); see `Sources/Ursa/Snapshots.swift`.
`Tests/UrsaTests/SeedTests.swift` fills an account with sample photos for that.

## Releases

[`release-macos.yml`](../../.github/workflows/release-macos.yml) builds a universal `Ursa.app`
and publishes `Ursa-X.Y.Z-macos-universal.zip` on a `macos-vX.Y.Z` GitHub release when the
Conventional Commits since the last one ship something. The app is signed ad hoc until there is a
Developer ID (`URSA_SIGN_IDENTITY` in `bundle.sh` signs with one): Gatekeeper asks to confirm the
first launch, and the keychain asks once after each update before Ursa can read its key again.

## How it is put together

- `Sources/Ursa/Core/AppModel.swift`: the app's state (`@Observable`, on the main actor): the
  phase (starting, onboarding, offline, ready), the account and pool, the library, transfers and
  protection switches. The counterpart of the Windows app's `Services/Library.cs`.
- `Sources/Ursa/Core/Bridge.swift`: the core's callbacks (progress, link codes) handed to the main
  actor, and the library edits as `ursa_client::Edit` reads them.
- `Sources/Ursa/Core/MediaInfo.swift`: what an upload carries besides the file: type, the date a
  photo was taken (EXIF) or a video made, pixel size, and a 640 px JPEG thumbnail, from ImageIO
  and AVFoundation (HEIC and RAW included).
- `Sources/Ursa/Views/`: one view per screen. Photos is a justified wall of photos and videos by
  month, laid out whole from their aspect ratios; Files is the folder tree as a table (Quick Look
  with Space); Pool and Usage read the same storage report as the other apps; Account holds
  devices and linking, recovery, nodes and friends; Settings (⌘,) holds this Mac and the relays.
- Files dropped on the window, on the Dock icon, or opened with Ursa are uploaded.
- `scripts/make-icon.swift` draws the icon from the Windows logo's constellation.
