import AppKit
import SwiftUI

@main
struct UrsaApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var model = AppModel()

    var body: some Scene {
        Window("Ursa", id: "main") {
            RootView()
                .environment(model)
                .frame(minWidth: 820, minHeight: 540)
                .task {
                    delegate.model = model
                    await model.start()
                    await delegate.uploadPending()
                    #if DEBUG
                    await Snapshots.runIfAsked(model)
                    #endif
                }
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
                    Task { await model.refreshIfStale() }
                }
        }
        .defaultSize(width: 1180, height: 780)
        .commands { UrsaCommands() }

        Settings {
            SettingsView().environment(model)
        }
    }
}

/// Files handed to Ursa from outside (dropped on its Dock icon, or Open With) go up to the top of
/// the library, as Windows' "Send to" does.
final class AppDelegate: NSObject, NSApplicationDelegate {
    @MainActor var model: AppModel?
    @MainActor private var pending: [URL] = []

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    @MainActor
    func application(_ application: NSApplication, open urls: [URL]) {
        pending += urls.filter(\.isFileURL)
        Task { await uploadPending() }
    }

    @MainActor
    func uploadPending() async {
        guard let model, model.phase == .ready, model.connected, !pending.isEmpty else { return }
        let urls = pending
        pending = []
        await model.attempt { try await model.upload(urls, into: nil) }
    }
}

struct UrsaCommands: Commands {
    @FocusedValue(\.pageActions) private var actions

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Folder") { actions?.newFolder?() }
                .keyboardShortcut("n", modifiers: [.command, .shift])
                .disabled(actions?.newFolder == nil)
            Button("Upload…") { actions?.upload?() }
                .keyboardShortcut("u")
                .disabled(actions?.upload == nil)
        }
        CommandGroup(after: .sidebar) {
            Button("Refresh") { actions?.refresh?() }
                .keyboardShortcut("r")
                .disabled(actions?.refresh == nil)
            Divider()
        }
    }
}
