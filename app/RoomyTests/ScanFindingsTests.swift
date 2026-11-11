import XCTest
@testable import Roomy

/// "Found so far" on the scanning screen: its words, and how the model keeps it up to date.
@MainActor
final class ScanFindingsTests: XCTestCase {
    private let suiteName = "RoomyTests.ScanFindings"

    private lazy var defaults: UserDefaults = {
        let store = UserDefaults(suiteName: suiteName) ?? .standard
        store.removePersistentDomain(forName: suiteName)
        return store
    }()

    private func finding(_ title: String, _ size: UInt64, count: UInt32 = 1) -> ScanFinding {
        ScanFinding(title: title, category: .cache, count: count, sizeBytes: size)
    }

    // MARK: Words

    func testOnlyTheLargestFewKindsAreShown() {
        let findings = (1...9).map { finding("Kind \($0)", UInt64(1_000 - $0)) } + [finding("Empty", 0)]
        let shown = ScanFindingsText.shown(findings)
        XCTAssertEqual(shown.count, ScanFindingsText.rowLimit)
        XCTAssertEqual(shown.first?.title, "Kind 1", "the core's order, largest first, is kept")
        XCTAssertEqual(ScanFindingsText.more(findings), "and 3 more kinds", "a kind with nothing counted isn't one")
    }

    func testAShortListHasNoMoreLine() {
        let findings = [finding("A", 10), finding("B", 5)]
        XCTAssertEqual(ScanFindingsText.shown(findings).map(\.title), ["A", "B"])
        XCTAssertNil(ScanFindingsText.more(findings))
        XCTAssertEqual(ScanFindingsText.more((1...7).map { finding("K\($0)", 10) }), "and 1 more kind")
        XCTAssertTrue(ScanFindingsText.shown([finding("Empty", 0)]).isEmpty)
    }

    func testARowSaysHowManyPlacesOnlyWhenMoreThanOne() {
        XCTAssertNil(ScanFindingsText.places(finding("Chrome cache", 10)))
        XCTAssertEqual(ScanFindingsText.places(finding("Project packages", 10, count: 11)), "in 11 places")
        let spoken = ScanFindingsText.spoken(finding("Project packages", 2_000_000_000, count: 11))
        XCTAssertTrue(spoken.hasPrefix("Project packages, in 11 places, "))
        XCTAssertTrue(spoken.hasSuffix(" so far"))
    }

    func testTheNotePromisesNothingBeforeTheScanEnds() {
        XCTAssertTrue(ScanFindingsText.note.contains("When the scan finishes"))
        XCTAssertFalse(ScanFindingsText.note.lowercased().contains("junk"))
    }

    // MARK: Model

    func testTheMockFindsNothingAtFirstThenMore() async throws {
        let client = MockCoreClient(scenario: .normal, totalSteps: 10)
        let handle = await client.startScan(mode: .developer)
        let atStart = await client.scanFindings(handle: handle)
        XCTAssertTrue(atStart.isEmpty)
        for _ in 0..<4 { _ = try await client.scanProgress(handle: handle) }
        let early = await client.scanFindings(handle: handle)
        for _ in 0..<4 { _ = try await client.scanProgress(handle: handle) }
        let later = await client.scanFindings(handle: handle)
        XCTAssertFalse(early.isEmpty)
        XCTAssertGreaterThan(later[0].sizeBytes, early[0].sizeBytes)
        await client.closeScan(handle: handle)
        let closed = await client.scanFindings(handle: handle)
        XCTAssertTrue(closed.isEmpty)
    }

    func testAScanFillsFoundSoFarAndTheNextScanStartsEmpty() async {
        let client = MockCoreClient(scenario: .normal, totalSteps: 10)
        let model = AppModel(client: client, defaults: defaults, pollInterval: 0)
        XCTAssertTrue(model.scanFindings.isEmpty)

        model.startScan()
        await model.waitForScan()
        XCTAssertFalse(model.scanFindings.isEmpty, "read while the scan ran")
        XCTAssertTrue(model.checkedItemIDs.isEmpty, "finding something never ticks it")

        model.startScan()
        XCTAssertTrue(model.scanFindings.isEmpty, "a new scan starts with nothing found")
        await model.waitForScan()
    }
}
