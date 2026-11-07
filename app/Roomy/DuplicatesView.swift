import AppKit
import Combine
import SwiftUI

/// The duplicate finder's state: the search, what it found, and which copies are ticked.
///
/// The core compares files by content (size, then SHA-256), never by name or date. Nothing is ticked for
/// the user, and the last unticked copy of a file can't be ticked, so at least one copy always stays. The
/// core refuses a review that would leave no copy anyway, and checks each copy byte for byte against the
/// one you keep just before it moves. The Review button opens the same cleanup review as every other
/// screen, so nothing moves until the user confirms, and a move can be put back.
@MainActor
final class DuplicatesModel: ObservableObject {
    /// What the screen shows.
    enum State: Equatable {
        /// Not looked yet, or the mode changed since.
        case idle
        /// The core is looking. Holds its latest progress.
        case searching(DuplicateSearch)
        /// What the core found.
        case finished(DuplicateReport)
        /// The user stopped the search.
        case cancelled
        /// The core couldn't look. The text is ready to show.
        case failed(String)
    }

    /// Shown when the core hasn't started looking, which only happens if something went wrong.
    static let couldNotStart = "Roomy couldn't start looking for copies. Please try again."

    @Published private(set) var state: State = .idle
    /// The ticked copies. Empty to begin with.
    @Published private(set) var checkedCopyIDs: Set<String> = []
    /// Set when a cleanup was put back since the search, so the list may be missing copies.
    @Published private(set) var isOutOfDate = false

    private weak var app: AppModel?
    private let pollInterval: UInt64
    private var searchTask: Task<Void, Never>?
    /// A stop sent to the core that may not have arrived yet. A new search waits for it, so an old
    /// stop can never land after the new start and stop that one instead.
    private var pendingCancel: Task<Void, Never>?
    private var observers: Set<AnyCancellable> = []

    init(app: AppModel, pollInterval: UInt64) {
        self.app = app
        self.pollInterval = pollInterval
        app.$cleanupState
            .sink { [weak self] state in self?.cleanupStateDidChange(state) }
            .store(in: &observers)
        app.$mode
            .dropFirst()
            .removeDuplicates()
            .sink { [weak self] _ in self?.modeDidChange() }
            .store(in: &observers)
    }

    var isSearching: Bool {
        if case .searching = state { return true }
        return false
    }

    /// Starts a new search, forgetting the earlier one and its ticks.
    func startSearch() {
        guard let app else { return }
        searchTask?.cancel()
        checkedCopyIDs = []
        isOutOfDate = false
        state = .searching(.looking(filesSeen: 0))
        let client = app.client
        let mode = app.mode
        let interval = pollInterval
        let cancelling = pendingCancel
        searchTask = Task { [weak self] in
            await cancelling?.value
            await client.startDuplicateSearch(mode: mode)
            while !Task.isCancelled {
                let progress = await client.duplicateSearchProgress()
                guard !Task.isCancelled, let self else { return }
                switch progress {
                case .finished(let report):
                    self.state = .finished(report)
                    return
                case .cancelled:
                    self.state = .cancelled
                    return
                case .failed(let reason):
                    self.state = .failed(reason)
                    return
                case .notStarted:
                    self.state = .failed(Self.couldNotStart)
                    return
                case .looking, .comparing:
                    self.state = .searching(progress)
                }
                if interval > 0 {
                    try? await Task.sleep(nanoseconds: interval)
                } else {
                    await Task.yield()
                }
            }
        }
    }

    /// Stops the search.
    func cancelSearch() {
        guard isSearching, let client = app?.client else { return }
        searchTask?.cancel()
        searchTask = nil
        state = .cancelled
        sendCancel(to: client)
    }

    /// Tells the core to stop, after any stop already on its way.
    private func sendCancel(to client: CoreClient) {
        let earlier = pendingCancel
        pendingCancel = Task {
            await earlier?.value
            await client.cancelDuplicateSearch()
        }
    }

    /// Waits for the search to end. Used by tests.
    func waitForSearch() async {
        await searchTask?.value
    }

    /// The groups whose name matches the search field.
    var visibleGroups: [DuplicateGroup] {
        guard case .finished(let report) = state else { return [] }
        let query = (app?.searchText ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return report.groups }
        return report.groups.filter { $0.name.localizedCaseInsensitiveContains(query) }
    }

    func isChecked(_ copy: DuplicateCopy) -> Bool { checkedCopyIDs.contains(copy.copyId) }

    /// Whether the copy's tick box can be used: the core must allow moving it, and it can't be the last
    /// unticked copy of its file.
    func canCheck(_ copy: DuplicateCopy, in group: DuplicateGroup) -> Bool {
        guard copy.canMove else { return false }
        if isChecked(copy) { return true }
        return group.copies.filter { !isChecked($0) }.count > 1
    }

