import Foundation
@_exported import UrsaFFI

/// A file of the library. (SwiftUI has a `LibraryItem` of its own.)
typealias LibraryFile = UrsaFFI.LibraryItem

// The core calls these from its own threads; each hands what it hears to the main actor.

final class Progress: ProgressListener, @unchecked Sendable {
    private let report: @MainActor (UInt64, UInt64) -> Void
    init(_ report: @escaping @MainActor (UInt64, UInt64) -> Void) { self.report = report }
    func onProgress(done: UInt64, total: UInt64) {
        Task { @MainActor in report(done, total) }
    }
}

final class JoinWatcher: JoinListener, @unchecked Sendable {
    private let confirm: @MainActor (String) -> Void
    init(_ confirm: @escaping @MainActor (String) -> Void) { self.confirm = confirm }
    func onConfirm(code: String) {
        Task { @MainActor in confirm(code) }
    }
}

final class OfferWatcher: OfferListener, @unchecked Sendable {
    private let code: @MainActor (String) -> Void
    private let request: @MainActor (String, String, String) -> Void
    init(code: @escaping @MainActor (String) -> Void, request: @escaping @MainActor (String, String, String) -> Void) {
        self.code = code
        self.request = request
    }
    func onCode(code: String) {
        Task { @MainActor in self.code(code) }
    }
    func onRequest(name: String, platform: String, confirm: String) {
        Task { @MainActor in request(name, platform, confirm) }
    }
}

/// What a failure says, without the enum's spelling around it.
func describe(_ error: Error) -> String {
    if let e = error as? UrsaError, case .Failed(let message) = e { return message }
    return error.localizedDescription
}

/// "123456" as "123 456", as both screens show a confirmation code.
func spaced(_ code: String) -> String {
    guard code.count == 6 else { return code }
    return "\(code.prefix(3)) \(code.suffix(3))"
}

/// One change to the library tree, as `ursa_client::Edit` reads it.
enum Edit {
    case createFolder(id: String? = nil, parent: String?, name: String)
    case renameFolder(id: String, name: String)
    case moveFolder(id: String, parent: String?)
    case deleteFolder(id: String)
    case renameFile(fileId: String, name: String)
    case moveFile(fileId: String, dir: String?)
    case deleteFile(fileId: String)

    var json: [String: Any] {
        func opt(_ s: String?) -> Any { s ?? NSNull() }
        switch self {
        case let .createFolder(id, parent, name):
            var d: [String: Any] = ["op": "create_folder", "parent": opt(parent), "name": name]
            if let id { d["id"] = id }
            return d
        case let .renameFolder(id, name): return ["op": "rename_folder", "id": id, "name": name]
        case let .moveFolder(id, parent): return ["op": "move_folder", "id": id, "parent": opt(parent)]
        case let .deleteFolder(id): return ["op": "delete_folder", "id": id]
        case let .renameFile(fileId, name): return ["op": "rename_file", "file_id": fileId, "name": name]
        case let .moveFile(fileId, dir): return ["op": "move_file", "file_id": fileId, "dir": opt(dir)]
        case let .deleteFile(fileId): return ["op": "delete_file", "file_id": fileId]
        }
    }

    static func encode(_ edits: [Edit]) -> String {
        let data = try! JSONSerialization.data(withJSONObject: edits.map(\.json))
        return String(decoding: data, as: UTF8.self)
    }

    /// A folder id made here, so a whole tree can be made in one change.
    static func newFolderId() -> String { UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased() }
}
