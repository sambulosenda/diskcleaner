import SwiftUI

/// The Overview screen: picks the right state, then shows the scan's results.
struct OverviewScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        switch model.scanState {
        case .neverScanned:
            EmptyStateView()
        case .scanning(let progress):
            ScanningView(progress: progress)
        case .failed(let message):
            ErrorStateView(message: message)
        case .loaded(let overview, let scannedAt):
            OverviewContent(overview: overview, scannedAt: scannedAt)
        }
    }
}

/// The results of a finished scan: one answer, the disk as one bar, what's ready to clean, what's
/// worth a look, and a footer. Everything else sits behind a link or a disclosure, so the screen reads
/// in a few seconds. Nothing is ticked here: the buttons open the item list, where the user chooses.
struct OverviewContent: View {
    @EnvironmentObject private var model: AppModel
    let overview: Overview
    let scannedAt: Date
    @State private var showingChanges = false
    /// The widest the Overview's column grows, padding included.
    static let columnWidth: CGFloat = 1080

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                OverviewHeadline(overview: overview)
                DiskBar(overview: overview)
                if overview.likelySafeBytes > 0 {
                    ReadyToCleanCard(overview: overview)
                }
                WorthALookList(overview: overview, items: model.worthReviewing(in: overview),
                               searchText: model.searchText)
                DiskMapSection(overview: overview)
                VStack(alignment: .leading, spacing: 16) {
                    Divider()
                    OverviewFooter(scannedAt: scannedAt, unreadableCount: overview.unreadableCount,
                                   quickRescan: overview.quickRescan,
                                   unscanned: model.shouldOfferAccess(for: overview) ? overview : nil,
                                   hasChanges: model.scanChanges != nil, showingChanges: $showingChanges)
                    if showingChanges, let changes = model.scanChanges {
                        ChangesSinceLastScan(changes: changes)
                    }
                }
            }
            .padding(.horizontal, 36)
            .padding(.vertical, 32)
            // One readable column, centred, so a wide window gets even margins instead of an empty right side.
            .frame(maxWidth: Self.columnWidth)
            .frame(maxWidth: .infinity)
        }
    }
}

/// "You can likely free 34 GB", then free space now and after cleaning on one quiet line.
struct OverviewHeadline: View {
    let overview: Overview

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(Self.headline(for: overview))
                .font(.largeTitle.weight(.bold))
                .accessibilityAddTraits(.isHeader)
            Text(Self.freeLine(for: overview))
                .font(.title3)
                .foregroundStyle(.secondary)
                .help("After cleaning counts only Likely safe items. Items to check first aren't counted.")
        }
    }

    static func headline(for overview: Overview) -> String {
        overview.likelySafeBytes > 0
            ? "You can likely free \(Format.bytes(overview.likelySafeBytes))"
            : "Nothing is clearly safe to remove right now"
    }

    /// "20.4 GB free now · 24.4 GB after cleaning", or just the free space when nothing is likely safe.
    static func freeLine(for overview: Overview) -> String {
        let now = "\(Format.bytes(overview.diskFree)) free now"
        guard overview.likelySafeBytes > 0 else { return now }
        return "\(now) · \(Format.bytes(freeAfter(overview))) after cleaning"
    }

    /// Free space if every likely safe item were removed. Never more than the disk.
    static func freeAfter(_ overview: Overview) -> UInt64 {
        let (sum, overflow) = overview.diskFree.addingReportingOverflow(overview.likelySafeBytes)
        return overflow ? overview.diskTotal : min(sum, overview.diskTotal)
    }
}

/// The disk as one thin bar, the three biggest parts named underneath, and the full breakdown behind
/// a disclosure.
struct DiskBar: View {
    let overview: Overview
    @State private var showingBreakdown = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            CategoryStrip(overview: overview)
            HStack(spacing: 18) {
                ForEach(Self.labelled(overview)) { part in
                    HStack(spacing: 6) {
                        Circle()
                            .fill(part.kind == .outsideHome ? Color.secondary.opacity(0.4) : part.color)
                            .frame(width: 8, height: 8)
                            .accessibilityHidden(true)
                        Text("\(part.title) \(Format.bytes(part.bytes))")
                    }
                    .help(part.help ?? "")
                }
                Spacer(minLength: 12)
                Button(showingBreakdown ? "Hide breakdown" : "Show breakdown") {
                    withAnimation(.easeInOut(duration: 0.2)) { showingBreakdown.toggle() }
                }
                .buttonStyle(.link)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            if showingBreakdown {
                CategoryLegend(overview: overview)
                    .padding(.top, 6)
            }
        }
    }

    /// The three biggest parts of used space, biggest first.
    static func labelled(_ overview: Overview) -> [DiskPart] {
        Array(DiskPart.parts(for: overview).filter { $0.kind != .free }.sorted { $0.bytes > $1.bytes }.prefix(3))
    }
}

