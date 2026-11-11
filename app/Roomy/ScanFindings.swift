import SwiftUI

/// The words and choices for "Found so far" on the scanning screen.
///
/// The core's `scanFindings` is an early look: a title, a count and a size that is still growing. It has
/// no safety level and nothing to tick, so this list is only ever read.
enum ScanFindingsText {
    /// The most rows shown. The scanning screen stays a glance, not a list to scroll.
    static let rowLimit = 6

    static let heading = "Found so far"

    static let note = "Still counting. When the scan finishes, Roomy checks each one and tells you what is safe to clean."

    /// The rows to show: the largest kinds, leaving out any with nothing counted yet.
    static func shown(_ findings: [ScanFinding]) -> [ScanFinding] {
        Array(findings.filter { $0.sizeBytes > 0 }.prefix(rowLimit))
    }

    /// "and 4 more kinds", when there are more kinds than rows.
    static func more(_ findings: [ScanFinding]) -> String? {
        let hidden = findings.filter { $0.sizeBytes > 0 }.count - shown(findings).count
        guard hidden > 0 else { return nil }
        return hidden == 1 ? "and 1 more kind" : "and \(hidden) more kinds"
    }

    /// "in 3 places", when the same kind of thing was found more than once.
    static func places(_ finding: ScanFinding) -> String? {
        finding.count > 1 ? "in \(Format.count(UInt64(finding.count))) places" : nil
    }

    /// What VoiceOver reads for a row.
    static func spoken(_ finding: ScanFinding) -> String {
        let parts = [finding.title, places(finding), "\(Format.bytes(finding.sizeBytes)) so far"]
        return parts.compactMap { $0 }.joined(separator: ", ")
    }
}

/// The scanning screen's short list of what the scan has recognised so far.
struct ScanFindingsList: View {
    let findings: [ScanFinding]
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let rows = ScanFindingsText.shown(findings)
        VStack(alignment: .leading, spacing: 10) {
            Text(ScanFindingsText.heading)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            VStack(spacing: 8) {
                ForEach(rows, id: \.title) { finding in
                    ScanFindingRow(finding: finding)
                        .transition(.opacity)
                }
            }
            if let more = ScanFindingsText.more(findings) {
                Text(more)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Text(ScanFindingsText.note)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(16)
        .frame(maxWidth: 440, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.primary.opacity(0.04)))
        .animation(reduceMotion ? nil : .easeInOut(duration: 0.25), value: rows.map(\.title))
    }
}

/// One kind of thing found so far: what it is, in how many places, and its size up to now.
private struct ScanFindingRow: View {
    let finding: ScanFinding

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: finding.category.symbol)
                .foregroundStyle(.secondary)
                .frame(width: 20)
            Text(finding.title)
                .lineLimit(1)
                .truncationMode(.tail)
            if let places = ScanFindingsText.places(finding) {
                Text(places)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .layoutPriority(-1)
            }
            Spacer(minLength: 12)
            Text(Format.bytes(finding.sizeBytes))
                .font(.body.monospacedDigit())
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(ScanFindingsText.spoken(finding))
    }
}
