import AppKit
import SwiftUI

/// The bar under a category list once something is ticked: how much is chosen, and the button that
/// opens the cleanup review. Pressing it moves nothing.
struct CleanupBar: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: Space.m) {
            Text(summary)
                .font(.callout.monospacedDigit())
            Spacer()
            Button("Review\u{2026}") { model.reviewCleanup() }
                .buttonStyle(.borderedProminent)
                .accessibilityHint("Checks what would move to the Trash. Nothing is moved.")
        }
        .padding(.horizontal, Space.l)
        .padding(.vertical, Space.s)
    }

    private var summary: String {
        let count = model.checkedItemIDs.count
        let noun = count == 1 ? "item" : "items"
        return "\(count) \(noun) chosen, \(Format.bytes(model.checkedBytes))"
    }
}

/// Where the cleanup bar goes under a list. Empty until something is ticked, then the bar fades in.
struct CleanupBarSlot: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            if !model.checkedItemIDs.isEmpty {
                Divider()
                CleanupBar()
                    .transition(.opacity)
            }
        }
        .motion(value: model.checkedItemIDs.isEmpty)
    }
}

/// The cleanup sheet. It opens as a dry run: the core checks each item exactly as a cleanup would, and
/// nothing moves until the user presses Move to Trash and confirms. Afterwards it shows what moved, and
/// offers to put it back.
struct CleanupReviewSheet: View {
    @EnvironmentObject private var model: AppModel
    @State private var confirming = false

    var body: some View {
        ReviewSheet(title: title) {
            content
                .transition(.opacity)
                .id(step)
        } footer: {
            footer
        }
        .motion(value: step)
        .onChange(of: step) { _ in Announce.say(CleanupText.spoken(model.cleanupState)) }
        .interactiveDismissDisabled(model.cleanupState.isBusy)
        .alert(confirmTitle, isPresented: $confirming) {
            Button("Move to Trash") { model.moveToTrash() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(confirmMessage)
        }
    }

    private var title: String {
        switch model.cleanupState {
        case .done, .puttingBack: return "Cleanup finished"
        case .putBack: return "Put back"
        default: return "Review before cleaning up"
        }
    }

    /// Which step of the sheet shows, so the content fades from one step to the next.
    private var step: Int {
        switch model.cleanupState {
        case .closed, .checking: return 0
        case .ready: return 1
        case .moving: return 2
        case .done: return 3
        case .puttingBack: return 4
        case .putBack: return 5
        case .failed: return 6
        }
    }

    @ViewBuilder
    private var content: some View {
        switch model.cleanupState {
        case .closed, .checking:
            ReviewWorking(text: "Checking each item\u{2026}")
        case .moving:
            ReviewWorking(text: "Moving to the Trash\u{2026}")
        case .puttingBack:
            ReviewWorking(text: "Putting things back\u{2026}")
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "Roomy couldn't finish this", message: message) {
                // macOS may refuse to let Roomy open the Trash without Full Disk Access.
                if model.fullDiskAccess == .notGranted {
                    Button("Open Full Disk Access Settings") { FullDiskAccessSettings.open() }
                        .accessibilityHint("Opens System Settings. Turn on Roomy, then try again.")
                }
            }
            .task { await model.refreshFullDiskAccess() }
        case .ready(let preview):
            CleanupPreviewList(preview: preview)
        case .done(let result):
            VStack(alignment: .leading, spacing: Space.m) {
                if result.bytesMoved > 0 {
                    // Copies can share their space with the one that's kept, so moving them may free
                    // less than their size. No estimate is shown for them.
                    CleanupFinish(result: result,
                                  overview: model.selection == .duplicates ? nil : model.overview,
                                  estimatesSpace: model.selection != .duplicates)
                        .padding(.horizontal, Space.xl)
                }
                CleanupResultList(
                    headline: CleanupText.movedHeadline(result),
                    note: CleanupText.movedNote(result, showsFinish: result.bytesMoved > 0),
                    items: result.items,
                    doneText: "Moved to the Trash."
                )
            }
        case .putBack(let result):
            CleanupResultList(
                headline: CleanupText.putBackHeadline(result),
                note: nil,
                items: result.items,
                doneText: "Back where it was."
            )
        }
    }

