import AVFoundation
import SwiftUI
import UniformTypeIdentifiers
import Vision

/// The kinds of `ursa-…:` code a field takes, as Android's scanner reads them.
enum CodeKind: String {
    case link, claim, invite

    var prefix: String { "ursa-\(rawValue):" }

    var noun: String {
        switch self {
        case .link: "link code"
        case .claim: "claim link"
        case .invite: "invite"
        }
    }

    static func of(_ text: String) -> CodeKind? {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return [CodeKind.link, .claim, .invite].first { t.hasPrefix($0.prefix) }
    }
}

/// A field for an `ursa-…:` code, filled by typing, pasting, or scanning the other screen's QR code.
struct CodeField: View {
    var title: String
    @Binding var text: String
    var kind: CodeKind
    var onSubmit: (() -> Void)?
    @State private var scanning = false

    var body: some View {
        HStack(spacing: 6) {
            TextField(title, text: $text, prompt: Text("\(kind.prefix)…"))
                .textFieldStyle(.roundedBorder)
                .onSubmit { onSubmit?() }
            Button {
                if let s = NSPasteboard.general.string(forType: .string)?.trimmingCharacters(in: .whitespacesAndNewlines), !s.isEmpty {
                    text = s
                }
            } label: {
                Image(systemName: "doc.on.clipboard")
            }
            .help("Paste")
            Button {
                scanning = true
            } label: {
                Image(systemName: "qrcode.viewfinder")
            }
            .help("Scan the QR code")
        }
        .sheet(isPresented: $scanning) {
            ScanSheet(kind: kind) { code in
                text = code
                onSubmit?()
            }
        }
    }
}

/// Reads one QR code of `kind` from a camera (the Mac's, or an iPhone through Continuity Camera),
/// an image file or a pasted image, and hands it back. Nothing is recorded.
struct ScanSheet: View {
    @Environment(\.dismiss) private var dismiss
    var kind: CodeKind
    var deliver: (String) -> Void

    @State private var camera = Camera()
    @State private var note: String?
    @State private var importing = false

    var body: some View {
        VStack(spacing: 14) {
            HStack {
                Text("Scan the \(kind.noun)").font(.title2.bold())
                Spacer()
                if camera.devices.count > 1 {
                    Picker("Camera", selection: Binding(get: { camera.deviceId ?? "" }, set: { camera.use($0) })) {
                        ForEach(camera.devices, id: \.uniqueID) { Text($0.localizedName).tag($0.uniqueID) }
                    }
                    .labelsHidden()
                    .frame(maxWidth: 220)
                }
            }

            ZStack {
                RoundedRectangle(cornerRadius: 12).fill(.black)
                switch camera.state {
                case .running:
                    CameraPreview(session: camera.session, mirrored: camera.mirrored)
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                    RoundedRectangle(cornerRadius: 24)
                        .strokeBorder(.white.opacity(0.9), lineWidth: 3)
                        .frame(width: 220, height: 220)
                        .allowsHitTesting(false)
                case .starting:
                    ProgressView().tint(.white)
                case .denied:
                    unavailable("Ursa may not use the camera",
                                "Allow it in System Settings › Privacy & Security › Camera, or open an image of the code.",
                                action: ("Open System Settings", {
                                    NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera")!)
                                }))
                case .noCamera:
                    unavailable("No camera", "Open an image of the code instead, or paste a screenshot (⌘V).", action: nil)
                case .failed(let message):
                    unavailable("The camera didn't start", message, action: nil)
                }
            }
            .frame(height: 340)

            Text(note ?? "Point the camera at the code on your other device.")
                .foregroundStyle(note == nil ? Color.secondary : Color.orange)
                .frame(maxWidth: .infinity, alignment: .leading)

            HStack {
                Button("Open Image…") { importing = true }
                Button("Paste Image") { readPasteboard() }.keyboardShortcut("v")
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
            }
        }
        .padding(20)
        .frame(width: 520)
        .onAppear {
            camera.onCode = handle
            camera.start()
        }
        .onDisappear { camera.stop() }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.image]) { result in
            guard case .success(let url) = result else { return }
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
                  let image = CGImageSourceCreateImageAtIndex(src, 0, nil) else {
                note = "Couldn't read that image."
                return
            }
            read(image)
        }
        .onDrop(of: [.image, .fileURL], isTargeted: nil) { providers in
            guard let p = providers.first else { return false }
            _ = p.loadDataRepresentation(for: .image) { data, _ in
                guard let data, let image = NSImage(data: data)?.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return }
                Task { @MainActor in read(image) }
            }
            return true
        }
    }

    private func unavailable(_ title: String, _ message: String, action: (String, () -> Void)?) -> some View {
        VStack(spacing: 10) {
            Image(systemName: "video.slash").font(.largeTitle)
            Text(title).font(.headline)
            Text(message).multilineTextAlignment(.center).foregroundStyle(.white.opacity(0.75))
            if let action { Button(action.0, action: action.1) }
        }
        .foregroundStyle(.white)
        .padding(30)
    }

    private func readPasteboard() {
        guard let image = NSImage(pasteboard: NSPasteboard.general)?.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            note = "There's no image on the clipboard."
            return
        }
        read(image)
    }

    private func read(_ image: CGImage) {
        let codes = QRReader.codes(in: image)
        if codes.isEmpty {
            note = "No QR code found in that image."
        } else {
            for c in codes { handle(c) }
        }
    }

    /// Takes the first code of the wanted kind; says so when it sees another kind.
    private func handle(_ payload: String) {
        let text = payload.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let seen = CodeKind.of(text) else { return }
        guard seen == kind else {
            note = "That's \(seen == .invite ? "an" : "a") \(seen.noun), not \(kind == .invite ? "an" : "a") \(kind.noun)."
            return
        }
        camera.onCode = nil
        camera.stop()
        NSSound(named: "Tink")?.play()
        dismiss()
        deliver(text)
    }
}

