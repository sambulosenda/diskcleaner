import SwiftUI

/// The headline numbers of the last finished scan, kept between launches so Roomy has an answer to
/// show the moment it opens, while it checks again. Sizes and a time only: no names and no paths.
struct LastScanSummary: Codable, Equatable {
    /// Where it is kept in the app's settings.
    static let key = "roomy.lastScanSummary"

    let likelySafeBytes: UInt64
    let reviewFirstBytes: UInt64
    let scannedAt: Date

    init(likelySafeBytes: UInt64, reviewFirstBytes: UInt64, scannedAt: Date) {
        self.likelySafeBytes = likelySafeBytes
        self.reviewFirstBytes = reviewFirstBytes
        self.scannedAt = scannedAt
    }

    init(overview: Overview, scannedAt: Date) {
        self.init(likelySafeBytes: overview.likelySafeBytes, reviewFirstBytes: overview.reviewFirstBytes,
                  scannedAt: scannedAt)
    }

    /// The stored summary, or `nil` if there is none or it can't be read.
    static func load(from defaults: UserDefaults) -> LastScanSummary? {
        guard let data = defaults.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(LastScanSummary.self, from: data)
    }

    /// Stores this summary, replacing the earlier one.
    func save(to defaults: UserDefaults) {
        guard let data = try? JSONEncoder().encode(self) else { return }
        defaults.set(data, forKey: Self.key)
    }

    /// "Last scan, 10 Nov 2026 at 13:12: 5 GB ready to clean and 36 GB worth a look."
    var sentence: String {
        let when = scannedAt.formatted(date: .abbreviated, time: .shortened)
        let found: String
        switch (likelySafeBytes > 0, reviewFirstBytes > 0) {
        case (true, true):
            found = "\(Format.bytes(likelySafeBytes)) ready to clean and \(Format.bytes(reviewFirstBytes)) worth a look"
        case (true, false):
            found = "\(Format.bytes(likelySafeBytes)) ready to clean"
        case (false, true):
            found = "\(Format.bytes(reviewFirstBytes)) worth a look"
        case (false, false):
            found = "nothing to clean"
        }
        return "Last scan, \(when): \(found)."
    }
}

/// What the last scan found, shown while a new scan runs or before one has started this time.
struct LastScanNote: View {
    let summary: LastScanSummary
    /// Whether a scan is running now.
    var isChecking = false

    var body: some View {
        Text(isChecking ? "\(summary.sentence) Checking again now." : summary.sentence)
            .font(.callout)
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: 420)
    }
}
