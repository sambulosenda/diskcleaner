import SwiftUI

/// The Settings window (⌘,). For now it holds the mode picker.
struct SettingsView: View {
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
                .padding(.top, 8)

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
