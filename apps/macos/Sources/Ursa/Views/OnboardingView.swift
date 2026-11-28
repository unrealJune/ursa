import SwiftUI
import UniformTypeIdentifiers

/// First run: create an account (and save its recovery key), or link to / recover an existing
/// one; then add a node the account owns, which every account needs before its library opens.
struct OnboardingView: View {
    @Environment(AppModel.self) private var model

    enum Step { case welcome, create, recovery, existing, node }
    @State private var step: Step = .welcome
    @State private var problem: String?
    @State private var working = false

    // Create
    @State private var name = ""
    @State private var words: [String] = []
    @State private var saved = false
    @State private var savingKeyFile = false

    // Existing
    @State private var linkCode = ""
    @State private var confirmCode: String?
    @State private var keyFile: (name: String, json: String)?
    @State private var keyPass = ""
    @State private var phrase = ""
    @State private var pickingKeyFile = false

    @State private var showNetwork = false

    // Node
    @State private var claimLink = ""
    @State private var nodeLabel = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                RelayTokenWarning()
                Image(nsImage: NSApplication.shared.applicationIconImage)
                    .resizable().frame(width: 72, height: 72)
                switch step {
                case .welcome: welcome
                case .create: create
                case .recovery: recovery
                case .existing: existing
                case .node: node
                }
                if let problem {
                    Label(problem, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: 520, alignment: .leading)
            .padding(40)
            .frame(maxWidth: .infinity)
        }
        .disabled(working)
        .onAppear {
            if model.needsNode { step = .node }
            if let e = model.error {
                problem = e
                model.error = nil
            }
        }
        .sheet(isPresented: $savingKeyFile) { KeyFileSheet() }
        .sheet(isPresented: $showNetwork) { NetworkSheet() }
        .fileImporter(isPresented: $pickingKeyFile, allowedContentTypes: [.data, .json, .text]) { result in
            guard case .success(let url) = result else { return }
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            if let json = try? String(contentsOf: url, encoding: .utf8) {
                keyFile = (url.lastPathComponent, json)
            } else {
                problem = "Couldn't read \(url.lastPathComponent)."
            }
        }
    }

    private func go(_ s: Step) {
        problem = nil
        withAnimation(.snappy) { step = s }
    }

    private func run(_ failure: String, _ action: @escaping () async throws -> Void) {
        problem = nil
        working = true
        Task {
            do { try await action() } catch { problem = failure.isEmpty ? describe(error) : "\(failure): \(describe(error))" }
            working = false
        }
    }

    // ---------------------------------------------------------------------------------------

    private var welcome: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Welcome to Ursa").font(.largeTitle.bold())
            Text("Your photos and files, encrypted on your own devices and kept on nodes you and your friends run. No company holds them, and none of the nodes can read them.")
                .font(.title3).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                Button("Create an Account") { go(.create) }.buttonStyle(.borderedProminent)
                Button("I Already Use Ursa") { go(.existing) }
            }
            .controlSize(.large)
            Button("Network…", systemImage: "network") { showNetwork = true }
                .buttonStyle(.link)
                .help("Relays and relay token")
        }
    }

    private var create: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Create your account").font(.largeTitle.bold())
            Hint("Your account is a key made on this Mac. Your name is only shown to friends you add.")
            TextField("Your name", text: $name, prompt: Text("June"))
                .textFieldStyle(.roundedBorder)
                .onSubmit(createAccount)
            HStack {
                Button("Create", action: createAccount)
                    .buttonStyle(.borderedProminent)
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                Button("Back") { go(.welcome) }
                if working { ProgressView().controlSize(.small) }
            }
            .controlSize(.large)
        }
    }

    private func createAccount() {
        let n = name.trimmingCharacters(in: .whitespaces)
        guard !n.isEmpty else { return }
        run("") {
            try await model.createAccount(name: n)
            words = try await model.recoveryPhrase().split(separator: " ").map(String.init)
            go(.recovery)
        }
    }

    private var recovery: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Save your recovery key").font(.largeTitle.bold())
            Hint("Everything you store is encrypted with your account key. If every device you own is lost, these 24 words, or a key file, are the only way back. Nobody can reset them for you.")
            RecoveryWords(words: words)
            HStack {
                Button("Copy Words") { Pasteboard.copy(words.joined(separator: " ")) }
                Button("Save a Key File…") { savingKeyFile = true }
            }
            Toggle("I've stored my recovery words or a key file somewhere safe", isOn: $saved)
            Button("Continue") {
                words = []
                go(.node)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(!saved)
        }
    }

    private var existing: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Use your account").font(.largeTitle.bold())

            GroupBox {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Link with another device").font(.headline)
                    Hint("On a device that already uses Ursa, open Account › Add a Device (Settings › Devices on Windows and Android). Scan the QR code it shows, or paste the link code.")
                    HStack {
                        CodeField(title: "Link code", text: $linkCode, kind: .link, onSubmit: join)
                        Button("Link", action: join).disabled(linkCode.isEmpty)
                    }
                    if let confirmCode {
                        HStack(spacing: 12) {
                            ProgressView().controlSize(.small)
                            Text("Check the other device shows")
                            Text(spaced(confirmCode)).font(.system(.title2, design: .monospaced).bold())
                        }
                    }
                }
                .padding(6)
            }

            GroupBox {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Recover with your recovery key").font(.headline)
                    Hint("Ursa finds your account on your nodes and your friends' nodes, even if your own node is gone.")
                    HStack {
                        Button(keyFile == nil ? "Choose Key File…" : "Change Key File…") { pickingKeyFile = true }
                        if let keyFile {
                            Label(keyFile.name, systemImage: "key.fill").lineLimit(1)
                        }
                    }
                    if keyFile != nil {
                        SecureField("Key file passphrase", text: $keyPass).textFieldStyle(.roundedBorder)
                    } else {
                        Text("or").foregroundStyle(.secondary)
                        TextField("Your 24 recovery words", text: $phrase, axis: .vertical)
                            .lineLimit(3...5)
                            .textFieldStyle(.roundedBorder)
                    }
                    Button("Recover", action: recover)
                }
                .padding(6)
            }

            HStack {
                Button("Back") { go(.welcome) }
                Button("Network…", systemImage: "network") { showNetwork = true }.buttonStyle(.link)
                if working { ProgressView().controlSize(.small) }
            }
        }
    }

    private func join() {
        let code = linkCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard code.hasPrefix("ursa-link:") else {
            problem = "A link code starts with ursa-link:. Get one from Add a Device on the other device."
            return
        }
        run("Couldn't link") {
            defer { confirmCode = nil }
            try await model.join(code: code) { confirmCode = $0 }
            if model.needsNode { go(.node) }
        }
    }

    private func recover() {
        let words = phrase.split(whereSeparator: \.isWhitespace)
        if let keyFile {
            let pass = keyPass
            run("Couldn't recover") {
                try await model.recover(keyFile: keyFile.json, passphrase: pass)
                if model.needsNode { go(.node) }
            }
        } else if words.count == 24 {
            let p = words.joined(separator: " ")
            run("Couldn't recover") {
                try await model.recover(phrase: p)
                if model.needsNode { go(.node) }
            }
        } else {
            problem = "Choose your key file and enter its passphrase, or type all 24 recovery words."
        }
    }

    private var node: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Add a node you own").font(.largeTitle.bold())
            Hint("Ursa stores your library across nodes: always-on computers you and your friends run. Every account brings at least one, which is the space you contribute to your friends in return for theirs.")
            Hint("Start ursa-node on a home server, NAS or spare computer. When it first starts it prints a claim link (ursa-claim:…). Paste it here.")
            CodeField(title: "Claim link", text: $claimLink, kind: .claim)
            TextField("Name", text: $nodeLabel, prompt: Text("A name for it, like Home server")).textFieldStyle(.roundedBorder)
            HStack {
                Button("Add Node") {
                    let link = claimLink.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard link.hasPrefix("ursa-claim:") else {
                        problem = "A claim link starts with ursa-claim:. The node prints it when it first starts."
                        return
                    }
                    let label = nodeLabel.trimmingCharacters(in: .whitespaces)
                    run("Couldn't add the node") { try await model.claimNode(link: link, label: label) }
                }
                .buttonStyle(.borderedProminent)
                .disabled(claimLink.isEmpty)
                if working { ProgressView().controlSize(.small) }
            }
            .controlSize(.large)
        }
    }
}

