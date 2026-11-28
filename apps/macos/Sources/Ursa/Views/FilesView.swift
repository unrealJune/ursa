import QuickLook
import SwiftUI

/// A folder or a file in the folder being shown.
struct FileRow: Identifiable {
    var folder: FolderEntry?
    var file: LibraryFile?

    var id: String { folder.map { "d:" + $0.id } ?? "f:" + file!.fileId }
    var isFolder: Bool { folder != nil }
    var name: String { folder?.name ?? file!.name }
    var symbol: String { isFolder ? "folder.fill" : file!.symbol }
    var modified: Date? { file?.modified }
    var bytes: UInt64 { file?.size ?? 0 }
}

/// The library as folders: browse, make folders, upload files and folders into the one shown,
/// download, rename, move and delete. The same tree every device shows.
struct FilesView: View {
    @Environment(AppModel.self) private var model
    /// The folder shown; nil is the top. Kept while the app runs.
    @SceneStorage("filesFolder") private var current: String?
    @State private var selection: Set<FileRow.ID> = []
    @State private var sortOrder = [KeyPathComparator(\FileRow.name, comparator: .localizedStandard)]
    @State private var dropTargeted = false
    @State private var preview: URL?

    @State private var naming: Naming?
    @State private var nameText = ""
    @State private var moving: [FileRow] = []
    @State private var confirmDelete: [FileRow] = []

    struct Naming {
        enum Kind { case newFolder, rename(FileRow) }
        var kind: Kind
        var title: String
        var action: String
    }

    var body: some View {
        VStack(spacing: 0) {
            breadcrumb
            Divider()
            table
        }
        .navigationSubtitle(summary)
        .toolbar { toolbar }
        .focusedSceneValue(\.pageActions, PageActions(
            upload: model.connected ? upload : nil,
            newFolder: model.connected ? newFolder : nil,
            refresh: refresh))
        .quickLookPreview($preview)
        .onAppear {
            if current != nil && model.folder(current) == nil { current = nil }
            if model.connected && model.items.isEmpty && model.folders.isEmpty { refresh() }
        }
        .onChange(of: model.libraryVersion) {
            // The folder may be gone (deleted here or on another device): show the top.
            if current != nil && model.folder(current) == nil { current = nil }
            let ids = Set(rows.map(\.id))
            selection.formIntersection(ids)
        }
        .alert(naming?.title ?? "", isPresented: Binding(get: { naming != nil }, set: { if !$0 { naming = nil } })) {
            TextField("Name", text: $nameText)
            Button(naming?.action ?? "OK", action: commitName)
            Button("Cancel", role: .cancel) {}
        }
        .alert(deleteTitle, isPresented: Binding(get: { !confirmDelete.isEmpty }, set: { if !$0 { confirmDelete = [] } })) {
            Button("Delete", role: .destructive) { delete(confirmDelete) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(deleteMessage)
        }
        .sheet(isPresented: Binding(get: { !moving.isEmpty }, set: { if !$0 { moving = [] } })) {
            MoveSheet(rows: moving, current: current) { to in move(moving, to: to) }
        }
    }

    // ---------------------------------------------------------------------------------------

    private var rows: [FileRow] {
        let folders = model.folders.filter { $0.parent == current }.map { FileRow(folder: $0) }
        let files = model.items.filter { $0.dir == current }.map { FileRow(file: $0) }
        // Folders first, as in Finder's default.
        return folders.sorted(using: sortOrder) + files.sorted(using: sortOrder)
    }

    private var chain: [FolderEntry] {
        var out: [FolderEntry] = []
        var at = model.folder(current)
        while let f = at {
            out.insert(f, at: 0)
            at = model.folder(f.parent)
        }
        return out
    }

    private var breadcrumb: some View {
        HStack(spacing: 4) {
            Button {
                open(nil)
            } label: {
                Label("Library", systemImage: "externaldrive.connected.to.line.below")
            }
            .buttonStyle(.borderless)
            .fontWeight(current == nil ? .semibold : .regular)
            ForEach(chain) { f in
                Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary)
                Button(f.name) { open(f.id) }
                    .buttonStyle(.borderless)
                    .fontWeight(f.id == current ? .semibold : .regular)
            }
            Spacer()
        }
        .foregroundStyle(.primary)
        .lineLimit(1)
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
    }

