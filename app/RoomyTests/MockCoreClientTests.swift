import XCTest
@testable import Roomy

/// The mock must keep the promises the real core makes in `docs/command-contract.md`,
/// so T11 can swap it for `RoomyCore` without the UI changing behaviour.
@MainActor
final class MockCoreClientTests: XCTestCase {
    private func total(_ items: [ItemSummary]) -> UInt64 {
        items.reduce(UInt64(0)) { $0 + $1.sizeBytes }
    }

    func testLikelySafeBytesIsTheExactSumOfLikelySafeItems() {
        let overview = MockData.overview(for: .normal)
        XCTAssertEqual(overview.likelySafeBytes, total(MockData.items.filter { $0.tier == .likelySafe }))
        XCTAssertEqual(overview.likelySafeBytes, 34_000_000_000, "The wireframe headline is 34 GB")
    }

    func testCategoryTotalsFollowTheContract() {
        let overview = MockData.overview(for: .normal)
        XCTAssertEqual(overview.categories.reduce(UInt64(0)) { $0 + $1.likelySafeBytes }, overview.likelySafeBytes)
        XCTAssertEqual(overview.categories.map(\.bytes), overview.categories.map(\.bytes).sorted(by: >),
                       "categories come largest first")
        for entry in overview.categories {
            let inCategory = MockData.items.filter { $0.category == entry.category }
            XCTAssertEqual(entry.bytes, total(inCategory), entry.category.title)
            XCTAssertEqual(entry.itemCount, UInt64(inCategory.count), entry.category.title)
            XCTAssertLessThanOrEqual(entry.likelySafeBytes, entry.bytes, entry.category.title)
        }
    }

    func testDiskNumbersFit() {
        let overview = MockData.overview(for: .normal)
        let categorised = overview.categories.reduce(UInt64(0)) { $0 + $1.bytes }
        XCTAssertLessThanOrEqual(categorised, overview.scannedBytes)
        XCTAssertLessThanOrEqual(overview.scannedBytes + overview.diskFree, overview.diskTotal)
    }

    func testTopItemsAreFiveToEightBiggestFirstAndNeverKeep() {
        let top = MockData.overview(for: .normal).topItems
        XCTAssertTrue((5...8).contains(top.count), "got \(top.count)")
        XCTAssertEqual(top.map(\.sizeBytes), top.map(\.sizeBytes).sorted(by: >))
        XCTAssertFalse(top.contains { $0.tier == .keep })
        XCTAssertEqual(Set(top.map(\.id)).count, top.count, "ids must be unique")
    }

    func testNothingFoundHasNoItemsAndNothingToFree() {
        let overview = MockData.overview(for: .nothingFound)
        XCTAssertTrue(overview.topItems.isEmpty)
        XCTAssertTrue(overview.categories.isEmpty)
        XCTAssertEqual(overview.likelySafeBytes, 0)
    }

    func testScanRunsToFinishedThenGivesTheOverview() async throws {
        let client = MockCoreClient(totalSteps: 4)
        let handle = await client.startScan(mode: .simple)
        var phase = ScanPhase.scanning
        for _ in 0..<10 where !phase.hasEnded {
            phase = try await client.scanProgress(handle: handle).phase
        }
        XCTAssertEqual(phase, .finished)
        let overview = try await client.overview(handle: handle)
        XCTAssertEqual(overview, MockData.overview(for: .normal))
    }

    func testOverviewBeforeTheScanFinishesIsRefused() async {
        let client = MockCoreClient(totalSteps: 4)
        let handle = await client.startScan(mode: .simple)
        do {
            _ = try await client.overview(handle: handle)
            XCTFail("Expected ScanNotFinished")
        } catch {
            guard case .ScanNotFinished = error as? CoreError else { return XCTFail("got \(error)") }
        }
    }

    func testCancelledScanReportsCancelled() async throws {
        let client = MockCoreClient(totalSteps: 4)
        let handle = await client.startScan(mode: .simple)
        await client.cancelScan(handle: handle)
        let progress = try await client.scanProgress(handle: handle)
        XCTAssertEqual(progress.phase, .cancelled)
    }

    func testClosedHandleIsUnknown() async {
        let client = MockCoreClient(totalSteps: 4)
        let handle = await client.startScan(mode: .simple)
        await client.closeScan(handle: handle)
        do {
            _ = try await client.scanProgress(handle: handle)
            XCTFail("Expected UnknownScan")
        } catch {
            guard case .UnknownScan = error as? CoreError else { return XCTFail("got \(error)") }
        }
    }

