import Foundation

/// A stand-in for the Rust core, so the UI can be built and tested before T11 wires up `RoomyCore`.
///
/// It follows the same rules as the real core (see `docs/command-contract.md`): `likelySafeBytes` is the
/// exact sum of likely safe items, `topItems` never holds `keep` items, and no path appears anywhere.
/// Each call to `scanProgress` moves the pretend scan one step on. Nothing here reads the disk.
/// The app uses it only when launched with `-RoomyMockScenario <name>`; otherwise it uses `RustCoreClient`.
@MainActor
final class MockCoreClient: CoreClient {
    /// What the pretend scan should do.
    enum Scenario: String {
        /// A normal scan that finds things worth reviewing.
        case normal
        /// A scan that finishes but finds nothing.
        case nothingFound
        /// A scan that fails part way through.
        case failure
    }

    let scenario: Scenario
    let totalSteps: Int
    private var nextHandle: UInt64 = 1
    private var steps: [ScanHandle: Int] = [:]
    private var cancelled: Set<ScanHandle> = []
    private var modes: [ScanHandle: Mode] = [:]
    private var nextPlan: UInt64 = 1
    /// Every plan `discardCleanup` was called with, in order. Tests read it.
    private(set) var discardedPlans: [PlanHandle] = []
    /// The rows of the one waiting plan, as the core keeps one plan at a time.
    private var pendingPlan: (handle: PlanHandle, rows: [CleanupPreviewItem])?
    /// The last cleanup that can be put back, and what it moved.
    private var lastCleanup: (handle: CleanupHandle, historyIndex: Int, moved: [CleanupResultItem])?
    private var nextCleanup: UInt64 = 1
    /// Pretend History, oldest first.
    private var historyEntries: [HistoryEntry] = []
    /// The clock History uses. Tests can set it.
    var now: () -> Date = Date.init
    /// When true, cleanups are not saved to History, as when its database can't be opened. Tests set it.
    var historyUnavailable = false
    /// When set, the next put back throws this before anything moves, and the cleanup can still be put
    /// back afterwards. Tests set it.
    var nextPutBackError: CoreError?
    /// How many times `putBackCleanup` was called. Tests read it.
    private(set) var putBackCalls = 0

    init(scenario: Scenario = .normal, totalSteps: Int = 30) {
        self.scenario = scenario
        self.totalSteps = max(totalSteps, 2)
    }

    /// The mock for the `-RoomyMockScenario <name>` launch argument, or `nil` if the argument is absent.
    /// A name that isn't a scenario still gives the mock (`.normal`), so a typo never starts a real scan.
    static func fromLaunchArguments(_ defaults: UserDefaults = .standard) -> MockCoreClient? {
        guard let name = defaults.string(forKey: "RoomyMockScenario") else { return nil }
        return MockCoreClient(scenario: Scenario(rawValue: name) ?? .normal)
    }

    func startScan(mode: Mode) async -> ScanHandle {
        let handle = ScanHandle(id: nextHandle)
        nextHandle += 1
        steps[handle] = 0
        modes[handle] = mode
        return handle
    }

    func scanProgress(handle: ScanHandle) async throws -> ScanProgress {
        guard let step = steps[handle] else { throw CoreError.UnknownScan }
        let data = MockData.overview(for: scenario)
        let fraction = Double(step) / Double(totalSteps)
        let seen = UInt64(Double(data.scannedBytes) * fraction)
        let items = UInt64(step) * 41_237

        if cancelled.contains(handle) {
            return ScanProgress(bytesSeen: seen, itemsSeen: items, phase: .cancelled)
        }
        if scenario == .failure && step >= totalSteps / 2 {
            return ScanProgress(bytesSeen: seen, itemsSeen: items,
                                phase: .failed(reason: "Roomy could not read your home folder."))
        }
        steps[handle] = min(step + 1, totalSteps)
        if step >= totalSteps {
            return ScanProgress(bytesSeen: data.scannedBytes, itemsSeen: items, phase: .finished)
        }
        return ScanProgress(bytesSeen: seen, itemsSeen: items, phase: fraction >= 0.8 ? .classifying : .scanning)
    }

