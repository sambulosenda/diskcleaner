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
    /// What `fullDiskAccess` answers. Tests and the `-RoomyMockAccess <value>` launch argument set it.
    var access: FullDiskAccess = .granted
    /// How many times `fullDiskAccess` was called. Tests read it.
    private(set) var accessChecks = 0
    /// What `scanChanges` answers for every finished scan after the first. Tests set it; `nil` by default.
    var changes: ScanChanges?
    /// Handles of scans that have finished, in order, so the first one has nothing to compare with.
    private var finishedScans: [ScanHandle] = []

    init(scenario: Scenario = .normal, totalSteps: Int = 30) {
        self.scenario = scenario
        self.totalSteps = max(totalSteps, 2)
    }

    /// The mock for the `-RoomyMockScenario <name>` launch argument, or `nil` if the argument is absent.
    /// A name that isn't a scenario still gives the mock (`.normal`), so a typo never starts a real scan.
    static func fromLaunchArguments(_ defaults: UserDefaults = .standard) -> MockCoreClient? {
        guard let name = defaults.string(forKey: "RoomyMockScenario") else { return nil }
        let mock = MockCoreClient(scenario: Scenario(rawValue: name) ?? .normal)
        if defaults.string(forKey: "RoomyMockAccess") == "notGranted" { mock.access = .notGranted }
        return mock
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
        if !finishedScans.contains(handle) { finishedScans.append(handle) }
        return MockData.overview(for: scenario)
    }

    func scanChanges(handle: ScanHandle) async -> ScanChanges? {
        guard (try? await overview(handle: handle)) != nil, finishedScans.first != handle else { return nil }
        return changes
    }

    func listItems(handle: ScanHandle, category: Category?, tier: Tier?, sort: Sort) async throws -> [ItemSummary] {
        _ = try await overview(handle: handle)
        return MockData.found(for: scenario)
            .filter { category == nil || $0.category == category }
            .filter { tier == nil || $0.tier == tier }
            .sorted(by: AppModel.order(for: sort))
    }

    func libraries(handle: ScanHandle) async throws -> [LibrarySize] {
        _ = try await overview(handle: handle)
        return MockData.libraries(for: scenario)
    }

    func itemDetail(handle: ScanHandle, itemId: String) async throws -> ItemDetail {
        _ = try await overview(handle: handle)
        guard let item = MockData.lookup(itemId, in: scenario) else { throw CoreError.UnknownItem }
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
            guard let item = MockData.lookup(choice.itemId, in: scenario) else { throw CoreError.UnknownItem }
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

    func fullDiskAccess() async -> FullDiskAccess {
        accessChecks += 1
        return access
    }

    /// The folders the app last sent. Like the core, refuses anything outside the home folder.
    private(set) var protectedFolders: [String] = []

    func setProtectedFolders(_ folders: [String]) async throws -> [String] {
        let home = NSHomeDirectory()
        for folder in folders where !folder.hasPrefix(home + "/") {
            throw CoreError.InvalidPath(reason: "Choose a folder inside your home folder.")
        }
        var unique: [String] = []
        for folder in folders where !unique.contains(folder) { unique.append(folder) }
        protectedFolders = unique
        return unique
    }

    // MARK: Docker

    /// What `dockerOverview` reports. Tests set it.
    var dockerStatus: DockerStatus = .ready
    /// The pretend unused images, as Docker's preview would list them. Tests set it.
    var dockerImages: [DockerPreviewItem] = [
        DockerPreviewItem(id: "4f2a9c1e7b3d", sizeBytes: 2_100_000_000, age: "5 weeks ago"),
        DockerPreviewItem(id: "9b8e1d0c6a52", sizeBytes: 640_000_000, age: "2 weeks ago"),
    ]
    /// The waiting Docker plan.
    private var pendingDockerPlan: (handle: DockerPlanHandle, preview: DockerCleanupPreview)?
    private var nextDockerPlan: UInt64 = 1
    /// Every Docker clean-up that "ran", in order. Tests read it. Nothing here runs Docker.
    private(set) var dockerRuns: [DockerCleanupKind] = []

    func dockerOverview() async -> DockerOverview {
        let zero = DockerUsageRow(count: 0, active: 0, sizeBytes: 0, reclaimableBytes: 0)
        guard dockerStatus == .ready else {
            return DockerOverview(status: dockerStatus, images: zero, containers: zero, volumes: zero,
                                  buildCache: zero, diskImageBytes: nil)
        }
        let unused = dockerImages.reduce(0) { $0 + $1.sizeBytes }
        return DockerOverview(
            status: .ready,
            images: DockerUsageRow(count: 14, active: 3, sizeBytes: 9_800_000_000, reclaimableBytes: unused),
            containers: DockerUsageRow(count: 4, active: 1, sizeBytes: 120_000_000, reclaimableBytes: 80_000_000),
            volumes: DockerUsageRow(count: 3, active: 1, sizeBytes: 1_400_000_000, reclaimableBytes: 600_000_000),
            buildCache: DockerUsageRow(count: 57, active: 0, sizeBytes: 3_100_000_000, reclaimableBytes: 3_100_000_000),
            diskImageBytes: 21_000_000_000
        )
    }

    func previewDockerCleanup(kind: DockerCleanupKind) async throws -> DockerCleanupPreview {
        if case .notRunning = dockerStatus {
            throw CoreError.DockerUnavailable(reason: "Docker isn't running. Open Docker and try again.")
        }
        let items: [DockerPreviewItem]
        switch kind {
        case .unusedImages: items = dockerImages
        case .buildCache: items = [DockerPreviewItem(id: "m3hqlq4tjoh2", sizeBytes: 3_100_000_000, age: "3 weeks ago")]
        }
        let command = kind == .unusedImages ? "docker image prune" : "docker builder prune"
        var plan: DockerPlanHandle?
        if !items.isEmpty {
            plan = DockerPlanHandle(id: nextDockerPlan)
            nextDockerPlan += 1
        }
        let preview = DockerCleanupPreview(plan: plan, kind: kind, commandLine: command, items: items,
                                           totalBytes: items.reduce(0) { $0 + $1.sizeBytes }, expiresInSeconds: 600)
        pendingDockerPlan = plan.map { ($0, preview) }
        return preview
    }

    func discardDockerCleanup(plan: DockerPlanHandle) async {
        if pendingDockerPlan?.handle == plan { pendingDockerPlan = nil }
    }

    func runDockerCleanup(plan: DockerPlanHandle) async throws -> DockerCleanupResult {
        guard let pending = pendingDockerPlan, pending.handle == plan else {
            throw CoreError.CleanupUnavailable(reason: "This review is no longer open. Please review again.")
        }
        pendingDockerPlan = nil
        dockerRuns.append(pending.preview.kind)
        if pending.preview.kind == .unusedImages { dockerImages = [] }
        return DockerCleanupResult(kind: pending.preview.kind, success: true,
                                   reclaimedBytes: pending.preview.totalBytes, savedToHistory: true)
    }

    // MARK: System Data

    /// Every mode `systemData` was asked for, in order. Tests read it. Nothing here reads the disk.
    private(set) var systemDataCalls: [Mode] = []
    /// When true, `systemData` says local snapshots couldn't be listed. Tests set it.
    var snapshotsUnavailable = false

    func systemData(mode: Mode) async -> SystemData {
        systemDataCalls.append(mode)
        return MockData.systemData(mode: mode, snapshotsUnavailable: snapshotsUnavailable)
    }

    // MARK: App uninstaller

    /// When true, `previewUninstall` refuses as if the app were open. Tests set it.
    var appIsOpen = false
    /// The app last reviewed, so `previewUninstall` refuses any other, as the core does.
    private var reviewedApp: String?

    func javascriptProjects(handle: ScanHandle) async throws -> JavaScriptOverview {
        _ = try await overview(handle: handle)
        guard scenario != .nothingFound, modes[handle] == .developer else {
            return JavaScriptOverview(projects: [], caches: [], totalBytes: 0)
        }
        return MockData.javascript(now: now())
    }

    func listApps() async -> [AppSummary] {
        reviewedApp = nil
        return MockData.apps
    }

    func reviewApp(appId: String, mode: Mode) async throws -> AppReview {
        guard let app = MockData.apps.first(where: { $0.appId == appId }) else { throw CoreError.UnknownApp }
        reviewedApp = appId
        let parts = MockData.appParts(for: app).map { part -> AppPart in
            var part = part
            if mode == .simple { part.path = nil }
            return part
        }
        return AppReview(app: app, parts: parts, note: nil)
    }

    /// Pretends every ticked part would move, and keeps the plan for `runCleanup`. Touches nothing.
    func previewUninstall(appId: String, partIds: [String]) async throws -> CleanupPreview {
        guard appId == reviewedApp, let app = MockData.apps.first(where: { $0.appId == appId }) else {
            throw CoreError.UnknownApp
        }
        if appIsOpen {
            throw CoreError.CleanupUnavailable(reason: "\(app.name) is open. Quit \(app.name), then try again.")
        }
        if partIds.isEmpty { throw CoreError.NothingSelected }
        let parts = MockData.appParts(for: app)
        var rows: [CleanupPreviewItem] = []
        for id in partIds {
            guard let part = parts.first(where: { $0.itemId == id }) else { throw CoreError.UnknownItem }
            rows.append(CleanupPreviewItem(itemId: part.itemId, title: part.title, sizeBytes: part.sizeBytes,
                                           check: .willMove, path: nil, deviceBackup: nil))
        }
        let handle = PlanHandle(id: nextPlan)
        nextPlan += 1
        pendingPlan = (handle, rows)
        return CleanupPreview(plan: handle, items: rows, bytesToMove: rows.reduce(0) { $0 + $1.sizeBytes },
                              expiresInSeconds: 600)
    }
}

