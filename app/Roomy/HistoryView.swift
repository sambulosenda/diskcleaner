import SwiftUI

/// The History screen: past cleanups, newest first, and Put back for the most recent one while the
/// Trash still holds it, even after Roomy was quit. History holds no paths.
struct HistoryScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if model.history.isEmpty && model.lastCleanupResult == nil && model.historyMessage == nil {
            CenteredMessage(
                symbol: "clock.arrow.circlepath",
                title: "No cleanups yet",
                message: "When you move things to the Trash with Roomy, they're listed here."
            ) {
                EmptyView()
            }
        } else {
            List {
                if let message = model.historyMessage {
                    Section {
                        Label(message, systemImage: "exclamationmark.circle")
                            .padding(.vertical, Space.xs)
                    }
                }
                // Tied to the cleanup Put back undoes, not to the newest History row, so it is right even
                // if that cleanup could not be saved to History.
                if let last = model.lastCleanupResult, model.lastCleanup != nil {
                    Section {
                        HStack {
                            Text(HistoryText.lastCleanup(last))
                            Spacer()
                            if model.isPuttingBack {
                                ProgressView().controlSize(.small)
                                    .accessibilityLabel("Putting things back")
                            }
                            Button("Put back") { model.putBack() }
                                .disabled(model.isPuttingBack)
                                .help("Moves these items from the Trash back to where they were.")
                        }
                        .padding(.vertical, Space.xs)
                    }
                }
                ForEach(model.history, id: \.id) { entry in
                    Section(header: Text(HistoryText.heading(entry))) {
                        ForEach(Array(entry.items.enumerated()), id: \.offset) { _, item in
                            HistoryRow(item: item)
                        }
                    }
                }
            }
            .listStyle(.inset)
            .accessibilityLabel("History")
        }
    }
}

/// One item of a past cleanup.
struct HistoryRow: View {
    let item: HistoryItem

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Space.s) {
            Image(systemName: HistoryText.symbol(item.state))
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: Space.xxs) {
                Text(item.title)
                    .font(.body.weight(.medium))
                Text(HistoryText.state(item.state))
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: Space.m)
            Text(Format.bytes(item.sizeBytes))
                .font(.body.monospacedDigit())
        }
        .padding(.vertical, Space.xxs)
        .accessibilityElement(children: .combine)
    }
}

/// The words of the History screen.
enum HistoryText {
    /// The line beside Put back. `bytesMoved` counts what the Trash still holds.
    static func lastCleanup(_ last: CleanupResult) -> String {
        "\(Format.bytes(last.bytesMoved)) from your last cleanup is still in the Trash. "
            + "You can put it back until you empty the Trash."
    }

    static func heading(_ entry: HistoryEntry) -> String {
        let when = Date(timeIntervalSince1970: TimeInterval(entry.finishedAt))
            .formatted(date: .abbreviated, time: .shortened)
        if entry.putBackAt != nil {
            return "\(when), put back"
        }
        return "\(when), \(Format.bytes(entry.bytesInTrash)) to the Trash"
    }

    static func state(_ state: HistoryItemState) -> String {
        switch state {
        case .moved: return "Moved to the Trash."
        case .notMoved(let reason): return "Not moved. \(reason)"
        case .putBack: return "Put back."
        case .stayedInTrash(let reason): return "Still in the Trash. \(reason)"
        }
    }

    static func symbol(_ state: HistoryItemState) -> String {
        switch state {
        case .moved, .stayedInTrash: return "trash"
        case .notMoved: return "minus.circle"
        case .putBack: return "arrow.uturn.backward"
        }
    }
}