/// "Ready to clean": the Likely safe total by category, and one button that opens those items. Nothing
/// is ticked for the user: the list opens showing only Likely safe items, and they choose there.
struct ReadyToCleanCard: View {
    @EnvironmentObject private var model: AppModel
    let overview: Overview

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .center, spacing: 16) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Ready to clean \u{00B7} \(Format.bytes(overview.likelySafeBytes))")
                        .font(.headline)
                        .accessibilityAddTraits(.isHeader)
                    Text("These come back by themselves when they\u{2019}re needed. You choose what goes, and it moves to the Trash.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 12)
                Button("Review and clean\u{2026}") { model.showItems(tier: .likelySafe) }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .accessibilityHint("Opens the Likely safe items. Nothing is chosen for you.")
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 200), spacing: 10)], spacing: 10) {
                ForEach(Self.tiles(overview), id: \.category) { entry in
                    HStack(spacing: 10) {
                        CategoryIcon(category: entry.category)
                        Text(entry.category.title)
                        Spacer(minLength: 8)
                        Text(Format.bytes(entry.likelySafeBytes))
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 10)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color(nsColor: .controlBackgroundColor)))
                    .accessibilityElement(children: .combine)
                }
            }
        }
        .padding(20)
        .background(RoundedRectangle(cornerRadius: 14).fill(Color.secondary.opacity(0.08)))
    }

    /// Categories with Likely safe space, most first.
    static func tiles(_ overview: Overview) -> [CategoryTotal] {
        overview.categories.filter { $0.likelySafeBytes > 0 }.sorted { $0.likelySafeBytes > $1.likelySafeBytes }
    }
}

/// Rows that share a title, such as several projects' `node_modules`, shown as one row.
struct ItemGroup: Identifiable, Equatable {
    let items: [ItemSummary]

    var id: String { items[0].id }
    var title: String { items[0].title }
    var category: Category { items[0].category }
    var bytes: UInt64 { items.reduce(0) { $0 &+ $1.sizeBytes } }
    /// "· shop" for one item with a name, "· 3 items" for a group.
    var detail: String? {
        items.count > 1 ? "\(items.count) items" : items[0].place
    }

    /// Groups items by title, in order of their biggest member, biggest group first.
    static func groups(_ items: [ItemSummary]) -> [ItemGroup] {
        var order: [String] = []
        var byTitle: [String: [ItemSummary]] = [:]
        for item in items {
            if byTitle[item.title] == nil { order.append(item.title) }
            byTitle[item.title, default: []].append(item)
        }
        return order.compactMap { byTitle[$0].map(ItemGroup.init) }.sorted { $0.bytes > $1.bytes }
    }
}

/// "Worth a look": the biggest Review first items as plain one-line rows. Each opens the item list,
/// showing only Review first items, with that item selected. Nothing is ticked.
struct WorthALookList: View {
    @EnvironmentObject private var model: AppModel
    let overview: Overview
    let items: [ItemSummary]
    let searchText: String
    /// How many rows to show.
    static let rowLimit = 4

    var body: some View {
        let groups = Array(ItemGroup.groups(items.filter { $0.tier == .reviewFirst }).prefix(Self.rowLimit))
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text("Worth a look")
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)
                if overview.reviewFirstBytes > 0 {
                    Text("+\(Format.bytes(overview.reviewFirstBytes)) to check first")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Show all") { model.showItems(tier: .reviewFirst) }
                    .buttonStyle(.link)
                    .accessibilityHint("Opens every item to check first.")
            }
            if groups.isEmpty {
                Text(emptyText)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 4)
            } else {
                VStack(spacing: 0) {
                    ForEach(groups) { group in
                        WorthALookRow(group: group)
                        if group.id != groups.last?.id {
                            Divider().padding(.leading, 48)
                        }
                    }
                }
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color(nsColor: .separatorColor)))
            }
        }
    }

    private var emptyText: String {
        searchText.isEmpty ? "Nothing else here takes much space." : "No items match \u{201C}\(searchText)\u{201D}."
    }
}

