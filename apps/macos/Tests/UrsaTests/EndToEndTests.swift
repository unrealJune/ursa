import AppKit
import Foundation
import Testing
@testable import Ursa

/// The app's model against a live node: create an account, claim the node, upload files and a
/// folder, read them back, edit the tree, and delete. Runs only when URSA_TEST_CLAIM holds an
/// unclaimed node's `ursa-claim:` link; URSA_DATA_DIR must name an empty directory, which keeps
/// this run's identity and keychain entries apart from the real app's.
///
///   URSA_DATA_DIR=$(mktemp -d) URSA_TEST_CLAIM='ursa-claim:…' swift test --filter EndToEnd
@Suite(.enabled(if: ProcessInfo.processInfo.environment["URSA_TEST_CLAIM"] != nil
                    && ProcessInfo.processInfo.environment["URSA_DATA_DIR"] != nil))
@MainActor
struct EndToEnd {
    @Test(.timeLimit(.minutes(5))) func libraryRoundTrip() async throws {
        let env = ProcessInfo.processInfo.environment
        let claim = try #require(env["URSA_TEST_CLAIM"])
        // n0's public relays (an empty list), which take no token, unless the run names some.
        var settings = LocalStore.Settings()
        settings.relays = env["URSA_TEST_RELAYS"].map { $0.split(separator: ",").map(String.init) } ?? []
        LocalStore.saveSettings(settings)
        defer { LocalStore.forgetAccountKey() }

        let model = AppModel()
        await model.start()
        #expect(model.phase == .onboarding)

        try await model.createAccount(name: "Test")
        #expect(model.hasAccount)
        #expect(model.needsNode)
        #expect(LocalStore.loadAccountKey()?.count == 64)
        let words = try await model.recoveryPhrase()
        #expect(words.split(separator: " ").count == 24)

        try await model.claimNode(link: claim, label: "Test node")
        #expect(model.phase == .ready)
        #expect(model.connected)

        // A photo, a text file, and a folder with a file and a hidden file in it.
        let src = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent(UUID().uuidString)
        let trip = src.appendingPathComponent("Trip")
        try FileManager.default.createDirectory(at: trip, withIntermediateDirectories: true)
        let photo = src.appendingPathComponent("photo.png")
        try png(width: 300, height: 200).write(to: photo)
        let note = src.appendingPathComponent("note.txt")
        try Data("hello ursa".utf8).write(to: note)
        try Data(repeating: 7, count: 300_000).write(to: trip.appendingPathComponent("big.bin"))
        try Data("x".utf8).write(to: trip.appendingPathComponent(".DS_Store"))

        try await model.upload([photo, note, trip], into: nil)
        #expect(model.error == nil)
        #expect(model.folders.map(\.name) == ["Trip"])
        #expect(Set(model.items.map(\.name)) == ["photo.png", "note.txt", "big.bin"])
        let p = try #require(model.items.first { $0.name == "photo.png" })
        #expect(p.mime == "image/png")
        #expect(p.width == 300 && p.height == 200)
        #expect(p.hasThumb)
        #expect(await Thumbs.shared.image(p.fileId, model: model) != nil)
        let big = try #require(model.items.first { $0.name == "big.bin" })
        #expect(big.dir == model.folders[0].id)
        #expect(!model.transferring)

        // Back again: one file, the original cache, and the folder.
        let out = src.appendingPathComponent("out")
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let n = try #require(model.items.first { $0.name == "note.txt" })
        try await model.download(n, to: out.appendingPathComponent("note.txt"))
        #expect(try String(contentsOf: out.appendingPathComponent("note.txt"), encoding: .utf8) == "hello ursa")
        let cached = try await model.original(p)
        #expect(try Data(contentsOf: cached) == Data(contentsOf: photo))
        let folder = try await model.downloadFolder(model.folders[0].id, into: out)
        #expect(folder.lastPathComponent == "Trip")
        #expect(try Data(contentsOf: folder.appendingPathComponent("big.bin")).count == 300_000)

        // The tree: rename, move, delete.
        try await model.edit([.renameFile(fileId: n.fileId, name: "notes.txt"), .moveFile(fileId: n.fileId, dir: model.folders[0].id)])
        let moved = try #require(model.items.first { $0.fileId == n.fileId })
        #expect(moved.name == "notes.txt")
        #expect(moved.dir == model.folders[0].id)
        #expect(model.freeName(in: nil, "Trip", isFolder: true) == "Trip 2")

        let report = try await model.storage()
        #expect(report.files == 3)
        #expect(report.members.contains { $0.own })

        try await model.delete([p])
        #expect(!model.items.contains { $0.fileId == p.fileId })
        try await model.edit([.deleteFolder(id: model.folders[0].id)])
        #expect(model.items.isEmpty)
        #expect(model.folders.isEmpty)

        await model.signOut()
        #expect(model.phase == .onboarding)
        #expect(LocalStore.loadAccountKey() == nil)
        try? FileManager.default.removeItem(at: src)
    }

    private func png(width: Int, height: Int) -> Data {
        let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
                                   samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                   bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        NSColor.systemTeal.setFill()
        NSRect(x: 0, y: 0, width: width, height: height).fill()
        NSGraphicsContext.restoreGraphicsState()
        return rep.representation(using: .png, properties: [:])!
    }
}