    func testFailureScenarioEndsWithFailedPhase() async throws {
        let client = MockCoreClient(scenario: .failure, totalSteps: 4)
        let handle = await client.startScan(mode: .simple)
        var phase = ScanPhase.scanning
        for _ in 0..<10 where !phase.hasEnded {
            phase = try await client.scanProgress(handle: handle).phase
        }
        guard case .failed(let reason) = phase else { return XCTFail("got \(phase)") }
        XCTAssertFalse(reason.isEmpty)
        XCTAssertFalse(reason.contains("/"), "reasons never hold a path")
    }

    func testItemSummariesCarryNoPaths() {
        for item in MockData.items {
            XCTAssertFalse(item.title.contains("/"), item.id)
            XCTAssertFalse(item.explain.contains("/"), item.id)
        }
    }

    func testCleanupPreviewFollowsTheContract() async throws {
        let mock = MockCoreClient(totalSteps: 2)
        let handle = await mock.startScan(mode: .simple)
        while try await mock.scanProgress(handle: handle).phase != .finished {}
        let choices = [
            CleanupChoice(itemId: "app-caches", chosenAlone: true),
            CleanupChoice(itemId: "iphone-backup", chosenAlone: false),
            CleanupChoice(itemId: "photos-library", chosenAlone: true),
            CleanupChoice(itemId: "app-caches", chosenAlone: true),
        ]
        let preview = try await mock.previewCleanup(handle: handle, choices: choices)
        XCTAssertEqual(preview.items.map(\.itemId), ["app-caches", "iphone-backup", "photos-library"])
        XCTAssertEqual(preview.items[0].check, .willMove)
        XCTAssertNotEqual(preview.items[1].check, .willMove, "Review first needs choosing alone")
        XCTAssertNotEqual(preview.items[2].check, .willMove, "keep never moves")
        XCTAssertEqual(preview.bytesToMove, preview.items[0].sizeBytes)
        XCTAssertTrue(preview.items.allSatisfy { $0.path == nil }, "no path in simple mode")
        XCTAssertNotNil(preview.plan)
        do {
            _ = try await mock.previewCleanup(handle: handle, choices: [])
            XCTFail("an empty preview must throw")
        } catch CoreError.NothingSelected {}
    }
}

/// The Docker screen's model, against the mock. Nothing here runs Docker.
@MainActor
final class DockerModelTests: XCTestCase {
    private func waitUntil(_ condition: () -> Bool) async {
        for _ in 0..<400 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
    }

    func testLoadShowsDockersAnswer() async {
        let model = DockerModel(client: MockCoreClient())
        await model.load()
        guard case .loaded(let overview) = model.phase else { return XCTFail("\(model.phase)") }
        XCTAssertEqual(overview.status, .ready)
        XCTAssertEqual(overview.images.reclaimableBytes, 2_740_000_000)
    }

    func testReviewThenConfirmRunsOnceAndReloads() async {
        let mock = MockCoreClient()
        let model = DockerModel(client: mock)
        await model.load()
        model.startReview(.unusedImages)
        await waitUntil { if case .ready = model.review { return true } else { return false } }
        guard case .ready(let preview) = model.review else { return XCTFail("\(model.review)") }
        XCTAssertEqual(preview.items.count, 2)
        XCTAssertTrue(mock.dockerRuns.isEmpty, "a review removes nothing")
        model.confirm()
        await waitUntil { if case .done = model.review { return true } else { return false } }
        XCTAssertEqual(mock.dockerRuns, [.unusedImages])
        await waitUntil {
            if case .loaded(let o) = model.phase { return o.images.reclaimableBytes == 0 } else { return false }
        }
        guard case .loaded(let overview) = model.phase else { return XCTFail("\(model.phase)") }
        XCTAssertEqual(overview.images.reclaimableBytes, 0)
    }

    func testClosingAReviewRunsNothing() async {
        let mock = MockCoreClient()
        let model = DockerModel(client: mock)
        model.startReview(.buildCache)
        await waitUntil { if case .ready = model.review { return true } else { return false } }
        model.closeReview()
        XCTAssertEqual(model.review, .closed)
        XCTAssertTrue(mock.dockerRuns.isEmpty)
    }

    func testAStoppedDockerFailsInPlainWords() async {
        let mock = MockCoreClient()
        mock.dockerStatus = .notRunning
        let model = DockerModel(client: mock)
        await model.load()
        guard case .loaded(let overview) = model.phase else { return XCTFail("\(model.phase)") }
        XCTAssertEqual(overview.status, .notRunning)
        model.startReview(.unusedImages)
        await waitUntil { if case .failed = model.review { return true } else { return false } }
        XCTAssertEqual(model.review, .failed("Docker isn't running. Open Docker and try again."))
    }
}