    func cancelScan(handle: ScanHandle) async {
        guard let step = steps[handle], step < totalSteps else { return }
        cancelled.insert(handle)
    }

    func closeScan(handle: ScanHandle) async {
        steps[handle] = nil
        modes[handle] = nil
        cancelled.remove(handle)
    }

    func overview(handle: ScanHandle) async throws -> Overview {
        guard let step = steps[handle] else { throw CoreError.UnknownScan }
        if cancelled.contains(handle) { throw CoreError.ScanCancelled }
        if scenario == .failure { throw CoreError.ScanFailed(reason: "Roomy could not read your home folder.") }
        guard step >= totalSteps else { throw CoreError.ScanNotFinished }
        return MockData.overview(for: scenario)
    }

    func listItems(handle: ScanHandle, category: Category?, tier: Tier?, sort: Sort) async throws -> [ItemSummary] {
        _ = try await overview(handle: handle)
        return MockData.found(for: scenario)
            .filter { category == nil || $0.category == category }
            .filter { tier == nil || $0.tier == tier }
            .sorted(by: AppModel.order(for: sort))
    }

    func itemDetail(handle: ScanHandle, itemId: String) async throws -> ItemDetail {
        _ = try await overview(handle: handle)
        guard let item = MockData.found(for: scenario).first(where: { $0.id == itemId }) else {
            throw CoreError.UnknownItem
        }
        return MockData.detail(for: item, mode: modes[handle] ?? .simple)
    }

    /// Follows the core's rules: `keep` items never move, and Review first items move only when
    /// chosen on their own. Nothing here touches the disk.
    func previewCleanup(handle: ScanHandle, choices: [CleanupChoice]) async throws -> CleanupPreview {
        _ = try await overview(handle: handle)
        guard !choices.isEmpty else { throw CoreError.NothingSelected }
        var seen: Set<String> = []
        var rows: [CleanupPreviewItem] = []
        for choice in choices where seen.insert(choice.itemId).inserted {
            guard let item = MockData.found(for: scenario).first(where: { $0.id == choice.itemId }) else {
                throw CoreError.UnknownItem
            }
            let check: CleanupCheck
            switch item.tier {
            case .keep:
                check = .wontMove(reason: "The app keeps this, so it is not offered for cleanup.")
            case .reviewFirst where !choice.chosenAlone:
                check = .wontMove(reason: "This is something you need to choose yourself.")
            default:
                check = .willMove
            }
            let path = modes[handle] == .developer ? "~/Library/Mock/\(item.id)" : nil
            rows.append(CleanupPreviewItem(itemId: item.id, title: item.title, sizeBytes: item.sizeBytes,
                                           check: check, path: path,
                                           deviceBackup: MockData.deviceBackup(for: item)))
        }
        let moving = rows.filter { $0.check == .willMove }
        var plan: PlanHandle?
        pendingPlan = nil
        if !moving.isEmpty {
            let handle = PlanHandle(id: nextPlan)
            plan = handle
            nextPlan += 1
            pendingPlan = (handle, moving)
        }
        return CleanupPreview(plan: plan, items: rows, bytesToMove: moving.reduce(0) { $0 + $1.sizeBytes },
                              expiresInSeconds: 600)
    }

    func discardCleanup(plan: PlanHandle) async {
        discardedPlans.append(plan)
        if pendingPlan?.handle == plan { pendingPlan = nil }
    }