/// Made-up numbers shaped like a real Mac with a 256 GB disk.
enum MockData {
    /// A made-up System Data breakdown, like the core's: paths in Developer mode only.
    static func systemData(mode: Mode, snapshotsUnavailable: Bool = false) -> SystemData {
        func place(_ id: String, _ owner: SystemDataOwner, _ bytes: UInt64, _ path: String,
                   state: SystemDataPlaceState = .measured) -> SystemDataPlace {
            SystemDataPlace(id: id, owner: owner, state: state, sizeBytes: state == .measured ? bytes : 0,
                            fileCount: state == .measured ? 12 : 0, partlyUnreadable: false,
                            path: mode == .developer ? path : nil)
        }
        func part(_ kind: SystemDataKind, _ places: [SystemDataPlace]) -> SystemDataPart {
            SystemDataPart(kind: kind, sizeBytes: places.reduce(0) { $0 + $1.sizeBytes },
                           partlyUnreadable: places.contains { $0.state == .unreadable || $0.partlyUnreadable },
                           places: places)
        }
        let parts = [
            part(.caches, [place("user_caches", .you, 6_200_000_000, "~/Library/Caches"),
                           place("shared_caches", .system, 900_000_000, "/Library/Caches")]),
            part(.logs, [place("user_logs", .you, 310_000_000, "~/Library/Logs"),
                         place("shared_logs", .system, 40_000_000, "/Library/Logs"),
                         place("system_logs", .system, 120_000_000, "/private/var/log"),
                         place("unified_logs", .system, 0, "/private/var/db/diagnostics", state: .unreadable)]),
            part(.softwareUpdates, [place("iphone_updates", .you, 7_100_000_000, "~/Library/iTunes/iPhone Software Updates"),
                                    place("ipad_updates", .you, 0, "~/Library/iTunes/iPad Software Updates", state: .missing),
                                    place("mac_updates", .system, 0, "/Library/Updates", state: .missing)]),
            part(.temporaryFiles, [place("user_temporary", .system, 2_400_000_000, "/private/var/folders/zz/example")]),
            part(.swap, [place("swap_files", .system, 3_221_225_472, "/private/var/vm")]),
            part(.sleepImage, [place("sleep_image", .system, 1_073_741_824, "/private/var/vm/sleepimage")]),
        ]
        let snapshots = snapshotsUnavailable
            ? LocalSnapshots(status: .unavailable(reason: "Roomy couldn't ask Time Machine on this Mac."),
                             count: 0, oldestUnixSeconds: nil, newestUnixSeconds: nil)
            // 4 Nov 2026 08:00 and 6 Nov 2026 09:30:12, as the core reads snapshot names.
            : LocalSnapshots(status: .listed, count: 3, oldestUnixSeconds: 1_793_779_200,
                             newestUnixSeconds: 1_793_957_412)
        return SystemData(snapshots: snapshots, parts: parts,
                          totalBytes: parts.reduce(0) { $0 + $1.sizeBytes })
    }

