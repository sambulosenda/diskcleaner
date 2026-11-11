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
        let shared = DiskMapContent.smallerID(for: .likelySafe)
        XCTAssertEqual(tiles.map(\.id), ["b", "d", shared], "the catch-all comes last, whatever its size")
        XCTAssertEqual(tiles.map(\.bytes), [500, 30, 35])
        let smaller = tiles[2]
        XCTAssertEqual(smaller.count, 2)
        XCTAssertEqual(smaller.title, "2 smaller items")
        XCTAssertNil(smaller.itemID, "the shared block opens the whole list")
        XCTAssertEqual(smaller.tier, .likelySafe, "items of one tier keep its colour")
    }

    func testAFewItemsNeedNoSharedBlock() {
        let tiles = DiskMapContent.tiles(for: [item("a", 10), item("b", 20, .reviewFirst)])
        XCTAssertEqual(tiles.map(\.id), ["b", "a"])
        XCTAssertEqual(tiles[0].tier, .reviewFirst)
        XCTAssertEqual(tiles[0].explain, "What b is.")
    }

    func testKeptItemsAreNotDrawn() {
        let tiles = DiskMapContent.tiles(for: [item("kept", 9_000, .keep), item("a", 10), item("b", 5, .reviewFirst)])
        XCTAssertEqual(tiles.map(\.id), ["a", "b"], "only what the user can clean or check")
        XCTAssertTrue(DiskMapContent.tiles(for: [item("kept", 9_000, .keep)]).isEmpty)
    }

    func testAnItemTooSmallToClickJoinsTheSharedBlock() {
        let tiles = DiskMapContent.tiles(for: [item("big", 1_000_000_000), item("tiny", 1_000_000, .reviewFirst)])
        XCTAssertEqual(tiles.map(\.id), ["big", DiskMapContent.smallerID(for: .reviewFirst)])
        XCTAssertEqual(tiles[1].title, "1 smaller item")
        XCTAssertEqual(tiles[1].bytes, 1_000_000)
    }

    func testABlockTooSmallForItsNameJoinsTheSharedBlock() {
        // One big item and several that would each get a sliver of a small map.
        let items = [item("big", 900)] + (1...6).map { item("s\($0)", 15) }
        let tiles = DiskMapContent.tiles(for: items)
        XCTAssertEqual(tiles.count, 7, "all big enough by share alone")
        let size = CGSize(width: 240, height: 200)
        let fitted = DiskMapContent.fitted(tiles, in: size)
        let rects = Treemap.layout(fitted.map { Double($0.bytes) }, in: CGRect(origin: .zero, size: size))
        for (tile, rect) in zip(fitted, rects) where tile.itemID != nil {
            XCTAssertTrue(DiskMapBlock.showsName(in: rect.size), "\(tile.id) would be a blank block")
        }
        XCTAssertEqual(fitted.first?.id, "big")
        XCTAssertNil(fitted.last?.itemID, "the shared block stays last")
        XCTAssertEqual(fitted.reduce(0) { $0 + $1.count }, 7, "no item is lost")
        XCTAssertEqual(DiskMapContent.sum(fitted.map(\.bytes)), 990, "and no space")
        XCTAssertEqual(fitted.last?.title, "\(fitted.last?.count ?? 0) smaller items")
    }

    func testBlocksWithRoomAreLeftAlone() {
        let tiles = DiskMapContent.tiles(for: [item("a", 60), item("b", 40)])
        XCTAssertEqual(DiskMapContent.fitted(tiles, in: CGSize(width: 600, height: 300)), tiles)
        XCTAssertEqual(DiskMapBlock.titleLines(in: CGSize(width: 90, height: 100)), 2)
        XCTAssertEqual(DiskMapBlock.titleLines(in: CGSize(width: 90, height: 50)), 1)
    }

    func testTheMapHasOneSidePerTierLikelySafeFirst() {
        let items = [item("r1", 300, .reviewFirst), item("s1", 40), item("kept", 9_000, .keep),
                     item("r2", 100, .reviewFirst)]
        let groups = DiskMapContent.groups(for: items)
        XCTAssertEqual(groups.map(\.tier), [.likelySafe, .reviewFirst])
        XCTAssertEqual(groups.map(\.bytes), [40, 400])
        XCTAssertEqual(groups[1].tiles.map(\.id), ["r1", "r2"])
        XCTAssertEqual(DiskMapContent.groups(for: [item("r1", 300, .reviewFirst)]).map(\.tier), [.reviewFirst])
        XCTAssertTrue(DiskMapContent.groups(for: [item("kept", 9_000, .keep)]).isEmpty)
    }

    func testASmallSideStillGetsRoomToBeRead() {
        let widths = DiskMapContent.widths(for: [5, 95], in: 1_014, gap: 14)
        XCTAssertEqual(Double(widths[0]), 240, accuracy: 0.5, "the least share of the map")
        XCTAssertEqual(Double(widths[0] + widths[1]), 1_000, accuracy: 0.5)
        let even = DiskMapContent.widths(for: [50, 50], in: 1_014, gap: 14)
        XCTAssertEqual(Double(even[0]), 500, accuracy: 0.5)
        XCTAssertEqual(DiskMapContent.widths(for: [7], in: 800, gap: 14), [800])
        XCTAssertTrue(DiskMapContent.widths(for: [], in: 800, gap: 14).isEmpty)
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
