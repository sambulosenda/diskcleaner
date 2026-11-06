import Foundation

// Draft Swift mirrors of the T09 command contract.
//
// T09 (`docs/command-contract.md`) is not merged yet, so these types follow the shapes sketched in
// `docs/tasks/T09-command-contract.md`. T11 replaces them with the UniFFI-generated types and keeps
// the `CoreClient` protocol as the seam. Sizes are bytes as `UInt64`; formatting happens in the UI.

/// Which view of the Mac the user wants. Developer mode adds the Developer group and more detail.
enum Mode: String, CaseIterable, Identifiable, Equatable {
    case simple
    case developer

    var id: String { rawValue }

    /// Short name, used in the picker and the toolbar.
    var label: String {
        switch self {
        case .simple: return "Simple"
        case .developer: return "Developer"
        }
    }

    /// One plain sentence on what the mode changes, shown in Settings.
    var explanation: String {
        switch self {
        case .simple:
            return "Shows everyday items in plain words."
        case .developer:
            return "Adds a Developer section for Xcode, Android, JavaScript and Docker, with more detail on each item."
        }
    }
}

/// How safe an item is to remove. Always shown as text, never as colour alone.
enum Tier: String, CaseIterable, Equatable {
    case likelySafe
    case reviewFirst
    case keep

    var label: String {
        switch self {
        case .likelySafe: return "Likely safe"
        case .reviewFirst: return "Review first"
        case .keep: return "Keep"
        }
    }
}

/// The groups the disk strip and the sidebar categories are built from.
enum Category: String, CaseIterable, Identifiable, Equatable {
    case appsAndCaches
    case developer
    case largeFiles
    case backups
    case downloads
    case systemData
    case other

    var id: String { rawValue }

    var title: String {
        switch self {
        case .appsAndCaches: return "Apps and caches"
        case .developer: return "Developer tools"
        case .largeFiles: return "Large files"
        case .backups: return "Backups"
        case .downloads: return "Downloads and installers"
        case .systemData: return "System Data"
        case .other: return "Everything else"
        }
    }
}

/// Opaque id for one scan. Only the core knows what it refers to.
struct ScanHandle: Hashable {
    let id: UInt64
}

/// Where a scan has got to.
enum ScanPhase: Equatable {
    case measuring
    case sorting
    case done
}

/// Live numbers while a scan runs (`scan_progress` in T09).
struct ScanProgress: Equatable {
    var bytesSeen: UInt64
    var itemsSeen: UInt64
    var phase: ScanPhase
}

/// Size of one category, and how much of it is likely safe to free.
struct CategoryTotal: Equatable, Identifiable {
    var category: Category
    var bytes: UInt64
    var likelySafeBytes: UInt64

    var id: Category { category }
}

/// One recommendation as a list row. Carries no path: the UI only ever holds item ids.
struct ItemSummary: Equatable, Identifiable {
    var id: String
    var title: String
    var explanation: String
    var bytes: UInt64
    var tier: Tier
    var category: Category
}

/// What the Overview screen shows (`overview` in T09).
/// `likelySafeBytes` is the exact sum of every likely safe item's size, not just the top items.
struct Overview: Equatable {
    var likelySafeBytes: UInt64
    var diskTotal: UInt64
    var diskFree: UInt64
    var categories: [CategoryTotal]
    var topItems: [ItemSummary]
    var unreadableCount: UInt64
}
