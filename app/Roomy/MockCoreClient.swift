import Foundation

/// A stand-in for the Rust core, so the UI can be built and tested before T09 and T11.
///
/// Each call to `scanProgress` moves the pretend scan one step on. Nothing here reads the disk.
@MainActor
final class MockCoreClient: CoreClient {
    /// What the pretend scan should do.
    enum Scenario: String {
        /// A normal scan that finds things worth reviewing.
        case normal
        /// A scan that finishes but finds nothing likely safe.
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

    func startScan(mode: Mode) async throws -> ScanHandle {
        let handle = ScanHandle(id: nextHandle)
        nextHandle += 1
        steps[handle] = 0
        return handle
    }

    func scanProgress(_ handle: ScanHandle) async throws -> ScanProgress {
        if cancelled.contains(handle) { throw CoreClientError.cancelled }
        guard let step = steps[handle] else { throw CoreClientError.unknownScan }
        let next = min(step + 1, totalSteps)
        steps[handle] = next

        if scenario == .failure && next >= totalSteps / 2 {
            throw CoreClientError.scanFailed("Roomy couldn't finish reading your home folder.")
        }

        let data = MockData.overview(for: scenario)
        let used = data.diskTotal - data.diskFree
        let fraction = Double(next) / Double(totalSteps)
        let phase: ScanPhase
        if next >= totalSteps {
            phase = .done
        } else if fraction >= 0.8 {
            phase = .sorting
        } else {
            phase = .measuring
        }
        return ScanProgress(
            bytesSeen: UInt64(Double(used) * fraction),
            itemsSeen: UInt64(next) * 41_237,
            phase: phase
        )
    }

    func cancelScan(_ handle: ScanHandle) async {
        cancelled.insert(handle)
    }

    func overview(_ handle: ScanHandle) async throws -> Overview {
        if cancelled.contains(handle) { throw CoreClientError.cancelled }
        guard let step = steps[handle] else { throw CoreClientError.unknownScan }
        guard step >= totalSteps else { throw CoreClientError.scanNotFinished }
        return MockData.overview(for: scenario)
    }
}

/// Made-up numbers shaped like a real Mac with a 500 GB disk.
enum MockData {
    static let diskTotal: UInt64 = 500_000_000_000
    static let diskFree: UInt64 = 61_000_000_000
    static let unreadableCount: UInt64 = 212

    /// Every recommendation the pretend scan finds. The Overview shows the biggest few.
    static let items: [ItemSummary] = [
        item("xcode-derived-data", "Xcode build data",
             "Files Xcode makes while building apps. Xcode makes them again when needed.",
             12_400_000_000, .likelySafe, .developer),
        item("simulator-runtimes", "Old Simulator versions",
             "iPhone and iPad Simulator versions you no longer use. Xcode can download them again.",
             6_900_000_000, .likelySafe, .developer),
        item("app-caches", "App caches",
             "Temporary files apps keep so they open faster. Apps rebuild them.",
             5_300_000_000, .likelySafe, .appsAndCaches),
        item("npm-cache", "JavaScript package caches",
             "Copies of downloaded packages. They download again when a project needs them.",
             4_100_000_000, .likelySafe, .developer),
        item("trash", "Items in the Trash",
             "Things you have already moved to the Trash.",
             3_200_000_000, .likelySafe, .other),
        item("homebrew-cache", "Homebrew downloads",
             "Installers Homebrew has already used. It downloads them again if needed.",
             1_300_000_000, .likelySafe, .developer),
        item("browser-caches", "Browser caches",
             "Saved copies of web pages. Browsers fetch them again.",
             800_000_000, .likelySafe, .appsAndCaches),
        item("iphone-backup", "iPhone backup",
             "A backup of an iPhone, last updated in March. Check you have a newer one first.",
             18_100_000_000, .reviewFirst, .backups),
        item("downloads-installers", "Installers in Downloads",
             "Disk images and installers. You may have installed these already.",
             6_200_000_000, .reviewFirst, .downloads),
        item("large-video", "Screen recording.mov",
             "A large video you haven't opened for a year.",
             4_700_000_000, .reviewFirst, .largeFiles),
        item("xcode-archives", "Old app archives",
             "Past app builds made for the App Store. You may want to keep recent ones.",
             3_900_000_000, .reviewFirst, .developer),
        item("photos-library", "Photos library",
             "Your photos. Shown so you know where the space goes.",
             22_000_000_000, .keep, .largeFiles),
    ]

    /// How much each category takes on the disk, whether or not any of it can be freed.
    static let categoryBytes: [(Category, UInt64)] = [
        (.appsAndCaches, 96_000_000_000),
        (.developer, 74_000_000_000),
        (.largeFiles, 58_000_000_000),
        (.backups, 41_000_000_000),
        (.systemData, 38_000_000_000),
        (.downloads, 22_000_000_000),
        (.other, 110_000_000_000),
    ]

    /// The Overview a finished scan returns. `.nothingFound` keeps the disk numbers but finds no items.
    static func overview(for scenario: MockCoreClient.Scenario) -> Overview {
        let found = scenario == .nothingFound ? [] : items
        let likelySafe = found.filter { $0.tier == .likelySafe }
        let categories = categoryBytes.map { category, bytes in
            CategoryTotal(
                category: category,
                bytes: bytes,
                likelySafeBytes: likelySafe.filter { $0.category == category }.reduce(0) { $0 + $1.bytes }
            )
        }
        return Overview(
            likelySafeBytes: likelySafe.reduce(0) { $0 + $1.bytes },
            diskTotal: diskTotal,
            diskFree: diskFree,
            categories: categories,
            topItems: Array(found.sorted { $0.bytes > $1.bytes }.prefix(8)),
            unreadableCount: unreadableCount
        )
    }

    private static func item(
        _ id: String, _ title: String, _ explanation: String,
        _ bytes: UInt64, _ tier: Tier, _ category: Category
    ) -> ItemSummary {
        ItemSummary(id: id, title: title, explanation: explanation, bytes: bytes, tier: tier, category: category)
    }
}
