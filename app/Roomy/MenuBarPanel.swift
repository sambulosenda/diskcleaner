import AppKit
import SwiftUI

// The panel that opens from the menu bar item: free space, a gauge, what the last scan found grouped by
// kind, and a button that opens the usual cleanup review in Roomy's main window.
//
// Safety: the panel never cleans anything. Ticking a row only ticks it in Roomy, exactly as the
// Overview's checkboxes do. "Review" opens the same dry-run review sheet, where the core checks every
// item again and nothing moves until the user confirms. Rows the extra checks would hold back (an app
// is open, it changed in the last few minutes, a folder you keep) show a lock and the core's reason,
// and can't be ticked here. There is no "tick the whole group" control: every tick is one row the user
// chose on its own, which is what the core assumes.

// MARK: - What the panel shows (pure, so tests can check it without a window)

/// One kind of item in the "Ready to free" list, such as Caches, with its largest items.
struct ReadyGroup: Equatable, Identifiable {
    let category: Category
    /// Sum of every item's size in this kind, shown or not.
    let totalBytes: UInt64
    /// The items shown when the group is open, largest first.
    let items: [ItemSummary]
    /// How many more items Roomy's main window lists for this kind.
    let hiddenCount: Int

    var id: Category { category }

    /// "3 items": every item of this kind, shown or not.
    var countText: String {
        let all = items.count + hiddenCount
        return Format.items(UInt64(all))
    }
}

/// How much of the gauge each part fills, as fractions of the disk that add up to 1.
struct GaugeParts: Equatable {
    /// Used by everything Roomy doesn't offer.
    let other: Double
    /// Items marked Review first.
    let reviewFirst: Double
    /// Items marked Likely safe.
    let likelySafe: Double
    /// Free now.
    let free: Double
}

enum MenuBarPanelContent {
    /// Most rows shown per kind. The rest are in Roomy's main window.
    static let rowsPerGroup = 8

    /// The "Ready to free" groups: every item that isn't `keep`, by kind, largest kind first. Items in a
    /// group are largest first, ties by title so the order is stable.
    static func groups(from items: [ItemSummary], rowsPerGroup: Int = rowsPerGroup) -> [ReadyGroup] {
        let offered = items.filter { $0.tier != .keep }
        let byCategory = Dictionary(grouping: offered, by: \.category)
        let groups = byCategory.map { category, members -> ReadyGroup in
            let sorted = members.sorted {
                $0.sizeBytes != $1.sizeBytes ? $0.sizeBytes > $1.sizeBytes : $0.title < $1.title
            }
            let shown = Array(sorted.prefix(max(rowsPerGroup, 0)))
            return ReadyGroup(
                category: category,
                totalBytes: members.reduce(0) { $0 &+ $1.sizeBytes },
                items: shown,
                hiddenCount: sorted.count - shown.count
            )
        }
        return groups.sorted {
            $0.totalBytes != $1.totalBytes ? $0.totalBytes > $1.totalBytes : $0.category.title < $1.category.title
        }
    }

    /// The gauge for a disk of `total` bytes with `free` left. The two tiers are taken out of the used
    /// part and never make it go past the disk, even if the scan and the free space reading disagree.
    static func gauge(total: UInt64, free: UInt64, likelySafe: UInt64, reviewFirst: UInt64) -> GaugeParts {
        guard total > 0 else { return GaugeParts(other: 0, reviewFirst: 0, likelySafe: 0, free: 1) }
        let freeBytes = min(free, total)
        let used = total - freeBytes
        let safe = min(likelySafe, used)
        let review = min(reviewFirst, used - safe)
        let other = used - safe - review
        let t = Double(total)
        return GaugeParts(other: Double(other) / t, reviewFirst: Double(review) / t,
                          likelySafe: Double(safe) / t, free: Double(freeBytes) / t)
    }

