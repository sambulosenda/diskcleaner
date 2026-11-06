import Foundation

/// The app's state: which screen is showing, the mode, and where the scan has got to.
/// Views read it; only its methods change it.
@MainActor
final class AppModel: ObservableObject {
    /// What the Overview shows.
    enum ScanState: Equatable {
        /// No scan has run yet (first launch).
        case neverScanned
        /// A scan is running.
        case scanning(ScanProgress)
        /// A scan finished.
        case loaded(Overview, scannedAt: Date)
        /// The last scan failed. The text is ready to show.
        case failed(String)
    }

    static let modeKey = "roomy.mode"

    @Published private(set) var scanState: ScanState = .neverScanned
    @Published var selection: SidebarItem? = .overview
    @Published var searchText = ""
    @Published var mode: Mode {
        didSet { modeDidChange() }
    }

    let client: CoreClient
    private let defaults: UserDefaults
    private let pollInterval: UInt64
    private let now: () -> Date
    private var scanTask: Task<Void, Never>?
    private var activeHandle: ScanHandle?
    private var stateBeforeScan: ScanState = .neverScanned

    /// - Parameters:
    ///   - pollInterval: nanoseconds between progress checks. Tests pass 0.
    init(
        client: CoreClient,
        defaults: UserDefaults = .standard,
        pollInterval: UInt64 = 100_000_000,
        now: @escaping () -> Date = Date.init
    ) {
        self.client = client
        self.defaults = defaults
        self.pollInterval = pollInterval
        self.now = now
        self.mode = Mode(rawValue: defaults.string(forKey: Self.modeKey) ?? "") ?? .simple
    }

    var isScanning: Bool {
        if case .scanning = scanState { return true }
        return false
    }

    /// The sidebar groups for the current mode.
    var sidebarSections: [[SidebarItem]] { SidebarItem.sections(for: mode) }

    /// The Overview's "Worth reviewing" rows, narrowed by the toolbar search.
    func worthReviewing(in overview: Overview) -> [ItemSummary] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return overview.topItems }
        return overview.topItems.filter {
            $0.title.localizedCaseInsensitiveContains(query)
                || $0.explanation.localizedCaseInsensitiveContains(query)
        }
    }

    /// Starts a scan, unless one is already running.
    func startScan() {
        guard !isScanning else { return }
        stateBeforeScan = scanState
        scanState = .scanning(ScanProgress(bytesSeen: 0, itemsSeen: 0, phase: .measuring))
        scanTask = Task { [weak self] in
            await self?.runScan()
        }
    }

    /// Stops the running scan and goes back to what was showing before it.
    func cancelScan() {
        guard isScanning else { return }
        scanTask?.cancel()
        scanTask = nil
        if let handle = activeHandle {
            Task { await client.cancelScan(handle) }
        }
        activeHandle = nil
        scanState = stateBeforeScan
    }

    /// Waits for the running scan, if any, to end. Used by tests.
    func waitForScan() async {
        await scanTask?.value
    }

    #if DEBUG
    /// Shows a given state without scanning. For previews and screenshot tests only.
    func showForPreview(_ state: ScanState) {
        scanTask?.cancel()
        scanTask = nil
        scanState = state
    }
    #endif

    private func runScan() async {
        do {
            let handle = try await client.startScan(mode: mode)
            guard !Task.isCancelled else { return await client.cancelScan(handle) }
            activeHandle = handle
            while true {
                let progress = try await client.scanProgress(handle)
                guard !Task.isCancelled else { return }
                if progress.phase == .done { break }
                scanState = .scanning(progress)
                try await Task.sleep(nanoseconds: pollInterval)
            }
            let overview = try await client.overview(handle)
            guard !Task.isCancelled else { return }
            activeHandle = nil
            scanState = .loaded(overview, scannedAt: now())
        } catch {
            guard !Task.isCancelled, !(error is CancellationError) else { return }
            activeHandle = nil
            scanState = .failed(Self.message(for: error))
        }
    }

    private func modeDidChange() {
        defaults.set(mode.rawValue, forKey: Self.modeKey)
        if mode == .simple, selection?.isDeveloperOnly == true {
            selection = .overview
        }
    }

    private static func message(for error: Error) -> String {
        (error as? LocalizedError)?.errorDescription ?? "Something went wrong while scanning."
    }
}
