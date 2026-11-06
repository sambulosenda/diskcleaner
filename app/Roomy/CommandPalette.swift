import Foundation

// The ⌘K palette: type a few letters to jump to a screen or an item, or to start a scan, open the
// cleanup review, change the mode or open Settings.
//
// Safety: nothing here moves, deletes or puts back files. The strongest thing the palette can do is open
// the cleanup review, which is a dry run; files only move after the user presses Move to Trash in that
// review and confirms. `PaletteAction` has no case that moves files, and `CommandPaletteTests` runs every
// command the palette offers and checks that nothing moved.

/// What a palette entry does when picked.
enum PaletteAction: Equatable {
    /// Shows a sidebar screen.
    case goTo(SidebarItem)
    /// Shows the screen that lists an item, and selects it so its detail panel opens.
    case showItem(id: String, screen: SidebarItem)
    /// Starts a scan. Reads only.
    case scan
    /// Stops the running scan.
    case stopScan
    /// Opens the cleanup review for the ticked items. A dry run: nothing moves.
    case reviewCleanup
    /// Changes the mode. Scans again, as changing it in Settings does.
    case switchMode(Mode)
    /// Opens the Settings window.
    case openSettings
    /// Opens the Full Disk Access screen.
    case showFullDiskAccess
}

/// One entry in the palette.
struct PaletteCommand: Identifiable, Equatable {
    /// What kind of entry it is, shown at the end of its row.
    enum Kind: Equatable {
        case screen
        case action
        case item

        var label: String {
            switch self {
            case .screen: return "Go to"
            case .action: return "Action"
            case .item: return "Item"
            }
        }
    }

    let id: String
    let title: String
    /// One short line under the title, or empty.
    let subtitle: String
    /// SF Symbol name. All of these exist on macOS 13.
    let symbol: String
    let kind: Kind
    /// Other words that find this entry, such as "preferences" for Settings.
    let keywords: [String]
    let action: PaletteAction
}

/// What the palette needs to know about the app to list its entries.
struct PaletteContext: Equatable {
    var mode: Mode
    var isScanning: Bool
    /// Whether a scan has finished, so its items can be shown.
    var hasResults: Bool
    /// How many rows are ticked for cleanup.
    var checkedCount: Int
}

extension PaletteCommand {
    /// Every entry for the app as it is now: screens first, then actions, then the scan's items.
    /// Screens follow the sidebar, so Developer screens appear only in Developer mode.
    static func all(for context: PaletteContext, items: [ItemSummary]) -> [PaletteCommand] {
        screens(for: context.mode) + actions(for: context) + itemEntries(items, mode: context.mode)
    }

    static func screens(for mode: Mode) -> [PaletteCommand] {
        SidebarItem.sections(for: mode).flatMap { $0 }.map { screen in
            PaletteCommand(
                id: "screen.\(screen.rawValue)",
                title: screen.title,
                subtitle: "",
                symbol: screen.symbol,
                kind: .screen,
                keywords: screen.paletteKeywords,
                action: .goTo(screen)
            )
        }
    }

