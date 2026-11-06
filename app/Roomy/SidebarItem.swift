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
    case apps
    case cleanup
    case history

    var id: String { rawValue }

    /// The first group: Overview and the everyday categories.
    static let mainItems: [SidebarItem] = [.overview, .backups, .downloads, .largeFiles, .appsAndCaches, .systemData]
    /// Shown only in Developer mode.
    static let developerItems: [SidebarItem] = [.projects, .xcode, .android, .javascript, .docker]
    /// The last group: acting on things and looking back.
    static let actionItems: [SidebarItem] = [.apps, .cleanup, .history]

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
        case .apps: return "Remove apps"
        case .cleanup: return "Cleanup"
        case .history: return "History"
        }
    }

    /// SF Symbol name. All of these exist on macOS 13.
    var symbol: String {
        switch self {
        case .overview: return "internaldrive"
        case .backups: return "externaldrive"
        case .downloads: return "arrow.down.circle"
        case .largeFiles: return "doc"
        case .appsAndCaches: return "square.grid.2x2"
        case .systemData: return "gearshape"
        case .projects: return "folder"
        case .xcode: return "hammer"
        case .android: return "candybarphone"
        case .javascript: return "curlybraces"
        case .docker: return "shippingbox"
        case .apps: return "xmark.bin"
        case .cleanup: return "sparkles"
        case .history: return "clock.arrow.circlepath"
        }
    }

    var isDeveloperOnly: Bool { Self.developerItems.contains(self) }

    /// Which of the core's items a list screen shows.
    enum ItemFilter: Equatable {
        /// Items in any of these categories.
        case categories([Category])
        /// Every item, whatever its category.
        case everything
    }

    /// What this screen lists, or `nil` for screens that are not item lists (yet).
    ///
    /// The core sorts items into the rule categories of `docs/safety/rule-schema.md`; the sidebar groups
    /// them the way the wireframe names them. Android has its own screen (`AndroidScreen`), which asks the
    /// core's Android analyser rather than the scan. JavaScript needs the core to group items by tool,
    /// which it does not do yet, so it stays a placeholder. Every other `developer_tools` rule today is an
    /// Xcode one, so Xcode lists that category.
    var itemFilter: ItemFilter? {
        switch self {
        case .backups: return .categories([.backups])
        case .downloads: return .categories([.downloads])
        // The core has no "large file" item yet, so this lists everything it found, largest first.
        case .largeFiles: return .everything
        case .appsAndCaches: return .categories([.cache, .appData])
        case .systemData: return .categories([.system, .logs])
        case .projects: return .categories([.build])
        case .xcode: return .categories([.developerTools])
        case .docker: return .categories([.docker])
        case .overview, .android, .javascript, .apps, .cleanup, .history: return nil
        }
    }

    /// The sidebar entries to show for a mode, as groups in order.
    static func sections(for mode: Mode) -> [[SidebarItem]] {
        mode == .developer ? [mainItems, developerItems, actionItems] : [mainItems, actionItems]
    }
}
