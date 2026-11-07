import XCTest
@testable import Roomy

/// The menu bar panel's words and figures, and that it only reads: ticking and checking never clean.
@MainActor
final class MenuBarPanelTests: XCTestCase {
    private let suiteName = "RoomyTests.MenuBarPanel"
    private let now = Date(timeIntervalSince1970: 1_800_000_000)

    private lazy var defaults: UserDefaults = {
        let store = UserDefaults(suiteName: suiteName) ?? .standard
        store.removePersistentDomain(forName: suiteName)
        return store
    }()

    private func scannedModel(inUse: [String: String] = [:]) async -> (AppModel, MockCoreClient) {
        let client = MockCoreClient(scenario: .normal, totalSteps: 5)
        client.inUse = inUse
        let date = now
        let model = AppModel(client: client, defaults: defaults, pollInterval: 0, now: { date })
        model.startScan()
        await model.waitForScan()
        return (model, client)
    }

    private func item(_ id: String, _ size: UInt64, _ tier: Tier, _ category: Roomy.Category) -> ItemSummary {
        ItemSummary(id: id, title: id, explain: "", sizeBytes: size, tier: tier, category: category, place: nil)
    }

    // MARK: Groups

    func testGroupsLeaveOutKeepItemsAndPutTheLargestKindFirst() {
        let groups = MenuBarPanelContent.groups(from: [
            item("a", 100, .likelySafe, .cache),
            item("b", 300, .likelySafe, .cache),
            item("c", 350, .reviewFirst, .downloads),
            item("d", 9_000, .keep, .userData),
        ])
        XCTAssertEqual(groups.map(\.category), [.cache, .downloads])
        XCTAssertEqual(groups[0].totalBytes, 400)
        XCTAssertEqual(groups[0].items.map(\.id), ["b", "a"], "largest first")
        XCTAssertFalse(groups.contains { $0.category == .userData })
    }

    func testAGroupShowsAtMostItsRowLimitAndCountsTheRest() {
        let items = (1...11).map { item("i\($0)", UInt64($0), .likelySafe, .logs) }
        let group = MenuBarPanelContent.groups(from: items, rowsPerGroup: 8)[0]
        XCTAssertEqual(group.items.count, 8)
        XCTAssertEqual(group.hiddenCount, 3)
        XCTAssertEqual(group.totalBytes, 66, "the total counts hidden rows too")
        XCTAssertEqual(group.countText, "11 items")
    }

    // MARK: Gauge

    func testTheGaugeAddsUpToTheWholeDisk() {
        let parts = MenuBarPanelContent.gauge(total: 1_000, free: 200, likelySafe: 100, reviewFirst: 50)
        XCTAssertEqual(parts.free, 0.2, accuracy: 1e-9)
        XCTAssertEqual(parts.likelySafe, 0.1, accuracy: 1e-9)
        XCTAssertEqual(parts.reviewFirst, 0.05, accuracy: 1e-9)
        XCTAssertEqual(parts.other + parts.reviewFirst + parts.likelySafe + parts.free, 1, accuracy: 1e-9)
    }

    func testTheGaugeNeverGoesPastTheDisk() {
        // The scan found more than is used now (files removed since): the tiers fill the used part only.
        let parts = MenuBarPanelContent.gauge(total: 1_000, free: 900, likelySafe: 500, reviewFirst: 500)
        XCTAssertEqual(parts.likelySafe, 0.1, accuracy: 1e-9)
        XCTAssertEqual(parts.reviewFirst, 0, accuracy: 1e-9)
        XCTAssertEqual(parts.other, 0, accuracy: 1e-9)
        XCTAssertEqual(MenuBarPanelContent.gauge(total: 0, free: 0, likelySafe: 1, reviewFirst: 1).free, 1)
    }

    // MARK: Words

    func testLockReasonsComeFromTheCoreCheck() {
        let rows = [
            CleanupPreviewItem(itemId: "a", title: "A", sizeBytes: 1, check: .willMove, path: nil, deviceBackup: nil),
            CleanupPreviewItem(itemId: "b", title: "B", sizeBytes: 1,
                               check: .wontMove(reason: "Safari is open. Quit Safari, then try again."),
                               path: nil, deviceBackup: nil),
        ]
        XCTAssertEqual(MenuBarPanelContent.lockReasons(from: rows),
                       ["b": "Safari is open. Quit Safari, then try again."])
    }

