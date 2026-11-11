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

    func testTheFirstScanHasNoChangesAndTheNextShowsThem() async throws {
        let model = makeModel()
        let mock = try XCTUnwrap(model.client as? MockCoreClient)
        let changes = ScanChanges(
            previousScanAt: 1_799_990_000, scannedBytesBefore: 50_000_000_000, scannedBytesNow: 54_200_000_000,
            categories: [CategoryChange(category: .cache, bytesBefore: 1_000_000_000, bytesNow: 5_200_000_000)],
            items: [ItemChange(itemId: "safari-cache", title: "Safari cache", category: .cache, kind: .grew,
                               bytesBefore: 1_000_000_000, bytesNow: 5_200_000_000)]
        )
        mock.changes = changes
        model.startScan()
        await model.waitForScan()
        XCTAssertNil(model.scanChanges, "nothing to compare the first scan with")
        model.startScan()
        await model.waitForScan()
        XCTAssertEqual(model.scanChanges, changes)
    }

    func testChangeSummariesUsePlainWords() {
        func summary(_ before: UInt64, _ now: UInt64) -> String {
            ChangesSinceLastScan.summary(for: ScanChanges(
                previousScanAt: 1_800_000_000, scannedBytesBefore: before, scannedBytesNow: now,
                categories: [], items: []))
        }
        XCTAssertTrue(summary(50_000_000_000, 54_000_000_000).hasSuffix("It's \(Format.bytes(4_000_000_000)) bigger now."))
        XCTAssertTrue(summary(54_000_000_000, 50_000_000_000).hasSuffix("It's \(Format.bytes(4_000_000_000)) smaller now."))
        XCTAssertTrue(summary(50_000_000_000, 50_000_001_000).hasSuffix("It's about the same size now."))
        let gone = ItemChange(itemId: nil, title: "Old installer", category: .downloads, kind: .gone,
                              bytesBefore: 2_000_000_000, bytesNow: 0)
        XCTAssertEqual(ChangeRow.difference(for: gone), "\u{2212}\(Format.bytes(2_000_000_000))")
        XCTAssertFalse(ChangeKind.gone.label.isEmpty)
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
            "Overview", "Backups", "Photos, Mail and Music", "Downloads and installers", "Large files", "Duplicates",
            "Apps and caches",
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
        XCTAssertTrue(summary.contains("Apps and macOS"))
        XCTAssertTrue(summary.contains("Free space"))
    }

    func testDiskPartsNameEachLeftoverOnceAndCoverTheDisk() {
        let overview = MockData.overview(for: .normal)
        let parts = DiskPart.parts(for: overview)
        let total = parts.reduce(UInt64(0)) { $0 + $1.bytes }
        XCTAssertEqual(total, overview.diskTotal)
        XCTAssertEqual(Set(parts.map(\.title)).count, parts.count, "every part has its own name")
        XCTAssertFalse(parts.contains { $0.title == "Not sorted" || $0.title == "Everything else" })
        XCTAssertEqual(parts.last?.kind, .free)
    }

    func testFreeLineCountsOnlyLikelySafe() {
        let overview = MockData.overview(for: .normal)
        let line = OverviewHeadline.freeLine(for: overview)
        XCTAssertTrue(line.hasPrefix("\(Format.bytes(overview.diskFree)) free now"))
        XCTAssertTrue(line.contains(Format.bytes(OverviewHeadline.freeAfter(overview))))
        XCTAssertEqual(OverviewHeadline.freeAfter(overview), overview.diskFree + overview.likelySafeBytes)
    }

    func testLookAlikeRowsBecomeOneGroup() {
        let groups = ItemGroup.groups(MockData.javascriptProjectItems + [
            ItemSummary(id: "emu", title: "Android emulator", explain: "", sizeBytes: 9_000_000_000,
                        tier: .reviewFirst, category: .developerTools),
        ])
        XCTAssertEqual(groups.map(\.title), ["Android emulator", "Project packages (node_modules)"])
        XCTAssertEqual(groups[1].items.count, 4)
        XCTAssertEqual(groups[1].detail, "4 items")
        XCTAssertEqual(groups[1].bytes, MockData.javascriptProjectItems.reduce(0) { $0 + $1.sizeBytes })
        XCTAssertNil(groups[0].detail)
    }

    func testOverviewButtonsOpenTheListWithoutTickingAnything() async {
        let model = await scannedModel()
        let id = model.overview?.topItems.first { $0.tier == .reviewFirst }?.id
        model.showItems(tier: .reviewFirst, select: id)
        XCTAssertEqual(model.selection, .cleanup)
        XCTAssertEqual(model.tierFilter, .reviewFirst)
        XCTAssertEqual(model.selectedItemID, id)
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
        model.showItems(tier: .likelySafe)
        XCTAssertEqual(model.tierFilter, .likelySafe)
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
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
        let everything = await items(on: .cleanup, in: model)
        XCTAssertEqual(everything.count, MockData.items.count)
    }

    func testOverviewAndPlaceholdersAreNotLists() async {
        let model = await scannedModel()
        for screen in [SidebarItem.overview, .largeFiles, .history, .android, .javascript] {
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
        model.selection = .cleanup
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
        for item in await items(on: .cleanup, in: model) {
            model.selectedItemID = item.id
            await model.waitForItems()
            guard case .loaded(let detail) = model.detailState else { return XCTFail("\(model.detailState)") }
            XCTAssertNil(detail.path, item.id)
        }
    }

    func testNothingIsTickedAndKeepItemsCannotBe() async {
        let model = await scannedModel()
        let everything = await items(on: .cleanup, in: model)
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
        let everything = await items(on: .cleanup, in: model)
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

    func testOpeningRoomyStartsAScanOnce() async {
        let (model, _) = makeModel(access: .granted)
        XCTAssertTrue(model.scanOnOpen, "on by default")
        await model.windowDidAppear()
        XCTAssertTrue(model.isScanning)
        await model.waitForScan()
        guard case .loaded = model.scanState else { return XCTFail("\(model.scanState)") }
        // Opening the menu bar panel later doesn't scan again.
        model.scanIfOpening()
        XCTAssertFalse(model.isScanning)
    }

    func testNoScanOnOpenWhenTurnedOff() async {
        let (model, client) = makeModel(access: .granted)
        model.scanOnOpen = false
        let next = AppModel(client: client, defaults: defaults, pollInterval: 0)
        XCTAssertFalse(next.scanOnOpen, "the choice is saved")
        await next.windowDidAppear()
        next.scanIfOpening()
        XCTAssertFalse(next.isScanning)
        XCTAssertEqual(next.scanState, .neverScanned)
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
        let all = await items(on: .cleanup, in: model)
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

    // MARK: - Quick select (F13)

    func testSelectAllTicksOnlyLikelySafeRows() async {
        let model = await scannedModel()
        let all = await items(on: .cleanup, in: model)
        let safe = Set(all.filter { $0.tier == .likelySafe }.map(\.id))
        XCTAssertFalse(safe.isEmpty)
        XCTAssertTrue(all.contains { $0.tier == .reviewFirst })
        XCTAssertFalse(model.allVisibleLikelySafeChecked)

        model.selectAllLikelySafe()
        XCTAssertEqual(model.checkedItemIDs, safe)
        XCTAssertTrue(model.allVisibleLikelySafeChecked)
        XCTAssertEqual(model.checkedBytes, all.filter { $0.tier == .likelySafe }.reduce(0) { $0 + $1.sizeBytes })

        model.deselectAllLikelySafe()
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
    }

    func testDeselectingLikelySafeLeavesReviewFirstTicks() async throws {
        let model = await scannedModel()
        let all = await items(on: .cleanup, in: model)
        let backup = try XCTUnwrap(all.first { $0.id == "iphone-backup" })
        model.setChecked(true, for: backup)
        model.selectAllLikelySafe()
        model.deselectAllLikelySafe()
        XCTAssertEqual(model.checkedItemIDs, ["iphone-backup"])
    }

    func testRowsTickedTogetherAreNotSentAsChosenAlone() async throws {
        let model = await scannedModel()
        let all = await items(on: .cleanup, in: model)
        let backup = try XCTUnwrap(all.first { $0.id == "iphone-backup" })
        model.selectAllLikelySafe()
        model.setChecked(true, for: backup)
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        guard case .ready = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        let sent = try XCTUnwrap((model.client as? MockCoreClient)?.lastPreviewChoices)
        for choice in sent {
            // Only the row the user ticked by hand counts as chosen alone.
            XCTAssertEqual(choice.chosenAlone, choice.itemId == "iphone-backup", choice.itemId)
        }
    }

    func testATickByHandAfterSelectAllCountsAsChosenAlone() async throws {
        let model = await scannedModel()
        let all = await items(on: .cleanup, in: model)
        let first = try XCTUnwrap(all.first { $0.tier == .likelySafe })
        model.selectAllLikelySafe()
        model.setChecked(false, for: first)
        model.setChecked(true, for: first)
        XCTAssertFalse(model.groupCheckedIDs.contains(first.id))
        XCTAssertTrue(model.likelySafeCheckedIDs.contains(first.id))
    }

    func testOnlyLikelySafeReviewsSkipTheSecondQuestion() async throws {
        let model = await scannedModel()
        let all = await items(on: .cleanup, in: model)
        let backup = try XCTUnwrap(all.first { $0.id == "iphone-backup" })
        model.selectAllLikelySafe()
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        guard case .ready(let safeOnly) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertTrue(model.canSkipSecondConfirm(safeOnly))
        model.closeCleanupReview()

        // One Review first item in the plan brings the second question back.
        model.setChecked(true, for: backup)
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        guard case .ready(let mixed) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertFalse(model.canSkipSecondConfirm(mixed))
    }

    func testAReviewWithNothingToMoveNeverSkipsTheSecondQuestion() {
        let model = makeModel()
        let empty = CleanupPreview(plan: nil, items: [], bytesToMove: 0, expiresInSeconds: 0)
        XCTAssertFalse(model.canSkipSecondConfirm(empty))
    }

    func testQuickSelectAndFinishWords() {
        XCTAssertEqual(QuickSelectText.title(allChecked: false), "Select all Likely safe")
        XCTAssertEqual(QuickSelectText.title(allChecked: true), "Deselect Likely safe")
        let line = CleanupText.freeSpaceLine(freeNow: 20_000_000_000, moved: 18_000_000_000,
                                             diskTotal: 500_000_000_000)
        XCTAssertTrue(line.contains("free now"))
        XCTAssertTrue(line.contains("once you empty the Trash"))
        // Never claims more free space than the disk holds.
        let capped = CleanupText.freeSpaceLine(freeNow: 90, moved: 50, diskTotal: 100)
        XCTAssertTrue(capped.contains(Format.bytes(100)))
        // Copies may share space, so their finish headline promises no figure.
        XCTAssertTrue(CleanupText.finishHeadline(bytesMoved: 5_000_000, estimatesSpace: true).hasSuffix("ready to free"))
        XCTAssertTrue(CleanupText.finishHeadline(bytesMoved: 5_000_000, estimatesSpace: false).hasSuffix("moved to the Trash"))
        XCTAssertEqual(TrashFolder.url(home: URL(fileURLWithPath: "/Users/someone")).path, "/Users/someone/.Trash")
    }

    func testClosingTheReviewDiscardsItsPlan() async throws {
        let model = await scannedModel()
        let all = await items(on: .cleanup, in: model)
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
        let all = await items(on: .cleanup, in: model)
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
        let all = await items(on: .cleanup, in: relaunched)
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
        let all = await items(on: .cleanup, in: model)
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

    // MARK: - Android

    private func androidModel() async -> AppModel {
        let model = makeModel()
        model.mode = .developer
        model.selection = .android
        await model.waitForAndroid()
        return model
    }

    func testOpeningAndroidListsEveryPartWithNothingTicked() async {
        let model = await androidModel()
        XCTAssertEqual(model.androidState, .loaded(MockData.android))
        XCTAssertTrue(model.checkedAndroidIDs.isEmpty, "nothing is ticked for the user, even what no project uses")
        XCTAssertNil(SidebarItem.android.itemFilter, "Android has its own screen, not an item list")
        model.searchText = "ndk"
        XCTAssertEqual(model.visibleAndroidParts.map(\.title), ["NDK 21.4.7075529"])
    }

    func testEveryAndroidPartIsInExactlyOneGroup() {
        for part in MockData.android.parts {
            XCTAssertEqual(AndroidGroup.allCases.filter { $0.kinds.contains(part.kind) }.count, 1, part.title)
        }
    }

    func testAndroidCleanupGoesThroughTheCleanupReviewAndCanBePutBack() async {
        let model = await androidModel()
        let unused = MockData.android.parts.filter { $0.usage == .notUsed }
        for part in unused { model.setAndroidChecked(true, for: part) }
        XCTAssertEqual(model.checkedAndroidBytes, unused.reduce(0) { $0 + $1.sizeBytes })
        model.reviewAndroidCleanup()
        await model.waitForCleanupCheck()
        guard case .ready(let preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(Set(preview.items.map(\.itemId)), model.checkedAndroidIDs)
        model.moveToTrash()
        await model.waitForCleanupCheck()
        guard case .done(let result) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertTrue(result.items.allSatisfy(\.done))
        model.putBack()
        await model.waitForCleanupCheck()
        guard case .putBack = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        // Closing the review looks again, so the sizes are right.
        model.closeCleanupReview()
        await model.waitForAndroid()
        XCTAssertTrue(model.checkedAndroidIDs.isEmpty)
    }

    func testNothingTickedOpensNoAndroidReview() async {
        let model = await androidModel()
        model.reviewAndroidCleanup()
        XCTAssertEqual(model.cleanupState, .closed)
    }

    func testSwitchingToSimpleModeForgetsTheAndroidScreen() async {
        let model = await androidModel()
        model.setAndroidChecked(true, for: MockData.android.parts[1])
        model.mode = .simple
        XCTAssertEqual(model.selection, .overview)
        XCTAssertEqual(model.androidState, .idle)
        XCTAssertTrue(model.checkedAndroidIDs.isEmpty)
    }

    func testAndroidOverviewInSimpleModeCarriesNoPaths() async {
        let client = MockCoreClient(scenario: .normal, totalSteps: 5)
        let overview = await client.androidOverview(mode: .simple)
        XCTAssertTrue(overview.parts.allSatisfy { $0.path == nil })
        XCTAssertTrue(overview.projects.allSatisfy { $0.path == nil })
    }

    func testAStaleAndroidIdReadsPlainly() {
        XCTAssertEqual(AppModel.androidMessage(for: CoreError.UnknownItem),
                       "Some of these are no longer where Roomy last looked. Please look again.")
    }

    // MARK: - JavaScript

    private func javascriptModel() async -> AppModel {
        let model = await scannedModel(.developer)
        model.selection = .javascript
        await model.waitForJavaScript()
        return model
    }

    func testTheJavaScriptScreenListsProjectsAndCachesInDeveloperMode() async {
        let model = await javascriptModel()
        guard case .loaded(let overview) = model.javascriptState else {
            return XCTFail("\(model.javascriptState)")
        }
        XCTAssertEqual(overview.projects.map(\.name), ["shop", "blog", "landing", "cli"])
        XCTAssertEqual(model.visibleJavaScriptCaches.map(\.id), ["npm-cache"])
        // Nothing is ticked for the user.
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
    }

    func testJavaScriptProjectsSortByLastWorkedOnWithUndatedLast() async {
        let model = await javascriptModel()
        XCTAssertEqual(model.visibleJavaScriptProjects.map(\.name), ["shop", "blog", "landing", "cli"])
        model.javascriptSort = .leastRecent
        XCTAssertEqual(model.visibleJavaScriptProjects.map(\.name), ["landing", "blog", "shop", "cli"])
    }

    func testJavaScriptSearchMatchesNameFolderAndTool() async {
        let model = await javascriptModel()
        model.searchText = "pnpm"
        XCTAssertEqual(model.visibleJavaScriptProjects.map(\.name), ["shop"])
        model.searchText = "~/code"
        XCTAssertEqual(model.visibleJavaScriptProjects.map(\.name), ["landing"])
    }

    func testATickedProjectGoesThroughTheUsualCleanupReview() async throws {
        let model = await javascriptModel()
        let blog = try XCTUnwrap(model.visibleJavaScriptProjects.first { $0.name == "blog" })
        model.setChecked(true, for: blog.item)
        XCTAssertEqual(model.checkedBytes, blog.item.sizeBytes)
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        guard case .ready(let preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(preview.items.map(\.itemId), [blog.item.id])
        XCTAssertEqual(preview.items.first?.check, .willMove)
    }

    func testTheJavaScriptScreenIsEmptyInSimpleMode() async {
        let model = await scannedModel(.simple)
        model.selection = .javascript
        await model.waitForJavaScript()
        XCTAssertEqual(model.javascriptState, .loaded(JavaScriptOverview(projects: [], caches: [], totalBytes: 0)))
    }

    func testJavaScriptWordsSayWhenAndHowToGetPackagesBack() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let overview = MockData.javascript(now: now)
        let shop = overview.projects[0], cli = overview.projects[3]
        XCTAssertEqual(JavaScriptText.lastWorkedOn(cli, now: now), "No date found")
        XCTAssertTrue(JavaScriptText.subtitle(shop, now: now).contains("pnpm"))
        XCTAssertTrue(JavaScriptText.subtitle(shop, now: now).contains("~/Developer/shop"))
        XCTAssertTrue(JavaScriptText.sourceHelp(shop).contains("run pnpm install"))
        XCTAssertTrue(JavaScriptText.sourceHelp(cli).contains("no package.json"))
        XCTAssertEqual(
            JavaScriptText.summary(overview, now: now),
            "4 projects, \(Format.bytes(3_602_000_000)). 2 not worked on for 3 months or more, \(Format.bytes(1_552_000_000)).")
    }

    // MARK: - Photos, Mail and Music

    func testLibrariesScreenAsksForAScanFirst() async {
        let model = makeModel()
        let libraries = LibrariesModel(client: model.client)
        await libraries.load(handle: model.finishedScan)
        XCTAssertEqual(libraries.phase, .noScan)
    }

    func testLibrariesShowOnlyWhatWasFoundLargestFirst() async {
        let model = await scannedModel()
        let libraries = LibrariesModel(client: model.client)
        await libraries.load(handle: model.finishedScan)
        guard case .loaded(let rows) = libraries.phase else { return XCTFail("\(libraries.phase)") }
        XCTAssertEqual(rows.count, 6)
        let shown = LibrariesModel.shown(rows)
        XCTAssertEqual(shown.map(\.kind), [.photos, .messagesAttachments, .music, .mail])
        XCTAssertTrue(shown.first { $0.kind == .mail }?.needsAccess ?? false)
    }

    func testLibrariesScreenIsInTheSidebarAndIsNotAList() {
        XCTAssertEqual(SidebarItem.mainItems.prefix(3), [.overview, .backups, .libraries])
        XCTAssertNil(SidebarItem.libraries.itemFilter)
        XCTAssertFalse(SidebarItem.libraries.isDeveloperOnly)
    }

    func testEveryLibraryHasWordsAndAnApp() {
        let kinds: [LibraryKind] = [.photos, .music, .tv, .mail, .messagesAttachments, .finalCut]
        for kind in kinds {
            XCTAssertFalse(LibraryText.title(kind).isEmpty)
            XCTAssertFalse(LibraryText.explain(kind).isEmpty)
            XCTAssertFalse(LibraryText.howToFree(kind).isEmpty)
            XCTAssertTrue(LibraryText.appBundleID(kind).hasPrefix("com.apple."))
            XCTAssertFalse(LibraryText.howToFree(kind).lowercased().contains("junk"))
        }
        XCTAssertTrue(LibraryActions.messagesAttachmentsFolder.path.hasSuffix("/Library/Messages/Attachments"))
    }

    // MARK: - Large files and old Downloads

    private func filesModel(_ mode: Mode = .simple) async -> (AppModel, LargeFiles) {
        let model = await scannedModel(mode)
        model.selection = .largeFiles
        await model.waitForFiles()
        guard case .loaded(let files) = model.filesState else {
            XCTFail("\(model.filesState)")
            return (model, LargeFiles(largeFiles: [], oldDownloads: [], oldAfterDays: 0))
        }
        return (model, files)
    }

    func testLargeFilesListLargestFirstWithNothingTicked() async {
        let (model, files) = await filesModel()
        XCTAssertFalse(files.largeFiles.isEmpty)
        XCTAssertTrue(model.checkedItemIDs.isEmpty, "size is never a reason to tick something")
        XCTAssertEqual(model.listState, .idle, "Large files is not a rule item list")
        let sizes = model.visibleFiles.map(\.sizeBytes)
        XCTAssertEqual(sizes, sizes.sorted(by: >))
        XCTAssertEqual(model.visibleFiles.map(\.itemId), files.largeFiles.map(\.itemId))
    }

    func testOldDownloadsAndOrderByLastOpened() async {
        let (model, files) = await filesModel()
        model.filesList = .oldDownloads
        XCTAssertEqual(Set(model.visibleFiles.map(\.itemId)), Set(files.oldDownloads.map(\.itemId)))
        model.filesList = .largest
        model.fileOrder = .openedLongestAgo
        let when = model.visibleFiles.map { $0.lastOpenedUnixSeconds ?? $0.modifiedUnixSeconds }
        XCTAssertEqual(when, when.sorted())
    }

    func testSearchNarrowsTheFiles() async {
        let (model, _) = await filesModel()
        model.searchText = "wedding"
        XCTAssertEqual(model.visibleFiles.map(\.name), ["Wedding film final.mov"])
    }

    func testAFileInBothListsIsTickedInBoth() async throws {
        let (model, files) = await filesModel()
        let shared = try XCTUnwrap(files.oldDownloads.first { row in files.largeFiles.contains { $0.itemId == row.itemId } })
        model.setChecked(true, for: shared)
        model.filesList = .largest
        XCTAssertTrue(model.visibleFiles.contains { $0.itemId == shared.itemId && model.isChecked($0) })
        XCTAssertEqual(model.checkedBytes, shared.sizeBytes)
        model.setChecked(false, for: shared)
        XCTAssertTrue(model.checkedItemIDs.isEmpty)
    }

    func testTickedFilesGoToTheUsualTrashReviewAndCanBePutBack() async throws {
        let (model, files) = await filesModel()
        let file = try XCTUnwrap(files.largeFiles.first)
        model.setChecked(true, for: file)
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        guard case .ready(let preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(preview.items.map(\.itemId), [file.itemId])
        XCTAssertEqual(preview.items.first?.check, .willMove)
        model.moveToTrash()
        await model.waitForCleanupCheck()
        guard case .done(let result) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(result.bytesMoved, file.sizeBytes)
        XCTAssertEqual(model.history.first?.items.first?.title, file.name)
        model.putBack()
        await model.waitForCleanupCheck()
        guard case .putBack = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
    }

    func testSimpleModeFilesHaveNoPath() async {
        let (model, files) = await filesModel(.simple)
        for row in files.largeFiles + files.oldDownloads {
            XCTAssertNil(row.path)
            XCTAssertFalse(LargeFilesText.location(row).contains("~/"))
        }
        _ = model
    }

    func testLastOpenedWordsSayWhenThereIsNoRecord() {
        let utc = TimeZone(identifier: "UTC")!
        let locale = Locale(identifier: "en_GB")
        let opened = FileRow(itemId: "f", name: "a.mov", place: "Movies", sizeBytes: 1, isFolder: false,
                             lastOpenedUnixSeconds: 1_740_830_400, modifiedUnixSeconds: 0, path: nil)
        XCTAssertEqual(LargeFilesText.lastOpened(opened, locale: locale, timeZone: utc), "Last opened 1 Mar 2025")
        let unknown = FileRow(itemId: "g", name: "b.mov", place: "Home folder", sizeBytes: 1, isFolder: false,
                              lastOpenedUnixSeconds: nil, modifiedUnixSeconds: 1_740_830_400, path: nil)
        XCTAssertEqual(LargeFilesText.lastOpened(unknown, locale: locale, timeZone: utc), "Changed 1 Mar 2025")
        XCTAssertEqual(LargeFilesText.location(unknown), "In your home folder")
        XCTAssertEqual(LargeFilesText.location(opened), "In Movies")
    }

    // MARK: - Duplicate finder

    /// A model on the Duplicates screen with a finished pretend search.
    private func searchedDuplicates(mode: Mode = .simple) async -> (AppModel, DuplicatesModel, DuplicateReport) {
        let model = makeModel()
        model.mode = mode
        model.selection = .duplicates
        let duplicates = model.duplicates
        duplicates.startSearch()
        await duplicates.waitForSearch()
        guard case .finished(let report) = duplicates.state else {
            XCTFail("\(duplicates.state)")
            return (model, duplicates, DuplicateReport(groups: [], couldFreeBytes: 0, folders: []))
        }
        return (model, duplicates, report)
    }

    func testDuplicatesSitsWithTheEverydayScreens() {
        XCTAssertEqual(SidebarItem.duplicates.title, "Duplicates")
        XCTAssertNil(SidebarItem.duplicates.itemFilter)
        XCTAssertTrue(SidebarItem.mainItems.contains(.duplicates))
        XCTAssertFalse(SidebarItem.duplicates.isDeveloperOnly)
    }

    func testTheSearchFindsCopiesAndTicksNothing() async {
        let (_, duplicates, report) = await searchedDuplicates()
        XCTAssertEqual(report, MockData.duplicateReport(mode: .simple))
        XCTAssertTrue(duplicates.checkedCopyIDs.isEmpty, "nothing is ticked for the user")
        XCTAssertTrue(report.groups.flatMap(\.copies).allSatisfy { $0.path == nil }, "simple mode shows no paths")
    }

    func testTheLastUntickedCopyCanNeverBeTicked() async {
        let (_, duplicates, report) = await searchedDuplicates()
        let group = report.groups[0]
        duplicates.setChecked(true, for: group.copies[0], in: group)
        duplicates.setChecked(true, for: group.copies[1], in: group)
        XCTAssertFalse(duplicates.canCheck(group.copies[2], in: group))
        duplicates.setChecked(true, for: group.copies[2], in: group)
        XCTAssertEqual(duplicates.checkedCopyIDs, [group.copies[0].copyId, group.copies[1].copyId])
        // Unticking one frees another to be ticked instead.
        duplicates.setChecked(false, for: group.copies[0], in: group)
        XCTAssertTrue(duplicates.canCheck(group.copies[2], in: group))
    }

    func testRemovingCopiesGoesThroughTheCleanupReviewAndCanBePutBack() async {
        let (model, duplicates, report) = await searchedDuplicates()
        let group = report.groups[0]
        duplicates.setChecked(true, for: group.copies[1], in: group)
        duplicates.setChecked(true, for: group.copies[2], in: group)
        XCTAssertEqual(duplicates.checkedBytes, 2 * group.sizeBytes)
        duplicates.review()
        await model.waitForCleanupCheck()
        guard case .ready(let preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(preview.items.map(\.itemId), [group.copies[1].copyId, group.copies[2].copyId])
        model.moveToTrash()
        await model.waitForCleanupCheck()
        guard case .done(let result) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertTrue(result.items.allSatisfy(\.done))
        // The moved copies leave the list; a file with one copy left is no longer a duplicate.
        XCTAssertFalse(duplicates.visibleGroups.contains { $0.groupId == group.groupId })
        XCTAssertTrue(duplicates.checkedCopyIDs.isEmpty)
        model.putBack()
        await model.waitForCleanupCheck()
        guard case .putBack = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertTrue(duplicates.isOutOfDate)
    }

    func testDeveloperModeShowsPathsAndChangingModeForgetsTheSearch() async {
        let (model, duplicates, report) = await searchedDuplicates(mode: .developer)
        XCTAssertTrue(report.groups.flatMap(\.copies).allSatisfy { $0.path != nil })
        model.mode = .simple
        XCTAssertEqual(duplicates.state, .idle)
    }

    func testTheSearchCanBeStopped() async {
        let client = MockCoreClient(scenario: .normal, totalSteps: 5)
        let model = AppModel(client: client, defaults: defaults, pollInterval: 1_000_000_000)
        let duplicates = model.duplicates
        duplicates.startSearch()
        duplicates.cancelSearch()
        XCTAssertEqual(duplicates.state, .cancelled)
        await duplicates.waitForSearch()
        XCTAssertEqual(duplicates.state, .cancelled)
    }

    func testAStoppedSearchCanBeStartedAgain() async {
        let model = makeModel()
        let duplicates = model.duplicates
        duplicates.startSearch()
        duplicates.cancelSearch()
        // The earlier stop reaches the core before this start, so it can't stop the new search.
        duplicates.startSearch()
        await duplicates.waitForSearch()
        guard case .finished = duplicates.state else { return XCTFail("\(duplicates.state)") }
    }
}
