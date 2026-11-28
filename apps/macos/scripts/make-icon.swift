// Draws the app icon (the Windows logo's constellation, as apps/android/scripts/make-icons.py
// draws Android's) on a macOS rounded-square tile, at every size an .icns holds.
//
//   swift apps/macos/scripts/make-icon.swift <out.iconset>
//   iconutil -c icns <out.iconset> -o Ursa.icns
import AppKit

let out = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "Ursa.iconset")
try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

let top = NSColor(srgbRed: 56 / 255, green: 75 / 255, blue: 214 / 255, alpha: 1)
let bottom = NSColor(srgbRed: 119 / 255, green: 59 / 255, blue: 235 / 255, alpha: 1)
// The constellation on the Windows logo's 256 px grid: (x, y, radius).
let stars: [(CGFloat, CGFloat, CGFloat)] = [(47, 87, 13), (84, 77, 11), (120, 93, 11), (148, 122, 13), (215, 128, 14), (155, 170, 12), (207, 182, 12)]
let lines = [(0, 1), (1, 2), (2, 3), (3, 4), (4, 6), (6, 5), (5, 3)]

func draw(_ px: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4,
                               hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let s = CGFloat(px)
    // Apple's grid: an 824 tile in a 1024 canvas, corner radius ~185.
    let inset = s * 100 / 1024
    let tile = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    let shape = NSBezierPath(roundedRect: tile, xRadius: s * 185 / 1024, yRadius: s * 185 / 1024)
    NSGradient(starting: top, ending: bottom)!.draw(in: shape, angle: -90)

    // The 256 grid, scaled to 72% of the tile and centred (y flipped: AppKit's origin is bottom left).
    let k = tile.width * 0.72 / 256
    let ox = tile.minX + (tile.width - 256 * k) / 2 + 3 * k
    let oy = tile.minY + (tile.height - 256 * k) / 2
    func pt(_ i: Int) -> NSPoint { NSPoint(x: ox + stars[i].0 * k, y: oy + (256 - stars[i].1) * k) }
    NSColor.white.setStroke()
    NSColor.white.setFill()
    for (a, b) in lines {
        let p = NSBezierPath()
        p.move(to: pt(a))
        p.line(to: pt(b))
        p.lineWidth = 4 * k
        p.lineCapStyle = .round
        p.stroke()
    }
    for (i, star) in stars.enumerated() {
        let c = pt(i), r = star.2 * k
        NSBezierPath(ovalIn: NSRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r)).fill()
    }
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for size in [16, 32, 128, 256, 512] {
    try! draw(size).write(to: out.appendingPathComponent("icon_\(size)x\(size).png"))
    try! draw(size * 2).write(to: out.appendingPathComponent("icon_\(size)x\(size)@2x.png"))
}
