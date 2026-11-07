import AppKit
import SwiftUI

/// The menu bar item's label: a disk icon and the free space, or a warning icon when space is low.
struct MenuBarLabel: View {
    @ObservedObject var monitor: LowSpaceMonitor

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: monitor.isLow ? "exclamationmark.triangle.fill" : "internaldrive")
            if !monitor.menuBarTitle.isEmpty {
                Text(monitor.menuBarTitle)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(monitor.isLow ? "Space is low. \(monitor.summaryText)" : monitor.summaryText)
    }
}

/// What the menu says about the last scan, and what its button does. Pure, so tests can check the
/// words for every scan state without a menu.
struct MenuBarScanSummary: Equatable {
    /// What the button does. Neither choice removes anything.
    enum Action: Equatable {
        /// Opens Roomy at the Overview, where the user reviews and chooses.
        case review
        /// Opens Roomy and starts a scan, which only measures.
        case scan
        /// Nothing to press while a scan runs.
        case none
    }

    /// The first line, such as "You can likely free 34 GB".
    let headline: String
    /// When that was measured, such as "Scanned 2 hours ago". `nil` when there is no finished scan.
    let detail: String?
    /// The button's title. Empty when `action` is `.none`.
    let buttonTitle: String
    let action: Action

    /// The summary for `state`, with times said relative to `now`.
    static func make(for state: AppModel.ScanState, now: Date = Date()) -> MenuBarScanSummary {
        switch state {
        case .neverScanned:
            return MenuBarScanSummary(
                headline: "Scan to see what you can free", detail: nil,
                buttonTitle: "Scan Now\u{2026}", action: .scan
            )
        case .scanning:
            return MenuBarScanSummary(headline: "Scanning\u{2026}", detail: nil, buttonTitle: "", action: .none)
        case .failed:
            return MenuBarScanSummary(
                headline: "The last scan didn\u{2019}t finish", detail: nil,
                buttonTitle: "Scan Again\u{2026}", action: .scan
            )
        case .loaded(let overview, let scannedAt):
            return MenuBarScanSummary(
                headline: OverviewHeadline.headline(for: overview),
                detail: "Scanned \(Format.relative(scannedAt, now: now))",
                buttonTitle: "Review in Roomy\u{2026}", action: .review
            )
        }
    }
}

/// The menu that opens from the menu bar item. Shows free space, what the last scan found, and opens
/// Roomy. It never cleans anything; cleaning happens in the main window, where the user chooses.
struct MenuBarMenu: View {
    @ObservedObject var monitor: LowSpaceMonitor
    @ObservedObject var model: AppModel

    var body: some View {
        Text(monitor.summaryText)
        if let low = monitor.lowSpaceText {
            Text(low)
        }
        Divider()
        scanSummary
        Divider()
        Button("Open Roomy") { monitor.showRoomy() }
        Button("Check Again") { Task { await monitor.refresh() } }
        Divider()
        settings
        Button("Quit Roomy") { NSApp.terminate(nil) }
            .keyboardShortcut("q", modifiers: .command)
    }

    /// The last scan's headline and its button. The button only opens Roomy (and, before any scan,
    /// starts one); review and cleaning stay in the main window.
    @ViewBuilder
    private var scanSummary: some View {
        let summary = MenuBarScanSummary.make(for: model.scanState)
        Text(summary.headline)
        if let detail = summary.detail {
            Text(detail)
        }
        switch summary.action {
        case .review:
            Button(summary.buttonTitle) {
                model.selection = .overview
                monitor.showRoomy()
            }
        case .scan:
            Button(summary.buttonTitle) {
                model.selection = .overview
                monitor.showRoomy()
                model.startScan()
            }
        case .none:
            EmptyView()
        }
    }

    @ViewBuilder
    private var settings: some View {
        if #available(macOS 14.0, *) {
            SettingsLink { Text("Settings…") }
                .keyboardShortcut(",", modifiers: .command)
        } else {
            Button("Settings…") {
                NSApp.activate(ignoringOtherApps: true)
                // macOS 13's name for the Settings window action. macOS 14 and later use SettingsLink above.
                NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
            }
            .keyboardShortcut(",", modifiers: .command)
        }
    }
}

/// Hands SwiftUI's `openWindow` action to the monitor, so the menu and a clicked notification can
/// reopen the main window after it was closed. Sits in the main window's background.
struct MainWindowOpener: View {
    let monitor: LowSpaceMonitor
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .accessibilityHidden(true)
            .onAppear { monitor.openMainWindow = { openWindow(id: "main") } }
    }
}

/// The low-space part of Settings: the menu bar item, the alert and its threshold.
struct LowSpaceSettingsSection: View {
    @ObservedObject var monitor: LowSpaceMonitor

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Low space")
                .font(.headline)
            Toggle("Show free space in the menu bar", isOn: $monitor.showInMenuBar)
            Toggle("Tell me when space is low", isOn: $monitor.alertsEnabled)
            Picker("Warn when less than:", selection: $monitor.thresholdBytes) {
                ForEach(monitor.thresholdChoices, id: \.self) { bytes in
                    Text(Format.bytes(bytes)).tag(bytes)
                }
            }
            .disabled(!monitor.alertsEnabled)
            .fixedSize()
            Text("Roomy only tells you. It never removes anything by itself.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            if monitor.notificationsDenied {
                HStack {
                    Text("Notifications are off for Roomy.")
                        .font(.callout)
                    Button("Open Notification Settings…") { openNotificationSettings() }
                }
            }
        }
    }

    /// Opens System Settings at Notifications, where the user can allow Roomy's alerts.
    private func openNotificationSettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.notifications") {
            NSWorkspace.shared.open(url)
        }
    }
}
