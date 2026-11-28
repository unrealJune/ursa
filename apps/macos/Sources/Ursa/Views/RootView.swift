import SwiftUI

enum Screen: String, Hashable, CaseIterable {
    case photos, files, pool, usage, account

    var title: String {
        switch self {
        case .photos: "Photos"
        case .files: "Files"
        case .pool: "Pool"
        case .usage: "Usage"
        case .account: "Account"
        }
    }

    var symbol: String {
        switch self {
        case .photos: "photo.on.rectangle"
        case .files: "folder"
        case .pool: "circle.hexagongrid"
        case .usage: "chart.bar"
        case .account: "person.crop.circle"
        }
    }
}

/// One screen per phase: starting, onboarding, offline, or the library.
struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        switch model.phase {
        case .starting:
            ProgressView("Starting Ursa…").frame(maxWidth: .infinity, maxHeight: .infinity)
        case .failed(let message):
            // Most often a relay address that is wrong: Network is where to fix it, as on Android.
            VStack {
                EmptyState(symbol: "exclamationmark.octagon", title: "Ursa couldn't start", message: message, action: "Try Again") {
                    Task { await model.start() }
                }
                NetworkButton().padding(.bottom, 40)
            }
        case .offline(let message):
            OfflineView(message: message)
        case .onboarding:
            OnboardingView()
        case .ready:
            MainView()
        }
    }
}

struct NetworkButton: View {
    @State private var show = false
    var body: some View {
        Button("Network…", systemImage: "network") { show = true }
            .sheet(isPresented: $show) { NetworkSheet() }
    }
}

struct OfflineView: View {
    @Environment(AppModel.self) private var model
    var message: String
    @State private var trying = false
    @State private var confirmSignOut = false
    @State private var showNetwork = false

    var body: some View {
        ContentUnavailableView {
            Label("Can't reach your account", systemImage: "wifi.exclamationmark")
        } description: {
            Text("None of your account's nodes answered. Ursa keeps trying every 30 seconds.")
            Text(message).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
        } actions: {
            RelayTokenWarning().frame(maxWidth: 480)
            Button {
                trying = true
                Task {
                    await model.reopen()
                    trying = false
                }
            } label: {
                if trying { ProgressView().controlSize(.small) } else { Text("Try Again") }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(trying)
            Button("Network…", systemImage: "network") { showNetwork = true }
            Button("Sign Out of This Mac…") { confirmSignOut = true }
                .buttonStyle(.link)
        }
        .sheet(isPresented: $showNetwork) { NetworkSheet() }
        .confirmationDialog("Sign out of this Mac?", isPresented: $confirmSignOut) {
            Button("Sign Out", role: .destructive) { Task { await model.signOut() } }
        } message: {
            Text("Your library stays on your nodes. To use it here again you'll need another device to link, your recovery words or your key file.")
        }
    }
}

struct MainView: View {
    @Environment(AppModel.self) private var model
    @SceneStorage("screen") private var screen: Screen = .photos

    var body: some View {
        NavigationSplitView {
            List(selection: Binding($screen)) {
                Section("Library") {
                    ForEach([Screen.photos, .files], id: \.self) { s in
                        Label(s.title, systemImage: s.symbol).tag(s)
                    }
                }
                Section("Storage") {
                    ForEach([Screen.pool, .usage], id: \.self) { s in
                        Label(s.title, systemImage: s.symbol).tag(s)
                    }
                }
                Section {
                    Label(Screen.account.title, systemImage: Screen.account.symbol).tag(Screen.account)
                }
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 210)
            .safeAreaInset(edge: .bottom) { SidebarFooter() }
        } detail: {
            VStack(spacing: 0) {
                RelayTokenWarning(banner: true)
                ErrorBanner()
                switch screen {
                case .photos: PhotosView()
                case .files: FilesView()
                case .pool: PoolView()
                case .usage: UsageView()
                case .account: AccountView()
                }
            }
            .navigationTitle(screen.title)
        }
        #if DEBUG
        .onReceive(NotificationCenter.default.publisher(for: Snapshots.show)) { note in
            if let s = note.object as? Screen { screen = s }
        }
        #endif
    }
}

/// Connection, and what is being transferred.
struct SidebarFooter: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if model.transferring {
                VStack(alignment: .leading, spacing: 4) {
                    ProgressView(value: model.transferFraction).controlSize(.small)
                    Text(model.transferText).font(.caption2).foregroundStyle(.secondary).monospacedDigit()
                }
            }
            if model.switching {
                VStack(alignment: .leading, spacing: 4) {
                    ProgressView(value: model.switchFraction).controlSize(.small)
                    Text("Switching protection · \(Int(model.switchFraction * 100))%").font(.caption2).foregroundStyle(.secondary)
                }
            }
            HStack(spacing: 8) {
                StatusDot(on: model.connected)
                VStack(alignment: .leading, spacing: 1) {
                    Text(model.statusText).font(.caption).fontWeight(.medium)
                    if !model.detailText.isEmpty {
                        Text(model.detailText).font(.caption2).foregroundStyle(.secondary).lineLimit(2)
                    }
                }
            }
            .help(model.detailText)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
