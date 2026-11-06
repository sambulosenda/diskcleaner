import XCTest
@testable import Roomy

@MainActor
final class AppModelTests: XCTestCase {
    private let suiteName = "RoomyTests.AppModel"
    private let fixedDate = Date(timeIntervalSince1970: 1_800_000_000)

    /// A fresh, empty settings store for each test (XCTest makes a new instance per test).
    private lazy var defaults: UserDefaults = {
        let store = UserDefaults(suiteName: suiteName) ?? .standard
        store.removePersistentDomain(forName: suiteName)
        return store
    }()

    private func makeModel(_ scenario: MockCoreClient.Scenario = .normal) -> AppModel {
        let date = fixedDate
        return AppModel(
            client: MockCoreClient(scenario: scenario, totalSteps: 5),
            defaults: defaults,
            pollInterval: 0,
            now: { date }
        )
    }

    func testStartsBeforeAnyScanInSimpleMode() {
        let model = makeModel()
        XCTAssertEqual(model.scanState, .neverScanned)
        XCTAssertEqual(model.mode, .simple)
        XCTAssertEqual(model.selection, .overview)
    }

    func testScanEndsWithTheOverview() async {
        let model = makeModel()
        model.startScan()
        XCTAssertTrue(model.isScanning)
        await model.waitForScan()
        XCTAssertEqual(model.scanState, .loaded(MockData.overview(for: .normal), scannedAt: fixedDate))
    }

    func testFailedScanShowsAPlainMessage() async {
        let model = makeModel(.failure)
        model.startScan()
        await model.waitForScan()
        guard case .failed(let message) = model.scanState else { return XCTFail("got \(model.scanState)") }
        XCTAssertFalse(message.isEmpty)
    }

    func testCancelGoesBackToWhatWasShowing() async {
        let model = makeModel()
        model.startScan()
        model.cancelScan()
        XCTAssertEqual(model.scanState, .neverScanned)
        await model.waitForScan()
        XCTAssertEqual(model.scanState, .neverScanned, "a cancelled scan must not land later")
    }

    func testCancelAfterAScanKeepsTheEarlierResults() async {
        let model = makeModel()
        model.startScan()
        await model.waitForScan()
        let before = model.scanState
        model.startScan()
        model.cancelScan()
        XCTAssertEqual(model.scanState, before)
    }

    func testSecondScanWhileScanningIsIgnored() {
        let model = makeModel()
        model.startScan()
        let state = model.scanState
        model.startScan()
        XCTAssertEqual(model.scanState, state)
        model.cancelScan()
    }

    func testDeveloperGroupOnlyInDeveloperMode() {
        let model = makeModel()
        XCTAssertFalse(model.sidebarSections.flatMap { $0 }.contains(where: \.isDeveloperOnly))
        model.mode = .developer
        XCTAssertEqual(model.sidebarSections.count, 3)
        XCTAssertTrue(model.sidebarSections.contains(SidebarItem.developerItems))
    }

    func testSidebarListsEveryWireframeEntryInOrder() {
        let model = makeModel()
        XCTAssertEqual(model.sidebarSections.flatMap { $0 }.map(\.title), [
            "Overview", "Backups", "Downloads and installers", "Large files", "Apps and caches",
            "System Data", "Cleanup", "History",
        ])
        model.mode = .developer
        XCTAssertEqual(SidebarItem.developerItems.map(\.title), ["Projects", "Xcode", "Android", "JavaScript", "Docker"])
    }

    func testSwitchingToSimpleLeavesADeveloperScreen() {
        let model = makeModel()
        model.mode = .developer
        model.selection = .docker
        model.mode = .simple
        XCTAssertEqual(model.selection, .overview)
    }

    func testModeIsRemembered() {
        let model = makeModel()
        model.mode = .developer
        XCTAssertEqual(makeModel().mode, .developer)
    }

    func testSearchNarrowsWorthReviewing() {
        let model = makeModel()
        let overview = MockData.overview(for: .normal)
        XCTAssertEqual(model.worthReviewing(in: overview), overview.topItems)
        model.searchText = "BACKUP"
        XCTAssertEqual(model.worthReviewing(in: overview).map(\.id), ["iphone-backup"])
        model.searchText = "nothing matches this"
        XCTAssertTrue(model.worthReviewing(in: overview).isEmpty)
    }

    func testTierLabelsAreWords() {
        XCTAssertEqual(Tier.allCases.map(\.label), ["Likely safe", "Review first", "Keep"])
    }

    func testStripSegmentsCoverTheDisk() {
        let segments = CategoryStrip.segments(for: MockData.overview(for: .normal))
        let total = segments.reduce(CGFloat(0)) { $0 + $1.fraction }
        XCTAssertEqual(total, 1, accuracy: 0.0001)
        XCTAssertTrue(segments.allSatisfy { (0...1).contains($0.likelySafeFraction) })
    }

    func testStripSummaryNamesEveryCategory() {
        let overview = MockData.overview(for: .normal)
        let summary = CategoryLegend.spokenSummary(for: overview)
        for entry in overview.categories {
            XCTAssertTrue(summary.contains(entry.category.title), entry.category.title)
        }
        XCTAssertTrue(summary.contains("Free space"))
    }
}
