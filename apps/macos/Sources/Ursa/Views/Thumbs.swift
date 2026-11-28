import AppKit
import ImageIO
import SwiftUI

/// Thumbnails, decrypted by the core into its cache and decoded here once, a few at a time.
actor Thumbs {
    static let shared = Thumbs()

    // NSCache is thread-safe.
    nonisolated(unsafe) private let cache = NSCache<NSString, NSImage>()
    private var running = 0
    private var waiting: [CheckedContinuation<Void, Never>] = []

    init() { cache.countLimit = 2000 }

    nonisolated func cached(_ fileId: String) -> NSImage? { cache.object(forKey: fileId as NSString) }

    func image(_ fileId: String, model: AppModel) async -> NSImage? {
        if let hit = cache.object(forKey: fileId as NSString) { return hit }
        await acquire()
        defer { release() }
        if Task.isCancelled { return nil }
        if let hit = cache.object(forKey: fileId as NSString) { return hit }
        guard let path = try? await model.core.thumb(fileId: fileId),
              let image = Self.decode(URL(fileURLWithPath: path)) else { return nil }
        cache.setObject(image, forKey: fileId as NSString)
        return image
    }

    private func acquire() async {
        if running < 4 {
            running += 1
            return
        }
        await withCheckedContinuation { waiting.append($0) }
    }

    private func release() {
        if waiting.isEmpty { running -= 1 } else { waiting.removeFirst().resume() }
    }

    /// Decoded now, not when first drawn, so scrolling never waits on a JPEG.
    static func decode(_ url: URL, maxPixels: Int = 640) -> NSImage? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixels,
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { return nil }
        return NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
    }
}

/// A file's thumbnail, or its kind's symbol until there is one.
struct Thumbnail: View {
    @Environment(AppModel.self) private var model
    var item: LibraryFile
    var contentMode: ContentMode = .fill
    @State private var image: NSImage?

    var body: some View {
        ZStack {
            if let image = image ?? Thumbs.shared.cached(item.fileId) {
                Image(nsImage: image).resizable().aspectRatio(contentMode: contentMode)
            } else {
                Rectangle().fill(.quaternary)
                Image(systemName: item.symbol).font(.title2).foregroundStyle(.secondary)
            }
        }
        .task(id: item.fileId) {
            guard item.hasThumb, image == nil else { return }
            image = await Thumbs.shared.image(item.fileId, model: model)
        }
    }
}
