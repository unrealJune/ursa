import AppKit
import Foundation
import Observation
import UrsaFFI

/// The app's state: the core, the account and its pool, the library, and what is being
/// transferred. Views read it; every change happens on the main actor.
@MainActor
@Observable
final class AppModel {
    enum Phase: Equatable {
        case starting
        /// The core itself would not start.
        case failed(String)
        /// An account is stored on this Mac but none of its nodes answered.
        case offline(String)
        /// No account yet, or one without a node of its own.
        case onboarding
        case ready
    }

    let core = UrsaCore()

    var phase: Phase = .starting

    // Account
    private(set) var deviceId = ""
    private(set) var hasAccount = false
    private(set) var accountId = ""
    private(set) var accountName = ""
    private(set) var devices: [DeviceInfo] = []
    private(set) var homeNodes: [NodeEntry] = []
    private(set) var friends: [FriendInfo] = []
    /// The account has no node of its own yet: onboarding requires one.
    var needsNode: Bool { hasAccount && homeNodes.isEmpty }

    // Pool
    private(set) var pool: [PoolNode] = []
    /// At least one node that takes this account's data answered.
    private(set) var connected = false
    private(set) var statusText = "Starting"
    private(set) var detailText = ""

    /// Ursa's relays will refuse this device: see `LocalStore.relayTokenMissing`.
    private(set) var relayTokenMissing = false

    /// What the banner shows; set by any action that fails.
    var error: String?

    // Library
    private(set) var items: [LibraryFile] = []
    private(set) var folders: [FolderEntry] = []
    private(set) var busy = false
    /// Bumped whenever a refresh brought the library up to date.
    private(set) var libraryVersion = 0
    private var lastRefresh = Date.distantPast

    static let deviceName = Host.current().localizedName ?? "Mac"

    // -----------------------------------------------------------------------------------------
    // Start

    func start() async {
        phase = .starting
        relayTokenMissing = LocalStore.relayTokenMissing
        let settings = LocalStore.loadSettings()
        do {
            let s = try await core.`init`(
                dataDir: LocalStore.coreDataDir.path,
                accountKey: LocalStore.loadAccountKey(),
                relays: settings.relays,
                relayToken: LocalStore.loadRelayToken(),
                deviceName: Self.deviceName,
                platform: "macos"
            )
            apply(s)
            if let e = s.openError {
                phase = .offline(e)
                retryWhileOffline()
                return
            }
            if hasAccount {
                try await refreshPool()
                if connected {
                    try await refreshLibrary()
                    Task { await resumeSwitch() }
                }
            }
        } catch {
            phase = .failed(describe(error))
            return
        }
        decidePhase()
    }

    /// Stops the core's endpoint and starts it again: after the relays changed.
    func restart() async {
        try? await core.shutdown()
        await start()
    }

    private func decidePhase() {
        phase = !hasAccount || needsNode ? .onboarding : .ready
    }

    private var retrying: Task<Void, Never>?

