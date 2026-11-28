import SwiftUI

/// The account: its devices (and adding one), recovery, the nodes it owns, and friends.
struct AccountView: View {
    @Environment(AppModel.self) private var model
    @State private var problem: String?

    // Devices
    @State private var linking = false
    @State private var linkCode: String?
    @State private var linkStatus = ""
    @State private var request: (name: String, platform: String, confirm: String)?

    // Recovery
    @State private var words: [String]?
    @State private var savingKeyFile = false

    // Nodes
    @State private var claimLink = ""
    @State private var nodeLabel = ""
    @State private var repairing: String?
    @State private var repairReport: RepairReport?
    @State private var forgetting: PoolNode?

    // Friends
    @State private var invite: String?
    @State private var friendInvite = ""
    @State private var quotaGb = 50
    @State private var confirmSignOut = false

    var body: some View {
        Form {
            Section {
                LabeledContent("Name", value: model.accountName)
                LabeledContent("Account") {
                    Text(short(model.accountId)).textSelection(.enabled).font(.system(.body, design: .monospaced))
                        .help(model.accountId)
                }
            } header: {
                Text("Account")
            } footer: {
                if let problem {
                    Label(problem, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).textSelection(.enabled)
                }
            }

            devices
            recovery
            nodes
            friends

            Section {
                Button("Sign Out of This Mac…", role: .destructive) { confirmSignOut = true }
            } footer: {
                Hint("Your library stays on your nodes and your other devices. To use it here again you'll need another device to link, your recovery words or your key file.")
            }
        }
        .formStyle(.grouped)
        .task { await attempt { try await model.refreshAccount(fromNodes: true); try await model.refreshPool() } }
        .onDisappear { if linking { Task { await model.cancelLink() } } }
        .toolbar {
            Button("Refresh", systemImage: "arrow.clockwise") {
                Task { await attempt { try await model.refreshAccount(fromNodes: true); try await model.refreshPool() } }
            }
        }
        .focusedSceneValue(\.pageActions, PageActions(refresh: {
            Task { await attempt { try await model.refreshAccount(fromNodes: true); try await model.refreshPool() } }
        }))
        .alert(request.map { "Add \($0.name)?" } ?? "", isPresented: Binding(get: { request != nil }, set: { if !$0 { request = nil } })) {
            Button("It Matches: Add It") { decide(true) }
            Button("Don't Add", role: .cancel) { decide(false) }
        } message: {
            if let r = request {
                Text("A \(r.platform) device called \(r.name) wants to join your account. Only let it in if it shows this code:\n\n\(spaced(r.confirm))")
            }
        }
        .sheet(isPresented: Binding(get: { words != nil }, set: { if !$0 { words = nil } })) {
            WordsSheet(words: words ?? [])
        }
        .sheet(isPresented: $savingKeyFile) { KeyFileSheet() }
        .sheet(isPresented: Binding(get: { invite != nil }, set: { if !$0 { invite = nil } })) {
            InviteSheet(invite: invite ?? "")
        }
        .alert(repairReport.map { $0.unrecoverable.isEmpty ? "Library healthy" : "Some data could not be recovered" } ?? "",
               isPresented: Binding(get: { repairReport != nil }, set: { if !$0 { repairReport = nil } })) {
            Button("OK") {}
        } message: {
            if let r = repairReport {
                Text("\(r.files) files checked, \(r.objectsRebuilt) objects rebuilt onto live nodes\(r.unrecoverable.isEmpty ? "" : ", \(r.unrecoverable.count) lost").")
            }
        }
        .confirmationDialog(forgetting.map { "Forget \($0.title)?" } ?? "", isPresented: Binding(get: { forgetting != nil }, set: { if !$0 { forgetting = nil } })) {
            Button("Forget Node", role: .destructive) {
                if let n = forgetting { Task { await attempt { try await model.forgetNode(n.id) } } }
            }
        } message: {
            Text("Your account stops using this node. Pieces stored on it are rebuilt elsewhere the next time you repair.")
        }
        .confirmationDialog("Sign out of this Mac?", isPresented: $confirmSignOut) {
            Button("Sign Out", role: .destructive) { Task { await model.signOut() } }
        } message: {
            Text("Make sure you have your recovery words or a key file first.")
        }
    }

