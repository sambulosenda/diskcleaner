import XCTest
@testable import Roomy

/// The ⌘K palette: matching, what it lists, what each entry does, and that nothing it does moves files.
@MainActor
final class CommandPaletteTests: XCTestCase {
    private let suiteName = "RoomyTests.CommandPalette"

    private lazy var defaults: UserDefaults = {
        let store = UserDefaults(suiteName: suiteName) ?? .standard
        store.removePersistentDomain(forName: suiteName)
        // The Full Disk Access screen has been seen, so it never blocks the palette by itself.
        store.set(true, forKey: AppModel.accessOnboardingKey)
        return store
    }()

    private func makeModel(_ client: MockCoreClient = MockCoreClient(scenario: .normal, totalSteps: 3)) -> AppModel {
        AppModel(client: client, defaults: defaults, pollInterval: 0)
    }

    private func scannedModel(_ client: MockCoreClient = MockCoreClient(scenario: .normal, totalSteps: 3)) async -> AppModel {
        let model = makeModel(client)
        model.startScan()
        await model.waitForScan()
        return model
    }

    /// Opens the palette and waits for the scan's items to load.
    private func openPalette(_ model: AppModel) async -> CommandPaletteModel {
        let palette = model.palette
        palette.open()
        await palette.waitForItems()
        return palette
    }

    private func command(_ id: String, in palette: CommandPaletteModel) -> PaletteCommand? {
        PaletteCommand.all(for: palette.context, items: palette.items).first { $0.id == id }
    }

    // MARK: - Matching

    func testLettersMustAppearInOrder() {
        XCTAssertNotNil(FuzzyMatch.score("ovw", in: "Overview"))
        XCTAssertNotNil(FuzzyMatch.score("lgf", in: "Large files"))
        XCTAssertNil(FuzzyMatch.score("wvo", in: "Overview"))
        XCTAssertNil(FuzzyMatch.score("xyz", in: "Overview"))
        XCTAssertNil(FuzzyMatch.score("overviews", in: "Overview"))
    }

    func testMatchingIgnoresCaseAccentsAndSpaces() {
        XCTAssertNotNil(FuzzyMatch.score("HISTORY", in: "History"))
        XCTAssertNotNil(FuzzyMatch.score("cafe", in: "Café caches"))
        XCTAssertNotNil(FuzzyMatch.score("large files", in: "Largefiles"))
        XCTAssertEqual(FuzzyMatch.score("", in: "Anything"), 0)
    }

    func testPrefixAndWordStartsScoreHigher() throws {
        let prefix = try XCTUnwrap(FuzzyMatch.score("doc", in: "Docker"))
        let inside = try XCTUnwrap(FuzzyMatch.score("doc", in: "Old documents"))
        let scattered = try XCTUnwrap(FuzzyMatch.score("doc", in: "Downloads and installers, compressed"))
        XCTAssertGreaterThan(prefix, inside)
        XCTAssertGreaterThan(inside, scattered)
    }

    func testRankingPutsTheBestMatchFirst() {
        let commands = PaletteCommand.screens(for: .developer)
        XCTAssertEqual(PaletteCommand.ranked(commands, query: "hist").first?.action, .goTo(.history))
        XCTAssertEqual(PaletteCommand.ranked(commands, query: "dock").first?.action, .goTo(.docker))
        XCTAssertEqual(PaletteCommand.ranked(commands, query: "ovr").first?.action, .goTo(.overview))
    }

    func testKeywordsFindEntries() {
        let commands = PaletteCommand.all(
            for: PaletteContext(mode: .simple, isScanning: false, hasResults: false, checkedCount: 0), items: [])
        XCTAssertEqual(PaletteCommand.ranked(commands, query: "preferences").first?.action, .openSettings)
        XCTAssertEqual(PaletteCommand.ranked(commands, query: "undo").first?.action, .goTo(.history))
    }

