import AVKit
import SwiftUI

/// The library's photos and videos as a wall, newest first, in justified rows under month
/// headings (other files are in Files). Click to select (⌘ and ⇧ as in Finder), double-click or
/// Space to view.
struct PhotosView: View {
    @Environment(AppModel.self) private var model
    @AppStorage("photoRowHeight") private var rowHeight = 190.0
    @State private var selection: Set<String> = []
    /// Where a ⇧-click range starts.
    @State private var anchor: String?
    @State private var viewing: LibraryFile?
    @State private var confirmDelete: [LibraryFile] = []
    @State private var dropTargeted = false
    @FocusState private var gridFocused: Bool

    private let spacing: CGFloat = 4

    /// What the wall shows: photos and videos, from every folder.
    private var media: [LibraryFile] { model.items.filter(\.isMedia) }

    var body: some View {
        ZStack {
            content
            if let item = viewing {
                Viewer(item: item, items: media, open: { viewing = $0 }, close: close,
                       download: { download([$0]) }, delete: { confirmDelete = [$0] })
                    .transition(.opacity)
            }
        }
        .navigationSubtitle(summary)
        .toolbar { toolbar }
        .focusedSceneValue(\.pageActions, PageActions(upload: model.connected ? upload : nil, refresh: refresh))
        .alert(deleteTitle, isPresented: Binding(get: { !confirmDelete.isEmpty }, set: { if !$0 { confirmDelete = [] } })) {
            Button("Delete", role: .destructive) { delete(confirmDelete) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("They'll be removed from your library on every device.")
        }
        .onChange(of: model.libraryVersion) {
            let ids = Set(media.map(\.fileId))
            selection.formIntersection(ids)
            if let v = viewing, !ids.contains(v.fileId) { viewing = nil }
        }
    }

    // ---------------------------------------------------------------------------------------

    @ViewBuilder
    private var content: some View {
        if media.isEmpty && !model.busy {
            empty
                .dropDestination(for: URL.self) { urls, _ in drop(urls) } isTargeted: { dropTargeted = $0 }
                .overlay { if dropTargeted { DropHint(text: "Drop to upload") } }
        } else {
            GeometryReader { geo in
                let sections = Self.layout(media, width: geo.size.width - 2 * 12, target: rowHeight, spacing: spacing)
                ScrollViewReader { scroller in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: spacing, pinnedViews: .sectionHeaders) {
                            ForEach(sections) { section in
                                Section {
                                    ForEach(section.rows) { row in
                                        HStack(spacing: spacing) {
                                            ForEach(row.tiles, id: \.item.fileId) { tile in
                                                tileView(tile.item)
                                                    .frame(width: tile.width, height: row.height)
                                                    .id(tile.item.fileId)
                                            }
                                        }
                                    }
                                } header: {
                                    Text(section.title)
                                        .font(.headline)
                                        .padding(.vertical, 6)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .background(.background.opacity(0.92))
                                }
                            }
                        }
                        .padding(.horizontal, 12)
                        .padding(.bottom, 12)
                        // A click between tiles clears the selection; tiles take their own clicks.
                        .background {
                            Color.clear.contentShape(Rectangle()).onTapGesture { selection = [] }
                        }
                    }
                    .onChange(of: viewing?.fileId) { old, new in
                        if new == nil, let old { scroller.scrollTo(old) }
                    }
                }
            }
            .focusable()
            .focusEffectDisabled()
            .focused($gridFocused)
            .onAppear { gridFocused = true }
            .onKeyPress(.space) { openSelected() }
            .onKeyPress(.return) { openSelected() }
            .onDeleteCommand { confirmDelete = selectedItems }
            .onCommand(#selector(NSResponder.selectAll(_:))) { selection = Set(media.map(\.fileId)) }
            .dropDestination(for: URL.self) { urls, _ in drop(urls) } isTargeted: { dropTargeted = $0 }
            .overlay { if dropTargeted { DropHint(text: "Drop to upload") } }
        }
    }

