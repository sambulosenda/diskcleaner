import XCTest
@testable import Roomy

/// The app's model on the real core, scanning a T04 fixture home (never the real home folder).
///
/// Needs the core built with the `testing` feature, which `./scripts/build-ffi.sh` does by default.
@MainActor
final class RustCoreClientTests: XCTestCase {
    private let suiteName = "RoomyTests.RustCoreClient"

    // Fixture homes are left in the temporary directory on purpose: nothing in the app or its tests
    // deletes files except the Rust executor, and macOS clears the temporary directory itself.
    private func makeModel(mode: Mode) throws -> AppModel {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("roomy-t11-\(UUID().uuidString)")
        let home = try makeFixtureHome(dir: dir.path)
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suiteName))
        defaults.removePersistentDomain(forName: suiteName)
        let model = AppModel(
            client: RustCoreClient(core: try RoomyCore.withHomeForTesting(home: home)),
            defaults: defaults,
            pollInterval: 10_000_000
        )
        model.mode = mode
        return model
    }

    private func scanned(_ mode: Mode) async throws -> (AppModel, Overview) {
        let model = try makeModel(mode: mode)
        model.startScan()
        await model.waitForScan()
        let overview = try XCTUnwrap(model.overview, "scan ended as \(model.scanState)")
        return (model, overview)
    }

    func testAScanOnTheRealCoreReachesTheOverview() async throws {
        let (_, overview) = try await scanned(.developer)
        XCTAssertGreaterThan(overview.scannedBytes, 0)
        XCTAssertGreaterThan(overview.likelySafeBytes, 0)
        XCTAssertEqual(overview.likelySafeBytes, overview.categories.reduce(0) { $0 + $1.likelySafeBytes })
        XCTAssertFalse(overview.topItems.contains { $0.tier == .keep })
    }

    func testEveryListScreenAddsUpToItsCategories() async throws {
        let (model, overview) = try await scanned(.developer)
        for screen in SidebarItem.allCases {
            guard case .categories(let categories) = screen.itemFilter else { continue }
            model.selection = screen
            await model.waitForItems()
            guard case .loaded(let items) = model.listState else { return XCTFail("\(screen): \(model.listState)") }
            let expected = overview.categories.filter { categories.contains($0.category) }.reduce(0) { $0 + $1.bytes }
            XCTAssertEqual(items.reduce(0) { $0 + $1.sizeBytes }, expected, "\(screen)")
            XCTAssertEqual(items.map(\.sizeBytes), items.map(\.sizeBytes).sorted(by: >), "\(screen)")
        }
    }

    func testSimpleModeShowsNoPathAnywhere() async throws {
        let (model, overview) = try await scanned(.simple)
        model.selection = .largeFiles
        await model.waitForItems()
        guard case .loaded(let items) = model.listState else { return XCTFail("\(model.listState)") }
        XCTAssertEqual(items.count, Int(overview.categories.reduce(0) { $0 + $1.itemCount }))
        for item in items {
            model.selectedItemID = item.id
            await model.waitForItems()
            guard case .loaded(let detail) = model.detailState else { return XCTFail("\(model.detailState)") }
            XCTAssertNil(detail.path, detail.ruleId)
            let shown = [detail.title, detail.explain, detail.risk, detail.restore ?? "",
                         ItemDetailText.whyHere(detail), ItemDetailText.howItComesBack(detail)]
            for text in shown {
                XCTAssertFalse(text.contains("~/"), text)
                XCTAssertFalse(text.contains(NSHomeDirectory()), text)
                XCTAssertFalse(text.contains(FileManager.default.temporaryDirectory.path), text)
            }
        }
    }

    func testDeveloperModeDetailCanBeRevealed() async throws {
        let (model, _) = try await scanned(.developer)
        model.selection = .projects
        await model.waitForItems()
        guard case .loaded(let items) = model.listState, let first = items.first else {
            return XCTFail("\(model.listState)")
        }
        model.selectedItemID = first.id
        await model.waitForItems()
        guard case .loaded(let detail) = model.detailState else { return XCTFail("\(model.detailState)") }
        let path = try XCTUnwrap(detail.path)
        XCTAssertNotNil(Reveal.url(forDisplayPath: path))
    }

    func testTheMainThreadKeepsRunningDuringAScan() async throws {
        let model = try makeModel(mode: .developer)
        model.startScan()
        // While the scan runs, short main-actor waits must still come back on time.
        var longest: TimeInterval = 0
        while model.isScanning {
            let start = Date()
            try await Task.sleep(nanoseconds: 1_000_000)
            longest = max(longest, Date().timeIntervalSince(start))
        }
        XCTAssertLessThan(longest, 0.5, "the main thread was held for \(longest) s")
        XCTAssertNotNil(model.overview)
    }

    func testAMissingCoreFailsInPlainWords() async {
        let model = AppModel(client: FailingCore(), pollInterval: 0)
        model.startScan()
        await model.waitForScan()
        guard case .failed(let message) = model.scanState else { return XCTFail("\(model.scanState)") }
        XCTAssertEqual(message, "Roomy couldn't find your home folder.")
    }
}

