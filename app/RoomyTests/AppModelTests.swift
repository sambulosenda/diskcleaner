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
            "System Data", "Remove apps", "Cleanup", "History",
        ])
        model.mode = .developer
        XCTAssertEqual(SidebarItem.developerItems.map(\.title), ["Projects", "Xcode", "Android", "JavaScript", "Docker", "Tool clean-ups"])
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
        XCTAssertNotNil(FullDiskAccessSettings.url)
    }

    // MARK: - Full Disk Access

    private func makeModel(access: FullDiskAccess) -> (AppModel, MockCoreClient) {
        let client = MockCoreClient(scenario: .normal, totalSteps: 5)
        client.access = access
        let date = fixedDate
        return (AppModel(client: client, defaults: defaults, pollInterval: 0, now: { date }), client)
    }

    func testFirstLaunchWithoutAccessOpensTheAccessScreen() async {
        let (model, _) = makeModel(access: .notGranted)
        XCTAssertNil(model.fullDiskAccess)
        await model.windowDidAppear()
        XCTAssertEqual(model.fullDiskAccess, .notGranted)
        XCTAssertTrue(model.showsAccessOnboarding)
        XCTAssertFalse(model.isScanning, "nothing starts until the user chooses")
    }

    func testFirstLaunchWithAccessNeverShowsTheAccessScreen() async {
        let (model, client) = makeModel(access: .granted)
        await model.windowDidAppear()
        XCTAssertFalse(model.showsAccessOnboarding)
        // Even if access is switched off later, the screen doesn't open by itself again.
        client.access = .notGranted
        let again = AppModel(client: client, defaults: defaults, pollInterval: 0)
        await again.windowDidAppear()
        XCTAssertFalse(again.showsAccessOnboarding)
    }

    func testTheAccessScreenWhenTheCoreCantTellIsNotShown() async {
        let (model, _) = makeModel(access: .unknown)
        await model.windowDidAppear()
        XCTAssertFalse(model.showsAccessOnboarding)
    }

    func testContinuingWithoutAccessDoesNotAskAgainOnNextLaunch() async {
        let (model, client) = makeModel(access: .notGranted)
        await model.windowDidAppear()
        model.finishAccessOnboarding(scan: false)
        XCTAssertFalse(model.showsAccessOnboarding)
        XCTAssertFalse(model.isScanning)
        let next = AppModel(client: client, defaults: defaults, pollInterval: 0)
        await next.windowDidAppear()
        XCTAssertFalse(next.showsAccessOnboarding)
        XCTAssertEqual(next.fullDiskAccess, .notGranted)
    }

    func testQuittingBeforeChoosingAsksAgainOnNextLaunch() async {
        let (model, client) = makeModel(access: .notGranted)
        await model.windowDidAppear()
        XCTAssertTrue(model.showsAccessOnboarding)
        let next = AppModel(client: client, defaults: defaults, pollInterval: 0)
        await next.windowDidAppear()
        XCTAssertTrue(next.showsAccessOnboarding)
    }

    func testAccessSwitchedOnIsNoticedAndScanStarts() async {
        let (model, client) = makeModel(access: .notGranted)
        await model.windowDidAppear()
        client.access = .granted
        await model.refreshFullDiskAccess()
        XCTAssertEqual(model.fullDiskAccess, .granted)
        model.finishAccessOnboarding(scan: true)
        XCTAssertFalse(model.showsAccessOnboarding)
        XCTAssertTrue(model.isScanning)
        await model.waitForScan()
        XCTAssertNotNil(model.overview)
    }

    func testTheOverviewOffersAccessOnlyWhenFoldersWereMissedAndAccessIsOff() async {
        let (model, client) = makeModel(access: .notGranted)
        await model.refreshFullDiskAccess()
        let overview = MockData.overview(for: .normal)
        XCTAssertTrue(model.shouldOfferAccess(for: overview))
        client.access = .granted
        await model.refreshFullDiskAccess()
        XCTAssertFalse(model.shouldOfferAccess(for: overview))
        client.access = .notGranted
        await model.refreshFullDiskAccess()
        var nothingMissed = overview
        nothingMissed.unreadableCount = 0
        nothingMissed.unreadableAreas = []
        XCTAssertFalse(model.shouldOfferAccess(for: nothingMissed))
    }

    func testUnscannedAreasStayShortAndAddUp() {
        let areas = MockData.unreadableAreas
        XCTAssertEqual(areas.reduce(0) { $0 + $1.folderCount }, MockData.unreadableCount)
        let rows = UnscannedAreasBox.rows(for: areas)
        XCTAssertEqual(rows.count, UnscannedAreasBox.shownAreas)
        XCTAssertEqual(rows.reduce(0) { $0 + $1.folderCount }, MockData.unreadableCount)
        XCTAssertEqual(rows.last?.area, .other)
        XCTAssertEqual(rows.filter { $0.area == .other }.count, 1)
        XCTAssertEqual(UnscannedAreasBox.rows(for: Array(areas.prefix(2))), Array(areas.prefix(2)))
        XCTAssertTrue(UnscannedAreasBox.summary(for: 212).hasPrefix("Roomy couldn't open 212 folders"))
        XCTAssertTrue(UnscannedAreasBox.summary(for: 1).hasPrefix("Roomy couldn't open 1 folder,"))
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

    // MARK: - Cleanup review (dry run)

    func testReviewChecksTheTickedRowsAndMovesNothing() async {
        let model = await scannedModel()
        let all = await items(on: .largeFiles, in: model)
        guard let derived = all.first(where: { $0.id == "xcode-derived-data" }),
              let backup = all.first(where: { $0.id == "iphone-backup" }) else {
            return XCTFail("mock items missing")
        }
        model.setChecked(true, for: derived)
        model.setChecked(true, for: backup)
        XCTAssertEqual(model.checkedBytes, derived.sizeBytes + backup.sizeBytes)

        model.reviewCleanup()
        XCTAssertEqual(model.cleanupState, .checking)
        await model.waitForCleanupCheck()
        guard case .ready(let preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        // Largest first, each sent as chosen on its own, so the Review first backup would move too.
        XCTAssertEqual(preview.items.map(\.itemId), ["iphone-backup", "xcode-derived-data"])
        XCTAssertTrue(preview.items.allSatisfy { $0.check == .willMove })
        XCTAssertEqual(preview.bytesToMove, derived.sizeBytes + backup.sizeBytes)
        XCTAssertNotNil(preview.plan)
        // The ticks stay, so the user can change them and review again.
        XCTAssertEqual(model.checkedItemIDs, ["xcode-derived-data", "iphone-backup"])
    }

    func testClosingTheReviewDiscardsItsPlan() async throws {
        let model = await scannedModel()
        let all = await items(on: .largeFiles, in: model)
        let first = try XCTUnwrap(all.first { $0.tier == .likelySafe })
        model.setChecked(true, for: first)
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        guard case .ready(let preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        let plan = try XCTUnwrap(preview.plan)
        model.closeCleanupReview()
        XCTAssertEqual(model.cleanupState, .closed)
        // The discard runs on its own task.
        await Task.yield()
        for _ in 0..<10 where (model.client as? MockCoreClient)?.discardedPlans.isEmpty ?? true {
            await Task.yield()
        }
        XCTAssertEqual((model.client as? MockCoreClient)?.discardedPlans, [plan])
    }

    func testReviewNeedsATick() async {
        let model = await scannedModel()
        model.reviewCleanup()
        XCTAssertEqual(model.cleanupState, .closed)
    }

    func testANewScanClosesTheReviewAndClearsTicks() async throws {
        let model = await scannedModel()
        let all = await items(on: .largeFiles, in: model)
        model.setChecked(true, for: try XCTUnwrap(all.first { $0.tier == .likelySafe }))
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        model.startScan()
        await model.waitForScan()
        XCTAssertEqual(model.cleanupState, .closed)
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
        XCTAssertEqual(model.checkedBytes, 0)
    }

    func testCleanupErrorsAreInPlainWords() {
        XCTAssertEqual(AppModel.cleanupMessage(for: CoreError.CleanupUnavailable(reason: "Roomy couldn't open your Trash, so nothing can be moved.")),
                       "Roomy couldn't open your Trash, so nothing can be moved.")
        XCTAssertFalse(AppModel.cleanupMessage(for: CoreError.UnknownItem).isEmpty)
        XCTAssertFalse(AppModel.cleanupMessage(for: CancellationError()).isEmpty)
    }

    // MARK: - Folders to keep

    func testKeptFoldersAreSavedAndSentBeforeEveryReview() async throws {
        let client = MockCoreClient(scenario: .normal, totalSteps: 5)
        let model = AppModel(client: client, defaults: defaults, pollInterval: 0, now: { Date() })
        let folder = NSHomeDirectory() + "/Developer/keep-me"
        await model.addProtectedFolders([URL(fileURLWithPath: folder)])
        XCTAssertEqual(model.protectedFolders, [folder])
        XCTAssertNil(model.protectedFoldersMessage)
        XCTAssertEqual(defaults.stringArray(forKey: AppModel.protectedFoldersKey), [folder])

        // A new launch reads them back and sends them before checking anything.
        let fresh = MockCoreClient(scenario: .normal, totalSteps: 5)
        let relaunched = AppModel(client: fresh, defaults: defaults, pollInterval: 0, now: { Date() })
        XCTAssertEqual(relaunched.protectedFolders, [folder])
        relaunched.startScan()
        await relaunched.waitForScan()
        let all = await items(on: .largeFiles, in: relaunched)
        let row = try XCTUnwrap(all.first { $0.tier == .likelySafe })
        relaunched.setChecked(true, for: row)
        relaunched.reviewCleanup()
        await relaunched.waitForCleanupCheck()
        XCTAssertEqual(fresh.protectedFolders, [folder])

        await model.removeProtectedFolder(folder)
        XCTAssertEqual(model.protectedFolders, [])
    }

    func testAFolderOutsideHomeIsRefusedAndTheListStays() async {
        let model = makeModel()
        await model.addProtectedFolders([URL(fileURLWithPath: "/Library/Caches")])
        XCTAssertEqual(model.protectedFolders, [])
        XCTAssertEqual(model.protectedFoldersMessage, "Choose a folder inside your home folder.")
        XCTAssertNil(defaults.stringArray(forKey: AppModel.protectedFoldersKey))
    }

    // MARK: - Cleanup (moving to the Trash), put back, History

    /// A model with a finished scan, one likely safe row ticked and the review ready.
    private func reviewedModel() async throws -> (AppModel, ItemSummary) {
        let model = await scannedModel()
        let all = await items(on: .largeFiles, in: model)
        let first = try XCTUnwrap(all.first { $0.tier == .likelySafe })
        model.setChecked(true, for: first)
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        guard case .ready = model.cleanupState else {
            XCTFail("\(model.cleanupState)")
            return (model, first)
        }
        return (model, first)
    }

    func testMoveToTrashShowsWhatMovedAndClearsTicks() async throws {
        let (model, first) = try await reviewedModel()
        model.moveToTrash()
        guard case .moving = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertTrue(model.cleanupState.isBusy)
        await model.waitForCleanupCheck()
        guard case .done(let result) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(result.items.map(\.itemId), [first.id])
        XCTAssertEqual(result.bytesMoved, first.sizeBytes)
        XCTAssertNotNil(model.lastCleanup)
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
        XCTAssertEqual(model.history.count, 1)
        XCTAssertEqual(CleanupText.movedHeadline(result).hasPrefix("Moved 1 item"), true)
    }

    func testClosingAfterACleanupScansAgain() async throws {
        let (model, _) = try await reviewedModel()
        model.moveToTrash()
        await model.waitForCleanupCheck()
        model.closeCleanupReview()
        XCTAssertEqual(model.cleanupState, .closed)
        XCTAssertTrue(model.isScanning, "the sizes are out of date after a cleanup")
        await model.waitForScan()
    }

    func testPutBackUndoesTheLastCleanupOnce() async throws {
        let (model, first) = try await reviewedModel()
        model.moveToTrash()
        await model.waitForCleanupCheck()
        model.putBack()
        guard case .puttingBack = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        await model.waitForCleanupCheck()
        guard case .putBack(let result) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(result.bytesPutBack, first.sizeBytes)
        XCTAssertNil(model.lastCleanup, "only once")
        XCTAssertNotNil(model.history.first?.putBackAt)
    }

    func testAScanAfterTheCleanupLeavesTheResultOpenAndIsNotRepeated() async throws {
        let (model, _) = try await reviewedModel()
        model.moveToTrash()
        await model.waitForCleanupCheck()
        model.startScan()
        await model.waitForScan()
        guard case .done = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        model.closeCleanupReview()
        XCTAssertEqual(model.cleanupState, .closed)
        XCTAssertFalse(model.isScanning, "the scan already saw the cleanup")
    }

    func testPutBackFromHistoryRunsOnce() async throws {
        let (model, _) = try await reviewedModel()
        model.moveToTrash()
        await model.waitForCleanupCheck()
        model.closeCleanupReview()
        await model.waitForScan()
        model.putBack()
        XCTAssertTrue(model.isPuttingBack)
        model.putBack()
        await model.waitForCleanupCheck()
        XCTAssertFalse(model.isPuttingBack)
        XCTAssertEqual((model.client as? MockCoreClient)?.putBackCalls, 1)
        XCTAssertNil(model.lastCleanup)
        XCTAssertNil(model.lastCleanupResult)
        await model.waitForScan()
    }

    func testAFailedPutBackFromHistoryCanBeTriedAgain() async throws {
        let (model, _) = try await reviewedModel()
        model.moveToTrash()
        await model.waitForCleanupCheck()
        model.closeCleanupReview()
        await model.waitForScan()
        let mock = try XCTUnwrap(model.client as? MockCoreClient)
        mock.nextPutBackError = .CleanupUnavailable(reason: "Roomy couldn't open your Trash.")
        model.putBack()
        await model.waitForCleanupCheck()
        XCTAssertNotNil(model.historyMessage, "History says why")
        XCTAssertNotNil(model.lastCleanup, "nothing moved, so it can be tried again")
        model.putBack()
        await model.waitForCleanupCheck()
        XCTAssertNil(model.historyMessage)
        XCTAssertNil(model.lastCleanup)
        XCTAssertEqual(mock.putBackCalls, 2)
        await model.waitForScan()
    }

    func testPutBackStaysAvailableWhenHistoryIsUnavailable() async throws {
        let (model, first) = try await reviewedModel()
        (model.client as? MockCoreClient)?.historyUnavailable = true
        model.moveToTrash()
        await model.waitForCleanupCheck()
        XCTAssertTrue(model.history.isEmpty)
        XCTAssertNotNil(model.lastCleanup)
        XCTAssertEqual(model.lastCleanupResult?.bytesMoved, first.sizeBytes)
    }

    func testCancelLeavesEverythingWhereItIs() async throws {
        let (model, _) = try await reviewedModel()
        model.closeCleanupReview()
        XCTAssertEqual(model.cleanupState, .closed)
        XCTAssertNil(model.lastCleanup)
        await model.reloadHistory()
        XCTAssertTrue(model.history.isEmpty)
        XCTAssertFalse(model.isScanning)
    }

    func testConfirmTextSaysTheSpaceComesBackWhenTheTrashIsEmptied() {
        XCTAssertTrue(CleanupText.confirmMessage.contains("empty the Trash"))
    }

    // MARK: iPhone and iPad backups

    private let gb = Locale(identifier: "en_GB")
    private let utc = TimeZone(identifier: "UTC") ?? .current

    func testBackupWarningNamesTheDeviceAndDate() {
        let backup = DeviceBackup(deviceName: "Sam's iPhone", lastBackupUnixSeconds: 1_793_348_100)
        XCTAssertEqual(BackupWarning.headline(backup), "This may be your only copy of Sam's iPhone.")
        XCTAssertEqual(BackupWarning.lastBackedUp(backup, locale: gb, timeZone: utc),
                       "Last backed up to this Mac on 30 October 2026.")
        let details = BackupWarning.details(backup, locale: gb, timeZone: utc)
        XCTAssertTrue(details.contains("Check it's backed up to iCloud first"))
        XCTAssertTrue(details.contains("iCloud Backup"))
    }

    func testBackupWarningStillShowsWhenDetailsAreUnknown() {
        let backup = DeviceBackup(deviceName: nil, lastBackupUnixSeconds: nil)
        XCTAssertEqual(BackupWarning.headline(backup), "This may be your only copy of this iPhone or iPad.")
        XCTAssertTrue(BackupWarning.details(backup).contains("couldn't tell when"))
        XCTAssertTrue(BackupWarning.details(backup).contains("iCloud first"))
    }

    func testConfirmTextWarnsAboutABackupThatWouldMove() {
        let backup = DeviceBackup(deviceName: "Family iPad", lastBackupUnixSeconds: nil)
        func preview(_ check: CleanupCheck, _ device: DeviceBackup?) -> CleanupPreview {
            CleanupPreview(
                plan: PlanHandle(id: 1),
                items: [CleanupPreviewItem(itemId: "a", title: "iPhone or iPad backup", sizeBytes: 10,
                                           check: check, path: nil, deviceBackup: device)],
                bytesToMove: 10,
                expiresInSeconds: 600
            )
        }
        let warned = CleanupText.confirmText(preview(.willMove, backup))
        XCTAssertTrue(warned.hasPrefix("This may be your only copy of Family iPad."))
        XCTAssertTrue(warned.contains("iCloud first"))
        XCTAssertTrue(warned.hasSuffix(CleanupText.confirmMessage))
        XCTAssertEqual(CleanupText.confirmText(preview(.wontMove(reason: "No."), backup)), CleanupText.confirmMessage)
        XCTAssertEqual(CleanupText.confirmText(preview(.willMove, nil)), CleanupText.confirmMessage)
    }

    func testMockBackupDetailCarriesItsDevice() async throws {
        let mock = MockCoreClient(scenario: .normal, totalSteps: 1)
        let handle = await mock.startScan(mode: .simple)
        var phase = ScanPhase.scanning
        for _ in 0..<10 where !phase.hasEnded {
            phase = try await mock.scanProgress(handle: handle).phase
        }
        let detail = try await mock.itemDetail(handle: handle, itemId: "iphone-backup")
        XCTAssertEqual(detail.deviceBackup?.deviceName, "Sam's iPhone")
        let otherId = try XCTUnwrap(MockData.found(for: .normal).first { $0.id != "iphone-backup" }?.id)
        let other = try await mock.itemDetail(handle: handle, itemId: otherId)
        XCTAssertNil(other.deviceBackup)
    }

    // MARK: - App uninstaller

    func testRemoveAppsIsInTheCleanUpGroup() {
        XCTAssertEqual(SidebarItem.actionItems, [.apps, .cleanup, .history])
        XCTAssertEqual(SidebarItem.apps.title, "Remove apps")
        XCTAssertNil(SidebarItem.apps.itemFilter)
    }

    func testOpeningRemoveAppsListsTheApps() async {
        let model = makeModel()
        model.selection = .apps
        await model.waitForApps()
        XCTAssertEqual(model.appsState, .loaded(MockData.apps))
        model.searchText = "notes"
        XCTAssertEqual(model.visibleApps.map(\.name), ["Notes Pro"])
    }

    func testChoosingAnAppTicksOnlyWhatTheCoreSays() async throws {
        let model = makeModel()
        model.selection = .apps
        await model.waitForApps()
        model.selectedAppID = MockData.apps[0].appId
        await model.waitForApps()
        guard case .loaded(let review) = model.appReviewState else { return XCTFail("\(model.appReviewState)") }
        XCTAssertEqual(model.checkedPartIDs, Set(review.parts.filter(\.preselected).map(\.itemId)))
        XCTAssertFalse(model.checkedPartIDs.contains("part-1"), "app data is never ticked for the user")
        XCTAssertFalse(model.checkedPartIDs.contains("part-2"), "a name match is never ticked for the user")
        XCTAssertTrue(review.parts.allSatisfy { $0.path == nil }, "simple mode shows no paths")
    }

    func testUninstallGoesThroughTheCleanupReviewAndCanBePutBack() async throws {
        let model = makeModel()
        model.selection = .apps
        await model.waitForApps()
        model.selectedAppID = MockData.apps[1].appId
        await model.waitForApps()
        model.reviewUninstall()
        await model.waitForCleanupCheck()
        guard case .ready(let preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(Set(preview.items.map(\.itemId)), model.checkedPartIDs)
        XCTAssertTrue(preview.items.contains { $0.itemId == "app" })
        model.moveToTrash()
        await model.waitForCleanupCheck()
        guard case .done(let result) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertTrue(result.items.allSatisfy(\.done))
        XCTAssertNotNil(model.lastCleanup)
        model.putBack()
        await model.waitForCleanupCheck()
        guard case .putBack = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
    }

    func testAnOpenAppIsRefusedWithPlainWords() async {
        let client = MockCoreClient(scenario: .normal, totalSteps: 5)
        client.appIsOpen = true
        let model = AppModel(client: client, defaults: defaults, pollInterval: 0)
        model.selection = .apps
        await model.waitForApps()
        model.selectedAppID = MockData.apps[0].appId
        await model.waitForApps()
        model.reviewUninstall()
        await model.waitForCleanupCheck()
        XCTAssertEqual(model.cleanupState, .failed("Chat is open. Quit Chat, then try again."))
    }

    func testNothingTickedOpensNoReview() async {
        let model = makeModel()
        model.selection = .apps
        await model.waitForApps()
        model.selectedAppID = MockData.apps[0].appId
        await model.waitForApps()
        for id in model.checkedPartIDs {
            model.setPartChecked(false, for: AppPart(itemId: id, title: "", explain: "", sizeBytes: 0,
                                                     preselected: false, sure: true, path: nil))
        }
        model.reviewUninstall()
        XCTAssertEqual(model.cleanupState, .closed)
    }

    func testUnknownAppErrorReadsPlainly() {
        XCTAssertEqual(AppModel.message(for: CoreError.UnknownApp), "Roomy lost track of this app. Please look again.")
    }
}
