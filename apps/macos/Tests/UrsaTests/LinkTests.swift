import Foundation
import Testing
@testable import Ursa

/// Two devices on this Mac, each with its own identity: one creates an account and offers a link
/// code, the other joins with it, as a phone offering and this Mac joining do. Over the relays
/// in URSA_TEST_RELAYS (comma-separated; empty: n0's public ones). Runs only when URSA_TEST_LINK
/// is set, with URSA_TEST_CLAIM an unclaimed node's claim link on those relays.
@Suite(.enabled(if: ProcessInfo.processInfo.environment["URSA_TEST_LINK"] != nil))
struct LinkRoundTrip {
    final class Box: @unchecked Sendable {
        var code: String?
        var request: (name: String, platform: String, confirm: String)?
        var joinerConfirm: String?
    }

    @Test(.timeLimit(.minutes(2))) func offerAndJoin() async throws {
        let env = ProcessInfo.processInfo.environment
        let relays = env["URSA_TEST_RELAYS"].map { $0.split(separator: ",").map(String.init) } ?? []
        let tmp = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent(UUID().uuidString)
        let box = Box()

        let offerer = UrsaCore()
        _ = try await offerer.`init`(dataDir: tmp.appendingPathComponent("a").path, accountKey: nil, relays: relays,
                                     relayToken: nil, deviceName: "Phone", platform: "android")
        let keys = try await offerer.createAccount(name: "Link test")
        // The joiner opens the account from its nodes, so it needs one (URSA_TEST_CLAIM).
        if let claim = env["URSA_TEST_CLAIM"] { _ = try await offerer.claimNode(link: claim, label: "Test node") }

        let offered = Task {
            try await offerer.linkOffer(listener: OfferWatcher(
                code: { box.code = $0 },
                request: { name, platform, confirm in
                    box.request = (name, platform, confirm)
                    Task { try await offerer.linkDecide(accept: true) }
                }))
        }
        while box.code == nil { try await Task.sleep(for: .milliseconds(50)) }

        let joiner = UrsaCore()
        _ = try await joiner.`init`(dataDir: tmp.appendingPathComponent("b").path, accountKey: nil, relays: relays,
                                    relayToken: nil, deviceName: "Mac", platform: "macos")
        let joined = try await joiner.linkJoin(code: box.code!, listener: JoinWatcher { box.joinerConfirm = $0 })
        let linkedName = try await offered.value

        #expect(joined.accountId == keys.accountId)
        #expect(joined.accountKey == keys.accountKey)
        #expect(linkedName == "Mac")
        #expect(box.request?.platform == "macos")
        // Both screens showed the same code.
        #expect(box.request?.confirm != nil && box.request?.confirm == box.joinerConfirm)
        try? FileManager.default.removeItem(at: tmp)
    }
}
