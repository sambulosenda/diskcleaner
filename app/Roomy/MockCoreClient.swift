import Foundation

/// A stand-in for the Rust core, so the UI can be built and tested before T11 wires up `RoomyCore`.
///
/// It follows the same rules as the real core (see `docs/command-contract.md`): `likelySafeBytes` is the
/// exact sum of likely safe items, `topItems` never holds `keep` items, and no path appears anywhere.
/// Each call to `scanProgress` moves the pretend scan one step on. Nothing here reads the disk.
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

    init(scenario: Scenario = .normal, totalSteps: Int = 30) {
        self.scenario = scenario
        self.totalSteps = max(totalSteps, 2)
    }

    /// Picks the scenario from the `-RoomyMockScenario <name>` launch argument, defaulting to `.normal`.
    static func fromLaunchArguments(_ defaults: UserDefaults = .standard) -> MockCoreClient {
        let name = defaults.string(forKey: "RoomyMockScenario") ?? ""
        return MockCoreClient(scenario: Scenario(rawValue: name) ?? .normal)
    }

    func startScan(mode: Mode) async -> ScanHandle {
        let handle = ScanHandle(id: nextHandle)
        nextHandle += 1
        steps[handle] = 0
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
        cancelled.remove(handle)
    }

    func overview(handle: ScanHandle) async throws -> Overview {
        guard let step = steps[handle] else { throw CoreError.UnknownScan }
        if cancelled.contains(handle) { throw CoreError.ScanCancelled }
        if scenario == .failure { throw CoreError.ScanFailed(reason: "Roomy could not read your home folder.") }
        guard step >= totalSteps else { throw CoreError.ScanNotFinished }
        return MockData.overview(for: scenario)
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

    /// The Overview a finished scan returns, built the way the contract defines each field.
    static func overview(for scenario: MockCoreClient.Scenario) -> Overview {
        let found = scenario == .nothingFound ? [] : items
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