    /// Why each item the core checked would not move, by item id. Items that would move are left out.
    static func lockReasons(from rows: [CleanupPreviewItem]) -> [String: String] {
        var reasons: [String: String] = [:]
        for row in rows {
            if case let .wontMove(reason) = row.check {
                reasons[row.itemId] = reason
            }
        }
        return reasons
    }

    /// "12 GB ready" beside free space: the Likely safe total, or `nil` when there is none.
    static func readyText(for overview: Overview) -> String? {
        overview.likelySafeBytes > 0 ? "\(Format.bytes(overview.likelySafeBytes)) ready" : nil
    }

    /// The line under the headline about Review first items, or `nil` when there are none.
    static func reviewText(for overview: Overview) -> String? {
        overview.reviewFirstBytes > 0 ? "\(Format.bytes(overview.reviewFirstBytes)) more to look at first" : nil
    }

    /// The line under an item's name: its tier, and when it last changed if the scan knows, such as
    /// "Likely safe · changed 3 days ago".
    static func rowDetail(for item: ItemSummary, now: Date = Date()) -> String {
        guard let seconds = item.lastChangedUnixSeconds else { return item.tier.label }
        let changed = Date(timeIntervalSince1970: TimeInterval(seconds))
        let when = now.timeIntervalSince(changed) < 60 ? "just now" : Format.relative(changed, now: now)
        return "\(item.tier.label) \u{00B7} changed \(when)"
    }

    /// The main button's title for `count` ticked rows.
    static func reviewButtonTitle(count: Int) -> String {
        switch count {
        case 0: return "Open Roomy"
        case 1: return "Review 1 item\u{2026}"
        default: return "Review \(count) items\u{2026}"
        }
    }

    /// The footer's left side, such as "Scanned 2 hours ago".
    static func footerText(for state: AppModel.ScanState, now: Date = Date()) -> String {
        switch state {
        case .neverScanned: return "Not scanned yet"
        case .scanning: return "Scanning\u{2026}"
        case .failed: return "The last scan didn\u{2019}t finish"
        case let .loaded(_, scannedAt):
            let seconds = now.timeIntervalSince(scannedAt)
            return seconds < 60 ? "Scanned just now" : "Scanned \(Format.relative(scannedAt, now: now))"
        }
    }
}

// MARK: - The panel's state

/// What the panel has loaded: the scan's items, which groups are open, and which rows are locked.
/// Asks the core only to read; it never makes a plan.
@MainActor
final class MenuBarPanelModel: ObservableObject {
    enum ItemsState: Equatable {
        case idle
        case loading
        case loaded([ReadyGroup])
    }

    @Published private(set) var itemsState: ItemsState = .idle
    @Published private(set) var expanded: Set<Category> = []
    /// Why a row can't be cleaned now, by item id, from the core's last check.
    @Published private(set) var locks: [String: String] = [:]
    /// Groups whose rows the core is checking now.
    @Published private(set) var checking: Set<Category> = []
    /// Groups the core couldn't check. Their rows stay tickable; the review checks them again anyway.
    @Published private(set) var checkFailed: Set<Category> = []

    /// The scan the items came from, so a new scan loads them again.
    private var loadedScan: Date?

    /// Loads the items for the finished scan, if they aren't loaded yet, and checks the open groups
    /// again, since an app may have opened or closed since the panel was last open.
    func panelDidOpen(model: AppModel) async {
        // Roomy may have opened without its window: scan now so the panel isn't empty. Only measures.
        model.scanIfOpening()
        guard case let .loaded(_, scannedAt) = model.scanState else {
            itemsState = .idle
            loadedScan = nil
            locks = [:]
            return
        }
        if loadedScan != scannedAt {
            loadedScan = scannedAt
            itemsState = .loading
            locks = [:]
            checkFailed = []
            let items = await model.scannedItems()
            guard loadedScan == scannedAt else { return }
            itemsState = .loaded(MenuBarPanelContent.groups(from: items))
        }
        for category in expanded {
            await check(category, model: model)
        }
    }

