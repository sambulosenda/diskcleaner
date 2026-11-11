import XCTest
@testable import Roomy

/// The Overview's map: the layout's geometry and what each block says.
@MainActor
final class DiskMapTests: XCTestCase {
    private func item(_ id: String, _ size: UInt64, _ tier: Tier = .likelySafe) -> ItemSummary {
        ItemSummary(id: id, title: "Item \(id)", explain: "What \(id) is.", sizeBytes: size, tier: tier,
                    category: .cache)
    }

    // MARK: Layout

    func testEveryBlockGetsItsShareOfTheArea() {
        let values: [Double] = [50, 20, 12, 8, 5, 3, 1, 1]
        let bounds = CGRect(x: 0, y: 0, width: 600, height: 300)
        let rects = Treemap.layout(values, in: bounds)
        XCTAssertEqual(rects.count, values.count)
        let total = values.reduce(0, +)
        for (value, rect) in zip(values, rects) {
            let expected = value / total * Double(bounds.width * bounds.height)
            XCTAssertEqual(Double(rect.width * rect.height), expected, accuracy: 0.5)
        }
    }

    func testBlocksStayInsideAndNeverOverlap() {
        let values: [Double] = [40, 30, 10, 9, 6, 3, 1, 0.5, 0.5]
        let bounds = CGRect(x: 10, y: 20, width: 500, height: 320)
        let rects = Treemap.layout(values, in: bounds)
        let roomy = bounds.insetBy(dx: -0.01, dy: -0.01)
        for rect in rects {
            XCTAssertTrue(roomy.contains(rect), "\(rect) is outside \(bounds)")
        }
        for first in rects.indices {
            for second in rects.indices where second > first {
                let overlap = rects[first].intersection(rects[second])
                XCTAssertTrue(overlap.isNull || overlap.width * overlap.height < 0.01,
                              "\(rects[first]) overlaps \(rects[second])")
            }
        }
    }

    func testEqualBlocksAreNotThinStrips() {
        let rects = Treemap.layout([1, 1, 1, 1], in: CGRect(x: 0, y: 0, width: 400, height: 400))
        for rect in rects {
            let ratio = max(rect.width / rect.height, rect.height / rect.width)
            XCTAssertLessThanOrEqual(ratio, 2.01, "\(rect)")
        }
    }

    func testNothingToDrawGivesEmptyBlocks() {
        XCTAssertTrue(Treemap.layout([], in: CGRect(x: 0, y: 0, width: 10, height: 10)).isEmpty)
        XCTAssertEqual(Treemap.layout([0, 0], in: CGRect(x: 0, y: 0, width: 10, height: 10)), [.zero, .zero])
        XCTAssertEqual(Treemap.layout([5], in: .zero), [.zero])
        let rects = Treemap.layout([3, 0], in: CGRect(x: 0, y: 0, width: 10, height: 10))
        XCTAssertEqual(Double(rects[0].width * rects[0].height), 100, accuracy: 0.001)
        XCTAssertEqual(Double(rects[1].width * rects[1].height), 0, accuracy: 0.001)
    }

    // MARK: Blocks

    func testTheBiggestItemsGetABlockEachAndTheRestShareOne() {
        let items = [item("a", 10), item("b", 500), item("c", 0), item("d", 30), item("e", 25)]
        let tiles = DiskMapContent.tiles(for: items, limit: 2)
        XCTAssertEqual(tiles.map(\.id), ["b", DiskMapContent.smallerID, "d"])
        XCTAssertEqual(tiles.map(\.bytes), [500, 35, 30])
        let smaller = tiles[1]
        XCTAssertEqual(smaller.title, "2 smaller items")
        XCTAssertNil(smaller.itemID, "the shared block opens the whole list")
        XCTAssertNil(smaller.tier)
    }

    func testAFewItemsNeedNoSharedBlock() {
        let tiles = DiskMapContent.tiles(for: [item("a", 10), item("b", 20, .keep)])
        XCTAssertEqual(tiles.map(\.id), ["b", "a"])
        XCTAssertEqual(tiles[0].tier, .keep)
        XCTAssertEqual(tiles[0].explain, "What b is.")
    }

    func testABlockSaysItsTierInWords() {
        let tile = DiskMapContent.tiles(for: [item("a", 1_000_000, .reviewFirst)])[0]
        XCTAssertTrue(DiskMapBlock.spokenLabel(for: tile).hasSuffix("Review first"))
        XCTAssertTrue(DiskMapBlock.showsName(in: CGSize(width: 120, height: 60)))
        XCTAssertFalse(DiskMapBlock.showsName(in: CGSize(width: 30, height: 60)))
    }

    func testTheCaptionSaysWhatTheMapLeavesOut() {
        let overview = MockData.overview(for: .normal)
        let all = DiskMapContent.caption(mapped: overview.scannedBytes, overview: overview)
        XCTAssertFalse(all.contains("The other"))
        let some = DiskMapContent.caption(mapped: overview.scannedBytes / 2, overview: overview)
        XCTAssertTrue(some.contains("your own files and apps"))
    }
}
