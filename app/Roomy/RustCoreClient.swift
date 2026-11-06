import Foundation

/// The real core: wraps the UniFFI `RoomyCore` object described in `docs/command-contract.md`.
///
/// Every core call runs on a background task, never on the main thread, so the window stays
/// responsive however long a call takes. The scan itself already runs on the core's own thread;
/// `AppModel` polls `scanProgress` at most ten times a second.
///
/// The client sends scan handles, item ids, app ids and enum values. The one exception is the folders the user
/// asked Roomy to keep, which can only stop things being cleaned.
@MainActor
final class RustCoreClient: CoreClient {
    /// The core, or the error that stopped it being made (for example no home folder).
    private let core: Result<RoomyCore, Error>

    /// Makes the core for the real home folder. If that fails, every scan fails with plain words.
    init() {
        do {
            core = .success(try RoomyCore())
        } catch {
            core = .failure(error)
        }
    }

    /// Wraps a core that already exists. Tests use this with `RoomyCore.withHomeForTesting`.
    init(core: RoomyCore) {
        self.core = .success(core)
    }

    /// Runs `work` on a background task and hands back its result.
    private func offMain<T: Sendable>(_ work: @escaping @Sendable (RoomyCore) throws -> T) async throws -> T {
        let core = try self.core.get()
        return try await Task.detached(priority: .userInitiated) {
            try work(core)
        }.value
    }

    func startScan(mode: Mode) async -> ScanHandle {
        // With no core, hand back a handle the core would never issue; `scanProgress` then reports
        // why the core could not start.
        // `startRescan` reads again only what changed since the last scan when the core can be sure, and
        // otherwise scans everything, exactly like `startScan`.
        (try? await offMain { $0.startRescan(mode: mode) }) ?? ScanHandle(id: 0)
    }

    func scanProgress(handle: ScanHandle) async throws -> ScanProgress {
        try await offMain { try $0.scanProgress(handle: handle) }
    }

    func cancelScan(handle: ScanHandle) async {
        _ = try? await offMain { $0.cancelScan(handle: handle) }
    }

    func closeScan(handle: ScanHandle) async {
        _ = try? await offMain { $0.closeScan(handle: handle) }
    }

    func overview(handle: ScanHandle) async throws -> Overview {
        try await offMain { try $0.overview(handle: handle) }
    }

    func scanChanges(handle: ScanHandle) async -> ScanChanges? {
        // `try?` folds "no previous scan" and "the core couldn't say" into the same `nil`.
        try? await offMain { try $0.scanChanges(handle: handle) }
    }

    func libraries(handle: ScanHandle) async throws -> [LibrarySize] {
        try await offMain { try $0.libraries(handle: handle) }
    }

    func listItems(handle: ScanHandle, category: Category?, tier: Tier?, sort: Sort) async throws -> [ItemSummary] {
        try await offMain { try $0.listItems(handle: handle, category: category, tier: tier, sort: sort) }
    }

    func itemDetail(handle: ScanHandle, itemId: String) async throws -> ItemDetail {
        try await offMain { try $0.itemDetail(handle: handle, itemId: itemId) }
    }

    func previewCleanup(handle: ScanHandle, choices: [CleanupChoice]) async throws -> CleanupPreview {
        try await offMain { try $0.previewCleanup(handle: handle, choices: choices) }
    }

    func discardCleanup(plan: PlanHandle) async {
        _ = try? await offMain { $0.discardCleanup(plan: plan) }
    }

    func runCleanup(plan: PlanHandle) async throws -> CleanupResult {
        try await offMain { try $0.runCleanup(plan: plan) }
    }

    func putBackCleanup(cleanup: CleanupHandle) async throws -> PutBackResult {
        try await offMain { try $0.putBackCleanup(cleanup: cleanup) }
    }

    func cleanupHistory(limit: UInt32) async -> [HistoryEntry] {
        (try? await offMain { $0.cleanupHistory(limit: limit) }) ?? []
    }

    func fullDiskAccess() async -> FullDiskAccess {
        // With no core there is nothing to scan, so there is nothing to ask the user for either.
        (try? await offMain { $0.fullDiskAccess() }) ?? .unknown
    }

    func setProtectedFolders(_ folders: [String]) async throws -> [String] {
        try await offMain { try $0.setProtectedFolders(folders: folders) }
    }

    func dockerOverview() async -> DockerOverview {
        let fallback = DockerOverview(
            status: .unavailable(reason: "Roomy couldn't start, so it can't ask Docker."),
            images: DockerUsageRow(count: 0, active: 0, sizeBytes: 0, reclaimableBytes: 0),
            containers: DockerUsageRow(count: 0, active: 0, sizeBytes: 0, reclaimableBytes: 0),
            volumes: DockerUsageRow(count: 0, active: 0, sizeBytes: 0, reclaimableBytes: 0),
            buildCache: DockerUsageRow(count: 0, active: 0, sizeBytes: 0, reclaimableBytes: 0),
            diskImageBytes: nil
        )
        return (try? await offMain { $0.dockerOverview() }) ?? fallback
    }

    func systemData(mode: Mode) async -> SystemData {
        let fallback = SystemData(
            snapshots: LocalSnapshots(
                status: .unavailable(reason: "Roomy couldn't start, so it can't measure System Data."),
                count: 0, oldestUnixSeconds: nil, newestUnixSeconds: nil),
            parts: [],
            totalBytes: 0)
        return (try? await offMain { $0.systemData(mode: mode) }) ?? fallback
    }

    func previewDockerCleanup(kind: DockerCleanupKind) async throws -> DockerCleanupPreview {
        try await offMain { try $0.previewDockerCleanup(kind: kind) }
    }

    func discardDockerCleanup(plan: DockerPlanHandle) async {
        _ = try? await offMain { $0.discardDockerCleanup(plan: plan) }
    }

    func runDockerCleanup(plan: DockerPlanHandle) async throws -> DockerCleanupResult {
        try await offMain { try $0.runDockerCleanup(plan: plan) }
    }

    func javascriptProjects(handle: ScanHandle) async throws -> JavaScriptOverview {
        try await offMain { try $0.javascriptProjects(handle: handle) }
    }

    func listApps() async -> [AppSummary] {
        (try? await offMain { $0.listApps() }) ?? []
    }

    func reviewApp(appId: String, mode: Mode) async throws -> AppReview {
        try await offMain { try $0.reviewApp(appId: appId, mode: mode) }
    }

    func previewUninstall(appId: String, partIds: [String]) async throws -> CleanupPreview {
        try await offMain { try $0.previewUninstall(appId: appId, partIds: partIds) }
    }
}
