import SwiftUI

/// The bar under a category list once something is ticked: how much is chosen, and the button that
/// opens the cleanup review. Nothing here moves anything.
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

/// The cleanup review: what would move to the Trash, and what wouldn't and why.
///
/// This version is a dry run only. The core checks each item exactly as a cleanup would, just
/// before acting, and nothing moves.
struct CleanupReviewSheet: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Review before cleaning up")
                .font(.title2.weight(.semibold))
                .padding([.horizontal, .top], 20)
                .padding(.bottom, 12)
            content
            Divider()
            HStack {
                Text("This is a check only. Nothing has been moved.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Done") { model.closeCleanupReview() }
                    .keyboardShortcut(.cancelAction)
            }
            .padding(16)
        }
        .frame(width: 560, height: 460)
    }

    @ViewBuilder
    private var content: some View {
        switch model.cleanupState {
        case .closed, .checking:
            VStack(spacing: 12) {
                ProgressView()
                Text("Checking each item\u{2026}")
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityElement(children: .combine)
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "Roomy couldn't check these", message: message) {
                EmptyView()
            }
        case .ready(let preview):
            CleanupPreviewList(preview: preview)
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