    /// Made-up apps for the uninstaller. One is in ~/Applications, one is open.
    static let apps: [AppSummary] = [
        AppSummary(appId: "app-1-0", name: "Chat", version: "4.41", bundleId: "com.example.chat",
                   sizeBytes: 480_000_000, location: .applications, bundlePath: "/Applications/Chat.app", isOpen: false),
        AppSummary(appId: "app-1-1", name: "Notes Pro", version: "2.0", bundleId: "com.example.notespro",
                   sizeBytes: 95_000_000, location: .homeApplications,
                   bundlePath: NSHomeDirectory() + "/Applications/Notes Pro.app", isOpen: false),
        AppSummary(appId: "app-1-2", name: "Video Studio", version: nil, bundleId: "com.example.video",
                   sizeBytes: 2_100_000_000, location: .applications, bundlePath: "/Applications/Video Studio.app",
                   isOpen: true),
    ]

    /// What each made-up app left behind.
    static func appParts(for app: AppSummary) -> [AppPart] {
        var parts: [AppPart] = []
        if app.location == .homeApplications {
            parts.append(AppPart(itemId: "app", title: "The app itself",
                                 explain: "\(app.name) itself. You can download it again later.",
                                 sizeBytes: app.sizeBytes, preselected: true, sure: true,
                                 path: "~/Applications/\(app.name).app"))
        }
        parts += [
            AppPart(itemId: "part-0", title: "Caches", explain: "Copies the app keeps to work faster. It can download them again.",
                    sizeBytes: 1_200_000_000, preselected: true, sure: true, path: "~/Library/Caches/\(app.bundleId)"),
            AppPart(itemId: "part-1", title: "App data",
                    explain: "Files the app keeps for itself, such as downloads, settings and sign-ins. It can hold things you made in the app.",
                    sizeBytes: 640_000_000, preselected: false, sure: true,
                    path: "~/Library/Application Support/\(app.bundleId)"),
            AppPart(itemId: "part-2", title: "Logs",
                    explain: "Records the app kept of what it did. Roomy matched it by a similar name, so check it belongs to this app.",
                    sizeBytes: 12_000_000, preselected: false, sure: false, path: "~/Library/Logs/\(app.name)"),
            AppPart(itemId: "part-3", title: "Settings", explain: "The app's settings and preferences.",
                    sizeBytes: 40_960, preselected: true, sure: true, path: "~/Library/Preferences/\(app.bundleId).plist"),
        ]
        return parts
    }