    func testTheButtonAndFooterWords() {
        XCTAssertEqual(MenuBarPanelContent.reviewButtonTitle(count: 0), "Open Roomy")
        XCTAssertEqual(MenuBarPanelContent.reviewButtonTitle(count: 1), "Review 1 item\u{2026}")
        XCTAssertEqual(MenuBarPanelContent.reviewButtonTitle(count: 4), "Review 4 items\u{2026}")
        let overview = MockData.overview(for: .normal)
        XCTAssertEqual(MenuBarPanelContent.footerText(for: .loaded(overview, scannedAt: now.addingTimeInterval(-5)),
                                                      now: now), "Scanned just now")
        let earlier = now.addingTimeInterval(-2 * 60 * 60)
        XCTAssertEqual(MenuBarPanelContent.footerText(for: .loaded(overview, scannedAt: earlier), now: now),
                       "Scanned \(Format.relative(earlier, now: now))")
        XCTAssertEqual(MenuBarPanelContent.readyText(for: overview), "\(Format.bytes(overview.likelySafeBytes)) ready")
    }

    func testNoWordingSoundsAlarming() {
        let overview = MockData.overview(for: .normal)
        let texts = [
            MenuBarPanelContent.readyText(for: overview) ?? "",
            MenuBarPanelContent.reviewText(for: overview) ?? "",
            MenuBarPanelContent.reviewButtonTitle(count: 3),
            MenuBarPanelContent.footerText(for: .neverScanned),
            MenuBarPanelContent.footerText(for: .failed("x")),
        ]
        for text in texts {
            XCTAssertFalse(text.lowercased().contains("junk"), text)
            XCTAssertFalse(text.lowercased().contains("delete"), text)
        }
    }

    // MARK: The panel's state

    func testOpeningTheEarlierScanLoadsGroupsAndChecksNothingYet() async throws {
        let (model, client) = await scannedModel()
        let panel = MenuBarPanelModel()
        await panel.panelDidOpen(model: model)
        guard case let .loaded(groups) = panel.itemsState else { return XCTFail("\(panel.itemsState)") }
        XCTAssertFalse(groups.isEmpty)
        XCTAssertFalse(groups.flatMap(\.items).contains { $0.tier == .keep })
        XCTAssertEqual(client.checkItemsCalls, 0, "groups are checked only when opened")
    }

    func testOpeningAGroupLocksRowsTheExtraChecksHoldBack() async {
        let reason = "Safari is open. Quit Safari, then try again."
        let (model, client) = await scannedModel(inUse: ["browser-caches": reason])
        let panel = MenuBarPanelModel()
        await panel.panelDidOpen(model: model)
        await panel.toggle(.cache, model: model)
        XCTAssertTrue(panel.expanded.contains(.cache))
        XCTAssertEqual(panel.locks["browser-caches"], reason)
        XCTAssertNil(panel.locks["app-caches"])
        XCTAssertEqual(client.checkItemsCalls, 1)
        XCTAssertTrue(model.checkedItemIDs.isEmpty, "checking never ticks anything")
    }

    func testCheckingLeavesAReviewOpenInTheMainWindowAlone() async throws {
        let (model, client) = await scannedModel()
        let item = try XCTUnwrap(MockData.items.first { $0.id == "app-caches" })
        model.setChecked(true, for: item)
        model.reviewCleanup()
        await model.waitForCleanupCheck()
        guard case let .ready(preview) = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
        let plan = try XCTUnwrap(preview.plan)

        let panel = MenuBarPanelModel()
        await panel.panelDidOpen(model: model)
        await panel.toggle(.cache, model: model)

        XCTAssertEqual(model.cleanupState, .ready(preview))
        XCTAssertTrue(client.discardedPlans.isEmpty)
        let result = try await client.runCleanup(plan: plan)
        XCTAssertFalse(result.items.isEmpty, "the waiting plan still runs")
    }

    func testReviewFromTheMenuBarOpensTheUsualReview() async throws {
        let (model, _) = await scannedModel()
        model.selection = .history
        let item = try XCTUnwrap(MockData.items.first { $0.id == "app-caches" })
        model.setChecked(true, for: item)
        model.reviewFromMenuBar()
        XCTAssertEqual(model.selection, .overview)
        await model.waitForCleanupCheck()
        guard case .ready = model.cleanupState else { return XCTFail("\(model.cleanupState)") }
    }
}