enum QRReader {
    /// Every QR payload in `image`.
    static func codes(in image: CGImage) -> [String] {
        let request = VNDetectBarcodesRequest()
        request.symbologies = [.qr]
        try? VNImageRequestHandler(cgImage: image).perform([request])
        return (request.results ?? []).compactMap(\.payloadStringValue)
    }

    static func codes(in buffer: CVPixelBuffer) -> [String] {
        let request = VNDetectBarcodesRequest()
        request.symbologies = [.qr]
        try? VNImageRequestHandler(cvPixelBuffer: buffer).perform([request])
        return (request.results ?? []).compactMap(\.payloadStringValue)
    }
}

/// A capture session that looks for QR codes in its frames, a few times a second.
@Observable
final class Camera: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate, @unchecked Sendable {
    enum State: Equatable { case starting, running, denied, noCamera, failed(String) }

    @ObservationIgnored let session = AVCaptureSession()
    @ObservationIgnored private let queue = DispatchQueue(label: "ursa.scanner")
    @ObservationIgnored private var lastScan = Date.distantPast
    @ObservationIgnored var onCode: (@MainActor (String) -> Void)?

    var state: State = .starting
    var devices: [AVCaptureDevice] = []
    var deviceId: String?
    /// The Mac's own camera faces you, so its picture is shown as a mirror; codes read either way.
    var mirrored = true

    func start() {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            configure()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { ok in
                Task { @MainActor in ok ? self.configure() : (self.state = .denied) }
            }
        default:
            state = .denied
        }
    }

    func stop() {
        queue.async { if self.session.isRunning { self.session.stopRunning() } }
    }

    func use(_ id: String) {
        guard let d = devices.first(where: { $0.uniqueID == id }) else { return }
        attach(d)
    }

    private func configure() {
        let found = AVCaptureDevice.DiscoverySession(
            deviceTypes: [.builtInWideAngleCamera, .continuityCamera, .external, .deskViewCamera],
            mediaType: .video, position: .unspecified
        ).devices.filter { $0.deviceType != .deskViewCamera }
        devices = found
        guard let first = AVCaptureDevice.default(for: .video) ?? found.first else {
            state = .noCamera
            return
        }
        let output = AVCaptureVideoDataOutput()
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(self, queue: queue)
        session.beginConfiguration()
        session.sessionPreset = .high
        if session.canAddOutput(output) { session.addOutput(output) }
        session.commitConfiguration()
        attach(first)
    }

    private func attach(_ device: AVCaptureDevice) {
        deviceId = device.uniqueID
        // An iPhone through Continuity Camera, or a webcam pointed away, is not a mirror.
        mirrored = device.deviceType == .builtInWideAngleCamera
        queue.async {
            self.session.beginConfiguration()
            for input in self.session.inputs { self.session.removeInput(input) }
            do {
                let input = try AVCaptureDeviceInput(device: device)
                guard self.session.canAddInput(input) else { throw CocoaError(.featureUnsupported) }
                self.session.addInput(input)
                self.session.commitConfiguration()
                if !self.session.isRunning { self.session.startRunning() }
                Task { @MainActor in self.state = .running }
            } catch {
                self.session.commitConfiguration()
                Task { @MainActor in self.state = .failed(error.localizedDescription) }
            }
        }
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        let now = Date()
        guard now.timeIntervalSince(lastScan) > 0.2, let buffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        lastScan = now
        let codes = QRReader.codes(in: buffer)
        guard !codes.isEmpty else { return }
        Task { @MainActor in for c in codes { self.onCode?(c) } }
    }
}

/// The camera's picture, filling the view.
struct CameraPreview: NSViewRepresentable {
    var session: AVCaptureSession
    var mirrored: Bool

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        view.wantsLayer = true
        let layer = AVCaptureVideoPreviewLayer(session: session)
        layer.videoGravity = .resizeAspectFill
        view.layer = layer
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        guard let layer = view.layer as? AVCaptureVideoPreviewLayer, let c = layer.connection, c.isVideoMirroringSupported else { return }
        c.automaticallyAdjustsVideoMirroring = false
        c.isVideoMirrored = mirrored
    }
}