    @ViewBuilder
    private var footer: some View {
        Group {
            switch model.cleanupState {
            case .ready(let preview):
                Text(preview.plan == nil ? "Nothing has been moved." : CleanupText.readyNote)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer()
                if preview.plan == nil {
                    Button("Done") { model.closeCleanupReview() }
                        .keyboardShortcut(.cancelAction)
                } else {
                    Button("Cancel") { model.closeCleanupReview() }
                        .keyboardShortcut(.cancelAction)
                    // Only Likely safe items the user ticked in the list: this sheet already lists each
                    // one, so its button is the confirmation. Anything else gets the second question.
                    if model.canSkipSecondConfirm(preview) {
                        Button("Move to Trash") { model.moveToTrash() }
                            .buttonStyle(.borderedProminent)
                            .keyboardShortcut(.defaultAction)
                            .accessibilityHint("Moves the items listed to the Trash. You can put them back.")
                    } else {
                        Button("Move to Trash\u{2026}") { confirming = true }
                            .buttonStyle(.borderedProminent)
                            .keyboardShortcut(.defaultAction)
                    }
                }
            case .done(let result):
                if result.bytesMoved > 0 {
                    Button("Open Trash") { TrashFolder.open() }
                        .help("Shows the Trash in Finder. Empty it there when you want the space back.")
                        .accessibilityHint("Shows the Trash in Finder. Nothing is removed.")
                }
                Spacer()
                if model.lastCleanup != nil {
                    Button("Put back") { model.putBack() }
                        .help("Moves these items from the Trash back to where they were.")
                }
                Button("Done") { model.closeCleanupReview() }
                    .keyboardShortcut(.defaultAction)
            default:
                Spacer()
                Button("Done") { model.closeCleanupReview() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(model.cleanupState.isBusy)
            }
        }
    }

    private var confirmMessage: String {
        guard case .ready(let preview) = model.cleanupState else { return CleanupText.confirmMessage }
        return CleanupText.confirmText(preview)
    }

    private var confirmTitle: String {
        guard case .ready(let preview) = model.cleanupState else { return "Move to the Trash?" }
        return CleanupText.confirmTitle(preview)
    }
}

/// The words of the cleanup sheet, kept together so tests can check them.
enum CleanupText {
    static func confirmTitle(_ preview: CleanupPreview) -> String {
        let count = preview.items.filter { $0.check == .willMove }.count
        let noun = count == 1 ? "item" : "items"
        return "Move \(count) \(noun), \(Format.bytes(preview.bytesToMove)), to the Trash?"
    }

    static let confirmMessage =
        "Roomy checks each item again first. The space comes back when you empty the Trash. "
        + "Until then you can put them back, from here or from History. Roomy keeps this for its most recent cleanup."

    /// The confirm message, led by a warning for each iPhone or iPad backup that would move.
    static func confirmText(_ preview: CleanupPreview) -> String {
        let backups = preview.items
            .filter { $0.check == .willMove }
            .compactMap(\.deviceBackup)
        guard !backups.isEmpty else { return confirmMessage }
        let lines = backups.map { backup in
            "\(BackupWarning.headline(backup)) \(BackupWarning.lastBackedUp(backup))"
        }
        return (lines + [BackupWarning.checkICloud, confirmMessage]).joined(separator: "\n\n")
    }

    /// "3 items, 2.1 GB, would move to the Trash".
    static func previewHeadline(_ preview: CleanupPreview) -> String {
        let moving = preview.items.filter { $0.check == .willMove }.count
        if moving == 0 { return "Nothing here would move to the Trash" }
        let noun = moving == 1 ? "item" : "items"
        return "\(moving) \(noun), \(Format.bytes(preview.bytesToMove)), would move to the Trash"
    }

    /// What VoiceOver says when the sheet moves to a new step by itself. Empty for steps that
    /// need no announcement.
    static func spoken(_ state: AppModel.CleanupState) -> String {
        switch state {
        case .closed, .checking: return ""
        case .ready(let preview): return previewHeadline(preview)
        case .moving: return "Moving to the Trash"
        case .done(let result): return movedHeadline(result)
        case .puttingBack: return "Putting things back"
        case .putBack(let result): return putBackHeadline(result)
        case .failed(let message): return "Roomy couldn't finish this. \(message)"
        }
    }

    static func movedHeadline(_ result: CleanupResult) -> String {
        let count = result.items.filter(\.done).count
        if count == 0 { return "Nothing was moved" }
        let noun = count == 1 ? "item" : "items"
        return "Moved \(count) \(noun), \(Format.bytes(result.bytesMoved)), to the Trash"
    }

    /// The finish panel's headline. Promises space only when the bytes moved are what comes back.
    static func finishHeadline(bytesMoved: UInt64, estimatesSpace: Bool) -> String {
        estimatesSpace ? "\(Format.bytes(bytesMoved)) ready to free" : "\(Format.bytes(bytesMoved)) moved to the Trash"
    }

    static let emptyTrashNote = "The space comes back when you empty the Trash."
    static let sharedSpaceNote =
        "Empty the Trash to get space back. Copies that share space with the one you kept free less than their size."

    /// Under the Move to Trash button's sheet, before anything moves.
    static let readyNote = "Nothing has been moved yet. You can put things back afterwards."

    /// "20.4 GB free now. About 38.6 GB once you empty the Trash." Free space is the figure from the
    /// last scan, so the second number is an estimate and never more than the disk.
    static func freeSpaceLine(freeNow: UInt64, moved: UInt64, diskTotal: UInt64) -> String {
        let (sum, overflow) = freeNow.addingReportingOverflow(moved)
        let after = overflow ? diskTotal : min(sum, diskTotal)
        return "\(Format.bytes(freeNow)) free now. About \(Format.bytes(after)) once you empty the Trash."
    }