    /// Opens or closes a group. Opening one asks the core which of its rows are locked.
    func toggle(_ category: Category, model: AppModel) async {
        if expanded.contains(category) {
            expanded.remove(category)
            return
        }
        expanded.insert(category)
        await check(category, model: model)
    }

    /// Asks the core about one group's shown rows. Reads only.
    private func check(_ category: Category, model: AppModel) async {
        guard case let .loaded(groups) = itemsState,
              let group = groups.first(where: { $0.category == category }),
              !group.items.isEmpty, !checking.contains(category) else { return }
        checking.insert(category)
        defer { checking.remove(category) }
        do {
            let rows = try await model.checkItems(group.items.map(\.id))
            let reasons = MenuBarPanelContent.lockReasons(from: rows)
            for item in group.items {
                locks[item.id] = reasons[item.id]
            }
            checkFailed.remove(category)
        } catch {
            checkFailed.insert(category)
        }
    }
}

// MARK: - Views

/// The menu bar panel.
struct MenuBarPanel: View {
    @ObservedObject var monitor: LowSpaceMonitor
    @ObservedObject var model: AppModel
    @ObservedObject var weeklyNote: WeeklyNoteModel
    @StateObject private var panel = MenuBarPanelModel()

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            PanelHeader(monitor: monitor)
            Divider()
            if let note = weeklyNote.note {
                // Opens Roomy's Overview only. Nothing is ticked or removed from here.
                WeeklyNoteRow(text: note, review: {
                    weeklyNote.answer()
                    monitor.showRoomy()
                }, notNow: { weeklyNote.answer() })
                Divider()
            }
            FreeSpaceSection(monitor: monitor, overview: model.overview)
                .padding(16)
            Divider()
            readySection
            Divider()
            PanelFooter(monitor: monitor, model: model)
        }
        .frame(width: 360)
        // Loads again when a scan finishes, and checks the open groups again each time the panel opens.
        .task(id: scanKey) { await panel.panelDidOpen(model: model) }
        .onAppear {
            weeklyNote.refresh()
            Task { await panel.panelDidOpen(model: model) }
        }
    }

    /// Changes when a scan starts or finishes, so the panel loads again.
    private var scanKey: String {
        switch model.scanState {
        case .neverScanned: return "never"
        case .scanning: return "scanning"
        case .failed: return "failed"
        case let .loaded(_, scannedAt): return "loaded-\(scannedAt.timeIntervalSince1970)"
        }
    }

    @ViewBuilder
    private var readySection: some View {
        switch model.scanState {
        case .neverScanned:
            PanelMessage(symbol: "magnifyingglass", text: "Scan to see what you can free. Scanning only measures; nothing is removed.")
        case .scanning:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Scanning\u{2026}").foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity)
            .padding(24)
        case let .failed(message):
            PanelMessage(symbol: "exclamationmark.circle", text: message)
        case .loaded:
            ReadyList(openRoomy: { monitor.showRoomy() }, panel: panel, model: model)
        }
    }
}

/// The disk's name, and a menu with Roomy's other commands.
private struct PanelHeader: View {
    @ObservedObject var monitor: LowSpaceMonitor

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "internaldrive")
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            Text(Self.diskName)
                .font(.headline)
            Spacer()
            Menu {
                Button("Open Roomy") { monitor.showRoomy() }
                Button("Check Free Space Again") { Task { await monitor.refresh() } }
                Divider()
                PanelSettingsButton()
                Button("Quit Roomy") { NSApp.terminate(nil) }
                    .keyboardShortcut("q", modifiers: .command)
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.button)
            .buttonStyle(.borderless)
            .menuIndicator(.hidden)
            .fixedSize()
            .accessibilityLabel("More")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    /// The startup disk's name as Finder shows it, such as "Macintosh HD". Reads the name only.
    static var diskName: String {
        let home = FileManager.default.homeDirectoryForCurrentUser
        let name = try? home.resourceValues(forKeys: [.volumeLocalizedNameKey]).volumeLocalizedName
        return name ?? "Startup disk"
    }
}

