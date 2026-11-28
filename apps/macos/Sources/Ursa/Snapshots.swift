#if DEBUG
import AppKit
import SwiftUI

/// Development only: `URSA_SNAPSHOT_DIR=<dir>` renders the window to `<dir>/<screen>.png` for
/// every screen (or those in `URSA_SNAPSHOT_SCREENS`, comma-separated), then quits. Drawn by
/// the app itself, so it needs no screen-recording permission.
enum Snapshots {
    static let show = Notification.Name("UrsaSnapshotShow")

    @MainActor
    static func runIfAsked(_ model: AppModel) async {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["URSA_SNAPSHOT_DIR"] else { return }
        let out = URL(fileURLWithPath: dir, isDirectory: true)
        try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let wait = Double(env["URSA_SNAPSHOT_WAIT"] ?? "") ?? 3
        if model.phase != .ready {
            try? await Task.sleep(for: .seconds(wait))
            capture(out.appendingPathComponent("\(model.phase).png"))
        } else {
            let names = env["URSA_SNAPSHOT_SCREENS"]?.split(separator: ",").map(String.init)
            for screen in Screen.allCases where names?.contains(screen.rawValue) ?? true {
                NotificationCenter.default.post(name: show, object: screen)
                try? await Task.sleep(for: .seconds(wait))
                capture(out.appendingPathComponent("\(screen.rawValue).png"))
            }
        }
        NSApp.terminate(nil)
    }

    @MainActor
    static func capture(_ url: URL) {
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil }),
              let view = window.contentView?.superview ?? window.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
        view.cacheDisplay(in: view.bounds, to: rep)
        try? rep.representation(using: .png, properties: [:])?.write(to: url)
    }
}
#endif