    /// `showsFinish` is true when the finish panel above the list already says how to get the space back.
    static func movedNote(_ result: CleanupResult, showsFinish: Bool = false) -> String? {
        var lines: [String] = []
        if result.bytesMoved > 0 && !showsFinish {
            lines.append("The space comes back when you empty the Trash.")
        }
        if !result.savedToHistory {
            lines.append("Roomy couldn't save this to History.")
        }
        return lines.isEmpty ? nil : lines.joined(separator: " ")
    }

    /// What happens after an item that would move is removed, said where the decision is made.
    /// `nil` for an item that stays where it is.
    static func afterwards(_ item: CleanupPreviewItem) -> String? {
        guard item.check == .willMove else { return nil }
        if let restore = item.restore, !restore.isEmpty {
            return "Afterwards: \(restore)"
        }
        return "Afterwards: it doesn't come back by itself. You can put it back until you empty the Trash."
    }

    static func putBackHeadline(_ result: PutBackResult) -> String {
        let count = result.items.filter(\.done).count
        if count == 0 { return "Nothing could be put back" }
        let noun = count == 1 ? "item" : "items"
        return "Put back \(count) \(noun), \(Format.bytes(result.bytesPutBack))"
    }
}

/// What a cleanup or put back did: a headline, an optional note, then one row per item.
struct CleanupResultList: View {
    let headline: String
    let note: String?
    let items: [CleanupResultItem]
    let doneText: String

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            ReviewSummary(headline: headline, note: note)
            List(items, id: \.itemId) { item in
                ReviewRow(marker: item.done ? .included : .excluded, title: item.title,
                          sizeBytes: item.done ? item.sizeBytes : nil) {
                    ReviewDetailLine(item.reason ?? doneText)
                }
            }
            .listStyle(.inset)
            .accessibilityLabel(headline)
        }
    }
}

/// The checked items: a headline, then one row per item.
struct CleanupPreviewList: View {
    let preview: CleanupPreview

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            ReviewSummary(headline: headline, note: wontMoveCount > 0 ? wontMoveText : nil)
            List(preview.items, id: \.itemId) { item in
                CleanupPreviewRow(item: item)
            }
            .listStyle(.inset)
            .accessibilityLabel(headline)
        }
    }

    private var headline: String { CleanupText.previewHeadline(preview) }

    private var wontMoveCount: Int {
        preview.items.filter { $0.check != .willMove }.count
    }

    private var wontMoveText: String {
        wontMoveCount == 1 ? "1 item would stay where it is." : "\(wontMoveCount) items would stay where they are."
    }
}

/// One checked item: whether it would move, its size, and why not if it wouldn't.
struct CleanupPreviewRow: View {
    let item: CleanupPreviewItem

    var body: some View {
        ReviewRow(marker: willMove ? .included : .excluded, title: item.title, sizeBytes: item.sizeBytes) {
            if let path = item.path {
                Text(path)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
            }
            ReviewDetailLine(statusText)
            if let afterwards = CleanupText.afterwards(item) {
                ReviewDetailLine(afterwards)
            }
            if willMove, let backup = item.deviceBackup {
                BackupWarningBox(backup: backup, compact: true)
                    .padding(.top, Space.xxs)
            }
        }
    }

    private var willMove: Bool { item.check == .willMove }

    private var statusText: String {
        switch item.check {
        case .willMove:
            return "Would move to the Trash."
        case .wontMove(let reason):
            return reason
        }
    }
}

/// The top of the sheet after a cleanup: how much moved, in large type, and what that does to free
/// space once the Trash is emptied. Roomy never empties the Trash itself.
struct CleanupFinish: View {
    let result: CleanupResult
    /// The last scan's Overview, for the free space figures. `nil` leaves that line out.
    let overview: Overview?
    /// False when the bytes moved may be more than emptying the Trash frees, as with copies that
    /// share space. The panel then says what moved and promises no figure.
    var estimatesSpace = true

    var body: some View {
        FinishPanel(headline: CleanupText.finishHeadline(bytesMoved: result.bytesMoved, estimatesSpace: estimatesSpace),
                    detail: detail)
    }

    private var detail: String {
        if let overview {
            return CleanupText.freeSpaceLine(freeNow: overview.diskFree, moved: result.bytesMoved,
                                             diskTotal: overview.diskTotal)
        }
        return estimatesSpace ? CleanupText.emptyTrashNote : CleanupText.sharedSpaceNote
    }
}

/// Showing the user's Trash in Finder. Only opens a Finder window: Roomy never empties the Trash.
enum TrashFolder {
    static func url(home: URL = FileManager.default.homeDirectoryForCurrentUser) -> URL {
        home.appendingPathComponent(".Trash", isDirectory: true)
    }

    static func open() {
        NSWorkspace.shared.open(url())
    }
}
