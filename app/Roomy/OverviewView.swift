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

/// The results of a finished scan.
struct OverviewContent: View {
    @EnvironmentObject private var model: AppModel
    let overview: Overview
    let scannedAt: Date

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                OverviewHeadline(overview: overview)
                VStack(alignment: .leading, spacing: 12) {
                    CategoryStrip(overview: overview)
                    CategoryLegend(overview: overview)
                }
                WorthReviewingList(items: model.worthReviewing(in: overview), searchText: model.searchText)
                if let changes = model.scanChanges {
                    ChangesSinceLastScan(changes: changes)
                }
                ReviewAndCleanButton()
                if model.shouldOfferAccess(for: overview) {
                    UnscannedAreasBox(overview: overview)
                }
                Divider()
                OverviewFooter(scannedAt: scannedAt, unreadableCount: overview.unreadableCount,
                               quickRescan: overview.quickRescan)
            }
            .padding(28)
            .frame(maxWidth: 900, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .sheet(isPresented: Binding(
            get: { model.cleanupState != .closed },
            set: { if !$0 { model.closeCleanupReview() } }
        )) {
            CleanupReviewSheet()
                .environmentObject(model)
        }
    }
}

/// "You can likely free 34 GB", what more is worth reviewing, and free space now and after cleaning.
struct OverviewHeadline: View {
    let overview: Overview

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 6) {
                Text(Self.headline(for: overview))
                    .font(.largeTitle.weight(.semibold))
                    .accessibilityAddTraits(.isHeader)
                if let line = Self.reviewLine(for: overview) {
                    Text(line)
                        .font(.title3)
                        .foregroundStyle(.secondary)
                }
            }
            HStack(spacing: 40) {
                FreeSpaceFigure(caption: "Free now", bytes: overview.diskFree)
                FreeSpaceFigure(caption: "Free after cleaning", bytes: freeAfter)
                    .help("Free space once every Likely safe item has gone. Review first items aren't counted.")
            }
        }
    }

    static func headline(for overview: Overview) -> String {
        overview.likelySafeBytes > 0
            ? "You can likely free \(Format.bytes(overview.likelySafeBytes))"
            : "Nothing is clearly safe to remove right now"
    }

    /// The Review first total, so the headline doesn't undersell what could be freed. Never added to
    /// the headline itself: those items need a person to look first.
    static func reviewLine(for overview: Overview) -> String? {
        guard overview.reviewFirstBytes > 0 else { return nil }
        let size = Format.bytes(overview.reviewFirstBytes)
        return overview.likelySafeBytes > 0
            ? "Up to \(size) more once you\u{2019}ve checked the items marked Review first."
            : "\(size) is marked Review first. Check those items before you clean them."
    }

    /// Free space if every likely safe item were removed. Never more than the disk.
    private var freeAfter: UInt64 {
        let (sum, overflow) = overview.diskFree.addingReportingOverflow(overview.likelySafeBytes)
        return overflow ? overview.diskTotal : min(sum, overview.diskTotal)
    }
}

/// One labelled number, such as "Free now 61 GB".
struct FreeSpaceFigure: View {
    let caption: String
    let bytes: UInt64

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(caption)
                .font(.callout)
                .foregroundStyle(.secondary)
            Text(Format.bytes(bytes))
                .font(.title2.weight(.medium).monospacedDigit())
        }
        .accessibilityElement(children: .combine)
    }
}

