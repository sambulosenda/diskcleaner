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

/// The menu that opens from the menu bar item. Shows free space and opens Roomy. It never cleans
/// anything; scanning and cleaning happen in the main window, where the user chooses.
struct MenuBarMenu: View {
    @ObservedObject var monitor: LowSpaceMonitor

    var body: some View {
        Text(monitor.summaryText)
        if let low = monitor.lowSpaceText {
            Text(low)
        }
        Divider()
        Button("Open Roomy") { monitor.showRoomy() }
        Button("Check Again") { Task { await monitor.refresh() } }
        Divider()
        settings
        Button("Quit Roomy") { NSApp.terminate(nil) }
            .keyboardShortcut("q", modifiers: .command)
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
