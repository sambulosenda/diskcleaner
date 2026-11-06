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
                ReviewAndCleanButton()
                Divider()
                OverviewFooter(scannedAt: scannedAt, unreadableCount: overview.unreadableCount)
            }
            .padding(28)
            .frame(maxWidth: 900, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// "You can likely free 34 GB", with free space now and after cleaning.
struct OverviewHeadline: View {
    let overview: Overview

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(headline)
                .font(.largeTitle.weight(.semibold))
                .accessibilityAddTraits(.isHeader)
            HStack(spacing: 40) {
                FreeSpaceFigure(caption: "Free now", bytes: overview.diskFree)
                FreeSpaceFigure(caption: "Free after cleaning", bytes: freeAfter)
            }
        }
    }

    private var headline: String {
        overview.likelySafeBytes > 0
            ? "You can likely free \(Format.bytes(overview.likelySafeBytes))"
            : "Nothing is clearly safe to remove right now"
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

/// The 5 to 8 biggest recommendations.
struct WorthReviewingList: View {
    let items: [ItemSummary]
    let searchText: String

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Worth reviewing")
                .font(.title3.weight(.semibold))
                .accessibilityAddTraits(.isHeader)
            if items.isEmpty {
                Text(emptyText)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 8)
            } else {
                VStack(spacing: 0) {
                    ForEach(items) { item in
                        WorthReviewingRow(item: item)
                        if item.id != items.last?.id {
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

    private var emptyText: String {
        searchText.isEmpty ? "Nothing here takes much space." : "No items match \u{201C}\(searchText)\u{201D}."
    }
}

/// Icon, title, one-line explanation, size and tier label.
struct WorthReviewingRow: View {
    let item: ItemSummary

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Image(systemName: item.category.symbol)
                .font(.title3)
                .foregroundStyle(.secondary)
                .frame(width: 24)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title)
                    .font(.body.weight(.medium))
                Text(item.explanation)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 12)
            Text(Format.bytes(item.bytes))
                .font(.body.monospacedDigit())
            TierLabel(tier: item.tier)
                .frame(width: 110, alignment: .trailing)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(item.title), \(Format.bytes(item.bytes)), \(item.tier.label)")
        .accessibilityHint(item.explanation)
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

/// Opens Cleanup with likely safe items chosen. Disabled until the Cleanup screen exists.
struct ReviewAndCleanButton: View {
    var body: some View {
        HStack(spacing: 12) {
            Button("Review and clean") {}
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(true)
                .help("Cleaning arrives in a later update.")
                .accessibilityHint("Not available yet. Cleaning arrives in a later update.")
            Text("Nothing is removed until you confirm.")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }
}

/// Last scan time, what couldn't be measured, and a link to grant Full Disk Access.
struct OverviewFooter: View {
    let scannedAt: Date
    let unreadableCount: UInt64

    /// Opens System Settings at Privacy & Security > Full Disk Access.
    static let fullDiskAccessURL = URL(
        string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
    )

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Last scan: \(scannedAt.formatted(date: .abbreviated, time: .shortened))")
            if unreadableCount == 0 {
                Text("Every folder was measured.")
            } else {
                HStack(spacing: 6) {
                    Text("\(Format.items(unreadableCount)) couldn't be measured because Roomy doesn't have access.")
                    if let url = Self.fullDiskAccessURL {
                        Link("Grant Full Disk Access", destination: url)
                            .foregroundColor(.accentColor)
                            .accessibilityHint("Opens System Settings.")
                    }
                }
            }
        }
        .font(.footnote)
        .foregroundStyle(.secondary)
    }
}

extension Tier {
    /// SF Symbol paired with the tier's text.
    var symbol: String {
        switch self {
        case .likelySafe: return "checkmark.circle.fill"
        case .reviewFirst: return "eye.circle.fill"
        case .keep: return "lock.circle.fill"
        }
    }

    /// Supporting colour. Never the only signal; no red, which is kept for real errors.
    var tint: Color {
        switch self {
        case .likelySafe: return .green
        case .reviewFirst: return .orange
        case .keep: return .gray
        }
    }
}

extension Category {
    /// SF Symbol for rows in this category.
    var symbol: String {
        switch self {
        case .appsAndCaches: return "square.grid.2x2"
        case .developer: return "hammer"
        case .largeFiles: return "doc"
        case .backups: return "externaldrive"
        case .downloads: return "arrow.down.circle"
        case .systemData: return "gearshape"
        case .other: return "ellipsis.circle"
        }
    }
}