    static func actions(for context: PaletteContext) -> [PaletteCommand] {
        var actions: [PaletteCommand] = []
        if context.isScanning {
            actions.append(PaletteCommand(
                id: "action.stopScan", title: "Stop scanning", subtitle: "Keeps the last results.",
                symbol: "stop.circle", kind: .action, keywords: ["cancel"], action: .stopScan))
        } else {
            actions.append(PaletteCommand(
                id: "action.scan", title: "Scan this Mac", subtitle: "Measures what is filling your Mac. Nothing is removed.",
                symbol: "arrow.clockwise", kind: .action, keywords: ["rescan", "refresh", "measure"], action: .scan))
        }
        if context.hasResults && context.checkedCount > 0 {
            let noun = context.checkedCount == 1 ? "item" : "items"
            actions.append(PaletteCommand(
                id: "action.reviewCleanup", title: "Review cleanup",
                subtitle: "Checks the \(context.checkedCount) chosen \(noun). Nothing is moved yet.",
                symbol: "sparkles", kind: .action, keywords: ["clean", "trash", "free space", "chosen"],
                action: .reviewCleanup))
        }
        let other: Mode = context.mode == .simple ? .developer : .simple
        actions.append(PaletteCommand(
            id: "action.mode.\(other.storageKey)", title: "Switch to \(other.label) mode",
            subtitle: other.explanation, symbol: other == .developer ? "hammer" : "person",
            kind: .action, keywords: ["mode", "developer", "simple"], action: .switchMode(other)))
        actions.append(PaletteCommand(
            id: "action.settings", title: "Open Settings", subtitle: "",
            symbol: "gearshape", kind: .action, keywords: ["preferences", "options", "folders to keep"],
            action: .openSettings))
        actions.append(PaletteCommand(
            id: "action.fullDiskAccess", title: "Full Disk Access", subtitle: "Lets Roomy measure the folders macOS keeps private.",
            symbol: "lock.shield", kind: .action, keywords: ["permission", "privacy"], action: .showFullDiskAccess))
        return actions
    }

    static func itemEntries(_ items: [ItemSummary], mode: Mode) -> [PaletteCommand] {
        items.map { item in
            let screen = SidebarItem.screen(for: item.category, mode: mode)
            return PaletteCommand(
                id: "item.\(item.id)",
                title: item.title,
                subtitle: "\(Format.bytes(item.sizeBytes)) · \(item.tier.label) · \(screen.title)",
                symbol: item.category.symbol,
                kind: .item,
                keywords: [item.category.title],
                action: .showItem(id: item.id, screen: screen)
            )
        }
    }

    /// The entries that match `query`, best first. With no query, every screen and action, but no items.
    static func ranked(_ commands: [PaletteCommand], query: String, limit: Int = 50) -> [PaletteCommand] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return Array(commands.filter { $0.kind != .item }.prefix(limit))
        }
        let scored = commands.enumerated().compactMap { index, command -> (Int, Int, PaletteCommand)? in
            guard let score = command.score(for: trimmed) else { return nil }
            return (score, index, command)
        }
        // Best score first; ties keep the list order (screens, then actions, then items by size).
        let sorted = scored.sorted { $0.0 != $1.0 ? $0.0 > $1.0 : $0.1 < $1.1 }
        return sorted.prefix(limit).map { $0.2 }
    }

    /// How well `query` matches this entry's title, or one of its keywords a little less well.
    func score(for query: String) -> Int? {
        let titleScore = FuzzyMatch.score(query, in: title)
        let keywordScore = keywords.compactMap { FuzzyMatch.score(query, in: $0) }.max().map { $0 - 5 }
        switch (titleScore, keywordScore) {
        case (nil, nil): return nil
        case (let t?, nil): return t
        case (nil, let k?): return k
        case (let t?, let k?): return max(t, k)
        }
    }
}

extension SidebarItem {
    /// Extra words that find a screen in the palette.
    var paletteKeywords: [String] {
        switch self {
        case .overview: return ["home", "summary", "disk"]
        case .backups: return ["iphone", "ipad"]
        case .libraries: return ["photos", "mail", "music", "messages", "attachments", "final cut", "tv"]
        case .downloads: return ["installers", "dmg"]
        case .largeFiles: return ["everything", "all items", "big"]
        case .appsAndCaches: return ["cache", "app data"]
        case .systemData: return ["logs", "system"]
        case .projects: return ["build", "node_modules", "target"]
        case .xcode: return ["simulators", "derived data", "archives"]
        case .android: return ["gradle", "emulator"]
        case .javascript: return ["npm", "node", "yarn", "pnpm"]
        case .docker: return ["images", "containers"]
        case .apps: return ["uninstall", "delete app", "leftovers"]
        case .cleanup: return ["trash", "free space"]
        case .history: return ["past cleanups", "put back", "undo"]
        }
    }