/// Settings… for macOS 14 and later, and the older way for macOS 13.
private struct PanelSettingsButton: View {
    var body: some View {
        if #available(macOS 14.0, *) {
            SettingsLink { Text("Settings\u{2026}") }
                .keyboardShortcut(",", modifiers: .command)
        } else {
            Button("Settings\u{2026}") {
                NSApp.activate(ignoringOtherApps: true)
                // macOS 13's name for the Settings window action.
                NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
            }
            .keyboardShortcut(",", modifiers: .command)
        }
    }
}

/// "42 GB free", "12 GB ready", the gauge and its key.
private struct FreeSpaceSection: View {
    @ObservedObject var monitor: LowSpaceMonitor
    let overview: Overview?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                freeText
                Spacer()
                if let overview, let ready = MenuBarPanelContent.readyText(for: overview) {
                    Text(ready)
                        .font(.title3.weight(.semibold).monospacedDigit())
                        .foregroundStyle(Tier.likelySafe.tint)
                        .help("Likely safe to remove, from the last scan.")
                }
            }
            if let low = monitor.lowSpaceText {
                Label(low, systemImage: "exclamationmark.triangle.fill")
                    .font(.callout)
                    .foregroundStyle(.orange)
            }
            if let parts {
                DiskGauge(parts: parts)
                GaugeKey(showsTiers: overview != nil)
            }
            if let overview, let review = MenuBarPanelContent.reviewText(for: overview) {
                Text(review)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    @ViewBuilder
    private var freeText: some View {
        switch monitor.reading {
        case let .read(free, _):
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(Format.bytes(free))
                    .font(.system(size: 30, weight: .semibold).monospacedDigit())
                Text("free")
                    .font(.title3)
                    .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
        default:
            Text(monitor.summaryText)
                .font(.title3)
                .foregroundStyle(.secondary)
        }
    }

    /// The gauge uses the live free-space reading; the tiers come from the last scan.
    private var parts: GaugeParts? {
        guard case let .read(free, total) = monitor.reading else { return nil }
        return MenuBarPanelContent.gauge(total: total, free: free,
                                         likelySafe: overview?.likelySafeBytes ?? 0,
                                         reviewFirst: overview?.reviewFirstBytes ?? 0)
    }
}

/// One rounded bar: what's used, the Review first and Likely safe parts of it, then free space.
struct DiskGauge: View {
    let parts: GaugeParts

    var body: some View {
        GeometryReader { geo in
            HStack(spacing: 2) {
                segment(parts.other, Color.primary.opacity(0.35), width: geo.size.width)
                segment(parts.reviewFirst, Tier.reviewFirst.tint, width: geo.size.width)
                segment(parts.likelySafe, Tier.likelySafe.tint, width: geo.size.width)
                Spacer(minLength: 0)
            }
            .frame(width: geo.size.width, alignment: .leading)
            .background(Color.primary.opacity(0.08))
            .clipShape(Capsule())
        }
        .frame(height: 10)
        .accessibilityElement()
        .accessibilityLabel(Self.description(parts))
    }

    @ViewBuilder
    private func segment(_ fraction: Double, _ color: Color, width: CGFloat) -> some View {
        if fraction > 0 {
            // At least 3 points, so a small part is still visible next to its key.
            color.frame(width: max(3, width * fraction))
        }
    }

    static func description(_ parts: GaugeParts) -> String {
        func pct(_ f: Double) -> String { "\(Int((f * 100).rounded())) percent" }
        return "Disk use. Likely safe \(pct(parts.likelySafe)), review first \(pct(parts.reviewFirst)), free \(pct(parts.free))."
    }
}