    /// While the stored account's nodes are out of reach, tries again every 30 seconds.
    private func retryWhileOffline() {
        retrying?.cancel()
        retrying = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(30))
                guard let self, case .offline = self.phase else { return }
                await self.reopen()
            }
        }
    }

    /// Tries again to open the stored account.
    func reopen() async {
        do {
            let s = try await core.`init`(
                dataDir: LocalStore.coreDataDir.path,
                accountKey: LocalStore.loadAccountKey(),
                relays: LocalStore.loadSettings().relays,
                relayToken: LocalStore.loadRelayToken(),
                deviceName: Self.deviceName,
                platform: "macos"
            )
            apply(s)
            if let e = s.openError {
                phase = .offline(e)
                return
            }
            retrying?.cancel()
            try await refreshPool()
            if connected { try await refreshLibrary() }
            decidePhase()
        } catch {
            phase = .offline(describe(error))
        }
    }

    // -----------------------------------------------------------------------------------------
    // Account

    private func apply(_ s: Status) {
        deviceId = s.deviceId
        hasAccount = s.hasAccount
        accountId = s.accountId ?? ""
        accountName = s.name ?? ""
        devices = s.devices
        homeNodes = s.homeNodes
        friends = s.friends
        if !hasAccount {
            statusText = "No account"
            detailText = ""
        }
    }

    /// - Parameter fromNodes: re-read the account from its nodes first, for what other devices
    ///   changed (a phone that joined, photos it uploaded); otherwise what this device last read.
    func refreshAccount(fromNodes: Bool = false) async throws {
        apply(fromNodes && hasAccount ? try await core.refresh() : try await core.status())
    }

    func refreshPool() async throws {
        pool = try await core.pool()
        let writable = pool.filter { $0.reachable && ($0.role ?? "none") != "none" }.count
        let direct = pool.filter { $0.path == "direct" }.count
        connected = writable > 0
        if !hasAccount {
            statusText = "No account"; detailText = ""
        } else if homeNodes.isEmpty {
            statusText = "No node"; detailText = "Add a node you own in Settings"
        } else if connected {
            statusText = "Connected"; detailText = "\(writable) of \(pool.count) nodes reachable · \(direct) direct"
        } else {
            statusText = "Offline"; detailText = "None of your nodes answered"
        }
    }

    /// Account (re-read from its nodes), pool, then library: the refresh a person asks for.
    func refreshAll() async throws {
        try await refreshAccount(fromNodes: true)
        guard hasAccount else { return }
        try await refreshPool()
        if connected { try await refreshLibrary() }
    }

    /// When Ursa comes back to the front: what other devices did meanwhile, at most once a minute.
    func refreshIfStale() async {
        guard phase == .ready, Date().timeIntervalSince(lastRefresh) > 60 else { return }
        try? await refreshAll()
    }

    private func adopt(_ keys: AccountKeys) async throws {
        LocalStore.saveAccountKey(keys.accountKey)
        // A link hands over the other device's relays and token, and the core already runs on
        // them: keep them for the next start (and for Settings › Relays).
        if let relays = keys.relays {
            var s = LocalStore.loadSettings()
            s.relays = relays.filter { !$0.isEmpty }
            LocalStore.saveSettings(s)
            LocalStore.saveRelayToken(keys.relayToken)
        }
        retrying?.cancel()
        try await refreshAccount()
        try await refreshPool()
        if connected { try await refreshLibrary() }
        decidePhase()
    }

    func createAccount(name: String) async throws {
        try await adopt(try await core.createAccount(name: name))
    }

    func recover(phrase: String) async throws {
        try await adopt(try await core.recoverPhrase(phrase: phrase))
    }

    func recover(keyFile: String, passphrase: String) async throws {
        try await adopt(try await core.recoverKeyFile(keyFile: keyFile, passphrase: passphrase))
    }

    /// Joins an account by a link code from another device; `confirm` gets the code to show.
    func join(code: String, confirm: @escaping @MainActor (String) -> Void) async throws {
        try await adopt(try await core.linkJoin(code: code.trimmingCharacters(in: .whitespacesAndNewlines), listener: JoinWatcher(confirm)))
    }

    /// Offers a link code until a device joins (its name) or the offer is cancelled (nil).
    /// `request` is told of each device asking to join; answer it with `decideLink`.
    func offerLink(code: @escaping @MainActor (String) -> Void,
                   request: @escaping @MainActor (_ name: String, _ platform: String, _ confirm: String) -> Void) async throws -> String? {
        let linked = try await core.linkOffer(listener: OfferWatcher(code: code, request: request))
        try? await refreshAccount()
        return linked
    }

    func decideLink(_ accept: Bool) async {
        try? await core.linkDecide(accept: accept)
    }

    func cancelLink() async {
        try? await core.linkCancel()
    }

    func claimNode(link: String, label: String) async throws {
        _ = try await core.claimNode(link: link, label: label)
        try await refreshAccount()
        try await refreshPool()
        if connected { try await refreshLibrary() }
        if phase == .onboarding { decidePhase() }
    }

    func forgetNode(_ id: String) async throws {
        try await core.forgetNode(nodeId: id)
        try await refreshAccount()
        try await refreshPool()
    }

    func invite() async throws -> String { try await core.invite() }

    func befriend(invite: String, quotaGb: UInt64) async throws {
        _ = try await core.befriend(invite: invite, quotaGb: quotaGb)
        try await refreshAccount()
        try await refreshPool()
    }

    func recoveryPhrase() async throws -> String { try await core.recoveryPhrase() }

    func keyFile(passphrase: String) async throws -> String { try await core.keyFile(passphrase: passphrase) }

    /// Forgets the account on this Mac. The library stays on the nodes.
    func signOut() async {
        try? await core.signOut()
        LocalStore.forgetAccountKey()
        items = []
        folders = []
        pool = []
        connected = false
        try? await refreshAccount()
        phase = .onboarding
    }

    // -----------------------------------------------------------------------------------------
    // Pool and protection

    /// The storage report; `extra` adds a shape being considered to the choices.
    func storage(extra: StorageReport.Shape? = nil) async throws -> StorageReport {
        let json = try await core.storageJson(k: extra.map { UInt16($0.k) }, n: extra.map { UInt16($0.n) })
        return try StorageReport.decode(json)
    }

    func setGive(bytes: Int64) async throws {
        try await core.setGive(bytes: UInt64(max(0, bytes)))
    }

    func repair(progress: @escaping @MainActor (UInt64, UInt64) -> Void) async throws -> RepairReport {
        let r = try await core.repair(listener: Progress(progress))
        try? await refreshPool()
        return r
    }

    // Switching protection runs here, not in a view: it keeps going while you use the rest of
    // the app, and the choice is saved first, so a switch cut short by quitting picks up again
    // at the next start.

    private(set) var switching = false
    private(set) var switchFraction = 0.0
    var switchError: String?
    /// Bumped when a switch ends, finished or not.
    private(set) var switchEnded = 0

    /// Chooses `k`-of-`n` and moves the library to it in the background.
    func startSwitch(to shape: StorageReport.Shape) async throws {
        guard !switching else { return }
        try await core.setProtection(k: UInt16(shape.k), n: UInt16(shape.n))
        Task { await recode(quiet: false) }
    }

    /// At start: finishes a switch that was cut short (and clears what it left), then sweeps
    /// pieces nothing uses that are old enough to be sure of. Quiet: the next start tries again.
    private func resumeSwitch() async {
        guard let r = try? await storage() else { return }
        if r.chosen && (r.pendingFiles > 0 || (r.leftoverPieces ?? 0) > 0) { await recode(quiet: true) }
        _ = try? await core.collectGarbage(graceHours: nil)
    }

    private func recode(quiet: Bool) async {
        guard !switching else { return }
        switching = true
        switchFraction = 0
        switchError = nil
        defer {
            switching = false
            switchEnded += 1
        }
        do {
            let r = try await core.recode(listener: Progress { [weak self] done, total in
                if total > 0 { self?.switchFraction = Double(done) / Double(total) }
            })
            if !r.failures.isEmpty && !quiet {
                switchError = "\(r.failures.count) file(s) could not be moved. Switch again when every node is up."
            }
        } catch {
            if !quiet { switchError = describe(error) }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Library

    func refreshLibrary() async throws {
        busy = true
        defer { busy = false }
        let t = try await core.tree()
        folders = t.folders
        // Newest first, as the photo grid shows them.
        items = t.files.sorted { a, b in
            a.createdMs != b.createdMs ? a.createdMs > b.createdMs : a.fileId < b.fileId
        }
        lastRefresh = Date()
        libraryVersion += 1
    }

    /// Changes the tree in one library change, then refreshes.
    func edit(_ edits: [Edit]) async throws {
        guard !edits.isEmpty else { return }
        do {
            _ = try await core.edit(editsJson: Edit.encode(edits))
        } catch {
            try? await refreshLibrary()
            throw error
        }
        try await refreshLibrary()
    }

    func folder(_ id: String?) -> FolderEntry? {
        guard let id else { return nil }
        return folders.first { $0.id == id }
    }

    /// The folder and every folder under it.
    func subtree(_ folderId: String) -> Set<String> {
        var all: Set<String> = [folderId]
        var grew = true
        while grew {
            grew = false
            for f in folders where f.parent.map(all.contains) == true && !all.contains(f.id) {
                all.insert(f.id)
                grew = true
            }
        }
        return all
    }

    /// `name`, or "name 2" and on if it is taken in `dir`, as Finder numbers copies.
    func freeName(in dir: String?, _ name: String, isFolder: Bool) -> String {
        func taken(_ n: String) -> Bool {
            folders.contains { $0.parent == dir && $0.name.caseInsensitiveCompare(n) == .orderedSame }
                || items.contains { $0.dir == dir && $0.name.caseInsensitiveCompare(n) == .orderedSame }
        }
        if !taken(name) { return name }
        let ext = isFolder ? "" : (name as NSString).pathExtension
        let stem = isFolder || ext.isEmpty ? name : (name as NSString).deletingPathExtension
        for i in 2... {
            let candidate = ext.isEmpty ? "\(stem) \(i)" : "\(stem) \(i).\(ext)"
            if !taken(candidate) { return candidate }
        }
        return name
    }

    /// Deletes files in one library change.
    func delete(_ victims: [LibraryFile]) async throws {
        guard !victims.isEmpty else { return }
        try await core.deleteMany(fileIds: victims.map(\.fileId))
        let gone = Set(victims.map(\.fileId))
        items.removeAll { gone.contains($0.fileId) }
        libraryVersion += 1
        for item in victims { try? FileManager.default.removeItem(at: cachedOriginal(item)) }
        try? await refreshLibrary()
    }

    // -----------------------------------------------------------------------------------------
    // Transfers: one bar for everything in flight.

    private(set) var transfersActive = 0
    private var transfersDone = 0
    private var transfersTotal = 0
    private var bytesDone: UInt64 = 0
    private var bytesTotal: UInt64 = 0
    var transferring: Bool { transfersActive > 0 }
    var transferFraction: Double { bytesTotal > 0 ? min(1, Double(bytesDone) / Double(bytesTotal)) : 0 }
    var transferText: String {
        transfersActive == 0 ? "" :
            "\(min(transfersDone + 1, transfersTotal)) of \(transfersTotal) · \(sizeText(bytesDone)) of \(sizeText(bytesTotal))"
    }

    /// One transfer's share of the bar.
    final class Counted { var bytes: UInt64 = 0 }

    /// What a batch of uploads did, kept by its tasks.
    final class Tally { var staged = 0; var failures: [String] = [] }

    private func begin(_ bytes: UInt64) {
        transfersActive += 1
        transfersTotal += 1
        bytesTotal += bytes
    }

    private func end(_ bytes: UInt64, _ counted: Counted) {
        transfersActive -= 1
        transfersDone += 1
        if bytes > counted.bytes { bytesDone += bytes - counted.bytes }
        if transfersActive == 0 { transfersDone = 0; transfersTotal = 0; bytesDone = 0; bytesTotal = 0 }
    }

    private func track(_ counted: Counted) -> Progress {
        Progress { [weak self] done, _ in
            guard let self, done > counted.bytes else { return }
            self.bytesDone += done - counted.bytes
            counted.bytes = done
        }
    }

    /// Uploads files and folders (with everything in them) into the library folder `dir` (nil:
    /// the top). Folders are made first, in one change; files go up staged, a few at a time, and
    /// are written to the library in batches, so a stop part-way keeps what went before.
    func upload(_ urls: [URL], into dir: String?) async throws {
        var edits: [Edit] = []
        var files: [(url: URL, dir: String?, size: UInt64)] = []
        let fm = FileManager.default
        let keys: [URLResourceKey] = [.isDirectoryKey, .isSymbolicLinkKey, .isHiddenKey, .fileSizeKey]
        func walk(_ url: URL, _ parent: String?, top: Bool) {
            guard let v = try? url.resourceValues(forKeys: Set(keys)), v.isSymbolicLink != true else { return }
            if v.isDirectory == true {
                let id = Edit.newFolderId()
                // A folder that is already there gets a number, as a second copy.
                let name = top ? freeName(in: parent, url.lastPathComponent, isFolder: true) : url.lastPathComponent
                edits.append(.createFolder(id: id, parent: parent, name: name))
                let children = (try? fm.contentsOfDirectory(at: url, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles])) ?? []
                for child in children.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
                    walk(child, id, top: false)
                }
            } else {
                files.append((url, parent, UInt64(v.fileSize ?? 0)))
            }
        }
        for url in urls { walk(url, dir, top: true) }
        if !edits.isEmpty { _ = try await core.edit(editsJson: Edit.encode(edits)) }
        guard !files.isEmpty else {
            try await refreshLibrary()
            return
        }

        for f in files { begin(f.size) }
        let tally = Tally()
        // Two at a time: each upload already keeps several chunks in flight.
        await withTaskGroup(of: Void.self) { group in
            var next = 0
            func add() {
                guard next < files.count else { return }
                let f = files[next]
                next += 1
                group.addTask { @MainActor in
                    let counted = Counted()
                    defer { self.end(f.size, counted) }
                    do {
                        let info = await MediaInfo.inspect(f.url)
                        defer { if let t = info.thumbPath { try? FileManager.default.removeItem(atPath: t) } }
                        _ = try await self.core.uploadStaged(request: UploadRequest(
                            path: f.url.path, name: f.url.lastPathComponent, mime: info.mime,
                            createdMs: info.createdMs, modifiedMs: info.modifiedMs,
                            width: info.width, height: info.height, thumbPath: info.thumbPath, dir: f.dir
                        ), listener: self.track(counted))
                        tally.staged += 1
                        if tally.staged % 25 == 0 { _ = try? await self.core.commitUploads() }
                    } catch {
                        tally.failures.append("\(f.url.lastPathComponent): \(describe(error))")
                    }
                }
            }
            add(); add()
            while await group.next() != nil { add() }
        }
        let failures = tally.failures
        defer { error = failures.isEmpty ? nil : "\(failures.count) upload(s) failed. \(failures[0])" }
        _ = try await core.commitUploads()
        try await refreshLibrary()
    }

    func download(_ item: LibraryFile, to url: URL) async throws {
        let counted = Counted()
        begin(item.size)
        defer { end(item.size, counted) }
        try await core.download(fileId: item.fileId, outPath: url.path, listener: track(counted))
    }

    /// Downloads a library folder and everything in it into `dir`; returns the folder written.
    func downloadFolder(_ folderId: String, into dir: URL) async throws -> URL {
        let inside = subtree(folderId)
        let bytes = items.filter { $0.dir.map(inside.contains) == true }.reduce(UInt64(0)) { $0 + $1.size }
        let counted = Counted()
        begin(bytes)
        defer { end(bytes, counted) }
        return URL(fileURLWithPath: try await core.downloadFolder(folderId: folderId, outDir: dir.path, listener: track(counted)))
    }

    private func cachedOriginal(_ item: LibraryFile) -> URL {
        let ext = (item.name as NSString).pathExtension
        return LocalStore.viewCache.appendingPathComponent(ext.isEmpty ? item.fileId : "\(item.fileId).\(ext)")
    }

    /// The whole file, fetched into a local view cache once and reused after that.
    func original(_ item: LibraryFile) async throws -> URL {
        let url = cachedOriginal(item)
        if FileManager.default.fileExists(atPath: url.path) { return url }
        // Fetched beside it first, so a download cut short is never taken for the file.
        let part = url.appendingPathExtension("part")
        try await download(item, to: part)
        try? FileManager.default.removeItem(at: url)
        try FileManager.default.moveItem(at: part, to: url)
        return url
    }

    /// Runs a UI action and turns a failure into the banner.
    func attempt(_ action: () async throws -> Void) async {
        do { try await action() } catch { self.error = describe(error) }
    }
}

