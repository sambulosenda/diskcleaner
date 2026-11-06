import Foundation

/// Turns core numbers into text. The core always works in bytes; only the UI formats them.
enum Format {
    /// A size the way Finder shows it, such as "34 GB".
    static func bytes(_ value: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: value), countStyle: .file)
    }

    /// A whole number with thousands separators, such as "123,711".
    static func count(_ value: UInt64) -> String {
        NumberFormatter.localizedString(from: NSNumber(value: value), number: .decimal)
    }

    /// How long ago `date` was, such as "3 weeks ago".
    static func relative(_ date: Date, now: Date = Date()) -> String {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .full
        return formatter.localizedString(for: min(date, now), relativeTo: now)
    }

    /// "1 item" or "212 items".
    static func items(_ value: UInt64) -> String {
        value == 1 ? "1 item" : "\(count(value)) items"
    }
}
