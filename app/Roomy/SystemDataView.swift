import SwiftUI

/// The System Data screen's state. Asks the core what macOS's "System Data" holds. Read only: nothing on
/// this screen removes anything. What Roomy can clean in these places is in the scan's items, reached
/// from the buttons here, and goes through the usual Trash-only cleanup.
@MainActor
final class SystemDataModel: ObservableObject {
    /// Where the breakdown has got to.
    enum Phase: Equatable {
        /// Measuring.
        case loading
        /// The core's answer.
        case loaded(SystemData)
    }

    @Published private(set) var phase: Phase = .loading

    private let client: CoreClient

    init(client: CoreClient) {
        self.client = client
    }

    /// Measures again. Paths come back in Developer mode only.
    func load(mode: Mode) async {
        phase = .loading
        phase = .loaded(await client.systemData(mode: mode))
    }
}

/// The words the System Data screen shows. Plain British English, no jargon.
enum SystemDataText {
    static let snapshotsTitle = "Time Machine snapshots"
    static let snapshotsExplain =
        "Time Machine keeps recent copies of your files on this disk, so you can go back even when your backup drive isn't connected."
    static let snapshotsAdvice =
        "macOS removes them by itself when it needs the space, usually within a day. macOS doesn't say how much space they take."

    static func title(_ kind: SystemDataKind) -> String {
        switch kind {
        case .caches: return "Caches"
        case .logs: return "Logs"
        case .softwareUpdates: return "Software updates"
        case .temporaryFiles: return "Temporary files"
        case .swap: return "Swap files"
        case .sleepImage: return "Sleep image"
        }
    }

    static func explain(_ kind: SystemDataKind) -> String {
        switch kind {
        case .caches:
            return "Files apps and macOS keep so things open faster. They come back as you use your Mac."
        case .logs:
            return "Notes apps and macOS write about what they did, used to find problems."
        case .softwareUpdates:
            return "Updates downloaded for an iPhone, iPad or this Mac, kept so they can be installed."
        case .temporaryFiles:
            return "Working files macOS and apps keep for you while they run."
        case .swap:
            return "When memory is full, macOS moves some of it onto the disk."
        case .sleepImage:
            return "macOS saves what is in memory here, so nothing is lost if the battery runs out while your Mac sleeps."
        }
    }

    static func advice(_ kind: SystemDataKind) -> String {
        switch kind {
        case .caches:
            return "Your apps' caches are in Apps and caches, where you can review them. Shared caches belong to macOS, so Roomy leaves them alone."
        case .logs:
            return "macOS trims its own logs. Your apps' logs are in Items to clean."
        case .softwareUpdates:
            return "iPhone updates are in Downloads and installers. macOS removes its own updates once they are installed."
        case .temporaryFiles:
            return "macOS clears these by itself. Restarting your Mac often frees some of this space."
        case .swap:
            return "These go away when you restart your Mac. Quitting apps you aren't using helps too."
        case .sleepImage:
            return "macOS needs this file. Leave it as it is."
        }
    }

    /// Where the user can act on a part, if Roomy lists any of it as items.
    static func destination(_ kind: SystemDataKind) -> SidebarItem? {
        switch kind {
        case .caches: return .appsAndCaches
        case .softwareUpdates: return .downloads
        case .logs, .temporaryFiles, .swap, .sleepImage: return nil
        }
    }

    /// The link to where a part's items are listed, or `nil` if Roomy lists none of it.
    static func actionTitle(_ kind: SystemDataKind) -> String? {
        if kind == .logs { return "Show items to clean" }
        guard let destination = destination(kind) else { return nil }
        return "Show in \(destination.title)"
    }

