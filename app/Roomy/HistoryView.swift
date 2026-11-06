import SwiftUI

/// The History screen: past cleanups, newest first, and Put back for the most recent one while it can
/// still be put back. History holds no paths.
struct HistoryScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if model.history.isEmpty {
            CenteredMessage(
                symbol: "clock.arrow.circlepath",
                title: "No cleanups yet",
                message: "When you move things to the Trash with Roomy, they're listed here."
            ) {
                EmptyView()
            }
        } else {
            List {
                if model.lastCleanup != nil, let newest = model.history.first, newest.putBackAt == nil {
                    Section {
                        HStack {
                            Text("Your last cleanup moved \(Format.bytes(newest.bytesInTrash)) to the Trash.")
                            Spacer()
                            Button("Put back") { model.putBack() }
                                .help("Moves these items from the Trash back to where they were.")
                        }
                        .padding(.vertical, 4)
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
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: HistoryText.symbol(item.state))
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title)
                    .font(.body.weight(.medium))
                Text(HistoryText.state(item.state))
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 12)
            Text(Format.bytes(item.sizeBytes))
                .font(.body.monospacedDigit())
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }
}

/// The words of the History screen.
enum HistoryText {
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