    @ViewBuilder
    private var table: some View {
        let rows = rows
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("Name", value: \.name, comparator: .localizedStandard) { row in
                Label {
                    Text(row.name)
                } icon: {
                    Image(systemName: row.symbol).foregroundStyle(row.isFolder ? Color.accentColor : .secondary)
                }
            }
            TableColumn("Date Modified", value: \.modifiedKey) { row in
                Text(row.modified?.formatted(date: .abbreviated, time: .shortened) ?? "--").foregroundStyle(.secondary)
            }
            .width(min: 140, ideal: 180)
            TableColumn("Size", value: \.bytes) { row in
                Text(row.isFolder ? "--" : sizeText(row.bytes)).foregroundStyle(.secondary).monospacedDigit()
            }
            .width(min: 70, ideal: 90)
        }
        .contextMenu(forSelectionType: FileRow.ID.self) { ids in
            let targets = rows.filter { ids.contains($0.id) }
            if !targets.isEmpty {
                if targets.count == 1 {
                    Button("Open") { activate(targets[0]) }
                    if !targets[0].isFolder { Button("Quick Look") { quickLook(targets[0]) } }
                }
                Button(targets.count > 1 ? "Download \(targets.count) Items…" : "Download…") { download(targets) }
                Divider()
                Button("Rename…") { rename(targets[0]) }.disabled(targets.count != 1)
                Button("Move To…") { moving = targets }
                Divider()
                Button(targets.count > 1 ? "Delete \(targets.count) Items…" : "Delete…", role: .destructive) { confirmDelete = targets }
            } else {
                Button("New Folder", action: newFolder)
                Button("Upload…", action: upload)
            }
        } primaryAction: { ids in
            if let row = rows.first(where: { ids.contains($0.id) }), ids.count == 1 { activate(row) }
        }
        .disabled(!model.connected)
        .onKeyPress(.space) {
            guard selection.count == 1, let row = rows.first(where: { selection.contains($0.id) }), !row.isFolder else { return .ignored }
            quickLook(row)
            return .handled
        }
        .onDeleteCommand { confirmDelete = rows.filter { selection.contains($0.id) } }
        .overlay {
            if rows.isEmpty && !model.busy {
                if model.connected {
                    EmptyState(symbol: "folder", title: current == nil ? "Your library is empty" : "This folder is empty",
                               message: "Drop files or folders here, or upload them. They're encrypted on this Mac before they leave it.",
                               action: "Upload…", run: upload)
                } else {
                    EmptyState(symbol: "wifi.exclamationmark", title: "Can't reach your nodes", message: model.detailText,
                               action: "Try Again", run: refresh)
                }
            }
        }
        .dropDestination(for: URL.self) { urls, _ in drop(urls) } isTargeted: { dropTargeted = $0 }
        .overlay { if dropTargeted { DropHint(text: "Drop to upload to \(model.folder(current)?.name ?? "Library")") } }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            Button("Back", systemImage: "chevron.left") { open(model.folder(current)?.parent) }
                .disabled(current == nil)
                .keyboardShortcut("[", modifiers: .command)
                .help("Enclosing folder (⌘[)")
        }
        ToolbarItemGroup {
            Button("New Folder", systemImage: "folder.badge.plus", action: newFolder)
                .disabled(!model.connected)
                .help("New folder (⇧⌘N)")
            Button("Upload…", systemImage: "square.and.arrow.up", action: upload)
                .disabled(!model.connected)
                .help("Upload files or folders here (⌘U)")
            Button("Download…", systemImage: "square.and.arrow.down") { download(selectedRows) }
                .disabled(selection.isEmpty || !model.connected)
            Button("Delete…", systemImage: "trash") { confirmDelete = selectedRows }
                .disabled(selection.isEmpty || !model.connected)
            Button("Refresh", systemImage: "arrow.clockwise", action: refresh)
                .help("Check for changes from your other devices (⌘R)")
        }
    }

    // ---------------------------------------------------------------------------------------

    private var selectedRows: [FileRow] { rows.filter { selection.contains($0.id) } }

    private var summary: String {
        guard model.connected else { return model.detailText }
        let r = rows
        let folders = r.filter(\.isFolder).count, files = r.count - folders
        let bytes = r.reduce(UInt64(0)) { $0 + $1.bytes }
        return "\(folders) folder\(folders == 1 ? "" : "s") · \(files.formatted()) file\(files == 1 ? "" : "s") · \(sizeText(bytes))"
    }

    private func open(_ folderId: String?) {
        current = folderId
        selection = []
    }

    /// A folder opens in place; a file opens in its app, fetched first.
    private func activate(_ row: FileRow) {
        if let f = row.folder {
            open(f.id)
            return
        }
        Task {
            await model.attempt {
                let url = try await model.original(row.file!)
                NSWorkspace.shared.open(url)
            }
        }
    }

    private func quickLook(_ row: FileRow) {
        guard let file = row.file else { return }
        Task { await model.attempt { preview = try await model.original(file) } }
    }

    private func refresh() {
        Task { await model.attempt { try await model.refreshAll() } }
    }

    private func newFolder() {
        nameText = model.freeName(in: current, "untitled folder", isFolder: true)
        naming = Naming(kind: .newFolder, title: "New Folder", action: "Create")
    }

    private func rename(_ row: FileRow) {
        nameText = row.name
        naming = Naming(kind: .rename(row), title: "Rename “\(row.name)”", action: "Rename")
    }

    private func commitName() {
        guard let n = naming else { return }
        let name = nameText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        switch n.kind {
        case .newFolder:
            let parent = current
            Task { await model.attempt { try await model.edit([.createFolder(parent: parent, name: name)]) } }
        case .rename(let row):
            guard name != row.name else { return }
            let edit: Edit = row.folder.map { .renameFolder(id: $0.id, name: name) } ?? .renameFile(fileId: row.file!.fileId, name: name)
            Task { await model.attempt { try await model.edit([edit]) } }
        }
    }

    private func move(_ rows: [FileRow], to: String?) {
        let edits: [Edit] = rows.map { r in
            r.folder.map { .moveFolder(id: $0.id, parent: to) } ?? .moveFile(fileId: r.file!.fileId, dir: to)
        }
        Task { await model.attempt { try await model.edit(edits) } }
    }

    private var deleteTitle: String {
        confirmDelete.count == 1 ? "Delete “\(confirmDelete[0].name)”?" : "Delete \(confirmDelete.count) items?"
    }

    private var deleteMessage: String {
        var inside = Set<String>()
        for r in confirmDelete { if let f = r.folder { inside.formUnion(model.subtree(f.id)) } }
        let n = model.items.filter { $0.dir.map(inside.contains) == true }.count
        return n > 0
            ? "This also deletes the \(n.formatted()) file\(n == 1 ? "" : "s") inside, from your library on every device."
            : "They'll be removed from your library on every device."
    }

    private func delete(_ rows: [FileRow]) {
        let edits: [Edit] = rows.map { r in r.folder.map { .deleteFolder(id: $0.id) } ?? .deleteFile(fileId: r.file!.fileId) }
        Task { await model.attempt { try await model.edit(edits) } }
    }

    private func upload() {
        let urls = Panels.chooseUploads()
        guard !urls.isEmpty else { return }
        let into = current
        Task { await model.attempt { try await model.upload(urls, into: into) } }
    }

    private func drop(_ urls: [URL]) -> Bool {
        let files = urls.filter(\.isFileURL)
        guard model.connected, !files.isEmpty else { return false }
        let into = current
        Task { await model.attempt { try await model.upload(files, into: into) } }
        return true
    }

    private func download(_ rows: [FileRow]) {
        guard !rows.isEmpty else { return }
        if rows.count == 1, let file = rows[0].file {
            guard let url = Panels.saveLocation(name: file.name) else { return }
            Task { await model.attempt { try await model.download(file, to: url) } }
            return
        }
        guard let dir = Panels.chooseFolder() else { return }
        Task {
            await model.attempt {
                for row in rows {
                    if let f = row.folder {
                        _ = try await model.downloadFolder(f.id, into: dir)
                    } else if let file = row.file {
                        try await model.download(file, to: Panels.unique(dir, file.name))
                    }
                }
                NSWorkspace.shared.activateFileViewerSelecting([dir])
            }
        }
    }
}