    /// The screen that lists items of `category` in `mode`: the first sidebar screen showing that
    /// category, or Large files, which shows everything.
    static func screen(for category: Category, mode: Mode) -> SidebarItem {
        for screen in sections(for: mode).flatMap({ $0 }) {
            if case .categories(let categories)? = screen.itemFilter, categories.contains(category) {
                return screen
            }
        }
        return .largeFiles
    }
}

/// Fuzzy text matching for the palette: the query's letters must appear in order, not next to each
/// other. Matches at the start of words and runs of letters score higher. Ignores case and accents.
enum FuzzyMatch {
    /// A score for `query` in `candidate`, higher is better, or `nil` if it doesn't match.
    static func score(_ query: String, in candidate: String) -> Int? {
        let fullQuery = normalize(query).trimmingCharacters(in: .whitespaces)
        let needle = Array(fullQuery.filter { !$0.isWhitespace })
        let fullCandidate = normalize(candidate)
        let haystack = Array(fullCandidate)
        guard !needle.isEmpty else { return 0 }
        guard needle.count <= haystack.count else { return nil }

        // Quick check first: every letter must appear, in order.
        var next = 0
        for letter in needle {
            guard let found = firstIndex(of: letter, in: haystack, from: next) else { return nil }
            next = found + 1
        }
        guard var score = bestAlignment(of: needle, in: haystack) else { return nil }
        if fullCandidate.hasPrefix(fullQuery) {
            score += 15
        } else if fullCandidate.contains(fullQuery) {
            score += 8
        }
        return score
    }

    /// The best score over every way of placing the query's letters, in order, in the candidate. Each
    /// matched letter scores 1, plus 8 at the very start, 6 at the start of a word, and 5 right after
    /// the previous matched letter. Each letter skipped costs 1, at most 4 per gap.
    private static func bestAlignment(of needle: [Character], in haystack: [Character]) -> Int? {
        let count = haystack.count
        let bonus: [Int] = (0..<count).map { index in
            if index == 0 { return 8 }
            return isWordBoundary(haystack[index - 1]) ? 6 : 0
        }
        // previous[j]: the best score with the last letter placed so far at j, or nil if it can't be.
        var previous: [Int?] = (0..<count).map { index in
            haystack[index] == needle[0] ? 1 + bonus[index] - min(index, 4) : nil
        }
        for letter in needle.dropFirst() {
            var current = [Int?](repeating: nil, count: count)
            // The best of previous[k] for every k at least 5 back, where the gap cost is capped at 4.
            var bestFar: Int?
            for index in 0..<count {
                if index >= 5, let far = previous[index - 5] { bestFar = max(bestFar ?? far, far) }
                guard haystack[index] == letter else { continue }
                var best: Int?
                if index >= 1, let adjacent = previous[index - 1] { best = adjacent + 5 }
                for gap in 1...3 where index - 1 - gap >= 0 {
                    if let near = previous[index - 1 - gap] { best = max(best ?? near - gap, near - gap) }
                }
                if let far = bestFar { best = max(best ?? far - 4, far - 4) }
                if let best { current[index] = best + 1 + bonus[index] }
            }
            previous = current
        }
        return previous.compactMap { $0 }.max()
    }

    private static func normalize(_ text: String) -> String {
        text.folding(options: [.caseInsensitive, .diacriticInsensitive, .widthInsensitive], locale: nil).lowercased()
    }

    private static func firstIndex(of letter: Character, in haystack: [Character], from start: Int) -> Int? {
        var index = start
        while index < haystack.count {
            if haystack[index] == letter { return index }
            index += 1
        }
        return nil
    }

    private static func isWordBoundary(_ character: Character) -> Bool {
        character.isWhitespace || character.isPunctuation || character == "_" || character == "-"
    }
}