    private func attempt(_ action: () async throws -> Void) async {
        do {
            try await action()
            problem = nil
        } catch {
            problem = describe(error)
        }
    }

    private func short(_ id: String) -> String {
        id.isEmpty ? "—" : id.count > 20 ? "\(id.prefix(10))…\(id.suffix(10))" : id
    }

    // ---------------------------------------------------------------------------------------
    // Devices

    private var devices: some View {
        Section {
            ForEach(model.devices, id: \.deviceId) { d in
                HStack {
                    Image(systemName: deviceSymbol(d.platform)).frame(width: 22).foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(d.name)
                        Text("\(d.platform) · \(d.deviceId.prefix(10))\(d.thisDevice ? " · this Mac" : "")")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            if linking {
                VStack(alignment: .leading, spacing: 12) {
                    Text("On the new device, choose “I already use Ursa” › “Link with another device”, and scan this code or paste the text below it:")
                        .fixedSize(horizontal: false, vertical: true)
                    if let code = linkCode {
                        HStack(alignment: .top, spacing: 16) {
                            QRCode(text: code).frame(width: 180, height: 180)
                            VStack(alignment: .leading, spacing: 8) {
                                Text(code).font(.system(.caption, design: .monospaced)).textSelection(.enabled).lineLimit(6)
                                Button("Copy") { Pasteboard.copy(code) }
                            }
                        }
                    }
                    HStack {
                        ProgressView().controlSize(.small)
                        Text(linkStatus).foregroundStyle(.secondary)
                        Spacer()
                        Button("Stop") { Task { await model.cancelLink() } }
                    }
                }
                .padding(.vertical, 4)
            } else {
                HStack {
                    Hint("Add a phone or another computer: it gets your account key over an encrypted, post-quantum link, after you confirm the same code on both screens.")
                    Spacer()
                    Button("Add a Device…", action: offerLink)
                }
            }
        } header: {
            Text("Devices")
        }
    }

    private func deviceSymbol(_ platform: String) -> String {
        switch platform.lowercased() {
        case "android", "ios": "iphone"
        case "macos": "laptopcomputer"
        case "windows": "pc"
        default: "desktopcomputer"
        }
    }

    private func offerLink() {
        linking = true
        linkCode = nil
        linkStatus = "Waiting for the other device…"
        problem = nil
        Task {
            do {
                _ = try await model.offerLink(code: { linkCode = $0 }, request: { request = ($0, $1, $2) })
            } catch {
                problem = "Couldn't add the device: \(describe(error))"
            }
            linking = false
            linkCode = nil
            request = nil
        }
    }

    private func decide(_ accept: Bool) {
        linkStatus = accept ? "Sending your account key…" : "Refused. Waiting for another attempt…"
        request = nil
        Task { await model.decideLink(accept) }
    }

    // ---------------------------------------------------------------------------------------
    // Recovery

    private var recovery: some View {
        Section {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Recovery words and key file")
                    Hint("Either one restores your whole account on a new device, even if every node you own is gone. Without one, nobody can.")
                }
                Spacer()
                Button("Show Words…") {
                    Task { await attempt { words = try await model.recoveryPhrase().split(separator: " ").map(String.init) } }
                }
                Button("Save Key File…") { savingKeyFile = true }
            }
        } header: {
            Text("Recovery")
        }
    }

    // ---------------------------------------------------------------------------------------
    // Nodes