    /// Pretends every item of the waiting plan moved. Nothing here touches the disk.
    func runCleanup(plan: PlanHandle) async throws -> CleanupResult {
        guard let pending = pendingPlan, pending.handle == plan else {
            throw CoreError.CleanupUnavailable(reason: "This review is no longer open. Please review again.")
        }
        pendingPlan = nil
        let moved = pending.rows.map {
            CleanupResultItem(itemId: $0.itemId, title: $0.title, sizeBytes: $0.sizeBytes, done: true, reason: nil)
        }
        let handle = CleanupHandle(id: nextCleanup)
        nextCleanup += 1
        if historyUnavailable {
            lastCleanup = (handle, -1, moved)
            return CleanupResult(cleanup: handle, items: moved, bytesMoved: moved.reduce(0) { $0 + $1.sizeBytes },
                                 savedToHistory: false)
        }
        historyEntries.append(HistoryEntry(
            id: Int64(historyEntries.count + 1),
            finishedAt: Int64(now().timeIntervalSince1970),
            putBackAt: nil,
            bytesInTrash: moved.reduce(0) { $0 + $1.sizeBytes },
            items: moved.map { HistoryItem(title: $0.title, sizeBytes: $0.sizeBytes, state: .moved) }
        ))
        lastCleanup = (handle, historyEntries.count - 1, moved)
        return CleanupResult(cleanup: handle, items: moved, bytesMoved: moved.reduce(0) { $0 + $1.sizeBytes },
                             savedToHistory: true)
    }

    func putBackCleanup(cleanup: CleanupHandle) async throws -> PutBackResult {
        guard let last = lastCleanup, last.handle == cleanup else {
            throw CoreError.NothingToPutBack
        }
        putBackCalls += 1
        if let error = nextPutBackError {
            nextPutBackError = nil
            throw error
        }
        lastCleanup = nil
        if historyEntries.indices.contains(last.historyIndex) {
            let old = historyEntries[last.historyIndex]
            historyEntries[last.historyIndex] = HistoryEntry(
                id: old.id, finishedAt: old.finishedAt, putBackAt: Int64(now().timeIntervalSince1970), bytesInTrash: 0,
                items: old.items.map { HistoryItem(title: $0.title, sizeBytes: $0.sizeBytes, state: .putBack) }
            )
        }
        return PutBackResult(items: last.moved, bytesPutBack: last.moved.reduce(0) { $0 + $1.sizeBytes })
    }

    func cleanupHistory(limit: UInt32) async -> [HistoryEntry] {
        Array(historyEntries.reversed().prefix(Int(limit)))
    }
}

/// Made-up numbers shaped like a real Mac with a 256 GB disk.
enum MockData {
    static let diskTotal: UInt64 = 256_000_000_000
    static let diskFree: UInt64 = 41_000_000_000
    static let scannedBytes: UInt64 = 180_000_000_000
    static let unreadableCount: UInt64 = 212

    /// Every item the pretend scan finds. Likely safe items add up to the wireframe's 34 GB.
    static let items: [ItemSummary] = [
        item("xcode-derived-data", "Xcode build data",
             "Files Xcode makes while building apps. Xcode makes them again when needed.",
             12_400_000_000, .likelySafe, .build),
        item("simulator-runtimes", "Old Simulator versions",
             "iPhone and iPad Simulator versions you no longer use. Xcode can download them again.",
             6_900_000_000, .likelySafe, .developerTools),
        item("app-caches", "App caches",
             "Temporary files apps keep so they open faster. Apps rebuild them.",
             5_300_000_000, .likelySafe, .cache),
        item("npm-cache", "JavaScript package caches",
             "Copies of downloaded packages. They download again when a project needs them.",
             4_100_000_000, .likelySafe, .cache),
        item("old-logs", "Old log files",
             "Records apps keep of what they did. Old ones are rarely needed.",
             3_200_000_000, .likelySafe, .logs),
        item("homebrew-cache", "Homebrew downloads",
             "Installers Homebrew has already used. It downloads them again if needed.",
             1_300_000_000, .likelySafe, .cache),
        item("browser-caches", "Browser caches",
             "Saved copies of web pages. Browsers fetch them again.",
             800_000_000, .likelySafe, .cache),
        item("iphone-backup", "iPhone backup",
             "A backup of an iPhone, last updated in March. Check you have a newer one first.",
             18_100_000_000, .reviewFirst, .backups),
        item("downloads-installers", "Installers in Downloads",
             "Disk images and installers. You may have installed these already.",
             6_200_000_000, .reviewFirst, .downloads),
        item("large-video", "Screen recording.mov",
             "A large video you haven't opened for a year.",
             4_700_000_000, .reviewFirst, .userData),
        item("xcode-archives", "Old app archives",
             "Past app builds made for the App Store. You may want to keep recent ones.",
             3_900_000_000, .reviewFirst, .developerTools),
        item("docker-images", "Docker images",
             "Saved images and build layers. Docker downloads or rebuilds them when needed.",
             14_500_000_000, .reviewFirst, .docker),
        item("music-library", "Music library",
             "Your music. Shown so you know where the space goes.",
             38_000_000_000, .keep, .userData),
        item("photos-library", "Photos library",
             "Your photos. Shown so you know where the space goes.",
             22_000_000_000, .keep, .userData),
    ]

