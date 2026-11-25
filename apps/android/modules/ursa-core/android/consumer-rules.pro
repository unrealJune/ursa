# Applied to the app's R8 pass (on for release builds since Expo SDK 58). Everything here is
# reached by name from native code or reflection, where R8 cannot see it; a missing rule does not
# fail the build, it ships an APK that dies the first time Rust calls back.

# JNA binds the UniFFI library interface by method name and reads `Structure` fields reflectively.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-dontwarn java.awt.**

# UniFFI's generated bindings: RustBuffer and friends are JNA Structures, the FFI functions are
# looked up by symbol name, and callback vtables are invoked through JNA proxies.
-keep class uniffi.ursa_ffi.** { *; }