/// The 24 words, in three columns, numbered.
struct RecoveryWords: View {
    var words: [String]
    var body: some View {
        let rows = (words.count + 2) / 3
        Grid(alignment: .leading, horizontalSpacing: 28, verticalSpacing: 6) {
            ForEach(0..<rows, id: \.self) { r in
                GridRow {
                    ForEach(0..<3, id: \.self) { c in
                        let i = c * rows + r
                        if i < words.count {
                            HStack(spacing: 6) {
                                Text("\(i + 1).").foregroundStyle(.secondary).frame(width: 24, alignment: .trailing)
                                Text(words[i])
                            }
                            .font(.system(.body, design: .monospaced))
                        }
                    }
                }
            }
        }
        .textSelection(.enabled)
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
    }
}

/// Asks for a passphrase twice, then where to save the key file.
struct KeyFileSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var pass1 = ""
    @State private var pass2 = ""
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Save a key file").font(.title2.bold())
            Hint("The key file is your account key sealed with this passphrase. Keep the file and the passphrase apart.")
            SecureField("Passphrase (8 characters or more)", text: $pass1).textFieldStyle(.roundedBorder)
            SecureField("Again", text: $pass2).textFieldStyle(.roundedBorder)
            if let problem { Text(problem).foregroundStyle(.red).font(.callout) }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save…", action: save)
                    .keyboardShortcut(.defaultAction)
                    .disabled(pass1.count < 8 || pass1 != pass2)
            }
        }
        .padding(20)
        .frame(width: 420)
    }

    private func save() {
        Task {
            do {
                let json = try await model.keyFile(passphrase: pass1)
                let panel = NSSavePanel()
                panel.nameFieldStringValue = "Ursa recovery key.ursa-key"
                panel.directoryURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
                guard panel.runModal() == .OK, let url = panel.url else { return }
                try json.write(to: url, atomically: true, encoding: .utf8)
                dismiss()
            } catch {
                problem = describe(error)
            }
        }
    }
}
