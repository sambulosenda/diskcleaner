import SwiftUI

/// The bar under a category list once something is ticked: how much is chosen, and the button that
/// opens the cleanup review. Pressing it moves nothing.
struct CleanupBar: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 12) {
            Text(summary)
                .font(.callout.monospacedDigit())
            Spacer()
            Button("Review\u{2026}") { model.reviewCleanup() }
                .buttonStyle(.borderedProminent)
                .accessibilityHint("Checks what would move to the Trash. Nothing is moved.")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    private var summary: String {
        let count = model.checkedItemIDs.count
        let noun = count == 1 ? "item" : "items"
        return "\(count) \(noun) chosen, \(Format.bytes(model.checkedBytes))"
    }
}

/// The cleanup sheet. It opens as a dry run: the core checks each item exactly as a cleanup would, and
/// nothing moves until the user presses Move to Trash and confirms. Afterwards it shows what moved, and
/// offers to put it back.
struct CleanupReviewSheet: View {
    @EnvironmentObject private var model: AppModel
    @State private var confirming = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(.title2.weight(.semibold))
                .padding([.horizontal, .top], 20)
                .padding(.bottom, 12)
            content
            Divider()
            footer
                .padding(16)
        }
        .frame(width: 560, height: 480)
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

    @ViewBuilder
    private var content: some View {
        switch model.cleanupState {
        case .closed, .checking:
            Working(text: "Checking each item\u{2026}")
        case .moving:
            Working(text: "Moving to the Trash\u{2026}")
        case .puttingBack:
            Working(text: "Putting things back\u{2026}")
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "\(AppInfo.name) couldn't finish this", message: message) {
                // macOS may refuse to let Roomy open the Trash without Full Disk Access.
                if model.fullDiskAccess == .notGranted {
                    Button("Open Full Disk Access Settings") { FullDiskAccessSettings.open() }
                        .accessibilityHint("Opens System Settings. Turn on \(AppInfo.name), then try again.")
                }
            }
            .task { await model.refreshFullDiskAccess() }
        case .ready(let preview):
            CleanupPreviewList(preview: preview)
        case .done(let result):
            CleanupResultList(
                headline: CleanupText.movedHeadline(result),
                note: CleanupText.movedNote(result),
                items: result.items,
                doneText: "Moved to the Trash."
            )
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
        HStack {
            switch model.cleanupState {
            case .ready(let preview):
                Text(preview.plan == nil ? "Nothing has been moved." : "Nothing has been moved yet.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer()
                if preview.plan == nil {
                    Button("Done") { model.closeCleanupReview() }
                        .keyboardShortcut(.cancelAction)
                } else {
                    Button("Cancel") { model.closeCleanupReview() }
                        .keyboardShortcut(.cancelAction)
                    Button("Move to Trash\u{2026}") { confirming = true }
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.defaultAction)
                }
            case .done:
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

/// A spinner and a line of text.
private struct Working: View {
    let text: String

    var body: some View {
        VStack(spacing: 12) {
            ProgressView()
            Text(text)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
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
        "\(AppInfo.name) checks each item again first. The space comes back when you empty the Trash. "
        + "Until then you can put them back, from here or from History, until \(AppInfo.name) quits."

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

    static func movedHeadline(_ result: CleanupResult) -> String {
        let count = result.items.filter(\.done).count
        if count == 0 { return "Nothing was moved" }
        let noun = count == 1 ? "item" : "items"
        return "Moved \(count) \(noun), \(Format.bytes(result.bytesMoved)), to the Trash"
    }

    static func movedNote(_ result: CleanupResult) -> String? {
        var lines: [String] = []
        if result.bytesMoved > 0 {
            lines.append("The space comes back when you empty the Trash.")
        }
        if !result.savedToHistory {
            lines.append("\(AppInfo.name) couldn't save this to History.")
        }
        return lines.isEmpty ? nil : lines.joined(separator: " ")
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
        VStack(alignment: .leading, spacing: 8) {
            Text(headline)
                .font(.headline)
                .padding(.horizontal, 20)
            if let note {
                Text(note)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 20)
            }
            List(items, id: \.itemId) { item in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Image(systemName: item.done ? "checkmark.circle.fill" : "minus.circle")
                        .foregroundStyle(item.done ? Color.green : Color.secondary)
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(item.title)
                            .font(.body.weight(.medium))
                        Text(item.reason ?? doneText)
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 12)
                    if item.done {
                        Text(Format.bytes(item.sizeBytes))
                            .font(.body.monospacedDigit())
                    }
                }
                .padding(.vertical, 3)
                .accessibilityElement(children: .combine)
            }
            .listStyle(.inset)
        }
    }
}

/// The checked items: a headline, then one row per item.
struct CleanupPreviewList: View {
    let preview: CleanupPreview

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(headline)
                .font(.headline)
                .padding(.horizontal, 20)
            if wontMoveCount > 0 {
                Text(wontMoveText)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 20)
            }
            List(preview.items, id: \.itemId) { item in
                CleanupPreviewRow(item: item)
            }
            .listStyle(.inset)
        }
    }

    private var headline: String {
        let moving = preview.items.count - wontMoveCount
        if moving == 0 { return "Nothing here would move to the Trash" }
        let noun = moving == 1 ? "item" : "items"
        return "\(moving) \(noun), \(Format.bytes(preview.bytesToMove)), would move to the Trash"
    }

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
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: willMove ? "checkmark.circle.fill" : "minus.circle")
                .foregroundStyle(willMove ? Color.green : Color.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title)
                    .font(.body.weight(.medium))
                if let path = item.path {
                    Text(path)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }
                Text(statusText)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                if willMove, let backup = item.deviceBackup {
                    BackupWarningBox(backup: backup, compact: true)
                        .padding(.top, 2)
                }
            }
            Spacer(minLength: 12)
            Text(Format.bytes(item.sizeBytes))
                .font(.body.monospacedDigit())
        }
        .padding(.vertical, 3)
        .accessibilityElement(children: .combine)
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
