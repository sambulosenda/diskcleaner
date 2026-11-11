import Foundation

/// One entry in the window's sidebar.
enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case overview
    case backups
    case libraries
    case downloads
    case largeFiles
    case duplicates
    case appsAndCaches
    case systemData
    case projects
    case xcode
    case android
    case javascript
    case docker
    case tools
    case apps
    case cleanup
    case history

    var id: String { rawValue }

    /// The first group, with no heading: the answer, then every item the scan found in one list.
    static let topItems: [SidebarItem] = [.overview, .cleanup]
    /// "Where space goes": the everyday places, one screen each.
    static let placeItems: [SidebarItem] = [.backups, .libraries, .downloads, .appsAndCaches, .systemData]
    /// "Find": searches for single files.
    static let findItems: [SidebarItem] = [.largeFiles, .duplicates]
    /// Shown only in Developer mode.
    static let developerItems: [SidebarItem] = [.projects, .xcode, .android, .javascript, .docker, .tools]
    /// The last group: removing apps and looking back.
    static let actionItems: [SidebarItem] = [.apps, .history]

    /// The heading above a group in the sidebar, or `nil` for the first group.
    static func heading(for items: [SidebarItem]) -> String? {
        switch items {
        case placeItems: return "Where space goes"
        case findItems: return "Find"
        case developerItems: return "Developer"
        case actionItems: return "More"
        default: return nil
        }
    }

    var title: String {
        switch self {
        case .overview: return "Overview"
        case .backups: return "Backups"
        case .libraries: return "Photos, Mail and Music"
        case .downloads: return "Downloads and installers"
        case .largeFiles: return "Large files"
        case .duplicates: return "Duplicates"
        case .appsAndCaches: return "Apps and caches"
        case .systemData: return "System Data"
        case .projects: return "Projects"
        case .xcode: return "Xcode"
        case .android: return "Android"
        case .javascript: return "JavaScript"
        case .docker: return "Docker"
        case .tools: return "Tool clean-ups"
        case .apps: return "Remove apps"
        case .cleanup: return "Everything found"
        case .history: return "History"
        }
    }

    /// SF Symbol name. All of these exist on macOS 13.
    var symbol: String {
        switch self {
        case .overview: return "internaldrive"
        case .backups: return "externaldrive"
        case .libraries: return "photo.on.rectangle"
        case .downloads: return "arrow.down.circle"
        case .largeFiles: return "doc"
        case .duplicates: return "doc.on.doc"
        case .appsAndCaches: return "square.grid.2x2"
        case .systemData: return "gearshape"
        case .projects: return "folder"
        case .xcode: return "hammer"
        case .android: return "candybarphone"
        case .javascript: return "curlybraces"
        case .docker: return "shippingbox"
        case .tools: return "wrench.and.screwdriver"
        case .apps: return "xmark.bin"
        case .cleanup: return "list.bullet"
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
    /// them the way the wireframe names them. JavaScript has its own screen (`JavaScriptScreen`), which
    /// groups `node_modules` by project. Android has its own screen (`AndroidScreen`), which asks the core's
    /// Android analyser rather than the scan. Every other `developer_tools` rule today is an Xcode one, so
    /// Xcode lists that category.
    var itemFilter: ItemFilter? {
        switch self {
        case .backups: return .categories([.backups])
        case .downloads: return .categories([.downloads])
        // Cleanup lists everything the scan found, so every ticked row can be seen in one place.
        case .cleanup: return .everything
        case .appsAndCaches: return .categories([.cache, .appData])
        case .systemData: return .categories([.system, .logs])
        case .projects: return .categories([.build])
        case .xcode: return .categories([.developerTools])
        case .docker: return .categories([.docker])
        // Large files has its own screen (`LargeFilesScreen`): files, not rule items.
        case .overview, .libraries, .largeFiles, .duplicates, .android, .javascript, .tools, .apps, .history: return nil
        }
    }

    /// The sidebar entries to show for a mode, as groups in order.
    static func sections(for mode: Mode) -> [[SidebarItem]] {
mode == .developer
            ? [topItems, placeItems, findItems, developerItems, actionItems]
            : [topItems, placeItems, findItems, actionItems]
    }
}