    private func tileView(_ item: LibraryFile) -> some View {
        let selected = selection.contains(item.fileId)
        return Thumbnail(item: item)
            .clipped()
            .overlay(alignment: .bottomLeading) {
                if item.isVideo {
                    Image(systemName: "play.fill").font(.caption).foregroundStyle(.white)
                        .shadow(radius: 2).padding(6)
                } else if !item.isMedia {
                    Text(item.name).font(.caption2).lineLimit(1).padding(6)
                }
            }
            .overlay {
                if selected {
                    Rectangle().strokeBorder(Color.accentColor, lineWidth: 3)
                        .background(Color.accentColor.opacity(0.15))
                }
            }
            .overlay(alignment: .topTrailing) {
                if selected {
                    Image(systemName: "checkmark.circle.fill")
                        .symbolRenderingMode(.palette)
                        .foregroundStyle(.white, Color.accentColor)
                        .font(.title3).padding(5)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture(count: 2) { viewing = item }
            .simultaneousGesture(TapGesture().onEnded { click(item) })
            .contextMenu {
                let targets = selection.contains(item.fileId) ? selectedItems : [item]
                Button("Open") { viewing = item }
                Button(targets.count > 1 ? "Download \(targets.count) Items…" : "Download…") { download(targets) }
                Divider()
                Button(targets.count > 1 ? "Delete \(targets.count) Items…" : "Delete…", role: .destructive) { confirmDelete = targets }
            }
            .help(item.name)
    }

    private var empty: some View {
        Group {
            if !model.connected {
                EmptyState(symbol: "wifi.exclamationmark", title: "Can't reach your nodes",
                           message: model.detailText.isEmpty ? "They may be offline. Ursa will keep trying." : model.detailText,
                           action: "Try Again", run: refresh)
            } else {
                EmptyState(symbol: "photo.on.rectangle.angled", title: model.items.isEmpty ? "Your library is empty" : "No photos or videos yet",
                           message: "Drop photos and videos here, or upload them. They're encrypted on this Mac before they leave it. Other files are in Files.",
                           action: "Upload…", run: upload)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup {
            Slider(value: $rowHeight, in: 110...360) {
                Text("Size")
            } minimumValueLabel: {
                Image(systemName: "square.grid.3x3").imageScale(.small)
            } maximumValueLabel: {
                Image(systemName: "square.grid.2x2").imageScale(.small)
            }
            .frame(width: 140)
            .help("Thumbnail size")
            Button("Upload…", systemImage: "square.and.arrow.up", action: upload)
                .disabled(!model.connected)
                .help("Upload files or folders (⌘U)")
            Button("Download…", systemImage: "square.and.arrow.down") { download(selectedItems) }
                .disabled(selection.isEmpty || !model.connected)
                .help("Download the selected items")
            Button("Delete…", systemImage: "trash") { confirmDelete = selectedItems }
                .disabled(selection.isEmpty || !model.connected)
                .help("Delete the selected items")
            Button("Refresh", systemImage: "arrow.clockwise", action: refresh)
                .help("Check for changes from your other devices (⌘R)")
        }
    }

    // ---------------------------------------------------------------------------------------

    private var summary: String {
        let all = media
        if all.isEmpty { return model.detailText }
        let photos = all.filter(\.isImage).count, videos = all.count - photos
        let bytes = all.reduce(UInt64(0)) { $0 + $1.size }
        let sel = selection.isEmpty ? "" : "\(selection.count) selected · "
        let counts = [photos > 0 ? "\(photos.formatted()) photo\(photos == 1 ? "" : "s")" : nil,
                      videos > 0 ? "\(videos.formatted()) video\(videos == 1 ? "" : "s")" : nil].compactMap { $0 }
        return "\(sel)\(counts.joined(separator: ", ")) · \(sizeText(bytes))"
    }

    private var selectedItems: [LibraryFile] { media.filter { selection.contains($0.fileId) } }

    private var deleteTitle: String {
        confirmDelete.count == 1 ? "Delete “\(confirmDelete[0].name)”?" : "Delete \(confirmDelete.count) items?"
    }

    private func click(_ item: LibraryFile) {
        gridFocused = true
        let flags = NSEvent.modifierFlags
        if flags.contains(.shift), let a = anchor,
           let i = media.firstIndex(where: { $0.fileId == a }),
           let j = media.firstIndex(where: { $0.fileId == item.fileId }) {
            selection.formUnion(media[min(i, j)...max(i, j)].map(\.fileId))
        } else if flags.contains(.command) {
            if selection.contains(item.fileId) { selection.remove(item.fileId) } else { selection.insert(item.fileId) }
            anchor = item.fileId
        } else {
            selection = [item.fileId]
            anchor = item.fileId
        }
    }

    private func openSelected() -> KeyPress.Result {
        guard viewing == nil, let first = media.first(where: { selection.contains($0.fileId) }) else { return .ignored }
        viewing = first
        return .handled
    }

    private func close() {
        if let v = viewing { selection = [v.fileId] }
        viewing = nil
        gridFocused = true
    }

    private func upload() {
        let urls = Panels.chooseUploads()
        guard !urls.isEmpty else { return }
        Task { await model.attempt { try await model.upload(urls, into: nil) } }
    }

    private func drop(_ urls: [URL]) -> Bool {
        let files = urls.filter(\.isFileURL)
        guard model.connected, !files.isEmpty else { return false }
        Task { await model.attempt { try await model.upload(files, into: nil) } }
        return true
    }

    private func refresh() {
        Task { await model.attempt { try await model.refreshAll() } }
    }

    private func download(_ items: [LibraryFile]) {
        guard !items.isEmpty else { return }
        if items.count == 1 {
            guard let url = Panels.saveLocation(name: items[0].name) else { return }
            Task { await model.attempt { try await model.download(items[0], to: url) } }
            return
        }
        guard let dir = Panels.chooseFolder() else { return }
        Task {
            await model.attempt {
                for item in items { try await model.download(item, to: Panels.unique(dir, item.name)) }
            }
        }
    }

    private func delete(_ items: [LibraryFile]) {
        if let v = viewing, items.contains(where: { $0.fileId == v.fileId }) {
            // The next one takes its place, as in Photos.
            let i = media.firstIndex(where: { $0.fileId == v.fileId }) ?? 0
            let rest = media.filter { item in !items.contains { $0.fileId == item.fileId } }
            viewing = rest.isEmpty ? nil : rest[min(i, rest.count - 1)]
        }
        selection.subtract(items.map(\.fileId))
        Task { await model.attempt { try await model.delete(items) } }
    }

    // ---------------------------------------------------------------------------------------
    // Layout: computed whole from the aspect ratios, so rows justify without measuring.

    struct Tile { var item: LibraryFile; var width: CGFloat }
    struct Row: Identifiable { var id: String; var tiles: [Tile]; var height: CGFloat }
    struct MonthSection: Identifiable { var id: String; var title: String; var rows: [Row] }

    static func layout(_ items: [LibraryFile], width: CGFloat, target: CGFloat, spacing: CGFloat) -> [MonthSection] {
        guard width > 50 else { return [] }
        let cal = Calendar.current
        var sections: [MonthSection] = []
        var group: [LibraryFile] = []
        var key: DateComponents?

        func flush() {
            guard let first = group.first else { return }
            sections.append(MonthSection(id: "\(key?.year ?? 0)-\(key?.month ?? 0)",
                                         title: first.created.formatted(.dateTime.month(.wide).year()),
                                         rows: rows(group)))
            group = []
        }

        func rows(_ items: [LibraryFile]) -> [Row] {
            var out: [Row] = []
            var row: [LibraryFile] = []
            var aspects = 0.0
            for item in items {
                row.append(item)
                aspects += item.aspect
                let gaps = spacing * CGFloat(row.count - 1)
                if CGFloat(aspects) * target + gaps >= width {
                    let h = (width - gaps) / CGFloat(aspects)
                    out.append(Row(id: row[0].fileId, tiles: row.map { Tile(item: $0, width: CGFloat($0.aspect) * h) }, height: h))
                    row = []
                    aspects = 0
                }
            }
            if !row.isEmpty {
                // The last row keeps its height rather than stretching a few tiles across.
                out.append(Row(id: row[0].fileId, tiles: row.map { Tile(item: $0, width: CGFloat($0.aspect) * target) }, height: target))
            }
            return out
        }

        for item in items {
            let k = cal.dateComponents([.year, .month], from: item.created)
            if k != key {
                flush()
                key = k
            }
            group.append(item)
        }
        flush()
        return sections
    }
}

struct DropHint: View {
    var text: String
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 14)
                .strokeBorder(Color.accentColor, style: StrokeStyle(lineWidth: 3, dash: [10, 6]))
                .background(Color.accentColor.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
            Label(text, systemImage: "square.and.arrow.up").font(.title2.weight(.semibold)).foregroundStyle(Color.accentColor)
        }
        .padding(10)
        .allowsHitTesting(false)
    }
}

/// One file, full size: the thumbnail at once, then the original when it has arrived.
/// ← and → step through the library, Esc or Space closes.
struct Viewer: View {
    @Environment(AppModel.self) private var model
    var item: LibraryFile
    var items: [LibraryFile]
    var open: (LibraryFile) -> Void
    var close: () -> Void
    var download: (LibraryFile) -> Void
    var delete: (LibraryFile) -> Void

    @State private var image: NSImage?
    @State private var player: AVPlayer?
    @State private var loading = false
    @State private var failed: String?
    @FocusState private var focused: Bool

    private var index: Int? { items.firstIndex { $0.fileId == item.fileId } }

    var body: some View {
        ZStack {
            Color.black.opacity(0.94).ignoresSafeArea()
            Group {
                if let player {
                    VideoPlayer(player: player)
                } else if let image {
                    Image(nsImage: image).resizable().aspectRatio(contentMode: .fit)
                } else if item.hasThumb {
                    Thumbnail(item: item, contentMode: .fit)
                } else {
                    VStack(spacing: 12) {
                        Image(systemName: item.symbol).font(.system(size: 56)).foregroundStyle(.secondary)
                        Text(failed ?? (item.isMedia ? "" : "No preview. Download it to open it.")).foregroundStyle(.secondary)
                    }
                }
            }
            .padding(.top, 52)
            .padding([.horizontal, .bottom], 20)

            if loading {
                ProgressView().controlSize(.large).tint(.white)
            }

            HStack {
                navButton("chevron.left", index.map { $0 > 0 } ?? false) { step(-1) }
                Spacer()
                navButton("chevron.right", index.map { $0 < items.count - 1 } ?? false) { step(1) }
            }
            .padding(.horizontal, 10)

            VStack {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(item.name).font(.headline).lineLimit(1)
                        Text(item.details).font(.caption).foregroundStyle(.white.opacity(0.7))
                    }
                    Spacer()
                    if let failed, image != nil || item.hasThumb {
                        Label(failed, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(.orange)
                    }
                    Button("Download…", systemImage: "square.and.arrow.down") { download(item) }
                    Button("Delete…", systemImage: "trash") { delete(item) }
                    Button("Close", systemImage: "xmark", action: close).keyboardShortcut(.cancelAction)
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.white)
                .font(.title3)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .background(.black.opacity(0.4))
                Spacer()
            }
        }
        .environment(\.colorScheme, .dark)
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onAppear { focused = true }
        .onKeyPress(.leftArrow) { step(-1); return .handled }
        .onKeyPress(.rightArrow) { step(1); return .handled }
        .onKeyPress(.space) { close(); return .handled }
        .onDeleteCommand { delete(item) }
        .task(id: item.fileId) { await load() }
        .onDisappear { player?.pause() }
    }

    private func navButton(_ symbol: String, _ enabled: Bool, _ run: @escaping () -> Void) -> some View {
        Button(action: run) {
            Image(systemName: symbol).font(.title.weight(.semibold)).frame(width: 44, height: 64)
                .background(.black.opacity(0.35), in: RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .foregroundStyle(.white)
        .opacity(enabled ? 1 : 0)
        .disabled(!enabled)
    }

    private func step(_ delta: Int) {
        guard let i = index, items.indices.contains(i + delta) else { return }
        open(items[i + delta])
    }

    private func load() async {
        player?.pause()
        player = nil
        image = nil
        failed = nil
        guard item.isMedia else { return }
        loading = true
        defer { loading = false }
        do {
            let url = try await model.original(item)
            guard !Task.isCancelled else { return }
            if item.isVideo {
                let p = AVPlayer(url: url)
                player = p
                p.play()
            } else if let decoded = await Task.detached(operation: { Thumbs.decode(url, maxPixels: 4096) }).value {
                image = decoded
            } else {
                failed = "macOS can't display this format. Download it to open it."
            }
        } catch {
            if !Task.isCancelled { failed = "Couldn't open it: \(describe(error))" }
        }
    }
}