extension FileRow {
    /// Folders have no date; they sort before every file.
    var modifiedKey: Date { modified ?? .distantPast }
}

/// Where to move items: every folder but the ones being moved and what is inside them.
struct MoveSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var rows: [FileRow]
    var current: String?
    var move: (String?) -> Void
    @State private var target: String = ""

    private static let top = "\u{0}top"

    var body: some View {
        let blocked = rows.compactMap(\.folder).reduce(into: Set<String>()) { $0.formUnion(model.subtree($1.id)) }
        let targets = model.folders.filter { !blocked.contains($0.id) }
            .sorted { $0.path.localizedStandardCompare($1.path) == .orderedAscending }
        VStack(alignment: .leading, spacing: 12) {
            Text(rows.count == 1 ? "Move “\(rows[0].name)” to" : "Move \(rows.count) items to").font(.headline)
            List(selection: $target) {
                Label("Library", systemImage: "externaldrive.connected.to.line.below").tag(Self.top)
                ForEach(targets) { f in
                    Label(f.path.replacingOccurrences(of: "/", with: " › "), systemImage: "folder").tag(f.id)
                }
            }
            .listStyle(.bordered)
            .frame(minHeight: 260)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Move Here") {
                    move(target == Self.top ? nil : target)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(target.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 460, height: 400)
        .onAppear { target = current ?? Self.top }
    }
}