/// Icon, title, distinguishing name or count, size and a chevron. The explanation is the tooltip and
/// the VoiceOver hint, so the row stays one line.
struct WorthALookRow: View {
    @EnvironmentObject private var model: AppModel
    let group: ItemGroup

    var body: some View {
        Button {
            model.showItems(tier: .reviewFirst, select: group.id)
        } label: {
            HStack(spacing: 12) {
                CategoryIcon(category: group.category)
                HStack(spacing: 4) {
                    Text(group.title)
                    if let detail = group.detail {
                        Text("\u{00B7} \(detail)")
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
                Spacer(minLength: 12)
                Text(Format.bytes(group.bytes))
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(group.items[0].explain)
        .accessibilityLabel(accessibleLabel)
        .accessibilityHint(group.items[0].explain)
    }

    private var accessibleLabel: String {
        let name = group.detail.map { "\(group.title), \($0)" } ?? group.title
        return "\(name), \(Format.bytes(group.bytes)), check first"
    }
}

/// The category's symbol in the category's colour, so a row matches its part of the strip.
struct CategoryIcon: View {
    let category: Category

    var body: some View {
        Image(systemName: category.symbol)
            .font(.title3)
            .foregroundStyle(category.color)
            .frame(width: 24)
            .accessibilityHidden(true)
    }
}

/// The item's title, then the name that tells it apart from items with the same title, such as the
/// project a `node_modules` folder belongs to. The core sends that name in Developer mode only.
struct ItemTitle: View {
    let item: ItemSummary

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(item.title)
                .font(.body.weight(.medium))
                .layoutPriority(1)
            if let place = item.place {
                Text(place)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 1)
                    .background(Capsule().fill(Color.secondary.opacity(0.12)))
            }
        }
    }
}

extension ItemSummary {
    /// The title with its distinguishing name, for VoiceOver and checkbox labels.
    var accessibleTitle: String {
        guard let place else { return title }
        return "\(title), \(place)"
    }
}

/// The tier as words with a matching symbol. Colour only backs up the text.
struct TierLabel: View {
    let tier: Tier

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: tier.symbol)
                .foregroundStyle(tier.tint)
                .accessibilityHidden(true)
            Text(tier.label)
                .foregroundStyle(.primary)
        }
        .font(.caption.weight(.medium))
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(Capsule().fill(tier.tint.opacity(0.14)))
        .accessibilityElement(children: .combine)
    }
}

/// One line: what changed since the last scan (opens below), last scan time, and folders not measured.
struct OverviewFooter: View {
    let scannedAt: Date
    let unreadableCount: UInt64
    var quickRescan = false
    /// When set, "Why?" lists the private areas Roomy couldn't read, with a way to turn on Full Disk
    /// Access, instead of a card on the Overview.
    var unscanned: Overview?
    var hasChanges = false
    @Binding var showingChanges: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            if hasChanges {
                Button(showingChanges ? "Hide changes since your last scan" : "What changed since your last scan") {
                    withAnimation(.easeInOut(duration: 0.2)) { showingChanges.toggle() }
                }
                .buttonStyle(.link)
            }
            Spacer(minLength: 12)
            Text("Scanned \(scannedAt.formatted(date: .abbreviated, time: .shortened))")
                .foregroundStyle(.secondary)
                .help(quickRescan ? "Roomy measured again only the folders that changed since the scan before." : "")
            UnmeasuredNote(unreadableCount: unreadableCount, unscanned: unscanned)
        }
        .font(.footnote)
    }
}

/// "212 folders not measured", with a link that explains Full Disk Access and opens System Settings.
struct UnmeasuredNote: View {
    @EnvironmentObject private var model: AppModel
    let unreadableCount: UInt64
    /// When set, the popover lists the areas Roomy couldn't read and offers Full Disk Access.
    var unscanned: Overview? = nil
    @State private var showingExplanation = false

    /// "1 folder not measured" or "212 folders not measured".
    static func text(for count: UInt64) -> String {
        count == 1 ? "1 folder not measured" : "\(Format.count(count)) folders not measured"
    }

