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
    /// The scan whose results are showing. Kept so later screens can list its items; closed on rescan.
    private var loadedHandle: ScanHandle?
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
        self.mode = Mode(storageKey: defaults.string(forKey: Self.modeKey) ?? "") ?? .simple
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
                || $0.explain.localizedCaseInsensitiveContains(query)
        }
    }

    /// Starts a scan, unless one is already running.
    func startScan() {
        guard !isScanning else { return }
        stateBeforeScan = scanState
        scanState = .scanning(ScanProgress(bytesSeen: 0, itemsSeen: 0, phase: .scanning))
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
            Task {
                await client.cancelScan(handle: handle)
                await client.closeScan(handle: handle)
            }
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
        let handle = await client.startScan(mode: mode)
        guard !Task.isCancelled else { return await client.closeScan(handle: handle) }
        activeHandle = handle
        do {
            while true {
                let progress = try await client.scanProgress(handle: handle)
                guard !Task.isCancelled else { return }
                switch progress.phase {
                case .scanning, .classifying:
                    scanState = .scanning(progress)
                    try await Task.sleep(nanoseconds: pollInterval)
                    continue
                case .finished:
                    break
                case .cancelled:
                    // Only `cancelScan()` cancels, and it has already restored the earlier state.
                    return
                case .failed(let reason):
                    finish(handle, with: .failed(reason))
                    return
                }
                break
            }
            let overview = try await client.overview(handle: handle)
            guard !Task.isCancelled else { return }
            activeHandle = nil
            if let old = loadedHandle {
                Task { await client.closeScan(handle: old) }
            }
            loadedHandle = handle
            scanState = .loaded(overview, scannedAt: now())
        } catch {
            guard !Task.isCancelled, !(error is CancellationError) else { return }
            finish(handle, with: .failed(Self.message(for: error)))
        }
    }

    /// Ends a scan that produced no results, and frees it in the core.
    private func finish(_ handle: ScanHandle, with state: ScanState) {
        activeHandle = nil
        scanState = state
        Task { await client.closeScan(handle: handle) }
    }

    private func modeDidChange() {
        defaults.set(mode.storageKey, forKey: Self.modeKey)
        if mode == .simple, selection?.isDeveloperOnly == true {
            selection = .overview
        }
    }

    /// Plain words for a core error. Never shows the error's debug text, which is not meant for people.
    static func message(for error: Error) -> String {
        guard let coreError = error as? CoreError else { return "Something went wrong while scanning." }
        switch coreError {
        case .ScanFailed(let reason):
            return reason
        case .UnknownScan, .ScanNotFinished, .ScanCancelled:
            return "Roomy lost track of this scan. Please scan again."
        case .HomeNotFound:
            return "Roomy couldn't find your home folder."
        case .InvalidPath, .Io, .RulesInvalid, .UnknownItem:
            return "Something went wrong while scanning."
        }
    }
}