    static func place(_ id: String) -> String {
        switch id {
        case "user_caches": return "Your apps' caches"
        case "shared_caches": return "Caches shared by all users"
        case "user_logs": return "Your apps' logs"
        case "shared_logs": return "Logs shared by all users"
        case "system_logs": return "macOS system logs"
        case "unified_logs": return "macOS activity logs"
        case "iphone_updates": return "iPhone updates"
        case "ipad_updates": return "iPad updates"
        case "mac_updates": return "Mac updates waiting to install"
        case "user_temporary": return "Your temporary files"
        case "swap_files": return "Swap files"
        case "sleep_image": return "Sleep image"
        default: return id
        }
    }

    /// The size column for one place.
    static func size(_ place: SystemDataPlace) -> String {
        switch place.state {
        case .missing: return "None"
        case .unreadable: return "Couldn't read"
        case .measured: return Format.bytes(place.sizeBytes)
        }
    }

    /// "None", "1 snapshot" or "3 snapshots".
    static func snapshotCount(_ count: UInt64) -> String {
        switch count {
        case 0: return "None"
        case 1: return "1 snapshot"
        default: return "\(Format.count(count)) snapshots"
        }
    }

    /// The day a snapshot's name gives. The core reads the name as UTC, so this shows it in UTC.
    static func snapshotDay(_ unixSeconds: Int64) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        formatter.timeZone = TimeZone(identifier: "UTC")
        return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(unixSeconds)))
    }

    /// "Taken on 6 Nov 2026" or "Taken between 4 Nov 2026 and 6 Nov 2026", or `nil` with no dates.
    static func snapshotDates(_ snapshots: LocalSnapshots) -> String? {
        guard let oldest = snapshots.oldestUnixSeconds, let newest = snapshots.newestUnixSeconds else { return nil }
        let first = snapshotDay(oldest)
        let last = snapshotDay(newest)
        return first == last ? "Taken on \(first)" : "Taken between \(first) and \(last)"
    }

    /// The parts, largest first. Ties keep the core's order.
    static func sorted(_ parts: [SystemDataPart]) -> [SystemDataPart] {
        parts.enumerated()
            .sorted { $0.element.sizeBytes != $1.element.sizeBytes ? $0.element.sizeBytes > $1.element.sizeBytes : $0.offset < $1.offset }
            .map(\.element)
    }
}

/// The System Data screen: what is in it, and the scan's items Roomy can clean here.
struct SystemDataScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        SystemDataHost(client: model.client)
    }
}

/// Holds the screen's own model, made once from the app's core client, so switching tabs doesn't
/// measure again.
private struct SystemDataHost: View {
    enum Tab: Hashable {
        case breakdown
        case items
    }

    @EnvironmentObject private var model: AppModel
    @StateObject private var data: SystemDataModel
    @State private var tab: Tab = .breakdown

    init(client: CoreClient) {
        _data = StateObject(wrappedValue: SystemDataModel(client: client))
    }

    var body: some View {
        VStack(spacing: 0) {
            Picker("Show", selection: $tab) {
                Text("What's in it").tag(Tab.breakdown)
                Text("Items to clean").tag(Tab.items)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .accessibilityLabel("Show")
            .fixedSize()
            .padding(.vertical, Space.s)
            Divider()
            switch tab {
            case .breakdown:
                breakdownTab
            case .items:
                ItemListScreen(item: .systemData)
            }
        }
        .task(id: model.mode) { await data.load(mode: model.mode) }
    }

    @ViewBuilder
    private var breakdownTab: some View {
        switch data.phase {
        case .loading:
            ProgressView("Measuring System Data\u{2026}")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .loaded(let systemData):
            breakdown(systemData)
        }
    }

    private func breakdown(_ systemData: SystemData) -> some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: Space.xs) {
                    Text("Roomy measured \(Format.bytes(systemData.totalBytes)) of System Data")
                        .font(.title3.weight(.semibold))
                        .heading()
                    Text("macOS counts more than this as System Data, such as app data in your Library folder and space macOS keeps for itself, so the two numbers won't match. Nothing on this page is removed.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.vertical, Space.xs)
            }
            Section {
                SnapshotsRow(snapshots: systemData.snapshots)
            }
            Section {
                ForEach(SystemDataText.sorted(systemData.parts), id: \.kind) { part in
                    PartRow(part: part, showsPaths: model.mode == .developer, open: open(_:))
                }
            }
            Section {
                Button("Measure again") { Task { await data.load(mode: model.mode) } }
            }
        }
        .listStyle(.inset)
        .accessibilityLabel("System Data")
    }

    /// Takes the user to where Roomy lists a part's items.
    private func open(_ kind: SystemDataKind) {
        if kind == .logs {
            tab = .items
        } else if let destination = SystemDataText.destination(kind) {
            model.selection = destination
        }
    }
}

