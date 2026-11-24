# Ursa for Android

Expo SDK 58 + UniFFI. The Rust core is crate `ursa-ffi` (`crates/ursa-ffi`): `ursa-client`, the
same device code the Windows host and the CLI run, behind a typed UniFFI interface. The local Expo
module `modules/ursa-core` wraps it in Kotlin and adds what only Android can do.

## Build

The core's library and Kotlin bindings are built, not committed (both are git-ignored, so they
cannot disagree):

```sh
# Android NDK (ANDROID_NDK_HOME), cargo-ndk, and the rustup targets
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
bash apps/android/scripts/build-core.sh            # arm64-v8a and x86_64
```

Then, from `apps/android`:

```sh
bun install
bunx expo run:android          # a dev build on a phone or emulator
bunx tsc --noEmit && bunx eslint src
```

CI (`.github/workflows/release-android.yml`) does all of it and keeps the APK as an artifact;
a push to main with a `feat`/`fix` touching the app or the core releases `android-vX.Y.Z`.

## How it is put together

- `src/core/store.tsx`: the app's state. Starts the core with the stored root, decides the phase
  (onboarding, offline, ready), keeps the library list and the upload queue, and runs camera-roll
  backup while the app is open.
- `src/core/settings.ts`: the account root and the relay token live in the Android Keystore
  through expo-secure-store; the relays default to the ones the Windows app uses.
- `src/ui/kit.tsx`: Material 3 pieces. Colours come from the wallpaper (Material You,
  `useMaterialColors`), the switches and progress indicators are Jetpack Compose (`@expo/ui`),
  the tab bar is Android's own (`NativeTabs`).
- `modules/ursa-core`: `UrsaCoreModule.kt` (the Expo module), `index.ts` (its typed API).
- `scripts/make-icons.py` draws the launcher icons from the Windows logo's constellation.
