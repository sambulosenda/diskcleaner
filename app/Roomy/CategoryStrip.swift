import SwiftUI

/// One horizontal bar for the whole disk, split by category, with the likely safe part of each
/// segment drawn solid and the rest pale. The legend underneath gives every number in words.
struct CategoryStrip: View {
    let overview: Overview

    private static let height: CGFloat = 26
    private static let gap: CGFloat = 2

    var body: some View {
        GeometryReader { geometry in
            let segments = Self.segments(for: overview)
            let usable = max(geometry.size.width - Self.gap * CGFloat(max(segments.count - 1, 0)), 0)
            HStack(spacing: Self.gap) {
                ForEach(segments) { segment in
                    SegmentView(segment: segment)
                        .frame(width: usable * segment.fraction)
                }
            }
        }
        .frame(height: Self.height)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Disk use by category")
        .accessibilityValue(CategoryLegend.spokenSummary(for: overview))
    }

    /// A slice of the bar.
    struct Segment: Identifiable {
        let id: String
        let color: Color
        /// Share of the whole disk, 0 to 1.
        let fraction: CGFloat
        /// Share of this segment that is likely safe, 0 to 1.
        let likelySafeFraction: CGFloat
        let isFreeSpace: Bool
    }

    /// Categories in order, then free space. Zero-sized categories are left out.
    static func segments(for overview: Overview) -> [Segment] {
        guard overview.diskTotal > 0 else { return [] }
        let total = Double(overview.diskTotal)
        var result = overview.categories.filter { $0.bytes > 0 }.map { entry in
            Segment(
                id: entry.category.rawValue,
                color: entry.category.color,
                fraction: CGFloat(Double(entry.bytes) / total),
                likelySafeFraction: CGFloat(min(Double(entry.likelySafeBytes) / Double(entry.bytes), 1)),
                isFreeSpace: false
            )
        }
        if overview.diskFree > 0 {
            result.append(Segment(
                id: "free",
                color: .secondary,
                fraction: CGFloat(Double(overview.diskFree) / total),
                likelySafeFraction: 0,
                isFreeSpace: true
            ))
        }
        return result
    }
}

/// Draws one segment: pale for the whole category, solid for the likely safe share.
private struct SegmentView: View {
    let segment: CategoryStrip.Segment

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Rectangle()
                    .fill(segment.isFreeSpace ? segment.color.opacity(0.15) : segment.color.opacity(0.4))
                Rectangle()
                    .fill(segment.color)
                    .frame(width: geometry.size.width * segment.likelySafeFraction)
            }
        }
    }
}

/// Swatch, name, size and how much can be freed, for every category and free space.
struct CategoryLegend: View {
    let overview: Overview

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 230), alignment: .leading)], alignment: .leading, spacing: 8) {
                ForEach(overview.categories.filter { $0.bytes > 0 }) { entry in
                    LegendEntry(
                        color: entry.category.color,
                        title: entry.category.title,
                        detail: Self.detail(for: entry)
                    )
                }
                LegendEntry(color: Color.secondary.opacity(0.3), title: "Free space", detail: Format.bytes(overview.diskFree))
            }
            Text("The solid part of each colour is space you can likely free.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
        }
        .accessibilityHidden(true) // The strip above reads the same information as one summary.
    }

    static func detail(for entry: CategoryTotal) -> String {
        entry.likelySafeBytes > 0
            ? "\(Format.bytes(entry.bytes)) · \(Format.bytes(entry.likelySafeBytes)) can be freed"
            : Format.bytes(entry.bytes)
    }

    /// The whole strip and legend as one sentence for VoiceOver.
    static func spokenSummary(for overview: Overview) -> String {
        let parts = overview.categories.filter { $0.bytes > 0 }.map { entry -> String in
            entry.likelySafeBytes > 0
                ? "\(entry.category.title) \(Format.bytes(entry.bytes)), \(Format.bytes(entry.likelySafeBytes)) can likely be freed"
                : "\(entry.category.title) \(Format.bytes(entry.bytes))"
        }
        return (parts + ["Free space \(Format.bytes(overview.diskFree))"]).joined(separator: "; ")
    }
}

private struct LegendEntry: View {
    let color: Color
    let title: String
    let detail: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            RoundedRectangle(cornerRadius: 3)
                .fill(color)
                .frame(width: 12, height: 12)
                .alignmentGuide(.firstTextBaseline) { $0[.bottom] - 1 }
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(.callout)
                Text(detail)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
    }
}

extension Category {
    /// Strip colour. The legend always names the category, so colour is never the only signal.
    var color: Color {
        switch self {
        case .appsAndCaches: return .blue
        case .developer: return .indigo
        case .largeFiles: return .orange
        case .backups: return .purple
        case .downloads: return .teal
        case .systemData: return .gray
        case .other: return .brown
        }
    }
}
