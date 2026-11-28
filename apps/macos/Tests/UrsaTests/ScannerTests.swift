import AppKit
import Testing
@testable import Ursa

@Suite struct ScannerTests {
    @Test func readsTheQRCodesUrsaDraws() throws {
        let code = "ursa-link:" + String(repeating: "ab", count: 32) + "@endpointabqxsl6ujfbbiltfrm4ztq36nfeltbheiai7xssstgqoendbylogibaaenuhi5dq"
        // As another device shows it: the QR drawn large, on a white margin.
        let small = try #require(QRCode.image(code)?.cgImage(forProposedRect: nil, context: nil, hints: nil))
        let side = 600
        let ctx = try #require(CGContext(data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: 0,
                                         space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue))
        ctx.setFillColor(.white)
        ctx.fill(CGRect(x: 0, y: 0, width: side, height: side))
        ctx.interpolationQuality = .none
        ctx.draw(small, in: CGRect(x: 60, y: 60, width: side - 120, height: side - 120))
        let image = try #require(ctx.makeImage())
        #expect(QRReader.codes(in: image) == [code])
    }

    @Test func tellsCodeKindsApart() {
        #expect(CodeKind.of("  ursa-link:abc@x ") == .link)
        #expect(CodeKind.of("ursa-claim:abc@x") == .claim)
        #expect(CodeKind.of("ursa-invite:abc") == .invite)
        #expect(CodeKind.of("https://example.com") == nil)
    }
}
