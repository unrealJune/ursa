import Foundation
import Security
import UrsaFFI

/// The app's own files under ~/Library/Application Support/Ursa, and its secrets in the login
/// keychain. `URSA_DATA_DIR` runs a second, separate copy while developing.
enum LocalStore {
    static let root: URL = {
        if let dir = ProcessInfo.processInfo.environment["URSA_DATA_DIR"], !dir.isEmpty {
            return URL(fileURLWithPath: dir, isDirectory: true)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("Ursa", isDirectory: true)
    }()

    /// The core's own directory: this device's identity and its caches.
    static var coreDataDir: URL { dir(root.appendingPathComponent("core", isDirectory: true)) }
    /// Originals fetched to view or open, kept to open again.
    static var viewCache: URL { dir(root.appendingPathComponent("view", isDirectory: true)) }
    /// Thumbnails made for an upload, deleted once it is done.
    static var thumbScratch: URL { dir(root.appendingPathComponent("thumbs-out", isDirectory: true)) }

    @discardableResult
    static func dir(_ url: URL) -> URL {
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    // -----------------------------------------------------------------------------------------
    // Secrets: the account root and the relay token, in the keychain, never on disk in the clear.

    /// A separate data directory keeps separate secrets, so a development copy never signs the
    /// real one out.
    private static var service: String {
        ProcessInfo.processInfo.environment["URSA_DATA_DIR"].map { "Ursa (\($0))" } ?? "Ursa"
    }

    static func loadAccountKey() -> String? {
        #if DEBUG
        // Development only: open a test account without its keychain entry.
        if let key = ProcessInfo.processInfo.environment["URSA_ACCOUNT_KEY"], !key.isEmpty { return key }
        #endif
        return Keychain.read(service: service, account: "account-key")
    }
    static func saveAccountKey(_ hex: String) { Keychain.write(hex, service: service, account: "account-key") }
    /// Signing out of this Mac: the library stays on the nodes; only this copy of the key goes.
    static func forgetAccountKey() { Keychain.delete(service: service, account: "account-key") }

    static func loadRelayToken() -> String? { Keychain.read(service: service, account: "relay-token") }
    static func saveRelayToken(_ token: String?) {
        if let t = token?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty {
            Keychain.write(t, service: service, account: "relay-token")
        } else {
            Keychain.delete(service: service, account: "relay-token")
        }
    }

    // -----------------------------------------------------------------------------------------
    // Settings

    /// Ursa's own relays (crates/ursa-proto/src/relays.rs), as every app starts with.
    static let defaultRelays = ["https://iroh-euw.streetcrypt.id", "https://iroh-wus.streetcrypt.id"]

    struct Settings: Codable {
        var relays: [String] = LocalStore.defaultRelays
    }

    /// Ursa's own relays refuse a device without their token. This build may carry it (release
    /// builds do), or the user may have saved one; otherwise nothing behind the relays answers.
    static var relayTokenMissing: Bool {
        func norm(_ u: String) -> String { u.trimmingCharacters(in: .whitespaces).lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "/")) }
        let relays = loadSettings().relays.map(norm).filter { !$0.isEmpty }
        let ours = Set(defaultRelays.map(norm))
        return !relays.isEmpty && relays.allSatisfy(ours.contains) && !hasBuiltInRelayToken() && loadRelayToken() == nil
    }

    private static var settingsURL: URL { root.appendingPathComponent("settings.json") }

    static func loadSettings() -> Settings {
        guard let data = try? Data(contentsOf: settingsURL),
              let s = try? JSONDecoder().decode(Settings.self, from: data) else { return Settings() }
        return s
    }

    static func saveSettings(_ s: Settings) {
        dir(root)
        let enc = JSONEncoder()
        enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        try? enc.encode(s).write(to: settingsURL, options: .atomic)
    }
}

/// Generic passwords in the login keychain, readable by this user only.
enum Keychain {
    private static func query(_ service: String, _ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    static func read(service: String, account: String) -> String? {
        var q = query(service, account)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func write(_ value: String, service: String, account: String) {
        let data = Data(value.utf8)
        let q = query(service, account)
        let update: [String: Any] = [kSecValueData as String: data]
        if SecItemUpdate(q as CFDictionary, update as CFDictionary) == errSecItemNotFound {
            var add = q
            add[kSecValueData as String] = data
            add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            add[kSecAttrLabel as String] = "Ursa \(account)"
            SecItemAdd(add as CFDictionary, nil)
        }
    }

    static func delete(service: String, account: String) {
        SecItemDelete(query(service, account) as CFDictionary)
    }
}