/// The 5 to 8 biggest recommendations. Nothing is ticked for the user.
struct WorthReviewingList: View {
    @EnvironmentObject private var model: AppModel
    let items: [ItemSummary]
    let searchText: String

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                Text("Worth reviewing")
                    .font(.title3.weight(.semibold))
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                Button("See everything") { model.selection = .cleanup }
                    .buttonStyle(.link)
                    .accessibilityHint("Opens the list of everything Roomy found.")
            }
            if items.isEmpty {
                Text(emptyText)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 8)
            } else {
                VStack(spacing: 0) {
                    ForEach(items) { item in
                        WorthReviewingRow(item: item)
                        if item.id != items.last?.id {
                            Divider().padding(.leading, 80)
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

    private var emptyText: String {
        searchText.isEmpty ? "Nothing here takes much space." : "No items match \u{201C}\(searchText)\u{201D}."
    }
}

/// Checkbox, icon, title, one-line explanation, size and tier label.
///
/// The Overview never shows `keep` items, but the checkbox is left out for them all the same, as in
/// `ItemRow`. Ticking changes nothing on disk; it only chooses the row for the cleanup review.
struct WorthReviewingRow: View {
    @EnvironmentObject private var model: AppModel
    let item: ItemSummary

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            checkbox
                .frame(width: 20)
            CategoryIcon(category: item.category)
            VStack(alignment: .leading, spacing: 2) {
                ItemTitle(item: item)
                Text(item.explain)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 12)
            Text(Format.bytes(item.sizeBytes))
                .font(.body.monospacedDigit())
            TierLabel(tier: item.tier)
                .frame(width: 110, alignment: .trailing)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(item.accessibleTitle), \(Format.bytes(item.sizeBytes)), \(item.tier.label)")
        .accessibilityHint(item.explain)
    }

    @ViewBuilder
    private var checkbox: some View {
        if item.tier == .keep {
            Color.clear
                .frame(width: 16, height: 16)
                .accessibilityHidden(true)
        } else {
            Toggle("Choose \(item.accessibleTitle)", isOn: Binding(
                get: { model.isChecked(item) },
                set: { model.setChecked($0, for: item) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .help("Choose this to review it for cleanup.")
        }
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

/// Opens the cleanup review for the ticked rows. Nothing is ticked for the user, and the review is a
/// dry run: nothing moves until the user confirms there.
struct ReviewAndCleanButton: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 12) {
            Button("Review and clean\u{2026}") { model.reviewCleanup() }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(model.checkedItemIDs.isEmpty)
                .accessibilityHint("Checks what would move to the Trash. Nothing is moved.")
            Text(Self.caption(count: model.checkedItemIDs.count, bytes: model.checkedBytes))
                .font(.callout.monospacedDigit())
                .foregroundStyle(.secondary)
        }
    }

    /// What is chosen so far, or how to choose.
    static func caption(count: Int, bytes: UInt64) -> String {
        guard count > 0 else {
            return "Tick the items you want to clean. Nothing is removed until you confirm."
        }
        let noun = count == 1 ? "item" : "items"
        return "\(count) \(noun) chosen, \(Format.bytes(bytes)). Nothing is removed until you confirm."
    }
}

/// Last scan time, and how many folders couldn't be measured.
struct OverviewFooter: View {
    let scannedAt: Date
    let unreadableCount: UInt64
    var quickRescan = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Last scan: \(scannedAt.formatted(date: .abbreviated, time: .shortened))")
            if quickRescan {
                Text("Roomy measured again only the folders that changed since the scan before.")
            }
            UnmeasuredNote(unreadableCount: unreadableCount)
        }
        .font(.footnote)
        .foregroundStyle(.secondary)
    }
}

/// "212 folders not measured", with a link that explains Full Disk Access and opens System Settings.
struct UnmeasuredNote: View {
    let unreadableCount: UInt64
    @State private var showingExplanation = false

    /// "1 folder not measured" or "212 folders not measured".
    static func text(for count: UInt64) -> String {
        count == 1 ? "1 folder not measured" : "\(Format.count(count)) folders not measured"
    }

    var body: some View {
        if unreadableCount == 0 {
            Text("Roomy could open every folder.")
        } else {
            HStack(spacing: 6) {
                Text(Self.text(for: unreadableCount))
                Button("Why?") { showingExplanation = true }
                    .buttonStyle(.link)
                    .accessibilityLabel("Why some folders weren't measured")
                    .popover(isPresented: $showingExplanation, arrowEdge: .bottom) {
                        FullDiskAccessExplanation()
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