    private var nodes: some View {
        Section {
            ForEach(model.pool) { n in
                HStack {
                    StatusDot(on: n.reachable)
                    Image(systemName: n.owner == "home" ? "server.rack" : "person.2").frame(width: 22).foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(n.title)
                        Text(n.detail).font(.caption).foregroundStyle(.secondary)
                        if let e = n.error, !n.reachable {
                            Text(e).font(.caption2).foregroundStyle(.tertiary).lineLimit(2)
                        }
                    }
                    Spacer()
                    if let v = n.version { Text("v\(v)").font(.caption).foregroundStyle(.tertiary) }
                }
                .contextMenu {
                    Button("Copy Node ID") { Pasteboard.copy(n.id) }
                    if n.owner == "home" {
                        Button("Forget Node…", role: .destructive) { forgetting = n }
                    }
                }
            }
            HStack {
                Hint("Your library is coded across these: yours and your friends'. Choose how in Pool.")
                Spacer()
                Button("Check") { Task { await attempt { try await model.refreshAccount(fromNodes: true); try await model.refreshPool() } } }
                Button(repairing ?? "Repair") {
                    repairing = "Repairing…"
                    Task {
                        await attempt {
                            repairReport = try await model.repair { done, total in repairing = "Repairing \(done)/\(total)…" }
                        }
                        repairing = nil
                    }
                }
                .disabled(repairing != nil)
                .help("Check every file and rebuild missing pieces onto nodes that are up")
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Add a node you own").fontWeight(.medium)
                Hint("Paste the claim link the node printed when it first started (ursa-claim:…).")
                HStack {
                    CodeField(title: "Claim link", text: $claimLink, kind: .claim)
                    TextField("Name", text: $nodeLabel, prompt: Text("Name (optional)")).frame(width: 160)
                    Button("Add", action: claim).disabled(claimLink.isEmpty)
                }
                .labelsHidden()
            }
            .padding(.vertical, 4)
        } header: {
            Text("Nodes")
        }
    }

    private func claim() {
        let link = claimLink.trimmingCharacters(in: .whitespacesAndNewlines)
        guard link.hasPrefix("ursa-claim:") else {
            problem = "That isn't a claim link. It starts with ursa-claim: and the node prints it when it first starts."
            return
        }
        let label = nodeLabel.trimmingCharacters(in: .whitespaces)
        Task {
            await attempt {
                try await model.claimNode(link: link, label: label)
                claimLink = ""
                nodeLabel = ""
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Friends

    private var friends: some View {
        Section {
            ForEach(model.friends, id: \.accountId) { f in
                HStack {
                    Image(systemName: "person.crop.circle").frame(width: 22).foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(f.name.isEmpty ? String(f.accountId.prefix(10)) : f.name)
                        Text("\(f.accountId.prefix(10)) · \(f.nodes) node\(f.nodes == 1 ? "" : "s")").font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            HStack {
                Hint("Friends store shards of your library on their nodes, and you of theirs. Both of you add each other; each sets how much space the other gets. Nobody can read anyone else's files.")
                Spacer()
                Button("Copy My Invite") { Task { await attempt { Pasteboard.copy(try await model.invite()) } } }
                Button("Show as QR Code…") { Task { await attempt { invite = try await model.invite() } } }
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Add a friend").fontWeight(.medium)
                HStack {
                    CodeField(title: "Their invite", text: $friendInvite, kind: .invite)
                    Stepper(value: $quotaGb, in: 1...100_000, step: 10) {
                        Text("\(quotaGb) GB").monospacedDigit().frame(minWidth: 64, alignment: .trailing)
                    }
                    Button("Add", action: befriend).disabled(friendInvite.isEmpty)
                }
                .labelsHidden()
                Hint("Space you give them, in GB, on each of your nodes.")
            }
            .padding(.vertical, 4)
        } header: {
            Text("Friends")
        }
    }

    private func befriend() {
        let code = friendInvite.trimmingCharacters(in: .whitespacesAndNewlines)
        guard code.hasPrefix("ursa-invite:") else {
            problem = "An invite starts with ursa-invite:. Ask your friend to copy theirs from their Account or Settings."
            return
        }
        let gb = UInt64(max(quotaGb, 0))
        Task {
            await attempt {
                try await model.befriend(invite: code, quotaGb: gb)
                friendInvite = ""
            }
        }
    }
}

struct WordsSheet: View {
    @Environment(\.dismiss) private var dismiss
    var words: [String]
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Recovery words").font(.title2.bold())
            Hint("Anyone with these words can read your whole library. Write them down and keep them offline.")
            RecoveryWords(words: words)
            HStack {
                Button("Copy") { Pasteboard.copy(words.joined(separator: " ")) }
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(22)
        .frame(width: 520)
    }
}

struct InviteSheet: View {
    @Environment(\.dismiss) private var dismiss
    var invite: String
    var body: some View {
        VStack(spacing: 14) {
            Text("Your invite").font(.title2.bold())
            Hint("In Ursa on your friend's phone, open Friends and tap the scan button beside “Their invite”.")
            QRCode(text: invite).frame(width: 300, height: 300)
            HStack {
                Button("Copy") { Pasteboard.copy(invite) }
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(22)
        .frame(width: 400)
    }
}