/// Time Machine local snapshots: a count and dates, never a size.
private struct SnapshotsRow: View {
    let snapshots: LocalSnapshots

    var body: some View {
        VStack(alignment: .leading, spacing: Space.xs) {
            HStack(alignment: .firstTextBaseline) {
                Text(SystemDataText.snapshotsTitle).font(.headline)
                Spacer()
                switch snapshots.status {
                case .listed:
                    Text(SystemDataText.snapshotCount(snapshots.count)).font(.body.monospacedDigit())
                case .unavailable:
                    Text("Unknown").foregroundStyle(.secondary)
                }
            }
            Text(SystemDataText.snapshotsExplain).font(.callout).foregroundStyle(.secondary)
            if case .unavailable(let reason) = snapshots.status {
                Text(reason).font(.caption).foregroundStyle(.secondary)
            } else if let dates = SystemDataText.snapshotDates(snapshots) {
                Text(dates).font(.caption).foregroundStyle(.secondary)
            }
            Text(SystemDataText.snapshotsAdvice).font(.callout)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, Space.xs)
        .accessibilityElement(children: .combine)
    }
}

/// One part, with the places it was measured in behind a disclosure.
private struct PartRow: View {
    let part: SystemDataPart
    let showsPaths: Bool
    let open: (SystemDataKind) -> Void
    @State private var expanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: Space.xs) {
            HStack(alignment: .firstTextBaseline) {
                Text(SystemDataText.title(part.kind)).font(.headline)
                Spacer()
                Text(Format.bytes(part.sizeBytes)).font(.body.monospacedDigit())
            }
            Text(SystemDataText.explain(part.kind)).font(.callout).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Text(SystemDataText.advice(part.kind)).font(.callout)
                .fixedSize(horizontal: false, vertical: true)
            if part.partlyUnreadable {
                Text("Roomy couldn't read all of it, so it may be larger.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                if let action = SystemDataText.actionTitle(part.kind) {
                    Button(action) { open(part.kind) }
                        .buttonStyle(.link)
                }
                Spacer()
                Button(expanded ? "Hide where" : "Where it is") { expanded.toggle() }
                    .buttonStyle(.link)
                    .accessibilityLabel(expanded ? "Hide where \(SystemDataText.title(part.kind)) is" : "Show where \(SystemDataText.title(part.kind)) is")
            }
            if expanded {
                ForEach(part.places, id: \.id) { place in
                    PlaceRow(place: place, showsPath: showsPaths)
                }
                .padding(.leading, Space.m)
            }
        }
        .padding(.vertical, Space.xs)
    }
}

/// One measured place: its name, size and, in Developer mode, its path.
private struct PlaceRow: View {
    let place: SystemDataPlace
    let showsPath: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Space.xxs) {
            HStack(alignment: .firstTextBaseline) {
                Text(SystemDataText.place(place.id))
                if place.owner == .system {
                    Text("macOS").font(.caption).foregroundStyle(.secondary)
                        .accessibilityLabel("belongs to macOS")
                }
                Spacer()
                Text(SystemDataText.size(place)).font(.callout.monospacedDigit())
                    .foregroundStyle(place.state == .measured ? Color.primary : Color.secondary)
            }
            if showsPath, let path = place.path {
                Text(path)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
        }
        .accessibilityElement(children: .combine)
    }
}