    static let diskTotal: UInt64 = 256_000_000_000
    static let diskFree: UInt64 = 41_000_000_000
    static let scannedBytes: UInt64 = 180_000_000_000
    static let unreadableCount: UInt64 = 212
    /// The 212 unreadable folders by area, most first, as the core orders them.
    static let unreadableAreas: [UnreadableArea] = [
        UnreadableArea(area: .appData, folderCount: 171),
        UnreadableArea(area: .mail, folderCount: 18),
        UnreadableArea(area: .messages, folderCount: 9),
        UnreadableArea(area: .safari, folderCount: 7),
        UnreadableArea(area: .deviceBackups, folderCount: 4),
        UnreadableArea(area: .trash, folderCount: 1),
        UnreadableArea(area: .other, folderCount: 2),
    ]

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

    /// The item with this id: one the scan lists, or a JavaScript project's `node_modules`. The projects
    /// are kept out of `items`, so the Overview's figures stay as the wireframe shows them.
    static func lookup(_ id: String, in scenario: MockCoreClient.Scenario) -> ItemSummary? {
        guard scenario != .nothingFound else { return nil }
        return (items + javascriptProjectItems).first { $0.id == id }
    }

    /// Made-up `node_modules` folders, one per JavaScript project, largest first.
    static let javascriptProjectItems: [ItemSummary] = [
        item("js-shop", "Project packages (node_modules)",
             "The packages one JavaScript project has downloaded. The project lists them, so they can be fetched again.",
             1_840_000_000, .reviewFirst, .build),
        item("js-blog", "Project packages (node_modules)",
             "The packages one JavaScript project has downloaded. The project lists them, so they can be fetched again.",
             912_000_000, .reviewFirst, .build),
        item("js-landing", "Project packages (node_modules)",
             "The packages one JavaScript project has downloaded. The project lists them, so they can be fetched again.",
             640_000_000, .reviewFirst, .build),
        item("js-cli", "Project packages (node_modules)",
             "The packages one JavaScript project has downloaded. The project lists them, so they can be fetched again.",
             210_000_000, .reviewFirst, .build),
    ]

