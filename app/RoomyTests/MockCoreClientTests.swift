import XCTest
@testable import Roomy

/// The mock must keep the same promises the real core will (T09), so T11 can swap it out safely.
@MainActor
final class MockCoreClientTests: XCTestCase {
    func testLikelySafeBytesIsTheExactSumOfLikelySafeItems() {
        let overview = MockData.overview(for: .normal)
        let expected = MockData.items.filter { $0.tier == .likelySafe }.reduce(UInt64(0)) { $0 + $1.bytes }
        XCTAssertEqual(overview.likelySafeBytes, expected)
        XCTAssertEqual(overview.likelySafeBytes, 34_000_000_000, "The wireframe headline is 34 GB")
    }

    func testCategoryLikelySafeTotalsAddUpAndFitInsideTheirCategory() {
        let overview = MockData.overview(for: .normal)
        let sum = overview.categories.reduce(UInt64(0)) { $0 + $1.likelySafeBytes }
        XCTAssertEqual(sum, overview.likelySafeBytes)
        for entry in overview.categories {
            XCTAssertLessThanOrEqual(entry.likelySafeBytes, entry.bytes, entry.category.title)
        }
    }

    func testDiskNumbersAddUp() {
        let overview = MockData.overview(for: .normal)
        let used = overview.categories.reduce(UInt64(0)) { $0 + $1.bytes }
        XCTAssertEqual(used + overview.diskFree, overview.diskTotal)
    }

    func testTopItemsAreFiveToEightBiggestFirst() {
        let top = MockData.overview(for: .normal).topItems
        XCTAssertTrue((5...8).contains(top.count), "got \(top.count)")
        XCTAssertEqual(top.map(\.bytes), top.map(\.bytes).sorted(by: >))
        XCTAssertEqual(Set(top.map(\.id)).count, top.count, "ids must be unique")
    }

    func testNothingFoundHasNoItemsAndNothingToFree() {
        let overview = MockData.overview(for: .nothingFound)
        XCTAssertTrue(overview.topItems.isEmpty)
        XCTAssertEqual(overview.likelySafeBytes, 0)
    }

    func testScanRunsToDoneThenGivesTheOverview() async throws {
        let client = MockCoreClient(totalSteps: 4)
        let handle = try await client.startScan(mode: .simple)
        var phases: [ScanPhase] = []
        for _ in 0..<4 { phases.append(try await client.scanProgress(handle).phase) }
        XCTAssertEqual(phases.last, .done)
        let overview = try await client.overview(handle)
        XCTAssertEqual(overview, MockData.overview(for: .normal))
    }

    func testOverviewBeforeTheScanFinishesIsRefused() async throws {
        let client = MockCoreClient(totalSteps: 4)
        let handle = try await client.startScan(mode: .simple)
        do {
            _ = try await client.overview(handle)
            XCTFail("Expected scanNotFinished")
        } catch {
            XCTAssertEqual(error as? CoreClientError, .scanNotFinished)
        }
    }

    func testCancelledScanStopsReporting() async throws {
        let client = MockCoreClient(totalSteps: 4)
        let handle = try await client.startScan(mode: .simple)
        await client.cancelScan(handle)
        do {
            _ = try await client.scanProgress(handle)
            XCTFail("Expected cancelled")
        } catch {
            XCTAssertEqual(error as? CoreClientError, .cancelled)
        }
    }

    func testUnknownHandleIsRefused() async {
        let client = MockCoreClient()
        do {
            _ = try await client.scanProgress(ScanHandle(id: 999))
            XCTFail("Expected unknownScan")
        } catch {
            XCTAssertEqual(error as? CoreClientError, .unknownScan)
        }
    }

    func testFailureScenarioThrowsPartWay() async throws {
        let client = MockCoreClient(scenario: .failure, totalSteps: 4)
        let handle = try await client.startScan(mode: .simple)
        _ = try await client.scanProgress(handle)
        do {
            _ = try await client.scanProgress(handle)
            XCTFail("Expected scanFailed")
        } catch {
            guard case .scanFailed = error as? CoreClientError else { return XCTFail("got \(error)") }
        }
    }

    func testItemSummariesCarryNoPaths() {
        for item in MockData.items {
            XCTAssertFalse(item.title.contains("/"), item.id)
            XCTAssertFalse(item.explanation.contains("/"), item.id)
        }
    }
}
