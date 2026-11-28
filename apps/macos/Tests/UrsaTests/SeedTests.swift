import AppKit
import Foundation
import Testing
@testable import Ursa

/// Development only: fills a fresh account on a live node with sample photos and folders, and
/// leaves it signed in, writing its root to `$URSA_DATA_DIR/seed-key.txt`, so a debug build can
/// open it (URSA_ACCOUNT_KEY) to look at the screens. Runs only when URSA_TEST_SEED is set.
@Suite(.enabled(if: ProcessInfo.processInfo.environment["URSA_TEST_SEED"] != nil))
@MainActor
struct Seed {
    @Test func seedLibrary() async throws {
        let env = ProcessInfo.processInfo.environment
        let claim = try #require(env["URSA_TEST_CLAIM"])
        let dataDir = try #require(env["URSA_DATA_DIR"])
        var settings = LocalStore.Settings()
        settings.relays = []
        LocalStore.saveSettings(settings)
        defer { LocalStore.forgetAccountKey() }

        let model = AppModel()
        await model.start()
        try await model.createAccount(name: "June")
        try await model.claimNode(link: claim, label: "Home server")
        try LocalStore.loadAccountKey()!.write(toFile: dataDir + "/seed-key.txt", atomically: true, encoding: .utf8)

        let src = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: src, withIntermediateDirectories: true)
        let sizes = [(1600, 1200), (1200, 1600), (1600, 900), (1200, 1200), (2000, 1000), (900, 1600)]
        var photos: [URL] = []
        for i in 0..<34 {
            let (w, h) = sizes[i % sizes.count]
            let url = src.appendingPathComponent(String(format: "IMG_%04d.png", 1000 + i))
            try picture(w / 4, h / 4, hue: Double(i) / 34).write(to: url)
            // Spread over three months, newest first.
            let date = Date().addingTimeInterval(-Double(i) * 3 * 86_400)
            try FileManager.default.setAttributes([.creationDate: date, .modificationDate: date], ofItemAtPath: url.path)
            photos.append(url)
        }
        try await model.upload(photos, into: nil)

        let trips = src.appendingPathComponent("Trips")
        let iceland = trips.appendingPathComponent("Iceland")
        try FileManager.default.createDirectory(at: iceland, withIntermediateDirectories: true)
        for i in 0..<4 { try picture(400, 300, hue: 0.55 + Double(i) / 40).write(to: iceland.appendingPathComponent("glacier-\(i + 1).png")) }
        try Data(repeating: 1, count: 2_400_000).write(to: trips.appendingPathComponent("itinerary.pdf"))
        let docs = src.appendingPathComponent("Documents")
        try FileManager.default.createDirectory(at: docs, withIntermediateDirectories: true)
        try Data("Ursa notes".utf8).write(to: docs.appendingPathComponent("notes.txt"))
        try Data(repeating: 2, count: 800_000).write(to: docs.appendingPathComponent("taxes-2025.xlsx"))
        try await model.upload([trips, docs], into: nil)
        #expect(model.error == nil)
        let expected = 41
        #expect(model.items.count == expected)
    }

    private func picture(_ w: Int, _ h: Int, hue: Double) -> Data {
        let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4,
                                   hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        let rect = NSRect(x: 0, y: 0, width: w, height: h)
        NSGradient(starting: NSColor(hue: hue, saturation: 0.55, brightness: 0.95, alpha: 1),
                   ending: NSColor(hue: hue + 0.08, saturation: 0.75, brightness: 0.55, alpha: 1))!.draw(in: rect, angle: -70)
        NSColor.white.withAlphaComponent(0.5).setFill()
        NSBezierPath(ovalIn: NSRect(x: w / 2, y: h / 2, width: w / 5, height: w / 5)).fill()
        NSGraphicsContext.restoreGraphicsState()
        return rep.representation(using: .png, properties: [:])!
    }
}
