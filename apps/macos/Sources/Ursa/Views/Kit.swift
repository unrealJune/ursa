import AppKit
import CoreImage.CIFilterBuiltins
import SwiftUI

// Small pieces the screens share.

/// A rounded panel, as the Pool, Usage and Account screens group what they show.
struct Card<Content: View>: View {
    var padding: CGFloat = 16
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 10) { content }
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.separator.opacity(0.6)))
    }
}

struct SectionTitle: View {
    var text: String
    var body: some View {
        Text(text).font(.headline).padding(.top, 14).padding(.bottom, 2)
    }
}

/// Text that reads as a hint beside the main thing.
struct Hint: View {
    var text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }
}

/// Label on the left, value on the right.
struct ValueRow: View {
    var label: String
    var value: String
    var strong = false
    var valueColor: Color?
    var body: some View {
        HStack {
            Text(label).foregroundStyle(.secondary)
            Spacer(minLength: 12)
            Text(value).fontWeight(strong ? .semibold : .regular).foregroundStyle(valueColor ?? .primary).monospacedDigit()
        }
    }
}

/// The series colours: the accent at three strengths, then neutrals.
enum Series {
    static func color(_ i: Int) -> Color {
        switch i {
        case 0: .accentColor
        case 1: .accentColor.opacity(0.62)
        case 2: .accentColor.opacity(0.32)
        case 3: .gray.opacity(0.55)
        default: .gray.opacity(0.25)
        }
    }
    static let track = Color.gray.opacity(0.18)
}

/// A horizontal bar of segments, each a share of `total`; optionally a mark at `tick` and a
/// highlighted band over a range.
struct SegmentBar: View {
    var segments: [(value: Double, color: Color)]
    var total: Double
    var height: CGFloat = 12
    var tick: Double?
    var band: ClosedRange<Double>?

    var body: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let scale = total > 0 ? w / total : 0
            ZStack(alignment: .leading) {
                Series.track
                HStack(spacing: 0) {
                    ForEach(Array(clipped.enumerated()), id: \.offset) { _, s in
                        s.color.frame(width: max(0, s.value * scale))
                    }
                    Spacer(minLength: 0)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: min(4, height / 2)))
            .overlay(alignment: .leading) {
                if let band, total > 0 {
                    let from = min(max(band.lowerBound, 0), total), to = min(max(band.upperBound, from), total)
                    RoundedRectangle(cornerRadius: 2).fill(.orange.opacity(0.8))
                        .frame(width: max(3, (to - from) * scale), height: height + 6)
                        .offset(x: from * scale)
                }
            }
            .overlay(alignment: .leading) {
                if let tick, total > 0 {
                    Rectangle().fill(.primary).frame(width: 2, height: height + 6)
                        .offset(x: min(max(tick * scale - 1, 0), w - 2))
                }
            }
        }
        .frame(height: height)
    }

    private var clipped: [(value: Double, color: Color)] {
        var used = 0.0
        return segments.compactMap { s in
            let v = max(0, min(s.value, total - used))
            used += v
            return v > 0 ? (v, s.color) : nil
        }
    }
}

struct Legend: View {
    var items: [(label: String, color: Color)]
    var body: some View {
        HStack(spacing: 16) {
            ForEach(Array(items.enumerated()), id: \.offset) { _, i in
                HStack(spacing: 6) {
                    RoundedRectangle(cornerRadius: 2).fill(i.color).frame(width: 10, height: 10)
                    Text(i.label).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }
}

struct Badge: View {
    var text: String
    var accent: Bool
    var body: some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8).padding(.vertical, 2)
            .foregroundStyle(accent ? Color.accentColor : .secondary)
            .background(accent ? Color.accentColor.opacity(0.15) : .clear, in: Capsule())
            .overlay(Capsule().strokeBorder(accent ? .clear : Color.secondary.opacity(0.4)))
    }
}

/// A small bar and the limit beside it.
struct LimitCell: View {
    var used: Int64
    var limit: Int64?
    var body: some View {
        if let limit {
            HStack(spacing: 8) {
                SegmentBar(segments: [(Double(used), Series.color(1))], total: Double(max(limit, 1)), height: 6)
                    .frame(minWidth: 80)
                Text(sizeText(limit)).font(.caption).foregroundStyle(.secondary).frame(width: 72, alignment: .trailing)
            }
        } else {
            Text("")
        }
    }
}

/// The status light: green when connected.
struct StatusDot: View {
    var on: Bool
    var body: some View {
        Circle().fill(on ? Color.green : Color.gray).frame(width: 8, height: 8)
    }
}

/// A QR code, sharp at any size.
struct QRCode: View {
    var text: String
    var body: some View {
        if let image = Self.image(text) {
            Image(nsImage: image)
                .interpolation(.none)
                .resizable()
                .scaledToFit()
                .padding(12)
                .background(.white, in: RoundedRectangle(cornerRadius: 8))
        }
    }