/// The palette's state: whether it is open, what is typed, which row is highlighted, and the scan's
/// items to search. Picking an entry closes the palette and asks `AppModel` to do it.
@MainActor
final class CommandPaletteModel: ObservableObject {
    @Published private(set) var isPresented = false
    @Published var query = "" {
        didSet { if query != oldValue { highlightedIndex = 0 } }
    }
    /// The highlighted row in `results`.
    @Published private(set) var highlightedIndex = 0
    /// Every item of the finished scan, loaded when the palette opens.
    @Published private(set) var items: [ItemSummary] = []

    /// `unowned` because the app model keeps the palette.
    unowned let app: AppModel
    private var itemsTask: Task<Void, Never>?

    init(app: AppModel) {
        self.app = app
    }

    /// The palette doesn't open over the cleanup review or the Full Disk Access screen.
    var canOpen: Bool {
        app.cleanupState == .closed && !app.showsAccessOnboarding
    }

    var context: PaletteContext {
        PaletteContext(
            mode: app.mode,
            isScanning: app.isScanning,
            hasResults: app.overview != nil,
            checkedCount: app.checkedItemIDs.count
        )
    }

    /// The entries matching what is typed, best first.
    var results: [PaletteCommand] {
        // Items are left out while scanning: they belong to the scan being replaced.
        let searchable = app.overview != nil && !app.isScanning ? items : []
        return PaletteCommand.ranked(PaletteCommand.all(for: context, items: searchable), query: query)
    }

    func open() {
        guard canOpen else { return }
        query = ""
        highlightedIndex = 0
        isPresented = true
        loadItems()
    }

    func close() {
        guard isPresented else { return }
        isPresented = false
        itemsTask?.cancel()
        itemsTask = nil
    }

    /// ⌘K opens the palette, and closes it again.
    func toggle() {
        if isPresented { close() } else { open() }
    }

    /// Moves the highlight up (negative) or down, staying within the results.
    func moveHighlight(by offset: Int) {
        let count = results.count
        guard count > 0 else {
            highlightedIndex = 0
            return
        }
        highlightedIndex = min(max(clampedHighlight(count: count) + offset, 0), count - 1)
    }

    /// The highlighted row, kept inside a list of `count` results.
    func clampedHighlight(count: Int) -> Int {
        count == 0 ? 0 : min(highlightedIndex, count - 1)
    }

    /// Runs the highlighted entry (Return).
    func runHighlighted(openSettings: () -> Void) {
        let results = self.results
        guard !results.isEmpty else { return }
        run(results[clampedHighlight(count: results.count)], openSettings: openSettings)
    }

    /// Closes the palette and does what `command` says. `openSettings` opens the Settings window; the
    /// view passes it in because SwiftUI only offers that action inside views.
    func run(_ command: PaletteCommand, openSettings: () -> Void) {
        close()
        switch command.action {
        case .goTo(let screen):
            app.selection = screen
        case .showItem(let id, let screen):
            // Make sure the item's row is in the list: no search text, every tier.
            app.searchText = ""
            if app.selection != screen { app.selection = screen }
            app.tierFilter = nil
            app.selectedItemID = id
        case .scan:
            app.startScan()
        case .stopScan:
            app.cancelScan()
        case .reviewCleanup:
            // The review sheet belongs to the item lists, so show one first. Large files lists
            // everything, so every ticked row is in it.
            if app.selection?.itemFilter == nil { app.selection = .largeFiles }
            app.reviewCleanup()
        case .switchMode(let mode):
            app.mode = mode
        case .openSettings:
            openSettings()
        case .showFullDiskAccess:
            app.showAccessOnboarding()
        }
    }

    /// Waits for the scan's items to load. Used by tests.
    func waitForItems() async {
        await itemsTask?.value
    }

    private func loadItems() {
        itemsTask?.cancel()
        let app = self.app
        itemsTask = Task { [weak self] in
            let found = await app.scannedItems()
            guard !Task.isCancelled, let self else { return }
            self.items = found
        }
    }
}
