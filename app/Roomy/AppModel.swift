import Foundation

/// The app's state: which screen is showing, the mode, where the scan has got to, and the item list
/// and detail panel of the category screens. Views read it; only its methods change it.
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

    /// What a category screen's list shows.
    enum ListState: Equatable {
        /// Not a list screen, or no scan has finished.
        case idle
        /// Asking the core for the items.
        case loading
        /// The items, already in the chosen order.
        case loaded([ItemSummary])
        /// The core could not list them. The text is ready to show.
        case failed(String)
    }

    /// What the detail panel shows.
    enum DetailState: Equatable {
        /// No row is selected.
        case none
        /// Asking the core for the selected row's detail.
        case loading
        /// The selected row's detail.
        case loaded(ItemDetail)
        /// The core could not give the detail. The text is ready to show.
        case failed(String)
    }

    /// What the cleanup review sheet shows. The review is a dry run: nothing moves.
    enum CleanupState: Equatable {
        /// No review is open.
        case closed
        /// The core is checking the ticked items.
        case checking
        /// What would move to the Trash, and what wouldn't and why.
        case ready(CleanupPreview)
        /// The core could not check the items. The text is ready to show.
        case failed(String)
    }

    static let modeKey = "roomy.mode"

    @Published private(set) var scanState: ScanState = .neverScanned
    @Published var selection: SidebarItem? = .overview {
        didSet { if selection != oldValue { selectionDidChange() } }
    }
    @Published var searchText = ""
    @Published var mode: Mode {
        didSet { if mode != oldValue { modeDidChange() } }
    }

    /// The category screen's items.
    @Published private(set) var listState: ListState = .idle
    /// The order of the category screen's items.
    @Published var sort: Sort = .largestFirst {
        didSet { if sort != oldValue { reloadItems() } }
    }
    /// Only items of this tier, or every tier when `nil`.
    @Published var tierFilter: Tier? = nil {
        didSet { if tierFilter != oldValue { reloadItems() } }
    }
    /// The selected row in the category screen's list.
    @Published var selectedItemID: String? = nil {
        didSet { if selectedItemID != oldValue { reloadDetail() } }
    }
    @Published private(set) var detailState: DetailState = .none
    /// Ticked rows, which the cleanup review checks. Nothing is ticked for the user, and `keep` items
    /// can never be ticked.
    @Published private(set) var checkedItemIDs: Set<String> = []
    /// The cleanup review sheet.
    @Published private(set) var cleanupState: CleanupState = .closed

    let client: CoreClient
    private let defaults: UserDefaults
    private let pollInterval: UInt64
    private let now: () -> Date
    private var scanTask: Task<Void, Never>?
    private var listTask: Task<Void, Never>?
    private var detailTask: Task<Void, Never>?
    private var cleanupTask: Task<Void, Never>?
    /// The size of each ticked row, for the total under the list.
    private var checkedSizes: [String: UInt64] = [:]
    private var activeHandle: ScanHandle?
    /// The scan whose results are showing. Category screens list its items; closed on rescan.
    private var loadedHandle: ScanHandle?
    private var stateBeforeScan: ScanState = .neverScanned

    /// - Parameters:
    ///   - pollInterval: nanoseconds between progress checks. The default, 100 ms, keeps progress
    ///     updates to at most ten a second. Tests pass 0.
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

    /// The finished scan's Overview, if there is one.
    var overview: Overview? {
        if case .loaded(let overview, _) = scanState { return overview }
        return nil
    }

    /// The sidebar groups for the current mode.
    var sidebarSections: [[SidebarItem]] { SidebarItem.sections(for: mode) }

    /// The Overview's "Worth reviewing" rows, narrowed by the toolbar search.
    func worthReviewing(in overview: Overview) -> [ItemSummary] {
        matchingSearch(overview.topItems)
    }

    /// The category screen's rows, narrowed by the toolbar search.
    var visibleItems: [ItemSummary] {
        guard case .loaded(let items) = listState else { return [] }
        return matchingSearch(items)
    }

    /// Whether a row is ticked.
    func isChecked(_ item: ItemSummary) -> Bool {
        checkedItemIDs.contains(item.id)
    }

    /// Ticks or unticks a row. `keep` items can never be ticked. Ticking changes nothing on disk.
    func setChecked(_ checked: Bool, for item: ItemSummary) {
        guard item.tier != .keep else { return }
        if checked {
            checkedItemIDs.insert(item.id)
            checkedSizes[item.id] = item.sizeBytes
        } else {
            checkedItemIDs.remove(item.id)
            checkedSizes[item.id] = nil
        }
    }

    /// The scan's size of every ticked row, added up.
    var checkedBytes: UInt64 {
        checkedSizes.values.reduce(0, +)
    }

    // MARK: - Cleanup review (dry run)

    /// Opens the cleanup review: asks the core to check every ticked row the way a cleanup would.
    /// Nothing moves. Each row was ticked on its own, so each is sent as chosen alone.
    func reviewCleanup() {
        guard let handle = loadedHandle, !checkedItemIDs.isEmpty, cleanupState == .closed else { return }
        let sizes = checkedSizes
        let ids = checkedItemIDs.sorted {
            let a = sizes[$0] ?? 0, b = sizes[$1] ?? 0
            return a != b ? a > b : $0 < $1
        }
        let choices = ids.map { CleanupChoice(itemId: $0, chosenAlone: true) }
        let client = self.client
        cleanupState = .checking
        cleanupTask = Task { [weak self] in
            do {
                let preview = try await client.previewCleanup(handle: handle, choices: choices)
                guard !Task.isCancelled, let self else {
                    // The review was closed while checking: let the core forget the plan.
                    if let plan = preview.plan { await client.discardCleanup(plan: plan) }
                    return
                }
                self.cleanupState = .ready(preview)
            } catch {
                guard !Task.isCancelled, !(error is CancellationError), let self else { return }
                self.cleanupState = .failed(Self.cleanupMessage(for: error))
            }
        }
    }

    /// Closes the cleanup review without cleaning up, and lets the core forget its plan.
    func closeCleanupReview() {
        cleanupTask?.cancel()
        cleanupTask = nil
        if case .ready(let preview) = cleanupState, let plan = preview.plan {
            let client = self.client
            Task { await client.discardCleanup(plan: plan) }
        }
        if cleanupState != .closed {
            cleanupState = .closed
        }
    }

    /// Waits for the cleanup review's check to finish. Used by tests.
    func waitForCleanupCheck() async {
        await cleanupTask?.value
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

    /// Waits for the item list and detail panel to finish loading. Used by tests.
    func waitForItems() async {
        await listTask?.value
        await detailTask?.value
    }

    #if DEBUG
    /// Shows a given state without scanning. For previews and screenshot tests only.
    func showForPreview(_ state: ScanState, list: ListState = .idle, detail: DetailState = .none,
                        selectedItemID: String? = nil) {
        scanTask?.cancel()
        scanTask = nil
        scanState = state
        listTask?.cancel()
        listState = list
        self.selectedItemID = selectedItemID
        detailTask?.cancel()
        detailState = detail
    }
    #endif

    // MARK: - Scanning

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
                    if scanState != .scanning(progress) {
                        scanState = .scanning(progress)
                    }
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
            resetItems()
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

    // MARK: - Item list and detail

    /// A new scan's results: forget the old selection and ticks, then list the current screen.
    private func resetItems() {
        closeCleanupReview()
        checkedItemIDs = []
        checkedSizes = [:]
        selectedItemID = nil
        reloadItems()
    }

    /// Asks the core for the current category screen's items.
    private func reloadItems() {
        listTask?.cancel()
        guard let filter = selection?.itemFilter, let handle = loadedHandle, overview != nil else {
            listState = .idle
            return
        }
        listState = .loading
        let sort = self.sort
        let tier = self.tierFilter
        let client = self.client
        listTask = Task { [weak self] in
            do {
                let items = try await Self.items(from: client, handle: handle, filter: filter, tier: tier, sort: sort)
                guard !Task.isCancelled, let self else { return }
                self.listState = .loaded(items)
                if let selected = self.selectedItemID, !items.contains(where: { $0.id == selected }) {
                    self.selectedItemID = nil
                }
            } catch {
                guard !Task.isCancelled, !(error is CancellationError), let self else { return }
                self.listState = .failed(Self.message(for: error))
            }
        }
    }

    /// The items for one screen. A screen showing several categories asks for each and merges them
    /// in the same order the core uses: by `sort`, then by id.
    static func items(
        from client: CoreClient, handle: ScanHandle, filter: SidebarItem.ItemFilter, tier: Tier?, sort: Sort
    ) async throws -> [ItemSummary] {
        switch filter {
        case .everything:
            return try await client.listItems(handle: handle, category: nil, tier: tier, sort: sort)
        case .categories(let categories):
            var merged: [ItemSummary] = []
            for category in categories {
                merged += try await client.listItems(handle: handle, category: category, tier: tier, sort: sort)
            }
            return categories.count > 1 ? merged.sorted(by: Self.order(for: sort)) : merged
        }
    }

    /// The core's order for `sort` (see `docs/command-contract.md`): ties broken by id.
    static func order(for sort: Sort) -> (ItemSummary, ItemSummary) -> Bool {
        switch sort {
        case .largestFirst:
            return { $0.sizeBytes != $1.sizeBytes ? $0.sizeBytes > $1.sizeBytes : $0.id < $1.id }
        case .smallestFirst:
            return { $0.sizeBytes != $1.sizeBytes ? $0.sizeBytes < $1.sizeBytes : $0.id < $1.id }
        case .title:
            return {
                let a = $0.title.lowercased(), b = $1.title.lowercased()
                return a != b ? a < b : $0.id < $1.id
            }
        }
    }

    /// Asks the core for the selected row's detail.
    private func reloadDetail() {
        detailTask?.cancel()
        guard let id = selectedItemID, let handle = loadedHandle else {
            detailState = .none
            return
        }
        detailState = .loading
        let client = self.client
        detailTask = Task { [weak self] in
            do {
                let detail = try await client.itemDetail(handle: handle, itemId: id)
                guard !Task.isCancelled, let self else { return }
                self.detailState = .loaded(detail)
            } catch {
                guard !Task.isCancelled, !(error is CancellationError), let self else { return }
                self.detailState = .failed(Self.message(for: error))
            }
        }
    }

    private func matchingSearch(_ items: [ItemSummary]) -> [ItemSummary] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return items }
        return items.filter {
            $0.title.localizedCaseInsensitiveContains(query)
                || $0.explain.localizedCaseInsensitiveContains(query)
        }
    }

    // MARK: - Changes

    private func selectionDidChange() {
        selectedItemID = nil
        tierFilter = nil
        reloadItems()
    }

    private func modeDidChange() {
        defaults.set(mode.storageKey, forKey: Self.modeKey)
        if mode == .simple, selection?.isDeveloperOnly == true {
            selection = .overview
        }
        // Results from the other mode use the other mode's rules, and Developer results carry paths.
        // Scan again so what shows always matches the mode.
        if overview != nil {
            selectedItemID = nil
            startScan()
        } else if isScanning {
            cancelScan()
            startScan()
        }
    }

    /// Plain words for a core error. Never shows the error's debug text, which is not meant for people.
    static func message(for error: Error) -> String {
        guard let coreError = error as? CoreError else { return "Something went wrong while scanning." }
        switch coreError {
        case .ScanFailed(let reason):
            return reason
        case .UnknownScan, .ScanNotFinished, .ScanCancelled, .UnknownItem:
            return "Roomy lost track of this scan. Please scan again."
        case .HomeNotFound:
            return "Roomy couldn't find your home folder."
        case .InvalidPath, .Io, .RulesInvalid:
            return "Something went wrong while scanning."
        case .NothingSelected:
            return "Nothing was chosen."
        case .CleanupUnavailable(let reason):
            return reason
        }
    }

    /// Plain words for an error from the cleanup review.
    static func cleanupMessage(for error: Error) -> String {
        guard let coreError = error as? CoreError else { return "Roomy couldn't check these items." }
        switch coreError {
        case .UnknownItem:
            return "Some of these items are no longer in the scan. Please scan again."
        default:
            return message(for: coreError)
        }
    }
}