    func testEmptyQueryListsScreensAndActionsButNoItems() {
        let items = MockData.found(for: .normal)
        let commands = PaletteCommand.all(
            for: PaletteContext(mode: .simple, isScanning: false, hasResults: true, checkedCount: 0), items: items)
        let shown = PaletteCommand.ranked(commands, query: "  ")
        XCTAssertFalse(shown.isEmpty)
        XCTAssertFalse(shown.contains { $0.kind == .item })
        XCTAssertEqual(shown.first?.action, .goTo(.overview))
    }

    // MARK: - What it lists

    func testDeveloperScreensOnlyInDeveloperMode() {
        let simple = PaletteCommand.screens(for: .simple).map(\.action)
        let developer = PaletteCommand.screens(for: .developer).map(\.action)
        for screen in SidebarItem.developerItems {
            XCTAssertFalse(simple.contains(.goTo(screen)), "\(screen) shown in Simple mode")
            XCTAssertTrue(developer.contains(.goTo(screen)), "\(screen) missing in Developer mode")
        }
        XCTAssertEqual(developer.count, SidebarItem.allCases.count)
    }

    func testReviewCleanupOnlyWhenSomethingIsTicked() {
        func ids(_ context: PaletteContext) -> [String] {
            PaletteCommand.actions(for: context).map(\.id)
        }
        XCTAssertFalse(ids(PaletteContext(mode: .simple, isScanning: false, hasResults: true, checkedCount: 0))
            .contains("action.reviewCleanup"))
        XCTAssertFalse(ids(PaletteContext(mode: .simple, isScanning: false, hasResults: false, checkedCount: 2))
            .contains("action.reviewCleanup"))
        XCTAssertTrue(ids(PaletteContext(mode: .simple, isScanning: false, hasResults: true, checkedCount: 2))
            .contains("action.reviewCleanup"))
    }

    func testScanOrStopDependingOnTheScan() {
        let idle = PaletteCommand.actions(for: PaletteContext(mode: .simple, isScanning: false, hasResults: false, checkedCount: 0))
        let busy = PaletteCommand.actions(for: PaletteContext(mode: .simple, isScanning: true, hasResults: false, checkedCount: 0))
        XCTAssertTrue(idle.contains { $0.action == .scan })
        XCTAssertFalse(idle.contains { $0.action == .stopScan })
        XCTAssertTrue(busy.contains { $0.action == .stopScan })
        XCTAssertFalse(busy.contains { $0.action == .scan })
    }

    func testItemsGoToTheScreenThatListsThem() {
        XCTAssertEqual(SidebarItem.screen(for: .backups, mode: .simple), .backups)
        XCTAssertEqual(SidebarItem.screen(for: .cache, mode: .simple), .appsAndCaches)
        XCTAssertEqual(SidebarItem.screen(for: .logs, mode: .simple), .systemData)
        XCTAssertEqual(SidebarItem.screen(for: .docker, mode: .developer), .docker)
        // Developer screens are hidden in Simple mode, so their items open in Large files.
        XCTAssertEqual(SidebarItem.screen(for: .docker, mode: .simple), .largeFiles)
        XCTAssertEqual(SidebarItem.screen(for: .userData, mode: .developer), .largeFiles)
        for mode in Mode.allCases {
            let shown = SidebarItem.sections(for: mode).flatMap { $0 }
            for item in MockData.found(for: .normal) {
                XCTAssertTrue(shown.contains(SidebarItem.screen(for: item.category, mode: mode)))
            }
        }
    }

    func testNoItemsBeforeAScan() async {
        let model = makeModel()
        let palette = await openPalette(model)
        XCTAssertTrue(palette.items.isEmpty)
        palette.query = "Xcode"
        XCTAssertFalse(palette.results.contains { $0.kind == .item })
    }

    func testFindsItemsFromTheScan() async {
        let model = await scannedModel()
        let palette = await openPalette(model)
        XCTAssertEqual(palette.items.count, MockData.found(for: .normal).count)
        palette.query = "iphone backup"
        XCTAssertEqual(palette.results.first?.id, "item.iphone-backup")
    }

    // MARK: - Opening, moving and closing

