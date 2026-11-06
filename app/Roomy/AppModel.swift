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

    /// What the cleanup sheet shows. It opens as a dry run; nothing moves until the user confirms.
    enum CleanupState: Equatable {
        /// No review is open.
        case closed
        /// The core is checking the ticked items.
        case checking
        /// What would move to the Trash, and what wouldn't and why.
        case ready(CleanupPreview)
        /// The user confirmed; the core is moving the items to the Trash.
        case moving(CleanupPreview)
        /// The cleanup finished.
        case done(CleanupResult)
        /// The user asked to put the cleanup back; the core is moving things back.
        case puttingBack(CleanupResult)
        /// Putting back finished.
        case putBack(PutBackResult)
        /// The core could not check, move or put back. The text is ready to show.
        case failed(String)

        /// Whether the core is moving files right now. The sheet can't be closed then.
        var isBusy: Bool {
            switch self {
            case .moving, .puttingBack: return true
            default: return false
            }
        }

        /// Whether the sheet shows what a cleanup or put back did. That doesn't depend on the scan, so a
        /// new scan leaves it open for the user to read.
        var showsOutcome: Bool {
            switch self {
            case .done, .putBack: return true
            default: return false
            }
        }
    }

    static let modeKey = "roomy.mode"
    /// Set once the Full Disk Access screen has been shown and closed, or once access was found on.
    static let accessOnboardingKey = "roomy.onboarding.fullDiskAccessDone"
    /// Where the folders the user asked Roomy to keep are stored.
    static let protectedFoldersKey = "roomy.protectedFolders"

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
    /// The most recent cleanup, while it can still be put back (until the app quits).
    @Published private(set) var lastCleanup: CleanupHandle?
    /// What the most recent cleanup did, for the Put back banner. Kept with `lastCleanup`, so the banner
    /// matches the cleanup it undoes even if that cleanup could not be saved to History.
    @Published private(set) var lastCleanupResult: CleanupResult?
    /// True while a put back runs, so a second tap can't send it twice.
    @Published private(set) var isPuttingBack = false
    /// Why the last put back from History did not work, shown on the History screen.
    @Published private(set) var historyMessage: String?
    /// Past cleanups for the History screen, newest first.
    @Published private(set) var history: [HistoryEntry] = []
    /// Whether Roomy can read the folders macOS keeps private. `nil` until the core has been asked.
    @Published private(set) var fullDiskAccess: FullDiskAccess?
    /// The Full Disk Access screen. It opens by itself on first launch when access is off, and from
    /// the Overview, Settings or a refused cleanup after that.
    @Published var showsAccessOnboarding = false
    /// Folders the user asked Roomy to keep, as full paths. Sent to the core before every review and
    /// cleanup, so nothing inside them is ever cleaned.
    @Published private(set) var protectedFolders: [String] = []
    /// Why the last change to the kept folders didn't work, shown in Settings.
    @Published private(set) var protectedFoldersMessage: String?
    /// Set once something has moved after the current scan started, so its sizes are out of date.
    /// Cleared when a new scan starts. A scan that was already running when files moved is followed by
    /// another one.
    private var needsRescan = false

    /// The ⌘K palette. It only shows screens and asks this model to act; it never moves files.
    private(set) lazy var palette = CommandPaletteModel(app: self)

    let client: CoreClient
    private let defaults: UserDefaults
    private let pollInterval: UInt64
    private let now: () -> Date
    private var scanTask: Task<Void, Never>?
    private var listTask: Task<Void, Never>?
    private var detailTask: Task<Void, Never>?
    private var cleanupTask: Task<Void, Never>?
    /// A put back started from History, with no sheet open.
    private var putBackTask: Task<Void, Never>?
    /// `needsRescan` as it was when the running scan started, restored if that scan is stopped.
    private var neededRescanBeforeScan = false
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
        self.protectedFolders = defaults.stringArray(forKey: Self.protectedFoldersKey) ?? []
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

    /// Every item of the finished scan, largest first, for the ⌘K palette. Reads only; empty before a
    /// scan has finished or if the core can't list them.
    func scannedItems() async -> [ItemSummary] {
        guard let handle = loadedHandle, overview != nil else { return [] }
        return (try? await client.listItems(handle: handle, category: nil, tier: nil, sort: .largestFirst)) ?? []
    }

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

    // MARK: - Full Disk Access

    /// Called once when the window first appears. Asks the core about Full Disk Access and, on first
    /// launch only, opens the Full Disk Access screen if access is off. If it is already on, or the core
    /// can't tell, the screen is never shown by itself.
    func windowDidAppear() async {
        await refreshFullDiskAccess()
        guard !defaults.bool(forKey: Self.accessOnboardingKey) else { return }
        if fullDiskAccess == .notGranted {
            showsAccessOnboarding = true
        } else {
            defaults.set(true, forKey: Self.accessOnboardingKey)
        }
    }

    /// Asks the core again whether Full Disk Access is on. Reads a few folder listings only.
    func refreshFullDiskAccess() async {
        let access = await client.fullDiskAccess()
        if access != fullDiskAccess { fullDiskAccess = access }
    }

    /// Opens the Full Disk Access screen.
    func showAccessOnboarding() {
        showsAccessOnboarding = true
    }

    /// Closes the Full Disk Access screen for good: it won't open by itself again. With `scan`, starts a
    /// scan, so newly readable folders are measured.
    func finishAccessOnboarding(scan: Bool) {
        defaults.set(true, forKey: Self.accessOnboardingKey)
        showsAccessOnboarding = false
        if scan { startScan() }
    }

    /// Whether the Overview should say what wasn't scanned and offer Full Disk Access: some folders
    /// couldn't be read and access is not known to be on.
    func shouldOfferAccess(for overview: Overview) -> Bool {
        overview.unreadableCount > 0 && fullDiskAccess != .granted
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
        let kept = protectedFolders
        cleanupState = .checking
        cleanupTask = Task { [weak self] in
            do {
                // The core must know the kept folders before it checks anything. If it can't take
                // them, nothing is checked and nothing can move.
                _ = try await client.setProtectedFolders(kept)
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

    /// Closes the cleanup sheet. A review that was not carried out lets the core forget its plan. After
    /// a cleanup or a put back, scans again so the sizes are right. Does nothing while files are moving.
    func closeCleanupReview() {
        guard !cleanupState.isBusy else { return }
        cleanupTask?.cancel()
        cleanupTask = nil
        if case .ready(let preview) = cleanupState, let plan = preview.plan {
            let client = self.client
            Task { await client.discardCleanup(plan: plan) }
        }
        if cleanupState != .closed {
            cleanupState = .closed
        }
        rescanIfNeeded()
    }

    /// Scans again if files moved since the last scan started. Waits while a scan is running (it is
    /// started again when that one ends) or while the cleanup sheet is open.
    private func rescanIfNeeded() {
        guard needsRescan, !isScanning, cleanupState == .closed else { return }
        startScan()
    }

    // MARK: - Folders to keep

    /// Adds folders the user picked to the kept folders. Symlinks are resolved, so keeping a link also
    /// keeps what it points to. The core checks each one; a folder outside the home folder is refused.
    func addProtectedFolders(_ urls: [URL]) async {
        let added = urls.map { $0.resolvingSymlinksInPath().standardizedFileURL.path }
        var wanted = protectedFolders
        for path in added where !wanted.contains(path) { wanted.append(path) }
        await saveProtectedFolders(wanted)
    }

    /// Stops keeping `folder`.
    func removeProtectedFolder(_ folder: String) async {
        await saveProtectedFolders(protectedFolders.filter { $0 != folder })
    }

    /// Sends `wanted` to the core and, if it takes them, shows and stores the list it will use. If the
    /// core refuses, the earlier list stays and the reason is shown.
    private func saveProtectedFolders(_ wanted: [String]) async {
        do {
            let applied = try await client.setProtectedFolders(wanted)
            protectedFolders = applied
            protectedFoldersMessage = nil
            defaults.set(applied, forKey: Self.protectedFoldersKey)
        } catch {
            if case CoreError.InvalidPath(let reason) = error {
                protectedFoldersMessage = reason
            } else {
                protectedFoldersMessage = "Roomy couldn't save that folder. Please try again."
            }
        }
    }

    // MARK: - Cleanup (moves files)

    /// Moves the reviewed items to the Trash. Call only after the user confirmed. The core checks every
    /// item again first, and an item that changed since the review stays where it is.
    func moveToTrash() {
        guard case .ready(let preview) = cleanupState, let plan = preview.plan else { return }
        let client = self.client
        let kept = protectedFolders
        cleanupState = .moving(preview)
        cleanupTask = Task { [weak self] in
            // Not cancelled by closing: the sheet can't close while this runs.
            do {
                _ = try await client.setProtectedFolders(kept)
                let result = try await client.runCleanup(plan: plan)
                guard let self else { return }
                self.needsRescan = true
                self.checkedItemIDs = []
                self.checkedSizes = [:]
                self.lastCleanup = result.cleanup
                self.lastCleanupResult = result.cleanup == nil ? nil : result
                self.historyMessage = nil
                self.cleanupState = .done(result)
                await self.reloadHistory()
            } catch {
                guard let self else { return }
                // A refused cleanup moved nothing, but the review is used up either way.
                self.cleanupState = .failed(Self.cleanupMessage(for: error))
            }
        }
    }

    /// Puts the most recent cleanup back where it was. Works once, until the app quits. Ignored while a
    /// put back is already running.
    func putBack() {
        guard let handle = lastCleanup, !isPuttingBack else { return }
        let client = self.client
        let shown: CleanupResult?
        if case .done(let result) = cleanupState { shown = result } else { shown = nil }
        if let shown { cleanupState = .puttingBack(shown) }
        isPuttingBack = true
        historyMessage = nil
        let work = Task { [weak self] in
            do {
                let result = try await client.putBackCleanup(cleanup: handle)
                guard let self else { return }
                self.isPuttingBack = false
                self.forgetLastCleanup()
                self.needsRescan = true
                if shown != nil {
                    self.cleanupState = .putBack(result)
                } else {
                    // Put back from History: no sheet is open, so scan again now (or after the
                    // running scan).
                    self.rescanIfNeeded()
                }
                await self.reloadHistory()
            } catch {
                guard let self else { return }
                self.isPuttingBack = false
                // `NothingToPutBack` means the core no longer holds this cleanup. Any other error came
                // before anything moved, and the core still holds it, so the user can try again.
                if let core = error as? CoreError, case .NothingToPutBack = core { self.forgetLastCleanup() }
                let message = Self.cleanupMessage(for: error)
                if shown != nil {
                    self.cleanupState = .failed(message)
                } else {
                    self.historyMessage = message
                }
                await self.reloadHistory()
            }
        }
        if shown != nil { cleanupTask = work } else { putBackTask = work }
    }

    /// Drops the record of the last cleanup once it can no longer be put back.
    private func forgetLastCleanup() {
        lastCleanup = nil
        lastCleanupResult = nil
    }

    /// Asks the core for past cleanups.
    func reloadHistory() async {
        history = await client.cleanupHistory(limit: 50)
    }

    /// Waits for the cleanup sheet's current step (check, move or put back) to finish. Used by tests.
    func waitForCleanupCheck() async {
        await cleanupTask?.value
        await putBackTask?.value
    }

    /// Starts a scan, unless one is already running.
    func startScan() {
        guard !isScanning else { return }
        // This scan sees any moves made so far.
        neededRescanBeforeScan = needsRescan
        needsRescan = false
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
        // The stopped scan didn't see the moves, so the sizes are still out of date.
        needsRescan = needsRescan || neededRescanBeforeScan
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
        if !cleanupState.isBusy && !cleanupState.showsOutcome { closeCleanupReview() }
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
        if selection == .history {
            Task { [weak self] in await self?.reloadHistory() }
        }
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
        case .CleanupUnavailable(let reason), .DockerUnavailable(let reason):
            return reason
        case .NothingToPutBack:
            return "Roomy can only put back its most recent cleanup, until Roomy quits. You can still drag items back from the Trash."
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
