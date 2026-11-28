import SwiftUI

/// Ursa › Settings (⌘,): this Mac, and the relays it connects through.
struct SettingsView: View {
    var body: some View {
        TabView {
            GeneralSettings().tabItem { Label("General", systemImage: "gearshape") }
            RelaySettings().tabItem { Label("Relays", systemImage: "point.3.connected.trianglepath.dotted") }
        }
        .frame(width: 520)
    }
}

struct GeneralSettings: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Form {
            Section("This Mac") {
                LabeledContent("Device name", value: AppModel.deviceName)
                LabeledContent("Device ID") {
                    Text(model.deviceId.isEmpty ? "—" : "\(model.deviceId.prefix(10))…\(model.deviceId.suffix(10))")
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                        .help(model.deviceId)
                }
                LabeledContent("Data") {
                    Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([LocalStore.root]) }
                }
            }
            Section("About") {
                LabeledContent("Version", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "dev")
                Text("Private, encrypted photo and file storage on your own and your friends' nodes. Free software under the GNU GPL v3 or later.")
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .formStyle(.grouped)
        .frame(height: 300)
    }
}

struct RelaySettings: View {
    var body: some View {
        NetworkForm().frame(height: 460)
    }
}

/// The relays this Mac uses, and the token they ask for: Settings › Relays, and the Network sheet
/// before there is an account (as Android's Network screen).
struct NetworkForm: View {
    @Environment(AppModel.self) private var model
    /// Called once the core has restarted on what was saved.
    var onSaved: (() -> Void)?
    @State private var urls = ""
    @State private var token = ""
    @State private var hasToken = false
    @State private var showToken = false
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        Form {
            Section {
                TextEditor(text: $urls)
                    .font(.system(.body, design: .monospaced))
                    .frame(height: 80)
                Button("Use Ursa's Relays") { urls = LocalStore.defaultRelays.joined(separator: "\n") }
            } header: {
                Text("Relays, one per line")
            } footer: {
                Hint("Your devices and nodes meet through relays when they can't reach each other directly. Every device and node of an account should use the same relays. Leave empty to use iroh's public relays.")
            }
            Section {
                HStack {
                    Group {
                        if showToken {
                            TextField("Relay token", text: $token, prompt: Text("None"))
                        } else {
                            SecureField("Relay token", text: $token, prompt: Text("None"))
                        }
                    }
                    .labelsHidden()
                    .font(.system(.body, design: .monospaced))
                    Button {
                        showToken.toggle()
                    } label: {
                        Image(systemName: showToken ? "eye.slash" : "eye")
                    }
                    .buttonStyle(.borderless)
                    .help(showToken ? "Hide token" : "Show token")
                }
            } header: {
                Text("Relay token")
            } footer: {
                Hint(hasToken ? "A token is saved on this Mac. Clear the field to remove it."
                    : hasBuiltInRelayToken() ? "This build carries the token for Ursa's own relays. A token saved here is used instead."
                    : "This build has no token for Ursa's own relays: they refuse it until you save one here. Ask whoever runs the relays.")
            }
            HStack {
                if let problem { Text(problem).foregroundStyle(.red) }
                Spacer()
                if saving { ProgressView().controlSize(.small) }
                Button("Save and Reconnect", action: save)
                    .buttonStyle(.borderedProminent)
                    .disabled(saving)
            }
        }
        .formStyle(.grouped)
        .onAppear {
            urls = LocalStore.loadSettings().relays.joined(separator: "\n")
            let saved = LocalStore.loadRelayToken()
            hasToken = saved != nil
            token = saved ?? ""
        }
    }

    private func save() {
        let list = urls.split(whereSeparator: { $0.isWhitespace }).map(String.init).filter { !$0.isEmpty }
        for u in list where URL(string: u)?.scheme != "https" {
            problem = "\(u) isn't an https URL."
            return
        }
        problem = nil
        var s = LocalStore.loadSettings()
        s.relays = list
        LocalStore.saveSettings(s)
        LocalStore.saveRelayToken(token)
        hasToken = LocalStore.loadRelayToken() != nil
        saving = true
        Task {
            await model.restart() // also re-checks the relay token
            saving = false
            onSaved?()
        }
    }
}

/// The Network form as a sheet, for onboarding and the offline screen.
struct NetworkSheet: View {
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Network").font(.title2.bold())
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            .padding([.horizontal, .top], 20)
            NetworkForm { dismiss() }
        }
        .frame(width: 520, height: 500)
    }
}