    static func image(_ text: String) -> NSImage? {
        let f = CIFilter.qrCodeGenerator()
        f.message = Data(text.utf8)
        f.correctionLevel = "M"
        guard let out = f.outputImage,
              let cg = CIContext().createCGImage(out, from: out.extent) else { return nil }
        return NSImage(cgImage: cg, size: NSSize(width: out.extent.width, height: out.extent.height))
    }
}

/// The error banner over a screen.
struct ErrorBanner: View {
    @Environment(AppModel.self) private var model
    var body: some View {
        if let error = model.error {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                Text(error).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                Button {
                    model.error = nil
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless)
                .help("Dismiss")
            }
            .padding(10)
            .background(.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.orange.opacity(0.35)))
            .padding(.horizontal, 16).padding(.top, 8)
        }
    }
}

/// Ursa's relays refuse a device without their token; says so, and where to put one.
struct RelayTokenWarning: View {
    @Environment(AppModel.self) private var model
    /// Inset from the window's edges, as a banner over a screen.
    var banner = false
    @State private var showNetwork = false
    var body: some View {
        if model.relayTokenMissing {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "lock.trianglebadge.exclamationmark").foregroundStyle(.orange)
                VStack(alignment: .leading, spacing: 6) {
                    Text("No access to Ursa's relays").fontWeight(.semibold)
                    Text("This build has no token for Ursa's relays, so it can't reach your other devices or nodes through them. Add the relay token in Network (Settings › Relays), or use a release build.")
                        .fixedSize(horizontal: false, vertical: true)
                    Button("Network…") { showNetwork = true }
                }
                Spacer(minLength: 0)
            }
            .padding(10)
            .background(.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.orange.opacity(0.35)))
            .padding(.horizontal, banner ? 16 : 0)
            .padding(.top, banner ? 8 : 0)
            .sheet(isPresented: $showNetwork) { NetworkSheet() }
        }
    }
}

/// What a screen shows when it has nothing to show.
struct EmptyState: View {
    var symbol: String
    var title: String
    var message: String
    var action: String?
    var run: (() -> Void)?
    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: symbol)
        } description: {
            Text(message)
        } actions: {
            if let action, let run {
                Button(action, action: run).controlSize(.large).buttonStyle(.borderedProminent)
            }
        }
    }
}

enum Pasteboard {
    static func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}

enum Panels {
    /// Files and folders to upload.
    @MainActor
    static func chooseUploads() -> [URL] {
        let p = NSOpenPanel()
        p.canChooseFiles = true
        p.canChooseDirectories = true
        p.allowsMultipleSelection = true
        p.prompt = "Upload"
        p.message = "Choose files or folders to upload. They're encrypted on this Mac before they leave it."
        return p.runModal() == .OK ? p.urls : []
    }

    /// A folder to download into.
    @MainActor
    static func chooseFolder(prompt: String = "Download Here") -> URL? {
        let p = NSOpenPanel()
        p.canChooseFiles = false
        p.canChooseDirectories = true
        p.canCreateDirectories = true
        p.prompt = prompt
        p.directoryURL = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
        return p.runModal() == .OK ? p.url : nil
    }

    /// Where to save one file.
    @MainActor
    static func saveLocation(name: String) -> URL? {
        let p = NSSavePanel()
        p.nameFieldStringValue = name
        p.canCreateDirectories = true
        p.directoryURL = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
        return p.runModal() == .OK ? p.url : nil
    }

    /// `name` in `dir`, or "name 2" and on if a file is there already.
    static func unique(_ dir: URL, _ name: String) -> URL {
        var url = dir.appendingPathComponent(name)
        let ext = (name as NSString).pathExtension
        let stem = ext.isEmpty ? name : (name as NSString).deletingPathExtension
        var i = 2
        while FileManager.default.fileExists(atPath: url.path) {
            url = dir.appendingPathComponent(ext.isEmpty ? "\(stem) \(i)" : "\(stem) \(i).\(ext)")
            i += 1
        }
        return url
    }
}

/// What the menu bar's commands do on the screen in front.
struct PageActions {
    var upload: (() -> Void)?
    var newFolder: (() -> Void)?
    var refresh: (() -> Void)?
}

extension FocusedValues {
    @Entry var pageActions: PageActions?
}