    func testOpenToggleAndClose() {
        let model = makeModel()
        let palette = model.palette
        XCTAssertFalse(palette.isPresented)
        palette.toggle()
        XCTAssertTrue(palette.isPresented)
        palette.toggle()
        XCTAssertFalse(palette.isPresented)
        palette.open()
        palette.close()
        XCTAssertFalse(palette.isPresented)
    }

    func testOpeningStartsEmpty() {
        let model = makeModel()
        let palette = model.palette
        palette.open()
        palette.query = "his"
        palette.close()
        palette.open()
        XCTAssertEqual(palette.query, "")
        XCTAssertEqual(palette.highlightedIndex, 0)
    }

    func testDoesNotOpenOverTheFullDiskAccessScreen() {
        let model = makeModel()
        model.showAccessOnboarding()
        XCTAssertFalse(model.palette.canOpen)
        model.palette.open()
        XCTAssertFalse(model.palette.isPresented)
    }

    func testDoesNotOpenOverTheCleanupReview() async {
        let model = await scannedModel()
        let palette = await openPalette(model)
        palette.close()
        guard let item = palette.items.first(where: { $0.tier == .likelySafe }) else { return XCTFail("no item") }
        model.selection = .largeFiles
        model.setChecked(true, for: item)
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        XCTAssertFalse(palette.canOpen)
        palette.open()
        XCTAssertFalse(palette.isPresented)
        model.closeCleanupReview()
        XCTAssertTrue(palette.canOpen)
    }

    func testHighlightStaysInsideTheResults() {
        let model = makeModel()
        let palette = model.palette
        palette.open()
        let count = palette.results.count
        XCTAssertGreaterThan(count, 2)
        palette.moveHighlight(by: -1)
        XCTAssertEqual(palette.highlightedIndex, 0)
        palette.moveHighlight(by: 1)
        palette.moveHighlight(by: 1)
        XCTAssertEqual(palette.highlightedIndex, 2)
        palette.moveHighlight(by: 1000)
        XCTAssertEqual(palette.highlightedIndex, count - 1)
        palette.query = "hist"
        XCTAssertEqual(palette.highlightedIndex, 0, "typing goes back to the best match")
    }

    func testNothingMatchesRunsNothing() {
        let model = makeModel()
        let palette = model.palette
        palette.open()
        palette.query = "qqqqzzzz"
        XCTAssertTrue(palette.results.isEmpty)
        palette.runHighlighted(openSettings: { XCTFail("ran something") })
        XCTAssertTrue(palette.isPresented)
    }

    // MARK: - What each entry does

    func testReturnRunsTheHighlightedEntryAndCloses() {
        let model = makeModel()
        let palette = model.palette
        palette.open()
        palette.query = "history"
        palette.runHighlighted(openSettings: {})
        XCTAssertFalse(palette.isPresented)
        XCTAssertEqual(model.selection, .history)
    }

    func testGoToShowsTheScreen() {
        let model = makeModel()
        let downloads = PaletteCommand.screens(for: .simple).first(where: { $0.action == .goTo(.downloads) })
        model.palette.run(downloads!, openSettings: {})
        XCTAssertEqual(model.selection, .downloads)
    }

    func testScanStartsAScan() async {
        let model = makeModel()
        let palette = model.palette
        palette.open()
        guard let scan = command("action.scan", in: palette) else { return XCTFail("no Scan entry") }
        palette.run(scan, openSettings: {})
        XCTAssertTrue(model.isScanning)
        await model.waitForScan()
        XCTAssertNotNil(model.overview)
    }

    func testShowItemOpensItsScreenAndDetail() async throws {
        let model = await scannedModel()
        model.searchText = "something else"
        let palette = await openPalette(model)
        let backup = try XCTUnwrap(command("item.iphone-backup", in: palette))
        palette.run(backup, openSettings: {})
        await model.waitForItems()
        XCTAssertEqual(model.selection, .backups)
        XCTAssertEqual(model.selectedItemID, "iphone-backup")
        XCTAssertEqual(model.searchText, "", "the search is cleared so the row shows")
        XCTAssertTrue(model.visibleItems.contains { $0.id == "iphone-backup" })
        guard case .loaded(let detail) = model.detailState else { return XCTFail("\(model.detailState)") }
        XCTAssertEqual(detail.id, "iphone-backup")
    }

