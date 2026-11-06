import Foundation

/// One entry in the window's sidebar.
enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case overview
    case backups
    case downloads
    case largeFiles
    case appsAndCaches
    case systemData
    case projects
    case xcode
    case android
    case javascript
    case docker
    case cleanup
    case history

    var id: String { rawValue }

    /// The first group: Overview and the everyday categories.
    static let mainItems: [SidebarItem] = [.overview, .backups, .downloads, .largeFiles, .appsAndCaches, .systemData]
    /// Shown only in Developer mode.
    static let developerItems: [SidebarItem] = [.projects, .xcode, .android, .javascript, .docker]
    /// The last group: acting on things and looking back.
    static let actionItems: [SidebarItem] = [.cleanup, .history]

    var title: String {
        switch self {
        case .overview: return "Overview"
        case .backups: return "Backups"
        case .downloads: return "Downloads and installers"
        case .largeFiles: return "Large files"
        case .appsAndCaches: return "Apps and caches"
        case .systemData: return "System Data"
        case .projects: return "Projects"
        case .xcode: return "Xcode"
        case .android: return "Android"
        case .javascript: return "JavaScript"
        case .docker: return "Docker"
        case .cleanup: return "Cleanup"
        case .history: return "History"
        }
    }

    /// SF Symbol name. All of these exist on macOS 13.
    var symbol: String {
        switch self {
        case .overview: return "internaldrive"
        case .backups: return "externaldrive.badge.timemachine"
        case .downloads: return "arrow.down.circle"
        case .largeFiles: return "doc"
        case .appsAndCaches: return "square.grid.2x2"
        case .systemData: return "gearshape"
        case .projects: return "folder"
        case .xcode: return "hammer"
        case .android: return "candybarphone"
        case .javascript: return "curlybraces"
        case .docker: return "shippingbox"
        case .cleanup: return "sparkles"
        case .history: return "clock.arrow.circlepath"
        }
    }

    var isDeveloperOnly: Bool { Self.developerItems.contains(self) }

    /// The sidebar entries to show for a mode, as groups in order.
    static func sections(for mode: Mode) -> [[SidebarItem]] {
        mode == .developer ? [mainItems, developerItems, actionItems] : [mainItems, actionItems]
    }
}