/// A core client whose every call fails as the real one does when `RoomyCore()` could not be made.
@MainActor
private final class FailingCore: CoreClient {
    func startScan(mode: Mode) async -> ScanHandle { ScanHandle(id: 0) }
    func scanProgress(handle: ScanHandle) async throws -> ScanProgress { throw CoreError.HomeNotFound }
    func cancelScan(handle: ScanHandle) async {}
    func closeScan(handle: ScanHandle) async {}
    func overview(handle: ScanHandle) async throws -> Overview { throw CoreError.HomeNotFound }
    func listItems(handle: ScanHandle, category: Roomy.Category?, tier: Tier?, sort: Sort) async throws -> [ItemSummary] {
        throw CoreError.HomeNotFound
    }
    func itemDetail(handle: ScanHandle, itemId: String) async throws -> ItemDetail { throw CoreError.HomeNotFound }
    func previewCleanup(handle: ScanHandle, choices: [CleanupChoice]) async throws -> CleanupPreview {
        throw CoreError.HomeNotFound
    }
    func discardCleanup(plan: PlanHandle) async {}
    func runCleanup(plan: PlanHandle) async throws -> CleanupResult { throw CoreError.HomeNotFound }
    func putBackCleanup(cleanup: CleanupHandle) async throws -> PutBackResult { throw CoreError.HomeNotFound }
    func cleanupHistory(limit: UInt32) async -> [HistoryEntry] { [] }
    func fullDiskAccess() async -> FullDiskAccess { .unknown }
    func setProtectedFolders(_ folders: [String]) async throws -> [String] { throw CoreError.HomeNotFound }
    func dockerOverview() async -> DockerOverview {
        let zero = DockerUsageRow(count: 0, active: 0, sizeBytes: 0, reclaimableBytes: 0)
        return DockerOverview(status: .notInstalled, images: zero, containers: zero, volumes: zero, buildCache: zero,
                              diskImageBytes: nil)
    }
    func previewDockerCleanup(kind: DockerCleanupKind) async throws -> DockerCleanupPreview {
        throw CoreError.HomeNotFound
    }
    func discardDockerCleanup(plan: DockerPlanHandle) async {}
    func runDockerCleanup(plan: DockerPlanHandle) async throws -> DockerCleanupResult { throw CoreError.HomeNotFound }
    func previewToolCleanup(kind: ToolCleanupKind) async throws -> ToolCleanupPreview { throw CoreError.HomeNotFound }
    func discardToolCleanup(plan: ToolPlanHandle) async {}
    func runToolCleanup(plan: ToolPlanHandle, package: String?) async throws -> ToolCleanupResult {
        throw CoreError.HomeNotFound
    }
    func listApps() async -> [AppSummary] { [] }
    func reviewApp(appId: String, mode: Mode) async throws -> AppReview { throw CoreError.HomeNotFound }
    func previewUninstall(appId: String, partIds: [String]) async throws -> CleanupPreview {
        throw CoreError.HomeNotFound
    }
}
