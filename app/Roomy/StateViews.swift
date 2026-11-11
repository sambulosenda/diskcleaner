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
            progressBar
                .progressViewStyle(.linear)
                .frame(maxWidth: 360)
                .accessibilityLabel("Scan progress")
                .accessibilityValue(spokenProgress)
            Text(summary)
                .font(.body.monospacedDigit())
                .accessibilityHidden(true)
            Text(detailText)
                .font(.callout)
                .foregroundStyle(.secondary)
            Button("Cancel", role: .cancel) { model.cancelScan() }
                .keyboardShortcut(.cancelAction)
                .accessibilityHint("Stops the scan. Nothing has been changed.")
            if !ScanFindingsText.shown(model.scanFindings).isEmpty {
                ScanFindingsList(findings: model.scanFindings)
                    .padding(.top, 8)
            }
            if let last = model.earlierScanSummary {
                LastScanNote(summary: last, isChecking: true)
                    .padding(.top, 4)
            }
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// A bar that fills when Roomy knows roughly how much there is to measure, from the last scan.
    /// The first scan has nothing to go on, so its bar just shows that work is happening.
    @ViewBuilder
    private var progressBar: some View {
        if let fraction {
            ProgressView(value: fraction)
        } else {
            ProgressView()
        }
    }

    private var fraction: Double? {
        ScanText.fraction(progress: progress, expectedBytes: model.expectedScanBytes)
    }

    private var spokenProgress: String {
        fraction.map { "\(ScanText.percent($0)), \(summary)" } ?? summary
    }

    private var detailText: String {
        guard let fraction, progress.phase == .scanning else { return phaseText }
        return "\(phaseText) \(ScanText.percent(fraction))"
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

/// How far along a scan is, worked out from how much the last scan measured.
enum ScanText {
    /// The most the bar shows while folders are still being measured. The disk may have grown since
    /// the last scan, so the bar never claims to be finished before the scan is.
    static let measuringLimit = 0.9

    /// 0 to 1, or `nil` when there is no earlier scan to compare with.
    static func fraction(progress: ScanProgress, expectedBytes: UInt64?) -> Double? {
        guard let expectedBytes, expectedBytes > 0 else { return nil }
        switch progress.phase {
        case .scanning:
            return min(Double(progress.bytesSeen) / Double(expectedBytes), 1) * measuringLimit
        case .classifying:
            return 0.95
        case .finished, .cancelled, .failed:
            return 1
        }
    }

    /// "About 40% done".
    static func percent(_ fraction: Double) -> String {
        "About \(Int((fraction * 100).rounded()))% done"
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
