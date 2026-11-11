import SwiftUI

/// Shown before the first scan.
struct EmptyStateView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        CenteredMessage(
            symbol: "internaldrive",
            title: "See what's filling your Mac",
            message: "Scan to see what takes up space, explained in plain words. Nothing is removed without your say-so."
        ) {
            Button("Scan now") { model.startScan() }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .accessibilityHint("Measures what is filling your Mac. Nothing is removed.")
            if let last = model.earlierScanSummary {
                LastScanNote(summary: last)
            }
        }
    }
}

/// Shown while a scan runs.
struct ScanningView: View {
    @EnvironmentObject private var model: AppModel
    let progress: ScanProgress

    var body: some View {
        VStack(spacing: 16) {
            Text("Scanning your Mac")
                .font(.title2.weight(.semibold))
            ProgressView()
                .progressViewStyle(.linear)
                .frame(maxWidth: 360)
                .accessibilityLabel("Scan progress")
                .accessibilityValue(summary)
            Text(summary)
                .font(.body.monospacedDigit())
                .accessibilityHidden(true)
            Text(phaseText)
                .font(.callout)
                .foregroundStyle(.secondary)
            Button("Cancel", role: .cancel) { model.cancelScan() }
                .keyboardShortcut(.cancelAction)
                .accessibilityHint("Stops the scan. Nothing has been changed.")
            if let last = model.earlierScanSummary {
                LastScanNote(summary: last, isChecking: true)
                    .padding(.top, 4)
            }
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var summary: String {
        "Looked at \(Format.bytes(progress.bytesSeen)) in \(Format.items(progress.itemsSeen))"
    }

    private var phaseText: String {
        switch progress.phase {
        case .scanning: return "Measuring folders…"
        case .classifying: return "Sorting what was found…"
        case .finished, .cancelled, .failed: return "Finishing…"
        }
    }
}

/// Shown when a scan fails.
struct ErrorStateView: View {
    @EnvironmentObject private var model: AppModel
    let message: String

    var body: some View {
        CenteredMessage(
            symbol: "exclamationmark.triangle",
            title: "The scan didn't finish",
            message: "\(message) Nothing on your Mac was changed."
        ) {
            Button("Try again") { model.startScan() }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
        }
    }
}

/// Stand-in for screens built in later tasks.
struct PlaceholderView: View {
    let item: SidebarItem

    var body: some View {
        CenteredMessage(
            symbol: item.symbol,
            title: item.title,
            message: "This screen arrives in a later update."
        ) {
            EmptyView()
        }
    }
}

/// An icon, a title, a sentence and an optional button, centred in the content area.
struct CenteredMessage<Actions: View>: View {
    let symbol: String
    let title: String
    let message: String
    @ViewBuilder let actions: () -> Actions

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: symbol)
                .font(.system(size: 44, weight: .regular))
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            Text(title)
                .font(.title2.weight(.semibold))
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
            Text(message)
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)
            actions()
                .padding(.top, 6)
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