    /// Ticks or unticks a copy. Refuses to tick the last unticked copy of a file. Changes nothing on disk.
    func setChecked(_ checked: Bool, for copy: DuplicateCopy, in group: DuplicateGroup) {
        if checked {
            guard canCheck(copy, in: group) else { return }
            checkedCopyIDs.insert(copy.copyId)
        } else {
            checkedCopyIDs.remove(copy.copyId)
        }
    }

    /// The ticked copies, in list order.
    private var checkedCopies: [(DuplicateGroup, DuplicateCopy)] {
        guard case .finished(let report) = state else { return [] }
        return report.groups.flatMap { group in
            group.copies.filter { checkedCopyIDs.contains($0.copyId) }.map { (group, $0) }
        }
    }

    /// How many copies are ticked.
    var checkedCount: Int { checkedCopies.count }

    /// The ticked copies' total size.
    var checkedBytes: UInt64 { checkedCopies.reduce(0) { $0 + $1.0.sizeBytes } }

    /// Opens the cleanup review for the ticked copies. Nothing moves until the user confirms.
    func review() {
        let ids = checkedCopies.map(\.1.copyId)
        guard !ids.isEmpty, let app else { return }
        app.reviewCleanup { client in try await client.previewDuplicateCleanup(copyIds: ids) }
    }

    /// After a cleanup, takes the copies that moved out of the list; after a put back, says the list may
    /// be missing copies.
    private func cleanupStateDidChange(_ cleanup: AppModel.CleanupState) {
        switch cleanup {
        case .done(let result):
            let moved = Set(result.items.filter(\.done).map(\.itemId))
            guard case .finished(var report) = state, !moved.isEmpty else { return }
            let ids = Set(report.groups.flatMap { $0.copies.map(\.copyId) })
            guard !moved.isDisjoint(with: ids) else { return }
            report.groups = report.groups.compactMap { group in
                var group = group
                group.copies.removeAll { moved.contains($0.copyId) }
                guard group.copies.count > 1, group.copies.contains(where: \.canMove) else { return nil }
                let frees = group.copies.filter(\.canMove).map(\.freesBytes)
                let sum = frees.reduce(0, +)
                group.couldFreeBytes = frees.count == group.copies.count ? sum - (frees.min() ?? 0) : sum
                return group
            }
            report.couldFreeBytes = report.groups.reduce(0) { $0 + $1.couldFreeBytes }
            checkedCopyIDs.subtract(moved)
            state = .finished(report)
        case .putBack:
            if case .finished = state { isOutOfDate = true }
        default:
            break
        }
    }

    /// Developer results carry paths, so a search from the other mode is forgotten.
    private func modeDidChange() {
        if isSearching, let client = app?.client {
            sendCancel(to: client)
        }
        searchTask?.cancel()
        searchTask = nil
        checkedCopyIDs = []
        isOutOfDate = false
        state = .idle
    }
}

extension DuplicateGroup: Identifiable {
    public var id: String { groupId }
}

extension DuplicateCopy: Identifiable {
    public var id: String { copyId }
}

/// The duplicate finder: files with exactly the same content, grouped, with a tick box for each copy.
struct DuplicatesScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        DuplicatesContent(duplicates: model.duplicates)
            .sheet(isPresented: Binding(
                get: { model.cleanupState != .closed },
                set: { if !$0 { model.closeCleanupReview() } }
            )) {
                CleanupReviewSheet()
                    .environmentObject(model)
            }
    }
}

/// The header, the search's progress or results, and the Review button.
struct DuplicatesContent: View {
    @EnvironmentObject private var model: AppModel
    @ObservedObject var duplicates: DuplicatesModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
                .padding(16)
            Divider()
            content
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Files with exactly the same content in Desktop, Documents, Downloads, Movies, Music and Pictures. Roomy compares what is inside each file, not its name.")
                    .fixedSize(horizontal: false, vertical: true)
                Text("Nothing is chosen for you, and Roomy always keeps at least one copy.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if duplicates.isSearching {
                Button("Stop") { duplicates.cancelSearch() }
            } else {
                Button {
                    duplicates.startSearch()
                } label: {
                    Label(duplicates.state == .idle ? "Look for copies" : "Look again", systemImage: "magnifyingglass")
                }
                .help("Reads your files to compare them. Nothing is moved.")
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        switch duplicates.state {
        case .idle:
            CenteredMessage(symbol: "doc.on.doc", title: "Find copies of your files",
                            message: "Roomy reads your files to compare them, which can take a few minutes. Nothing is moved.") {
                Button("Look for copies") { duplicates.startSearch() }
                    .buttonStyle(.borderedProminent)
            }
        case .searching(let progress):
            DuplicateSearchProgressView(progress: progress)
        case .cancelled:
            CenteredMessage(symbol: "stop.circle", title: "Stopped",
                            message: "Roomy stopped looking for copies.") {
                Button("Look again") { duplicates.startSearch() }
            }
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "Roomy couldn't look for copies",
                            message: message) {
                Button("Look again") { duplicates.startSearch() }
            }
        case .finished(let report):
            if report.groups.isEmpty {
                CenteredMessage(symbol: "checkmark.circle", title: "No copies found",
                                message: "Roomy didn't find two files with the same content of 1 MB or more.") {
                    EmptyView()
                }
            } else {
                results(report)
            }
        }
    }

    @ViewBuilder
    private func results(_ report: DuplicateReport) -> some View {
        List {
            Label("Removing copies could free up to \(Format.bytes(report.couldFreeBytes)).",
                  systemImage: "info.circle")
                .padding(.vertical, 4)
            if duplicates.isOutOfDate {
                Label("Some copies were put back. Look again to see them.", systemImage: "arrow.clockwise")
                    .padding(.vertical, 4)
            }
            ForEach(duplicates.visibleGroups) { group in
                Section {
                    ForEach(group.copies) { copy in
                        DuplicateCopyRow(duplicates: duplicates, group: group, copy: copy)
                    }
                } header: {
                    DuplicateGroupHeader(group: group)
                }
            }
        }
        .listStyle(.inset)
        .accessibilityLabel("Copies")
        Divider()
        footer
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
    }

    private var footer: some View {
        HStack(spacing: 12) {
            let count = duplicates.checkedCount
            Text("\(count) \(count == 1 ? "copy" : "copies") chosen, \(Format.bytes(duplicates.checkedBytes))")
                .font(.callout.monospacedDigit())
            Spacer()
            Button("Review\u{2026}") { duplicates.review() }
                .buttonStyle(.borderedProminent)
                .disabled(count == 0 || model.cleanupState != .closed)
                .accessibilityHint("Checks what would move to the Trash. Nothing is moved.")
        }
    }
}