/// The gauge's key, so colour is never the only signal.
private struct GaugeKey: View {
    let showsTiers: Bool

    var body: some View {
        HStack(spacing: 12) {
            if showsTiers {
                key(Tier.likelySafe.tint, Tier.likelySafe.label)
                key(Tier.reviewFirst.tint, Tier.reviewFirst.label)
            }
            key(Color.primary.opacity(0.35), "Other")
            key(Color.primary.opacity(0.08), "Free")
        }
        .font(.caption2)
        .foregroundStyle(.secondary)
        .accessibilityHidden(true)
    }

    private func key(_ color: Color, _ text: String) -> some View {
        HStack(spacing: 4) {
            Circle().fill(color).frame(width: 7, height: 7)
            Text(text)
        }
    }
}

/// "Ready to free": the groups, each opening to its largest items.
private struct ReadyList: View {
    let openRoomy: () -> Void
    @ObservedObject var panel: MenuBarPanelModel
    @ObservedObject var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Ready to free")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.secondary)
                Spacer()
                if !model.checkedItemIDs.isEmpty {
                    Text("\(model.checkedItemIDs.count) chosen, \(Format.bytes(model.checkedBytes))")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 12)
            .padding(.bottom, 6)
            content
        }
    }

    @ViewBuilder
    private var content: some View {
        switch panel.itemsState {
        case .idle, .loading:
            ProgressView()
                .controlSize(.small)
                .frame(maxWidth: .infinity)
                .padding(20)
        case let .loaded(groups) where groups.isEmpty:
            PanelMessage(symbol: "checkmark.circle", text: "Nothing to free right now.")
        case let .loaded(groups):
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    ForEach(groups) { group in
                        GroupRow(group: group, openRoomy: openRoomy, panel: panel, model: model)
                    }
                }
                .padding(.horizontal, 8)
                .padding(.bottom, 8)
            }
            .frame(maxHeight: 340)
        }
    }
}

/// One kind of item, and its rows when open.
private struct GroupRow: View {
    let group: ReadyGroup
    let openRoomy: () -> Void
    @ObservedObject var panel: MenuBarPanelModel
    @ObservedObject var model: AppModel
    @State private var hovering = false

    private var isOpen: Bool { panel.expanded.contains(group.category) }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button {
                Task { await panel.toggle(group.category, model: model) }
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: group.category.symbol)
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.white)
                        .frame(width: 26, height: 26)
                        .background(RoundedRectangle(cornerRadius: 7).fill(group.category.color.gradient))
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(group.category.title)
                            .font(.body.weight(.medium))
                        Text(group.countText)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text(Format.bytes(group.totalBytes))
                        .font(.callout.monospacedDigit())
                        .foregroundStyle(.secondary)
                    Image(systemName: "chevron.right")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.tertiary)
                        .rotationEffect(.degrees(isOpen ? 90 : 0))
                        .accessibilityHidden(true)
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(hovering ? 0.06 : 0)))
            .onHover { hovering = $0 }
            .accessibilityLabel("\(group.category.title), \(group.countText), \(Format.bytes(group.totalBytes))")
            .accessibilityHint(isOpen ? "Hides its items." : "Shows its items.")

            if isOpen {
                VStack(alignment: .leading, spacing: 0) {
                    if panel.checkFailed.contains(group.category) {
                        Text("Roomy couldn\u{2019}t check these just now. The review checks them again.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .padding(.vertical, 4)
                    }
                    ForEach(group.items) { item in
                        PanelItemRow(item: item, lock: panel.locks[item.id],
                                     isChecking: panel.checking.contains(group.category), model: model)
                    }
                    if group.hiddenCount > 0 {
                        Button("\(group.hiddenCount) more in Roomy\u{2026}") {
                            // Cleanup lists every item the scan found.
                            model.selection = .cleanup
                            openRoomy()
                        }
                        .buttonStyle(.link)
                        .font(.caption)
                        .padding(.vertical, 4)
                    }
                }
                .padding(.leading, 44)
                .padding(.trailing, 8)
            }
        }
    }
}