    /// Every item a scenario's scan finds.
    static func found(for scenario: MockCoreClient.Scenario) -> [ItemSummary] {
        scenario == .nothingFound ? [] : items
    }

    /// The detail panel for a mock item. `path` is filled in only in Developer mode, as in the core.
    static func detail(for item: ItemSummary, mode: Mode) -> ItemDetail {
        let regenerable = item.tier == .likelySafe
        return ItemDetail(
            id: item.id,
            title: item.title,
            explain: item.explain,
            risk: item.tier == .keep
                ? "These are your own files, so Roomy does not offer to remove them."
                : "You may need to download or rebuild it again later.",
            restore: regenerable ? "It comes back by itself when it is needed." : nil,
            ruleId: "mock.\(item.id)",
            ruleVersion: 1,
            confidence: .high,
            tier: item.tier,
            category: item.category,
            sizeBytes: item.sizeBytes,
            regenerable: regenerable,
            action: item.tier == .keep ? .showOnly : .trash,
            command: nil,
            cautions: item.tier == .reviewFirst ? [.reviewOnly] : [],
            path: mode == .developer ? "~/Library/Mock/\(item.id)" : nil,
            deviceBackup: deviceBackup(for: item)
        )
    }

    /// The device details the core reads from a backup's Info.plist, for the mock iPhone backup only.
    static func deviceBackup(for item: ItemSummary) -> DeviceBackup? {
        guard item.id == "iphone-backup" else { return nil }
        // 14 March 2026, 09:30 UTC.
        return DeviceBackup(deviceName: "Sam's iPhone", lastBackupUnixSeconds: 1_773_480_600)
    }

    /// The Overview a finished scan returns, built the way the contract defines each field.
    static func overview(for scenario: MockCoreClient.Scenario) -> Overview {
        let found = Self.found(for: scenario)
        return Overview(
            likelySafeBytes: sum(found.filter { $0.tier == .likelySafe }),
            scannedBytes: scannedBytes,
            diskTotal: diskTotal,
            diskFree: diskFree,
            categories: categoryTotals(found),
            topItems: Array(found.filter { $0.tier != .keep }.sorted { $0.sizeBytes > $1.sizeBytes }.prefix(8)),
            unreadableCount: unreadableCount
        )
    }

    /// One entry per category with any items, largest first.
    private static func categoryTotals(_ found: [ItemSummary]) -> [CategoryTotal] {
        var order: [Category] = []
        for item in found where !order.contains(item.category) { order.append(item.category) }
        return order.map { category in
            let inCategory = found.filter { $0.category == category }
            return CategoryTotal(
                category: category,
                bytes: sum(inCategory),
                likelySafeBytes: sum(inCategory.filter { $0.tier == .likelySafe }),
                itemCount: UInt64(inCategory.count)
            )
        }
        .sorted { $0.bytes > $1.bytes }
    }

    private static func sum(_ items: [ItemSummary]) -> UInt64 {
        items.reduce(0) { $0 + $1.sizeBytes }
    }

    private static func item(
        _ id: String, _ title: String, _ explain: String,
        _ bytes: UInt64, _ tier: Tier, _ category: Category
    ) -> ItemSummary {
        ItemSummary(id: id, title: title, explain: explain, sizeBytes: bytes, tier: tier, category: category)
    }
}
