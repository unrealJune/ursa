import Charts
import SwiftUI

/// What you store, what it takes in the pool, what you give back, and how it grows.
struct UsageView: View {
    @Environment(AppModel.self) private var model
    @State private var report: StorageReport?
    @State private var problem: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                if let problem {
                    Label(problem, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).textSelection(.enabled)
                }
                if let r = report {
                    content(r)
                } else if problem == nil {
                    ProgressView().frame(maxWidth: .infinity).padding(.top, 80)
                }
            }
            .frame(maxWidth: 880, alignment: .leading)
            .padding(24)
            .frame(maxWidth: .infinity)
        }
        .task { await load() }
        .onChange(of: model.switchEnded) { Task { await load() } }
        .onChange(of: model.libraryVersion) { Task { await load() } }
        .toolbar {
            Button("Refresh", systemImage: "arrow.clockwise") { Task { await load() } }
        }
        .focusedSceneValue(\.pageActions, PageActions(refresh: { Task { await load() } }))
    }

    private func load() async {
        do {
            report = try await model.storage()
            problem = nil
        } catch {
            problem = describe(error)
        }
    }

    @ViewBuilder
    private func content(_ r: StorageReport) -> some View {
        // Your files against what the current protection lets you store.
        Card {
            HStack(alignment: .lastTextBaseline, spacing: 8) {
                Text(sizeText(r.storedBytes)).font(.system(size: 40, weight: .semibold, design: .rounded))
                Text("of \(sizeText(r.limitBytes))").font(.title3).foregroundStyle(.secondary)
            }
            SegmentBar(segments: [(Double(r.kinds.photos), Series.color(0)), (Double(r.kinds.videos), Series.color(1)),
                                  (Double(r.kinds.files), Series.color(2))],
                       total: Double(max(r.limitBytes, r.storedBytes, 1)), height: 18)
            Legend(items: [("Photos \(sizeText(r.kinds.photos))", Series.color(0)),
                           ("Videos \(sizeText(r.kinds.videos))", Series.color(1)),
                           ("Files \(sizeText(r.kinds.files))", Series.color(2)),
                           ("Free \(sizeText(max(0, r.limitBytes - r.storedBytes)))", Series.track)])
        }

        // In the pool, and fair share.
        HStack(alignment: .top, spacing: 8) {
            Card {
                Text("In the pool").fontWeight(.semibold)
                ValueRow(label: "Your nodes", value: sizeText(r.homeBytes))
                ValueRow(label: "Friends' nodes", value: sizeText(r.friendsBytes))
                Divider()
                ValueRow(label: "Total", value: sizeText(r.homeBytes + r.friendsBytes), strong: true)
            }
            Card {
                Text("Fair share").fontWeight(.semibold)
                ValueRow(label: "On friends", value: "\(sizeText(r.friendsBytes)) of \(sizeText(r.giveBytes))")
                SegmentBar(segments: [(Double(r.friendsBytes), Series.color(0))],
                           total: Double(max(r.giveBytes, r.friendsBytes, 1)), height: 10)
                ValueRow(label: "Friends on you", value: sizeText(r.hosting.reduce(Int64(0)) { $0 + $1.usedBytes }))
            }
        }

        SectionTitle(text: "Your pieces")
        Card {
            table(header: "Member", rows: r.members.map { ($0.id, $0.own ? "You" : $0.name, $0.usedBytes, $0.givesYou) })
        }

        if !r.hosting.isEmpty {
            SectionTitle(text: "Hosting")
            Card {
                table(header: "Friend", rows: r.hosting.map { ($0.id, $0.name, $0.usedBytes, Optional($0.quotaBytes)) })
            }
        }

        if let d = r.disk {
            let other = max(0, d.totalBytes - d.freeBytes - d.yoursBytes - d.friendsBytes)
            SectionTitle(text: "Disk · \(sizeText(d.totalBytes))")
            Card {
                SegmentBar(segments: [(Double(d.yoursBytes), Series.color(0)), (Double(d.friendsBytes), Series.color(1)),
                                      (Double(d.reservedBytes), Series.color(2)), (Double(other), Series.color(3))],
                           total: Double(max(d.totalBytes, 1)), height: 18)
                Legend(items: [("Yours \(sizeText(d.yoursBytes))", Series.color(0)),
                               ("Friends \(sizeText(d.friendsBytes))", Series.color(1)),
                               ("Reserved \(sizeText(d.reservedBytes))", Series.color(2)),
                               ("Other \(sizeText(other))", Series.color(3)),
                               ("Free \(sizeText(max(0, d.freeBytes - d.reservedBytes)))", Series.track)])
            }
        }

        SectionTitle(text: "Growth")
        Hint("Stored by month, and where the last three months' pace leads. You can store up to \(sizeText(r.limitBytes)).")
        Card { GrowthChart(report: r).frame(height: 200) }
    }

    private func table(header: String, rows: [(id: String, name: String, used: Int64, limit: Int64?)]) -> some View {
        Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 10) {
            GridRow {
                Text(header)
                Text("Stored").gridColumnAlignment(.trailing)
                Text("Limit")
            }
            .font(.caption).foregroundStyle(.secondary)
            ForEach(rows, id: \.id) { row in
                GridRow {
                    Text(row.name).fontWeight(.semibold).lineLimit(1)
                    Text(sizeText(row.used)).monospacedDigit()
                    LimitCell(used: row.used, limit: row.limit)
                }
            }
        }
    }
}

