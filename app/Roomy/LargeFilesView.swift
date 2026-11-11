import SwiftUI

/// Large files and old Downloads (PRD E2): the 500 largest files, and what has sat in Downloads unopened.
///
/// Size alone is never a reason to remove something, so nothing here is ticked for the user, and every row
/// is Review first. Ticked rows go to the same cleanup review as every other screen: nothing moves until the
/// user confirms, everything goes to the Trash, and the cleanup can be put back and is kept in History.
struct LargeFilesScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        Group {
            switch model.scanState {
            case .neverScanned:
                EmptyStateView()
            case .scanning(let progress):
                ScanningView(progress: progress)
            case .failed(let message):
                ErrorStateView(message: message)
            case .loaded(let overview, _):
                LargeFilesBrowser(unreadableCount: overview.unreadableCount)
            }
        }
        .sheet(isPresented: Binding(
            get: { model.cleanupState != .closed },
            set: { if !$0 { model.closeCleanupReview() } }
        )) {
            CleanupReviewSheet()
                .environmentObject(model)
        }
    }
}

/// The list, its controls, and the bar that opens the review.
struct LargeFilesBrowser: View {
    @EnvironmentObject private var model: AppModel
    let unreadableCount: UInt64

    var body: some View {
        VStack(spacing: 0) {
            LargeFilesControls()
            Divider()
            LargeFilesBody()
            if !model.checkedItemIDs.isEmpty {
                Divider()
                CleanupBar()
            }
            Divider()
            VStack(alignment: .leading, spacing: 4) {
                Text(LargeFilesText.footnote)
                UnmeasuredNote(unreadableCount: unreadableCount)
            }
            .font(.footnote)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// Which list, and in which order.
struct LargeFilesControls: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 16) {
            Picker("Show:", selection: $model.filesList) {
                ForEach(AppModel.FilesList.allCases) { list in
                    Text(list.label).tag(list)
                }
            }
            .pickerStyle(.segmented)
            .fixedSize()
            Picker("Sort by:", selection: $model.fileOrder) {
                ForEach(AppModel.FileOrder.allCases) { order in
                    Text(order.label).tag(order)
                }
            }
            .fixedSize()
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

/// The rows, or what to say when there are none.
struct LargeFilesBody: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        switch model.filesState {
        case .idle, .loading:
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityLabel("Loading files")
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "\(AppInfo.name) couldn't list these", message: message) {
                Button("Scan again") { model.startScan() }
            }
        case .loaded(let files):
            let rows = model.visibleFiles
            if rows.isEmpty {
                CenteredMessage(symbol: SidebarItem.largeFiles.symbol, title: emptyTitle,
                                message: LargeFilesText.emptyMessage(model.filesList, days: files.oldAfterDays)) {
                    EmptyView()
                }
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    Text(LargeFilesText.intro(model.filesList, days: files.oldAfterDays))
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                    List {
                        ForEach(rows) { row in
                            FileRowView(row: row)
                        }
                    }
                    .listStyle(.inset)
                    .accessibilityLabel(model.filesList.label)
                }
            }
        }
    }

    private var emptyTitle: String {
        model.searchText.isEmpty ? "Nothing here takes much space" : "No files match \u{201C}\(model.searchText)\u{201D}"
    }
}

/// Checkbox, icon, name, where it is, when it was last opened, size and the Review first label.
struct FileRowView: View {
    @EnvironmentObject private var model: AppModel
    let row: FileRow

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Toggle("Choose \(row.name)", isOn: Binding(
                get: { model.isChecked(row) },
                set: { model.setChecked($0, for: row) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .help("Choose this to review it for cleanup.")
            .frame(width: 20)
            Image(systemName: row.isFolder ? "folder" : "doc")
                .font(.title3)
                .foregroundStyle(.secondary)
                .frame(width: 24)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(row.name)
                    .font(.body.weight(.medium))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(LargeFilesText.location(row))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: 12)
            Text(LargeFilesText.lastOpened(row))
                .font(.callout)
                .foregroundStyle(.secondary)
                .frame(width: 190, alignment: .trailing)
            Text(Format.bytes(row.sizeBytes))
                .font(.body.monospacedDigit())
                .frame(width: 80, alignment: .trailing)
            TierLabel(tier: .reviewFirst)
                .frame(width: 110, alignment: .trailing)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(row.name), \(Format.bytes(row.sizeBytes)), \(LargeFilesText.lastOpened(row))")
    }
}

extension FileRow: Identifiable {
    public var id: String { itemId }
}

extension AppModel.FilesList {
    var label: String {
        switch self {
        case .largest: return "Large files"
        case .oldDownloads: return "Old Downloads"
        }
    }
}

extension AppModel.FileOrder {
    var label: String {
        switch self {
        case .largestFirst: return "Largest first"
        case .openedLongestAgo: return "Opened longest ago"
        }
    }
}

/// The words of the Large files screen.
enum LargeFilesText {
    static let footnote = "Nothing here is chosen for you. Files that belong to apps, libraries such as Photos, "
        + "and things listed on other screens aren't shown here."

    static func intro(_ list: AppModel.FilesList, days: UInt32) -> String {
        switch list {
        case .largest:
            return "Your largest files. Being big isn't a reason to remove something, so check each one first."
        case .oldDownloads:
            return "Things in Downloads you haven't opened for \(days) days or more. Check each one first."
        }
    }

    static func emptyMessage(_ list: AppModel.FilesList, days: UInt32) -> String {
        switch list {
        case .largest: return "\(AppInfo.name) didn't find any large files of yours."
        case .oldDownloads: return "Everything in Downloads was opened or changed in the last \(days) days."
        }
    }

    /// "In Movies" in simple mode, or the path in Developer mode.
    static func location(_ row: FileRow) -> String {
        if let path = row.path { return path }
        return row.place == "Home folder" ? "In your home folder" : "In \(row.place)"
    }

    /// "Last opened 3 Mar 2025", or when it changed if macOS has no record of it being opened.
    static func lastOpened(_ row: FileRow, locale: Locale = .current, timeZone: TimeZone = .current) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.timeZone = timeZone
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        if let opened = row.lastOpenedUnixSeconds {
            return "Last opened \(formatter.string(from: Date(timeIntervalSince1970: TimeInterval(opened))))"
        }
        let changed = formatter.string(from: Date(timeIntervalSince1970: TimeInterval(row.modifiedUnixSeconds)))
        return "Changed \(changed)"
    }
}