    func testShowItemClearsATierFilterThatHidesIt() async throws {
        let model = await scannedModel()
        model.selection = .largeFiles
        model.tierFilter = .keep
        await model.waitForItems()
        let palette = await openPalette(model)
        let video = try XCTUnwrap(command("item.large-video", in: palette))
        palette.run(video, openSettings: {})
        await model.waitForItems()
        XCTAssertEqual(model.selection, .largeFiles)
        XCTAssertNil(model.tierFilter)
        XCTAssertEqual(model.selectedItemID, "large-video")
    }

    func testOpenSettingsUsesTheViewsAction() {
        let model = makeModel()
        let palette = model.palette
        palette.open()
        var opened = 0
        palette.run(command("action.settings", in: palette)!, openSettings: { opened += 1 })
        XCTAssertEqual(opened, 1)
    }

    func testSwitchModeChangesTheMode() {
        let model = makeModel()
        let palette = model.palette
        palette.open()
        palette.run(command("action.mode.developer", in: palette)!, openSettings: {})
        XCTAssertEqual(model.mode, .developer)
        palette.open()
        XCTAssertNotNil(command("action.mode.simple", in: palette))
    }

    func testFullDiskAccessOpensItsScreen() {
        let model = makeModel()
        let palette = model.palette
        palette.open()
        palette.run(command("action.fullDiskAccess", in: palette)!, openSettings: {})
        XCTAssertTrue(model.showsAccessOnboarding)
    }

    func testReviewCleanupOpensTheDryRunOnly() async throws {
        let model = await scannedModel()
        let palette = await openPalette(model)
        palette.close()
        let item = try XCTUnwrap(palette.items.first { $0.tier == .likelySafe })
        model.setChecked(true, for: item)
        XCTAssertEqual(model.selection, .overview)

        palette.open()
        let review = try XCTUnwrap(command("action.reviewCleanup", in: palette))
        palette.run(review, openSettings: {})
        // The review sheet lives on the item lists, so the palette shows one first.
        XCTAssertEqual(model.selection, .largeFiles)
        await model.waitForCleanupCheck()
        guard case .ready(let preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        XCTAssertEqual(preview.items.map(\.itemId), [item.id])
        XCTAssertNil(model.lastCleanup, "a review moves nothing")
        await model.reloadHistory()
        XCTAssertTrue(model.history.isEmpty)
        model.closeCleanupReview()
    }

    /// Runs every entry the palette can offer, with something ticked, and checks that none of them moved
    /// a file: no cleanup ran, nothing was put back, and History is empty.
    func testNoEntryMovesFiles() async {
        for mode in Mode.allCases {
            let probe = await scannedModel()
            probe.mode = mode
            await probe.waitForScan()
            let probePalette = await openPalette(probe)
            let ticked = probePalette.items.filter { $0.tier != .keep }
            for item in ticked { probe.setChecked(true, for: item) }
            let entries = PaletteCommand.all(for: probePalette.context, items: probePalette.items)
            XCTAssertTrue(entries.contains { $0.action == .reviewCleanup })

            for entry in entries {
                let client = MockCoreClient(scenario: .normal, totalSteps: 3)
                let model = await scannedModel(client)
                model.mode = mode
                await model.waitForScan()
                for item in ticked { model.setChecked(true, for: item) }
                let palette = await openPalette(model)
                palette.run(entry, openSettings: {})
                await model.waitForScan()
                await model.waitForItems()
                await model.waitForCleanupCheck()

                switch model.cleanupState {
                case .moving, .done, .puttingBack, .putBack:
                    XCTFail("\(entry.id) moved files: \(model.cleanupState)")
                default:
                    break
                }
                XCTAssertNil(model.lastCleanup, entry.id)
                XCTAssertEqual(client.putBackCalls, 0, entry.id)
                await model.reloadHistory()
                XCTAssertTrue(model.history.isEmpty, entry.id)
                model.closeCleanupReview()
            }
        }
    }
}
