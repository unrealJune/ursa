import AVFoundation
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// What the library needs to know about a file before it uploads: type, dates, size, thumbnail.
struct MediaInfo: Sendable {
    var mime: String
    var createdMs: Int64
    var modifiedMs: Int64
    var width: UInt32?
    var height: UInt32?
    /// A JPEG made for this upload, to delete once it is done.
    var thumbPath: String?

    private static let thumbLongSide = 640

    static func inspect(_ url: URL) async -> MediaInfo {
        let values = try? url.resourceValues(forKeys: [.contentTypeKey, .creationDateKey, .contentModificationDateKey])
        let type = values?.contentType ?? UTType(filenameExtension: url.pathExtension) ?? .data
        let mime = type.preferredMIMEType ?? "application/octet-stream"
        let modified = values?.contentModificationDate ?? Date()
        var info = MediaInfo(mime: mime, createdMs: ms(values?.creationDate ?? modified), modifiedMs: ms(modified))
        // Photos made in another app can carry a creation date later than when they were taken.
        if info.createdMs > info.modifiedMs { info.createdMs = info.modifiedMs }

        let thumb = LocalStore.thumbScratch.appendingPathComponent(UUID().uuidString + ".jpg")
        if type.conforms(to: .image) {
            image(url, thumb, &info)
        } else if type.conforms(to: .movie) || type.conforms(to: .video) {
            await video(url, thumb, &info)
        }
        return info
    }

    private init(mime: String, createdMs: Int64, modifiedMs: Int64) {
        self.mime = mime
        self.createdMs = createdMs
        self.modifiedMs = modifiedMs
    }

    private static func ms(_ d: Date) -> Int64 { Int64((d.timeIntervalSince1970 * 1000).rounded()) }

    private static func image(_ url: URL, _ thumb: URL, _ info: inout MediaInfo) {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any] else { return }
        if var w = props[kCGImagePropertyPixelWidth] as? Int, var h = props[kCGImagePropertyPixelHeight] as? Int {
            // EXIF orientations 5–8 turn the picture on its side.
            if let o = props[kCGImagePropertyOrientation] as? Int, (5...8).contains(o) { swap(&w, &h) }
            info.width = UInt32(w)
            info.height = UInt32(h)
        }
        if let exif = props[kCGImagePropertyExifDictionary] as? [CFString: Any],
           let taken = exif[kCGImagePropertyExifDateTimeOriginal] as? String {
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US_POSIX")
            f.dateFormat = "yyyy:MM:dd HH:mm:ss"
            if let offset = exif[kCGImagePropertyExifOffsetTimeOriginal] as? String {
                f.dateFormat = "yyyy:MM:dd HH:mm:ssXXXXX"
                if let d = f.date(from: taken + offset) { info.createdMs = ms(d) }
            } else if let d = f.date(from: taken) {
                info.createdMs = ms(d)
            }
        }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: thumbLongSide,
        ]
        if let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary), writeJPEG(cg, to: thumb) {
            info.thumbPath = thumb.path
        }
    }

    private static func video(_ url: URL, _ thumb: URL, _ info: inout MediaInfo) async {
        let asset = AVURLAsset(url: url)
        if let track = try? await asset.loadTracks(withMediaType: .video).first,
           let (natural, transform) = try? await track.load(.naturalSize, .preferredTransform) {
            let r = CGRect(origin: .zero, size: natural).applying(transform)
            if r.width > 0, r.height > 0 {
                info.width = UInt32(abs(r.width).rounded())
                info.height = UInt32(abs(r.height).rounded())
            }
        }
        if let made = try? await asset.load(.creationDate), let d = try? await made.load(.dateValue) {
            info.createdMs = ms(d)
        }
        let gen = AVAssetImageGenerator(asset: asset)
        gen.appliesPreferredTrackTransform = true
        gen.maximumSize = CGSize(width: thumbLongSide, height: thumbLongSide)
        if let (cg, _) = try? await gen.image(at: CMTime(seconds: 0.5, preferredTimescale: 600)), writeJPEG(cg, to: thumb) {
            info.thumbPath = thumb.path
        }
    }

    private static func writeJPEG(_ image: CGImage, to url: URL) -> Bool {
        guard let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil) else { return false }
        CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 0.82] as CFDictionary)
        return CGImageDestinationFinalize(dest)
    }
}
