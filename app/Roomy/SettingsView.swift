import AppKit
import SwiftUI

/// The Settings window (⌘,), in three tabs like other Mac apps: General (the mode, Full Disk Access
/// and scanning), Alerts (the menu bar item, low-space alerts and the weekly note) and Folders to Keep.
struct SettingsView: View {
    @EnvironmentObject private var model: AppModel
    /// The menu bar item and low-space alert settings.
    @ObservedObject var lowSpace: LowSpaceMonitor
    /// The weekly note's setting.
    @ObservedObject var weeklyNote: WeeklyNoteModel
    /// Settings opens on General, where the toolbar's mode button says the mode is changed.
    @State private var tab = SettingsTab.general

    var body: some View {
        TabView(selection: $tab) {
            GeneralSettings()
                .settingsPage()
                .tabItem { Label(SettingsTab.general.title, systemImage: SettingsTab.general.symbol) }
                .tag(SettingsTab.general)
            AlertSettings(lowSpace: lowSpace, weeklyNote: weeklyNote)
                .settingsPage()
                .tabItem { Label(SettingsTab.alerts.title, systemImage: SettingsTab.alerts.symbol) }
                .tag(SettingsTab.alerts)
            ProtectedFoldersSection()
                .settingsPage()
                .tabItem { Label(SettingsTab.folders.title, systemImage: SettingsTab.folders.symbol) }
                .tag(SettingsTab.folders)
        }
        .frame(width: SettingsTab.width)
    }
}

/// The tabs of the Settings window.
enum SettingsTab: String, CaseIterable {
    case general
    case alerts
    case folders

    /// Every tab is this wide, so the window only changes height between tabs.
    static let width: CGFloat = 480

    var title: String {
        switch self {
        case .general: return "General"
        case .alerts: return "Alerts"
        case .folders: return "Folders to Keep"
        }
    }

    var symbol: String {
        switch self {
        case .general: return "gearshape"
        case .alerts: return "bell"
        case .folders: return "lock.shield"
        }
    }
}

private extension View {
    /// The padding and alignment every Settings tab shares.
    func settingsPage() -> some View {
        padding(Space.xl)
            .frame(maxWidth: .infinity, alignment: .topLeading)
    }
}

/// General: the mode, Full Disk Access and scanning when Roomy opens.
struct GeneralSettings: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        Form {
            Picker("Mode:", selection: $model.mode) {
                ForEach(Mode.allCases, id: \.self) { mode in
                    Text(mode.label).tag(mode)
                }
            }
            .pickerStyle(.radioGroup)
            .accessibilityLabel("Mode")

            Text(model.mode.explanation)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            FullDiskAccessSetting()
                .padding(.top, Space.s)

            Toggle("Scan when Roomy opens", isOn: $model.scanOnOpen)
                .padding(.top, Space.s)
            Text("Measures your home folder so Roomy and the menu bar show what you can free. Nothing is removed.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            Text("Core version \(coreVersion())")
                .font(.footnote)
                .foregroundStyle(.tertiary)
                .padding(.top, Space.s)
        }
    }
}

/// Alerts: the menu bar item, the low-space alert and the weekly note.
struct AlertSettings: View {
    @ObservedObject var lowSpace: LowSpaceMonitor
    @ObservedObject var weeklyNote: WeeklyNoteModel

    var body: some View {
        VStack(alignment: .leading, spacing: Space.l) {
            LowSpaceSettingsSection(monitor: lowSpace)
            Divider()
            WeeklyNoteSettingsSection(weeklyNote: weeklyNote)
        }
    }
}

/// Whether Full Disk Access is on, with a way to turn it on.
struct FullDiskAccessSetting: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        LabeledContent("Full Disk Access:") {
            VStack(alignment: .leading, spacing: Space.xs) {
                Text(statusText)
                if model.fullDiskAccess != .granted {
                    Button("Turn On…") { model.showAccessOnboarding() }
                        .accessibilityHint("Shows how to let Roomy measure every folder.")
                }
            }
        }
        .task { await model.refreshFullDiskAccess() }
    }

    private var statusText: String {
        switch model.fullDiskAccess {
        case .granted: return "On. Roomy can measure every folder."
        case .notGranted: return "Off. Some folders aren't measured."
        case .unknown, nil: return "Roomy can't tell."
        }
    }
}

/// "Folders to keep": Roomy never cleans anything inside these folders, or anything that holds one.
struct ProtectedFoldersSection: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            Text("Folders to keep")
                .font(.headline)
                .heading()
            Text("Roomy never cleans anything inside these folders.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            if model.protectedFolders.isEmpty {
                Text("No folders yet.")
                    .font(.callout)
                    .foregroundStyle(.tertiary)
            } else {
                ForEach(model.protectedFolders, id: \.self) { folder in
                    HStack {
                        Image(systemName: "folder")
                            .foregroundStyle(.secondary)
                            .accessibilityHidden(true)
                        Text((folder as NSString).abbreviatingWithTildeInPath)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .help(folder)
                        Spacer()
                        Button {
                            Task { await model.removeProtectedFolder(folder) }
                        } label: {
                            Image(systemName: "minus.circle")
                        }
                        .buttonStyle(.borderless)
                        .help("Stop keeping this folder")
                        .accessibilityLabel("Stop keeping \((folder as NSString).lastPathComponent)")
                    }
                }
            }

            if let message = model.protectedFoldersMessage {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Button("Add Folder…") { chooseFolders() }
        }
    }

    /// Asks the user for one or more folders with the standard open panel.
    private func chooseFolders() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser
        panel.prompt = "Keep"
        panel.message = "Choose folders Roomy should never clean."
        guard panel.runModal() == .OK else { return }
        let urls = panel.urls
        Task { await model.addProtectedFolders(urls) }
    }
}
