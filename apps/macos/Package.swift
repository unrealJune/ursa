// swift-tools-version: 6.0
//
// Ursa for macOS: a SwiftUI app over the Rust core (crate ursa-ffi) through UniFFI.
//
// The core's static library and its Swift bindings are built, not committed (both are
// git-ignored, so they cannot disagree): run scripts/build-core.sh first, then `swift build`, or
// scripts/bundle.sh for Ursa.app.
import Foundation
import PackageDescription

let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path

let package = Package(
    name: "Ursa",
    platforms: [.macOS(.v14)],
    targets: [
        // The C side of the bindings: the header UniFFI writes and libursa_ffi.a.
        .systemLibrary(name: "ursa_ffiFFI", path: "Sources/ursa_ffiFFI"),
        // The Swift side UniFFI writes (Sources/UrsaFFI/ursa_ffi.swift).
        .target(
            name: "UrsaFFI",
            dependencies: ["ursa_ffiFFI"],
            linkerSettings: [
                .unsafeFlags(["-L\(here)/Core/lib"]),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreFoundation"),
                .linkedLibrary("resolv"),
            ]
        ),
        .executableTarget(
            name: "Ursa",
            dependencies: ["UrsaFFI"],
            path: "Sources/Ursa"
        ),
        // Unit tests, and an end-to-end run against a live node when URSA_TEST_CLAIM names one
        // (see Tests/UrsaTests/EndToEndTests.swift).
        .testTarget(name: "UrsaTests", dependencies: ["Ursa"]),
    ],
    swiftLanguageModes: [.v5]
)
