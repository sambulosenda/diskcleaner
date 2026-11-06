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

            Text("Core version \(coreVersion())")
                .font(.footnote)
                .foregroundStyle(.tertiary)
                .padding(.top, 8)
        }
        .padding(20)
        .frame(width: 440)
    }
}
