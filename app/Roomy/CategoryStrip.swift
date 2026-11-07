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

    /// One slice per part of the disk (see `DiskPart.parts(for:)`), so the segments always cover the
    /// disk exactly once.
    static func segments(for overview: Overview) -> [Segment] {
        guard overview.diskTotal > 0 else { return [] }
        let total = Double(overview.diskTotal)
        return DiskPart.parts(for: overview).map { part in
            Segment(
                id: part.id,
                color: part.color,
                fraction: CGFloat(Double(part.bytes) / total),
                likelySafeFraction: part.bytes == 0 ? 0 : CGFloat(min(Double(part.likelySafeBytes) / Double(part.bytes), 1)),
                isFreeSpace: part.kind == .free
            )
        }
    }
}

/// One part of the disk, as the strip draws it and the legend names it.
///
/// The core's categories come first, largest first. Then two parts that Roomy doesn't sort into
/// categories, named so they can't be mixed up: "Other files" is what Roomy found in the home folder
/// but doesn't recognise, and "Apps and macOS" is the used space outside the home folder, which Roomy
/// never cleans. Free space comes last. Zero-sized parts are left out.
struct DiskPart: Identifiable, Equatable {
    enum Kind: Equatable {
        case category(Category)
        case otherFiles
        case outsideHome
        case free
    }

    let kind: Kind
    let bytes: UInt64
    let likelySafeBytes: UInt64

    var id: String {
        switch kind {
        case .category(let category): return category.title
        case .otherFiles: return "other-files"
        case .outsideHome: return "outside-home"
        case .free: return "free"
        }
    }

    var title: String {
        switch kind {
        case .category(let category): return category.title
        case .otherFiles: return Category.unknown.title
        case .outsideHome: return "Apps and macOS"
        case .free: return "Free space"
        }
    }

    var color: Color {
        switch kind {
        case .category(let category): return category.color
        case .otherFiles: return Category.unknown.color
        case .outsideHome, .free: return .secondary
        }
    }

    /// The line under the title in the legend.
    var detail: String {
        let size = Format.bytes(bytes)
        switch kind {
        case .category:
            return likelySafeBytes > 0 ? "\(size) · \(Format.bytes(likelySafeBytes)) can be freed" : size
        case .otherFiles: return "\(size) · in your home folder"
        case .outsideHome: return "\(size) · outside your home folder"
        case .free: return size
        }
    }

    /// What the part holds, shown when the pointer rests on its legend entry.
    var help: String? {
        switch kind {
        case .category: return nil
        case .otherFiles:
            return "Files and folders in your home folder that Roomy doesn't recognise yet, such as your own documents and projects. Roomy leaves them alone."
        case .outsideHome:
            return "Apps, macOS itself, other people's accounts on this Mac and System Data. Roomy doesn't clean these. System Data has its own screen."
        case .free: return nil
        }
    }

    /// The parts for an overview, in strip order. Used space never goes below zero, and each part is
    /// counted once: home folder space no category accounts for joins "Other files".
    static func parts(for overview: Overview) -> [DiskPart] {
        let known = overview.categories.filter { $0.category != .unknown && $0.bytes > 0 }
        let unknown = overview.categories.first { $0.category == .unknown }
        let sorted = overview.categories.reduce(UInt64(0)) { $0 &+ $1.bytes }
        let homeRest = overview.scannedBytes > sorted ? overview.scannedBytes - sorted : 0
        let used = overview.diskTotal > overview.diskFree ? overview.diskTotal - overview.diskFree : 0
        let home = max(overview.scannedBytes, sorted)
        let outside = used > home ? used - home : 0

        var parts = known.map {
            DiskPart(kind: .category($0.category), bytes: $0.bytes, likelySafeBytes: $0.likelySafeBytes)
        }
        let other = (unknown?.bytes ?? 0) &+ homeRest
        if other > 0 {
            parts.append(DiskPart(kind: .otherFiles, bytes: other, likelySafeBytes: unknown?.likelySafeBytes ?? 0))
        }
        if outside > 0 {
            parts.append(DiskPart(kind: .outsideHome, bytes: outside, likelySafeBytes: 0))
        }
        if overview.diskFree > 0 {
            parts.append(DiskPart(kind: .free, bytes: overview.diskFree, likelySafeBytes: 0))
        }
        return parts
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

/// Swatch, name, size and how much can be freed, for every part of the disk.
struct CategoryLegend: View {
    let overview: Overview

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 230), alignment: .leading)], alignment: .leading, spacing: 8) {
                ForEach(DiskPart.parts(for: overview)) { part in
                    LegendEntry(
                        color: Self.swatch(for: part),
                        title: part.title,
                        detail: part.detail,
                        outlined: part.kind == .free
                    )
                    .help(part.help ?? "")
                }
            }
            if overview.likelySafeBytes > 0 {
                Text("The solid part of each colour is space you can likely free.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityHidden(true) // The strip above reads the same information as one summary.
    }

    /// The swatch matches the strip: grey parts are drawn pale there, so they are pale here too.
    private static func swatch(for part: DiskPart) -> Color {
        switch part.kind {
        case .outsideHome: return Color.secondary.opacity(0.4)
        case .free: return Color.secondary.opacity(0.25)
        case .category, .otherFiles: return part.color
        }
    }

    /// The whole strip and legend as one sentence for VoiceOver.
    static func spokenSummary(for overview: Overview) -> String {
        DiskPart.parts(for: overview).map { part -> String in
            switch part.kind {
            case .category:
                return part.likelySafeBytes > 0
                    ? "\(part.title) \(Format.bytes(part.bytes)), \(Format.bytes(part.likelySafeBytes)) can likely be freed"
                    : "\(part.title) \(Format.bytes(part.bytes))"
            case .otherFiles:
                return "\(part.title) in your home folder \(Format.bytes(part.bytes))"
            case .outsideHome:
                return "\(part.title), outside your home folder, \(Format.bytes(part.bytes))"
            case .free:
                return "\(part.title) \(Format.bytes(part.bytes))"
            }
        }
        .joined(separator: "; ")
    }
}

private struct LegendEntry: View {
    let color: Color
    let title: String
    let detail: String
    /// Draws an edge round the swatch, so a pale swatch still shows in dark mode.
    var outlined = false

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            RoundedRectangle(cornerRadius: 3)
                .fill(color)
                .frame(width: 12, height: 12)
                .overlay(
                    RoundedRectangle(cornerRadius: 3)
                        .strokeBorder(outlined ? Color.secondary : Color.clear, lineWidth: 1)
                )
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