extension LibraryFile: Identifiable {
    public var id: String { fileId }
    var isImage: Bool { mime.hasPrefix("image/") }
    var isVideo: Bool { mime.hasPrefix("video/") }
    var isMedia: Bool { isImage || isVideo }
    var created: Date { Date(timeIntervalSince1970: Double(createdMs) / 1000) }
    var modified: Date { Date(timeIntervalSince1970: Double(modifiedMs > 0 ? modifiedMs : createdMs) / 1000) }
    /// Width over height, for the justified rows. Files without a size are 3:2.
    var aspect: Double {
        guard let w = width, let h = height, w > 0, h > 0 else { return 1.5 }
        return min(max(Double(w) / Double(h), 0.4), 3.0)
    }
    var symbol: String { isVideo ? "film" : isImage ? "photo" : "doc" }
    var details: String {
        let date = created.formatted(date: .abbreviated, time: .shortened)
        if isMedia, let w = width, let h = height, w > 0 { return "\(w) × \(h) · \(sizeText(size)) · \(date)" }
        return "\(sizeText(size)) · \(date)"
    }
}

extension FolderEntry: Identifiable {}

extension PoolNode: Identifiable {
    var title: String {
        if !label.isEmpty { return label }
        return owner == "home" ? "Your node \(id.prefix(8))" : "\(owner)'s node"
    }
    var detail: String {
        guard reachable else { return "Unreachable · \(id.prefix(10))" }
        let p = path == "direct" ? "Direct" : path == "relay" ? "Relay" : "Connecting"
        let r = role == "owner" ? "you own it" : role == "granted" ? "shared with you" : "no access yet"
        let q = quotaBytes.map { " of \(sizeText($0))" } ?? ""
        return "\(p) · \(r) · \(sizeText(usedBytes ?? 0))\(q) used"
    }
}