/// How far the search has got.
struct DuplicateSearchProgressView: View {
    let progress: DuplicateSearch

    var body: some View {
        VStack(spacing: 12) {
            switch progress {
            case .comparing(let filesSeen, let done, let total):
                ProgressView(value: Double(min(done, total)), total: Double(max(total, 1))) {
                    Text("Comparing files that are the same size\u{2026}")
                } currentValueLabel: {
                    Text("\(Format.bytes(done)) of \(Format.bytes(total)) read, \(Format.count(filesSeen)) files looked at")
                        .monospacedDigit()
                }
            case .looking(let filesSeen):
                ProgressView("Looking at your files\u{2026}")
                Text("\(Format.count(filesSeen)) files looked at")
                    .font(.callout.monospacedDigit())
                    .foregroundStyle(.secondary)
            default:
                ProgressView()
            }
        }
        .frame(maxWidth: 420)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// A group's name, its size, and what removing copies could free.
struct DuplicateGroupHeader: View {
    let group: DuplicateGroup

    var body: some View {
        HStack {
            Text(group.name)
                .font(.headline)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer()
            Text("\(group.copies.count) copies, \(Format.bytes(group.sizeBytes)) each")
                .font(.callout.monospacedDigit())
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
    }
}

/// One copy: tick box, where it is, when it changed, and whether it is the one kept.
struct DuplicateCopyRow: View {
    @EnvironmentObject private var model: AppModel
    @ObservedObject var duplicates: DuplicatesModel
    let group: DuplicateGroup
    let copy: DuplicateCopy

    var body: some View {
        let checked = duplicates.isChecked(copy)
        HStack(alignment: .top, spacing: 10) {
            Toggle("Choose the copy in \(copy.location)", isOn: Binding(
                get: { duplicates.isChecked(copy) },
                set: { duplicates.setChecked($0, for: copy, in: group) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .disabled(!duplicates.canCheck(copy, in: group))
            .help(helpText)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(copy.location)
                        .font(.body.weight(.medium))
                    if !checked {
                        DuplicateTag(text: "Keep")
                    }
                    if copy.sharesSpace {
                        DuplicateTag(text: "Shares space")
                    }
                }
                Text(detailText)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if let reason = copy.staysReason {
                    Text(reason)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                if let path = copy.path {
                    Text(path)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            Spacer(minLength: 12)
            if let path = copy.path {
                Button("Show in Finder") {
                    let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
                    NSWorkspace.shared.activateFileViewerSelecting([url])
                }
                .help("Shows this copy in Finder. Nothing is moved.")
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(copy.name) in \(copy.location)\(checked ? ", chosen" : ", kept")")
    }

    private var detailText: String {
        let date = Date(timeIntervalSince1970: TimeInterval(copy.modifiedUnixSeconds))
        var text = "\(copy.name), changed \(date.formatted(date: .abbreviated, time: .shortened))."
        if copy.sharesSpace {
            text += " It shares its space with another file, so removing it frees little."
        }
        return text
    }

    private var helpText: String {
        if !copy.canMove { return "Roomy can't move this copy." }
        if !duplicates.canCheck(copy, in: group) { return "Roomy keeps at least one copy of each file." }
        return "Choose this copy to review it for removal."
    }
}

/// A small label next to a copy's place.
private struct DuplicateTag: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .background(Capsule().fill(Color.secondary.opacity(0.15)))
    }
}
