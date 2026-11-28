import SwiftUI

/// Who is in the pool, how the library is protected (any k of n pieces rebuild a file), what
/// each choice lets you store, how much you give friends, and switching between choices.
struct PoolView: View {
    @Environment(AppModel.self) private var model
    @State private var report: StorageReport?
    @State private var problem: String?
    @State private var selected: StorageReport.Shape?
    @State private var custom: StorageReport.Shape?
    @State private var customK = 3
    @State private var customN = 5
    @State private var giveGb = 0.0
    @State private var giveTask: Task<Void, Never>?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                if let problem {
                    Label(problem, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).textSelection(.enabled)
                }
                if let r = report {
                    SectionTitle(text: "Members")
                    Card { members(r) }
                    SectionTitle(text: "Protection")
                    Hint("Ursa codes every file into n pieces on different people's nodes; any k of them rebuild it. More pieces survive more nodes being down, and cost more space.")
                    choices(r)
                    customRow
                    SectionTitle(text: "Capacity")
                    Card { capacity(r) }
                    switchRow(r)
                    SectionTitle(text: "Give friends")
                    Card { give(r) }
                } else if problem == nil {
                    ProgressView().frame(maxWidth: .infinity).padding(.top, 80)
                }
            }
            .frame(maxWidth: 880, alignment: .leading)
            .padding(24)
            .frame(maxWidth: .infinity)
        }
        .task { await load(keepSelection: false) }
        .onChange(of: model.switchEnded) {
            if let e = model.switchError { problem = e }
            Task { await load(keepSelection: true) }
        }
        .toolbar {
            Button("Refresh", systemImage: "arrow.clockwise") { Task { await load(keepSelection: true) } }
        }
        .focusedSceneValue(\.pageActions, PageActions(refresh: { Task { await load(keepSelection: true) } }))
    }

    private func load(keepSelection: Bool) async {
        do {
            let r = try await model.storage(extra: custom)
            report = r
            if !keepSelection || selected == nil { selected = r.protection }
            giveGb = Double(r.giveBytes) / 1e9
            problem = nil
        } catch {
            problem = describe(error)
        }
    }

    private func selectedChoice(_ r: StorageReport) -> StorageReport.Choice? {
        r.choices.first { $0.is(selected ?? r.protection) } ?? r.current
    }

    // ---------------------------------------------------------------------------------------

    private func members(_ r: StorageReport) -> some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 8)], alignment: .leading, spacing: 8) {
            ForEach(r.members) { m in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        StatusDot(on: m.reachable)
                        Text(m.own ? "You" : m.name).fontWeight(.semibold).lineLimit(1)
                    }
                    Text(m.givesYou.map { "\(m.upText) · \(sizeText($0))" } ?? m.upText)
                        .font(.caption).foregroundStyle(.secondary)
                }
                .padding(.horizontal, 12).padding(.vertical, 10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
            }
        }
    }

    private func choices(_ r: StorageReport) -> some View {
        let groups = Dictionary(grouping: r.choices, by: \.survives).sorted { $0.key < $1.key }
        return ForEach(groups, id: \.key) { survives, group in
            Text("Survives \(survives) down").font(.subheadline.weight(.semibold)).padding(.top, 6)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 196), spacing: 8)], spacing: 8) {
                ForEach(group.sorted { Double($0.n) / Double($0.k) < Double($1.n) / Double($1.k) }) { c in
                    choiceCard(c, selected: c.is(selected ?? r.protection))
                }
            }
        }
    }

    private func choiceCard(_ c: StorageReport.Choice, selected isSelected: Bool) -> some View {
        Button {
            selected = c.id
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .lastTextBaseline, spacing: 8) {
                    Text(c.name).font(.title2.weight(.semibold)).foregroundStyle(c.available ? .primary : .tertiary)
                    if !c.shape.isEmpty { Text(c.shape).font(.caption).foregroundStyle(.secondary) }
                }
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 2) {
                    GridRow {
                        Text("Store").foregroundStyle(.secondary)
                        Text(c.available ? sizeText(c.limitBytes) : "—").fontWeight(.semibold)
                    }
                    GridRow {
                        Text("Offline").foregroundStyle(.secondary)
                        Text(c.offlineText)
                    }
                }
                .font(.callout)
                HStack {
                    if !c.available {
                        Badge(text: "Needs \(c.n) nodes", accent: false)
                    } else if c.recommended {
                        Badge(text: "Recommended", accent: true)
                    }
                }
                .frame(minHeight: 20)
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10)
                .strokeBorder(isSelected ? Color.accentColor : Color.secondary.opacity(0.25), lineWidth: isSelected ? 2 : 1))
            .contentShape(RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .disabled(!c.available || model.switching)
        .accessibilityLabel("\(c.name) \(c.shape), store \(sizeText(c.limitBytes))")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    private var customRow: some View {
        HStack(spacing: 8) {
            Text("Custom").foregroundStyle(.secondary)
            Stepper(value: $customK, in: 1...32) { Text("\(customK)").monospacedDigit() }
            Text("of")
            Stepper(value: $customN, in: 2...64) { Text("\(customN)").monospacedDigit() }
            Button("Consider") {
                guard customK >= 1, customN > customK else {
                    problem = "Use fewer pieces needed than pieces in total, e.g. 3 of 5."
                    return
                }
                custom = .init(k: customK, n: customN)
                selected = custom
                Task { await load(keepSelection: true) }
            }
        }
        .padding(.top, 4)
    }

    @ViewBuilder
    private func capacity(_ r: StorageReport) -> some View {
        if let sel = selectedChoice(r) {
            let home = 1.0 / Double(sel.k), friends = Double(sel.n - 1) / Double(sel.k)
            HStack(alignment: .center, spacing: 24) {
                Text(sizeText(sel.limitBytes))
                    .font(.system(size: 40, weight: .semibold, design: .rounded))
                    .foregroundStyle(r.storedBytes > sel.limitBytes ? .red : .primary)
                VStack(alignment: .leading, spacing: 6) {
                    SegmentBar(segments: [(home, Series.color(1)), (friends, Series.color(0))], total: home + friends, height: 10)
                    Legend(items: [("Yours \(StorageReport.Choice.ratio(home))×", Series.color(1)),
                                   ("Friends \(StorageReport.Choice.ratio(friends))×", Series.color(0))])
                }
                .frame(maxWidth: 420)
            }
            Hint("What you can store with \(sel.name)\(sel.shape.isEmpty ? "" : " (\(sel.shape))"): each file takes \(StorageReport.Choice.ratio(home + friends))× its size across the pool.")

            // A switch holds one batch of files twice (old pieces and new) until it commits it.
            let moving = !sel.is(r.protection) || r.pendingFiles > 0
            let peak = sel.n > 0 ? sel.switchExtraBytes * Int64(sel.k) / Int64(sel.n) : 0
            if moving && sel.available {
                ValueRow(label: "While switching", value: "+\(sizeText(sel.switchExtraBytes))", strong: true,
                         valueColor: sel.switchFits ? nil : .red)
                    .frame(maxWidth: 420)
            }

            let options = r.choices.filter(\.available)
            let top = Double(max(options.map(\.limitBytes).max() ?? 0, r.storedBytes)) * 1.08
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 8) {
                ForEach(options) { c in
                    let isSel = c.is(sel.id)
                    GridRow {
                        Text(c.name).font(.callout).fontWeight(isSel ? .semibold : .regular)
                            .foregroundStyle(isSel ? .primary : .secondary)
                            .frame(width: 80, alignment: .leading)
                        SegmentBar(segments: [(Double(c.limitBytes), isSel ? Series.color(0) : Series.color(2))],
                                   total: max(top, 1), height: 14, tick: Double(r.storedBytes),
                                   band: isSel && moving && peak > 0 ? Double(r.storedBytes)...Double(r.storedBytes + peak) : nil)
                            .help("Stored now: \(sizeText(r.storedBytes))")
                        Text(sizeText(c.limitBytes)).font(.callout).monospacedDigit()
                            .frame(width: 72, alignment: .trailing)
                    }
                }
            }
            .padding(.top, 4)
        }
    }

    @ViewBuilder
    private func switchRow(_ r: StorageReport) -> some View {
        if let sel = selectedChoice(r) {
            let needed = !sel.is(r.protection) || r.pendingFiles > 0
            HStack(spacing: 12) {
                Button {
                    problem = nil
                    Task {
                        do {
                            try await model.startSwitch(to: sel.id)
                            await load(keepSelection: true)
                        } catch {
                            problem = describe(error)
                        }
                    }
                } label: {
                    Text(model.switching ? "Switching…" : !needed ? "In Use" : !sel.switchFits ? "Not Enough Space"
                        : "Switch to \(sel.name)\(sel.shape.isEmpty ? "" : " (\(sel.shape))")")
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(model.switching || !needed || !sel.available || !sel.switchFits)
                if model.switching {
                    ProgressView(value: model.switchFraction).frame(maxWidth: 260)
                    Text("\(Int(model.switchFraction * 100))%").monospacedDigit().foregroundStyle(.secondary)
                } else if r.pendingFiles > 0 {
                    Hint("\(r.pendingFiles) file(s) still to move.")
                }
            }
            .padding(.top, 6)
            Hint("Switching re-codes every file and deletes the old pieces. It runs in the background, and picks up again where it stopped if Ursa quits.")
        }
    }

    @ViewBuilder
    private func give(_ r: StorageReport) -> some View {
        let maxGb = r.disk.map { max(Double($0.totalBytes - $0.yoursBytes) / 1e9, giveGb) } ?? max(giveGb * 4, 100)
        let hasFriends = r.members.contains { !$0.own }
        Hint("Space on each node you own for your friends' pieces, shared equally among them.")
        HStack(spacing: 16) {
            Slider(value: $giveGb, in: 0...max(maxGb.rounded(.up), 1))
                .disabled(!hasFriends)
                .onChange(of: giveGb) { _, gb in
                    // Applied once the slider rests.
                    guard Int64(gb * 1e9) != r.giveBytes else { return }
                    giveTask?.cancel()
                    giveTask = Task {
                        try? await Task.sleep(for: .milliseconds(700))
                        guard !Task.isCancelled else { return }
                        do {
                            try await model.setGive(bytes: Int64(gb * 1e9))
                            await load(keepSelection: true)
                        } catch {
                            problem = describe(error)
                        }
                    }
                }
            Text(sizeText(Int64(giveGb * 1e9))).monospacedDigit().frame(width: 80, alignment: .trailing)
        }
        if !hasFriends { Hint("Add a friend in Account to give them space.") }
    }
}
