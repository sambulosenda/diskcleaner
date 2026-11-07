import AppKit
import SwiftUI

/// The Settings window (⌘,): the mode picker, low-space alerts and the folders Roomy must keep.
struct SettingsView: View {
    @EnvironmentObject private var model: AppModel
    /// The menu bar item and low-space alert settings.
    @ObservedObject var lowSpace: LowSpaceMonitor

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
                .padding(.top, 8)

            Toggle("Scan when \(AppInfo.name) opens", isOn: $model.scanOnOpen)
                .padding(.top, 8)
            Text("Measures your home folder so \(AppInfo.name) and the menu bar show what you can free. Nothing is removed.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            Divider().padding(.vertical, 8)

            LowSpaceSettingsSection(monitor: lowSpace)

            Divider().padding(.vertical, 8)

            ProtectedFoldersSection()

            Text("Core version \(coreVersion())")
                .font(.footnote)
                .foregroundStyle(.tertiary)
                .padding(.top, 8)
        }
        .padding(20)
        .frame(width: 440)
    }
}

/// Whether Full Disk Access is on, with a way to turn it on.
struct FullDiskAccessSetting: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        LabeledContent("Full Disk Access:") {
            VStack(alignment: .leading, spacing: 6) {
                Text(statusText)
                if model.fullDiskAccess != .granted {
                    Button("Turn On…") { model.showAccessOnboarding() }
                        .accessibilityHint("Shows how to let \(AppInfo.name) measure every folder.")
                }
            }
        }
        .task { await model.refreshFullDiskAccess() }
    }

    private var statusText: String {
        switch model.fullDiskAccess {
        case .granted: return "On. \(AppInfo.name) can measure every folder."
        case .notGranted: return "Off. Some folders aren't measured."
        case .unknown, nil: return "\(AppInfo.name) can't tell."
        }
    }
}

/// "Folders to keep": Roomy never cleans anything inside these folders, or anything that holds one.
struct ProtectedFoldersSection: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Folders to keep")
                .font(.headline)
            Text("\(AppInfo.name) never cleans anything inside these folders.")
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
        panel.message = "Choose folders \(AppInfo.name) should never clean."
        guard panel.runModal() == .OK else { return }
        let urls = panel.urls
        Task { await model.addProtectedFolders(urls) }
    }
}