/// One item: a checkbox, or a lock with the reason it can't be cleaned now.
private struct PanelItemRow: View {
    let item: ItemSummary
    let lock: String?
    let isChecking: Bool
    @ObservedObject var model: AppModel

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            leading
                .frame(width: 16, height: 16)
                .padding(.top, 1)
            VStack(alignment: .leading, spacing: 1) {
                Text(item.place.map { "\(item.title) \u{00B7} \($0)" } ?? item.title)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .foregroundStyle(lock == nil ? HierarchicalShapeStyle.primary : HierarchicalShapeStyle.secondary)
                Text(lock ?? MenuBarPanelContent.rowDetail(for: item))
                    .font(.caption)
                    .foregroundStyle(lock == nil && item.tier == .reviewFirst ? Tier.reviewFirst.tint : Color.secondary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            Text(Format.bytes(item.sizeBytes))
                .font(.callout.monospacedDigit())
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 5)
        .help(lock ?? item.explain)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(accessibilityText)
    }

    @ViewBuilder
    private var leading: some View {
        if let lock {
            Image(systemName: "lock.fill")
                .font(.caption)
                .foregroundStyle(.secondary)
                .accessibilityLabel("Locked. \(lock)")
        } else if isChecking {
            ProgressView().controlSize(.mini)
        } else {
            Toggle("Choose \(item.accessibleTitle)", isOn: Binding(
                get: { model.isChecked(item) },
                set: { model.setChecked($0, for: item) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
        }
    }

    private var accessibilityText: String {
        var text = "\(item.accessibleTitle), \(Format.bytes(item.sizeBytes)), \(item.tier.label)"
        if let lock { text += ". Can\u{2019}t be cleaned now: \(lock)" }
        return text
    }
}

/// A symbol and a short sentence, for empty and error states.
private struct PanelMessage: View {
    let symbol: String
    let text: String

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: symbol)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            Text(text)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .font(.callout)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
    }
}

/// When the scan ran, a button to scan again, and the button that opens the review in Roomy.
private struct PanelFooter: View {
    @ObservedObject var monitor: LowSpaceMonitor
    @ObservedObject var model: AppModel

    var body: some View {
        HStack(spacing: 8) {
            Text(MenuBarPanelContent.footerText(for: model.scanState))
                .font(.caption)
                .foregroundStyle(.secondary)
            Button {
                model.startScan()
            } label: {
                Image(systemName: "arrow.clockwise")
            }
            .buttonStyle(.borderless)
            .disabled(model.isScanning)
            .help("Scan again. Scanning only measures.")
            .accessibilityLabel("Scan again")
            Spacer()
            mainButton
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    @ViewBuilder
    private var mainButton: some View {
        switch model.scanState {
        case .neverScanned, .failed:
            // "Scan Now…" or "Scan Again…". Scanning only measures.
            Button(MenuBarScanSummary.make(for: model.scanState).buttonTitle) {
                model.selection = .overview
                monitor.showRoomy()
                model.startScan()
            }
            .buttonStyle(.borderedProminent)
        case .scanning:
            Button("Open Roomy") { monitor.showRoomy() }
        case .loaded:
            let count = model.checkedItemIDs.count
            Button(MenuBarPanelContent.reviewButtonTitle(count: count)) {
                if count > 0 {
                    // Opens the usual review sheet. Nothing moves until the user confirms there.
                    model.reviewFromMenuBar()
                }
                monitor.showRoomy()
            }
            .buttonStyle(.borderedProminent)
            .keyboardShortcut(.defaultAction)
            .help(count > 0 ? "Opens Roomy to check them. Nothing is removed until you confirm." : "")
        }
    }
}