    /// The JavaScript screen for a Developer mode scan. Dates are counted back from `now`.
    static func javascript(now: Date) -> JavaScriptOverview {
        func daysAgo(_ days: Double) -> Int64 { Int64(now.timeIntervalSince1970 - days * 86_400) }
        let p = javascriptProjectItems
        let projects = [
            JsProject(item: p[0], name: "shop", folder: "~/Developer/shop", lastActivityUnixSeconds: daysAgo(2),
                      activitySource: .git, packageManager: .pnpm, installCommand: "pnpm install", hasManifest: true),
            JsProject(item: p[1], name: "blog", folder: "~/projects/blog", lastActivityUnixSeconds: daysAgo(210),
                      activitySource: .lockFile, packageManager: .npm, installCommand: "npm install", hasManifest: true),
            JsProject(item: p[2], name: "landing", folder: "~/code/landing", lastActivityUnixSeconds: daysAgo(400),
                      activitySource: .manifest, packageManager: nil, installCommand: nil, hasManifest: true),
            JsProject(item: p[3], name: "cli", folder: "~/dev/cli", lastActivityUnixSeconds: nil,
                      activitySource: nil, packageManager: nil, installCommand: nil, hasManifest: false),
        ]
        let caches = items.filter { $0.id == "npm-cache" }
        let total = (p + caches).reduce(0) { $0 + $1.sizeBytes }
        return JavaScriptOverview(projects: projects, caches: caches, totalBytes: total)
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
    /// The pretend library sizes. Mail is only partly readable, as without Full Disk Access.
    static func libraries(for scenario: MockCoreClient.Scenario) -> [LibrarySize] {
        let empty = scenario == .nothingFound
        func row(_ kind: LibraryKind, _ bytes: UInt64, count: UInt32 = 0, needsAccess: Bool = false) -> LibrarySize {
            LibrarySize(kind: kind, bytes: empty ? 0 : bytes, libraryCount: empty ? 0 : count,
                        found: !empty && bytes > 0, needsAccess: needsAccess)
        }
        return [
            row(.photos, 41_300_000_000, count: 1),
            row(.music, 8_200_000_000, count: 1),
            row(.tv, 0),
            row(.mail, 3_600_000_000, needsAccess: !empty),
            row(.messagesAttachments, 12_700_000_000),
            row(.finalCut, 0),
        ]
    }

    static func overview(for scenario: MockCoreClient.Scenario) -> Overview {
        let found = Self.found(for: scenario)
        return Overview(
            likelySafeBytes: sum(found.filter { $0.tier == .likelySafe }),
            scannedBytes: scannedBytes,
            diskTotal: diskTotal,
            diskFree: diskFree,
            categories: categoryTotals(found),
            topItems: Array(found.filter { $0.tier != .keep }.sorted { $0.sizeBytes > $1.sizeBytes }.prefix(8)),
            unreadableCount: unreadableCount,
            unreadableAreas: unreadableAreas,
            quickRescan: false
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