    var body: some View {
        if unreadableCount == 0 {
            Text("Roomy could open every folder.")
                .foregroundStyle(.secondary)
        } else {
            HStack(spacing: 6) {
                Text(Self.text(for: unreadableCount))
                    .foregroundStyle(.secondary)
                Button("Why?") { showingExplanation = true }
                    .buttonStyle(.link)
                    .accessibilityLabel("Why some folders weren't measured")
                    .popover(isPresented: $showingExplanation, arrowEdge: .bottom) {
                        if let unscanned {
                            UnscannedAreasBox(overview: unscanned, inPopover: true)
                                .environmentObject(model)
                                .frame(width: 400)
                        } else {
                            FullDiskAccessExplanation()
                        }
                    }
            }
        }
    }
}

/// What Full Disk Access is and how to turn it on.
struct FullDiskAccessExplanation: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Some folders weren't measured")
                .font(.headline)
            Text("Roomy couldn't open some folders. Most often that's because macOS keeps them private, such as Mail, Messages and Safari data.")
                .fixedSize(horizontal: false, vertical: true)
            Text("Full Disk Access lets Roomy measure the folders macOS protects. Roomy only reads sizes, and nothing leaves this Mac. Saying no is fine: the rest of the results are still right.")
                .fixedSize(horizontal: false, vertical: true)
            Text("To turn it on, open System Settings, go to Privacy & Security, then Full Disk Access, and switch on Roomy (or add it with the + button). Then scan again.")
                .fixedSize(horizontal: false, vertical: true)
            if let url = FullDiskAccessSettings.url {
                Link("Open Full Disk Access settings", destination: url)
                    .accessibilityHint("Opens System Settings.")
            }
        }
        .font(.callout)
        .padding(16)
        .frame(width: 360)
    }
}

/// "Since your last scan": how much the home folder grew or shrank, and the items that changed most.
struct ChangesSinceLastScan: View {
    let changes: ScanChanges
    /// How many changed items to show.
    static let rowLimit = 5

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Since your last scan")
                .font(.title3.weight(.semibold))
                .accessibilityAddTraits(.isHeader)
            Text(Self.summary(for: changes))
                .foregroundStyle(.secondary)
            if changes.items.isEmpty {
                Text("Nothing changed by more than 10 MB.")
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 4)
            } else {
                let rows = Array(changes.items.prefix(Self.rowLimit))
                VStack(spacing: 0) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { index, change in
                        ChangeRow(change: change)
                        if index < rows.count - 1 {
                            Divider().padding(.leading, 48)
                        }
                    }
                }
                .background(
                    RoundedRectangle(cornerRadius: 10)
                        .fill(Color(nsColor: .controlBackgroundColor))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(Color(nsColor: .separatorColor))
                )
            }
        }
    }

    /// "On 5 Nov 2026 at 14:02 your home folder took 61 GB. It's 4.2 GB bigger now."
    static func summary(for changes: ScanChanges) -> String {
        let then = Date(timeIntervalSince1970: TimeInterval(changes.previousScanAt))
        let when = then.formatted(date: .abbreviated, time: .shortened)
        let before = changes.scannedBytesBefore, now = changes.scannedBytesNow
        let start = "At your scan on \(when), your home folder took \(Format.bytes(before))."
        let change = before > now ? before - now : now - before
        if change < 10_000_000 {
            return "\(start) It's about the same size now."
        }
        let difference = Format.bytes(change)
        return now > before ? "\(start) It's \(difference) bigger now." : "\(start) It's \(difference) smaller now."
    }
}

/// One item that changed: what it is, how it changed, and by how much.
struct ChangeRow: View {
    let change: ItemChange

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            CategoryIcon(category: change.category)
            VStack(alignment: .leading, spacing: 2) {
                Text(change.title)
                    .font(.body.weight(.medium))
                Text(change.kind.label)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 12)
            Text(Self.difference(for: change))
                .font(.body.monospacedDigit())
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(change.title), \(change.kind.label), \(Self.difference(for: change))")
    }

    /// "+4.2 GB" or "−1.1 GB".
    static func difference(for change: ItemChange) -> String {
        let (before, now) = (change.bytesBefore, change.bytesNow)
        return now >= before ? "+\(Format.bytes(now - before))" : "\u{2212}\(Format.bytes(before - now))"
    }
}

extension ChangeKind {
    /// Plain words for how an item changed.
    var label: String {
        switch self {
        case .new: return "New since your last scan"
        case .grew: return "Bigger than at your last scan"
        case .shrank: return "Smaller than at your last scan"
        case .gone: return "No longer there"
        }
    }
}
