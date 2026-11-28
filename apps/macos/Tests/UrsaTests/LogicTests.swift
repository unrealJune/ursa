import Foundation
import Testing
@testable import Ursa

@Suite struct LogicTests {
    @Test func editsEncodeAsTheCoreReadsThem() throws {
        let json = Edit.encode([
            .createFolder(id: "abc", parent: nil, name: "Trips"),
            .moveFile(fileId: "f1", dir: nil),
            .renameFile(fileId: "f2", name: "b.jpg"),
        ])
        let parsed = try #require(try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [[String: Any]])
        #expect(parsed[0]["op"] as? String == "create_folder")
        #expect(parsed[0]["id"] as? String == "abc")
        #expect(parsed[0]["parent"] is NSNull)
        #expect(parsed[1]["op"] as? String == "move_file")
        #expect(parsed[1]["dir"] is NSNull)
        #expect(parsed[2]["name"] as? String == "b.jpg")
        #expect(Edit.newFolderId().count == 32)
    }

    @Test func storageReportDecodes() throws {
        let json = """
        {"protection":{"k":2,"n":3},"chosen":true,"files":4,"pending_files":0,"leftover_pieces":0,
         "stored_bytes":1000,"kinds":{"photos":600,"videos":300,"files":100},"codes":{"2-3":4},
         "limit_bytes":5000,
         "choices":[{"k":2,"n":3,"survives":1,"limit_bytes":5000,"offline_hours":0.5,"available":true,
                     "recommended":true,"switch_extra_bytes":0,"switch_fits":true},
                    {"k":1,"n":3,"survives":2,"limit_bytes":3000,"available":false,"recommended":false}],
         "give_bytes":100,"home_bytes":500,"friends_bytes":1000,
         "members":[{"name":"me","own":true,"nodes":1,"reachable":true,"up_30d":0.995,"used_bytes":500},
                    {"name":"Ana","own":false,"nodes":2,"reachable":false,"used_bytes":1000,"gives_you":2000}],
         "hosting":[{"name":"Ana","account":"acc","used_bytes":10,"quota_bytes":100}],
         "growth":[{"month":"2026-10","bytes":400},{"month":"2026-11","bytes":1000}]}
        """
        let r = try StorageReport.decode(json)
        #expect(r.protection == .init(k: 2, n: 3))
        #expect(r.current?.name == "1.5×")
        #expect(r.current?.shape == "2 of 3")
        #expect(r.current?.offlineText == "30 min/yr")
        #expect(r.choices[1].name == "3 copies")
        #expect(r.choices[1].switchFits == false)
        #expect(r.members[0].up30d == 0.995)
        #expect(r.members[0].upText == "99.5% up")
        #expect(r.members[1].givesYou == 2000)
        #expect(r.disk == nil)
        #expect(r.growth.count == 2)
    }

    @Test func rowsJustifyToTheWidth() {
        let items: [LibraryFile] = (0..<40).map { (i: Int) -> LibraryFile in
            let wide = i % 3 == 0
            let created: Int64 = 1_760_000_000_000 - Int64(i)
            return LibraryFile(fileId: "f\(i)", name: "\(i).jpg", size: 1, mime: "image/jpeg", createdMs: created,
                               modifiedMs: 0, addedMs: 0, width: wide ? 4000 : 3000, height: wide ? 3000 : 4000,
                               hasThumb: true, dir: nil, path: "")
        }
        let sections = PhotosView.layout(items, width: 1000, target: 200, spacing: 4)
        #expect(sections.count == 1)
        let rows = sections[0].rows
        #expect(rows.reduce(0) { $0 + $1.tiles.count } == 40)
        for row in rows.dropLast() {
            let w = row.tiles.reduce(0) { $0 + $1.width } + 4 * CGFloat(row.tiles.count - 1)
            #expect(abs(w - 1000) < 0.5)
            #expect(row.height <= 200)
        }
        #expect(rows.last!.height == 200)
    }

    @Test func choiceRatiosReadCleanly() {
        #expect(StorageReport.Choice.ratio(1.5) == "1.5")
        #expect(StorageReport.Choice.ratio(2) == "2")
        #expect(StorageReport.Choice.ratio(5.0 / 3.0) == "1.67")
    }
}
