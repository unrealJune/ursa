import Foundation

/// What `storage_json` reports (`ursa_client::storage::StorageReport`): everything the Pool and
/// Usage screens show. The Windows and Android apps read the same shape.
struct StorageReport: Decodable {
    struct Shape: Decodable, Hashable { var k: Int; var n: Int }
    struct Kinds: Decodable { var photos: Int64; var videos: Int64; var files: Int64 }

    /// One protection choice: any `k` of `n` pieces rebuild a file.
    struct Choice: Decodable, Identifiable {
        var k: Int
        var n: Int
        var survives: Int
        var limitBytes: Int64
        var offlineHours: Double?
        var available: Bool
        var recommended: Bool
        var switchExtraBytes: Int64 = 0
        var switchFits: Bool = false

        var id: Shape { Shape(k: k, n: n) }
        var name: String { k == 1 ? (n == 1 ? "1 copy" : "\(n) copies") : "\(Self.ratio(Double(n) / Double(k)))×" }
        var shape: String { k == 1 ? "" : "\(k) of \(n)" }
        func `is`(_ s: Shape) -> Bool { k == s.k && n == s.n }

        var offlineText: String {
            guard available, let h = offlineHours else { return "—" }
            if h < 1.0 / 60 { return "< 1 min/yr" }
            if h < 1 { return "\(Int((h * 60).rounded())) min/yr" }
            return String(format: "%.1f h/yr", h)
        }

        static func ratio(_ x: Double) -> String {
            let s = String(format: "%.2f", x)
            return s.replacingOccurrences(of: #"\.?0+$"#, with: "", options: .regularExpression)
        }

        enum CodingKeys: String, CodingKey {
            case k, n, survives, limitBytes, offlineHours, available, recommended, switchExtraBytes, switchFits
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            k = try c.decode(Int.self, forKey: .k)
            n = try c.decode(Int.self, forKey: .n)
            survives = try c.decode(Int.self, forKey: .survives)
            limitBytes = try c.decode(Int64.self, forKey: .limitBytes)
            offlineHours = try c.decodeIfPresent(Double.self, forKey: .offlineHours)
            available = try c.decode(Bool.self, forKey: .available)
            recommended = try c.decode(Bool.self, forKey: .recommended)
            switchExtraBytes = try c.decodeIfPresent(Int64.self, forKey: .switchExtraBytes) ?? 0
            switchFits = try c.decodeIfPresent(Bool.self, forKey: .switchFits) ?? false
        }
    }

    /// A person in the pool (you, or a friend), as one failure domain.
    struct Member: Decodable, Identifiable {
        var name: String
        var own: Bool
        var nodes: Int
        var reachable: Bool
        /// `up_30d`: snake-case conversion capitalises the "d" after the digits.
        var up30D: Double?
        var usedBytes: Int64
        var givesYou: Int64?
        var up30d: Double? { up30D }
        var id: String { own ? "\u{0}you" : name }
        var upText: String { up30d.map { String(format: "%.1f%% up", $0 * 100).replacingOccurrences(of: ".0%", with: "%") } ?? "—" }
    }

    struct Hosted: Decodable, Identifiable {
        var name: String
        var account: String
        var usedBytes: Int64
        var quotaBytes: Int64
        var id: String { account }
    }

    struct Disk: Decodable {
        var totalBytes: Int64
        var freeBytes: Int64
        var yoursBytes: Int64
        var friendsBytes: Int64
        var reservedBytes: Int64
    }

    struct Month: Decodable, Identifiable {
        /// `YYYY-MM`, UTC.
        var month: String
        var bytes: Int64
        var id: String { month }
        var date: Date {
            let f = DateFormatter()
            f.dateFormat = "yyyy-MM"
            f.timeZone = TimeZone(identifier: "UTC")
            return f.date(from: month) ?? Date()
        }
    }

    var protection: Shape
    var chosen: Bool
    var files: Int
    var pendingFiles: Int
    var leftoverPieces: Int?
    var storedBytes: Int64
    var kinds: Kinds
    var limitBytes: Int64
    var choices: [Choice]
    var giveBytes: Int64
    var homeBytes: Int64
    var friendsBytes: Int64
    var members: [Member]
    var hosting: [Hosted]
    var disk: Disk?
    var growth: [Month]

    var current: Choice? { choices.first { $0.is(protection) } ?? choices.first }

    static func decode(_ json: String) throws -> StorageReport {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return try d.decode(StorageReport.self, from: Data(json.utf8))
    }
}

/// Sizes as people read them: decimal units, like disks are sold and as Finder shows them.
func sizeText(_ bytes: Int64) -> String {
    if bytes >= 1_000_000_000_000_000 { return "∞" }
    return byteFormatter.string(fromByteCount: bytes)
}

private let byteFormatter: ByteCountFormatter = {
    let f = ByteCountFormatter()
    f.countStyle = .file
    f.allowsNonnumericFormatting = false // "0 KB", not "Zero KB"
    return f
}()

func sizeText(_ bytes: UInt64) -> String { sizeText(Int64(clamping: bytes)) }