/// Stored bytes by month, a projection at the recent pace, and the limit.
struct GrowthChart: View {
    var report: StorageReport

    private struct Point: Identifiable {
        var date: Date
        var bytes: Double
        var projected: Bool
        var id: String { "\(date.timeIntervalSince1970)-\(projected)" }
    }

    var body: some View {
        let hist = report.growth
        if hist.isEmpty {
            Hint("Nothing stored yet.").frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            let points = hist.map { Point(date: $0.date, bytes: Double($0.bytes), projected: false) }
            // Pace: the last three months' average.
            let back = min(3, hist.count - 1)
            let pace = back > 0 ? Double(hist[hist.count - 1].bytes - hist[hist.count - 1 - back].bytes) / Double(back) : 0
            let last = points[points.count - 1]
            let ahead = pace > 0 ? (0...6).map { i in
                Point(date: Calendar.current.date(byAdding: .month, value: i, to: last.date) ?? last.date,
                      bytes: last.bytes + pace * Double(i), projected: true)
            } : []
            Chart {
                ForEach(points) { p in
                    AreaMark(x: .value("Month", p.date, unit: .month), y: .value("Stored", p.bytes))
                        .foregroundStyle(Series.color(2))
                    LineMark(x: .value("Month", p.date, unit: .month), y: .value("Stored", p.bytes), series: .value("Kind", "Stored"))
                        .foregroundStyle(Series.color(0))
                        .lineStyle(StrokeStyle(lineWidth: 2))
                }
                ForEach(ahead) { p in
                    LineMark(x: .value("Month", p.date, unit: .month), y: .value("Stored", p.bytes), series: .value("Kind", "Projected"))
                        .foregroundStyle(Series.color(0))
                        .lineStyle(StrokeStyle(lineWidth: 1.5, dash: [4, 4]))
                }
                PointMark(x: .value("Month", last.date, unit: .month), y: .value("Stored", last.bytes))
                    .foregroundStyle(Series.color(0))
                // The limit, when it is near enough not to flatten the line to nothing.
                if report.limitBytes > 0 && Double(report.limitBytes) < 4 * max(last.bytes + pace * 6, 1) {
                    RuleMark(y: .value("Limit", Double(report.limitBytes)))
                        .foregroundStyle(.secondary)
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [2, 3]))
                        .annotation(position: .top, alignment: .trailing) {
                            Text("Limit \(sizeText(report.limitBytes))").font(.caption2).foregroundStyle(.secondary)
                        }
                }
            }
            .chartYAxis {
                AxisMarks { v in
                    AxisGridLine()
                    AxisValueLabel { if let b = v.as(Double.self) { Text(sizeText(Int64(b))) } }
                }
            }
            .chartXAxis {
                AxisMarks(values: .stride(by: .month)) { _ in
                    AxisGridLine()
                    AxisValueLabel(format: .dateTime.month(.abbreviated))
                }
            }
        }
    }
}
