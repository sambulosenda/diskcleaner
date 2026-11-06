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

    func testCoreErrorsBecomePlainWords() {
        XCTAssertEqual(AppModel.message(for: CoreError.ScanFailed(reason: "Roomy could not read your home folder.")),
                       "Roomy could not read your home folder.")
        XCTAssertFalse(AppModel.message(for: CoreError.UnknownScan).contains("UnknownScan"))
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
        XCTAssertTrue(summary.contains("Everything else"))
        XCTAssertTrue(summary.contains("Free space"))
    }
    // MARK: - Category screens (T11)

    private func scannedModel(_ mode: Mode = .simple) async -> AppModel {
        let model = makeModel()
        model.mode = mode
        model.startScan()
        await model.waitForScan()
        return model
    }

    private func items(on screen: SidebarItem, in model: AppModel) async -> [ItemSummary] {
        model.selection = screen
        await model.waitForItems()
        guard case .loaded(let items) = model.listState else {
            XCTFail("\(screen) did not load: \(model.listState)")
            return []
        }
        return items
    }

    func testListScreensShowTheirCategories() async {
        let model = await scannedModel()
        let backups = await items(on: .backups, in: model)
        XCTAssertEqual(backups.map(\.id), ["iphone-backup"])
        let caches = await items(on: .appsAndCaches, in: model)
        XCTAssertFalse(caches.isEmpty)
        XCTAssertTrue(caches.allSatisfy { [.cache, .appData].contains($0.category) })
        XCTAssertEqual(caches.map(\.sizeBytes), caches.map(\.sizeBytes).sorted(by: >))
        let everything = await items(on: .largeFiles, in: model)
        XCTAssertEqual(everything.count, MockData.items.count)
    }

    func testOverviewAndPlaceholdersAreNotLists() async {
        let model = await scannedModel()
        for screen in [SidebarItem.overview, .cleanup, .history, .android, .javascript] {
            model.selection = screen
            XCTAssertEqual(model.listState, .idle, "\(screen)")
        }
    }

    func testListsWaitForAScan() async {
        let model = makeModel()
        model.selection = .backups
        await model.waitForItems()
        XCTAssertEqual(model.listState, .idle)
    }

    func testTierFilterAndSortGoToTheCore() async {
        let model = await scannedModel()
        model.selection = .largeFiles
        model.tierFilter = .keep
        model.sort = .smallestFirst
        await model.waitForItems()
        guard case .loaded(let items) = model.listState else { return XCTFail("\(model.listState)") }
        XCTAssertFalse(items.isEmpty)
        XCTAssertTrue(items.allSatisfy { $0.tier == .keep })
        XCTAssertEqual(items.map(\.sizeBytes), items.map(\.sizeBytes).sorted())
    }

    func testSelectingARowShowsItsDetail() async {
        let model = await scannedModel()
        _ = await items(on: .backups, in: model)
        model.selectedItemID = "iphone-backup"
        await model.waitForItems()
        guard case .loaded(let detail) = model.detailState else { return XCTFail("\(model.detailState)") }
        XCTAssertEqual(detail.id, "iphone-backup")
        XCTAssertFalse(ItemDetailText.whyHere(detail).isEmpty)
        XCTAssertFalse(ItemDetailText.howItComesBack(detail).isEmpty)
        model.selectedItemID = nil
        XCTAssertEqual(model.detailState, .none)
    }

    func testSimpleModeDetailsHaveNoPath() async {
        let model = await scannedModel(.simple)
        for item in await items(on: .largeFiles, in: model) {
            model.selectedItemID = item.id
            await model.waitForItems()
            guard case .loaded(let detail) = model.detailState else { return XCTFail("\(model.detailState)") }
            XCTAssertNil(detail.path, item.id)
        }
    }

    func testNothingIsTickedAndKeepItemsCannotBe() async {
        let model = await scannedModel()
        let everything = await items(on: .largeFiles, in: model)
        XCTAssertTrue(model.checkedItemIDs.isEmpty, "nothing is preselected, Review first above all")
        let keep = try? XCTUnwrap(everything.first { $0.tier == .keep })
        let review = try? XCTUnwrap(everything.first { $0.tier == .reviewFirst })
        if let keep { model.setChecked(true, for: keep) }
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
        if let review {
            model.setChecked(true, for: review)
            XCTAssertTrue(model.isChecked(review))
            model.setChecked(false, for: review)
            XCTAssertFalse(model.isChecked(review))
        }
    }

    func testANewScanClearsTicksAndSelection() async {
        let model = await scannedModel()
        let everything = await items(on: .largeFiles, in: model)
        let first = everything.first { $0.tier != .keep }!
        model.setChecked(true, for: first)
        model.selectedItemID = first.id
        model.startScan()
        await model.waitForScan()
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
        XCTAssertNil(model.selectedItemID)
    }

    func testChangingModeAfterAScanScansAgain() async {
        let model = await scannedModel(.developer)
        model.mode = .simple
        XCTAssertTrue(model.isScanning, "results from the other mode must not stay on screen")
        await model.waitForScan()
        XCTAssertNotNil(model.overview)
    }

    func testMergedCategoriesKeepTheCoresOrder() {
        let a = ItemSummary(id: "a", title: "B", explain: "", sizeBytes: 5, tier: .keep, category: .cache)
        let b = ItemSummary(id: "b", title: "a", explain: "", sizeBytes: 5, tier: .keep, category: .appData)
        let c = ItemSummary(id: "c", title: "C", explain: "", sizeBytes: 9, tier: .keep, category: .cache)
        XCTAssertEqual([b, a, c].sorted(by: AppModel.order(for: .largestFirst)).map(\.id), ["c", "a", "b"])
        XCTAssertEqual([c, b, a].sorted(by: AppModel.order(for: .smallestFirst)).map(\.id), ["a", "b", "c"])
        XCTAssertEqual([c, a, b].sorted(by: AppModel.order(for: .title)).map(\.id), ["b", "a", "c"])
    }

    func testUnmeasuredFoldersAreCountedInWords() {
        XCTAssertEqual(UnmeasuredNote.text(for: 1), "1 folder not measured")
        XCTAssertEqual(UnmeasuredNote.text(for: 212), "212 folders not measured")
        XCTAssertNotNil(UnmeasuredNote.fullDiskAccessURL)
    }

    func testRevealStaysInsideHome() {
        let home = URL(fileURLWithPath: "/Users/sam", isDirectory: true)
        XCTAssertEqual(Reveal.url(forDisplayPath: "~/Library/Caches", home: home)?.path, "/Users/sam/Library/Caches")
        XCTAssertEqual(Reveal.url(forDisplayPath: "~", home: home)?.path, "/Users/sam")
        XCTAssertNil(Reveal.url(forDisplayPath: "/etc/passwd", home: home))
        XCTAssertNil(Reveal.url(forDisplayPath: "~/../other", home: home))
        XCTAssertNil(Reveal.url(forDisplayPath: "~/a//b", home: home))
        XCTAssertNil(Reveal.url(forDisplayPath: "~other/x", home: home))
    }
}
