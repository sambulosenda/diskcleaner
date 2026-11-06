import SwiftUI

/// App entry point. The UI only; all real work happens in the Rust core.
@main
struct RoomyApp: App {
    /// The real core, unless the app was launched with `-RoomyMockScenario <name>` to show made-up data.
    @StateObject private var model = AppModel(
        client: (MockCoreClient.fromLaunchArguments() as CoreClient?) ?? RustCoreClient()
    )
    /// Sparkle updates. Off in development builds; see UpdaterController.
    @StateObject private var updater = UpdaterController()

    var body: some Scene {
        Window("Roomy", id: "main") {
            ContentView()
                .environmentObject(model)
        }
        .defaultSize(width: 1120, height: 740)
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…") { updater.checkForUpdates() }
                    .disabled(!updater.canCheckForUpdates)
            }
            CommandGroup(after: .newItem) {
                Button("Scan") { model.startScan() }
                    .keyboardShortcut("r", modifiers: .command)
                    .disabled(model.isScanning)
            }
        }

        Settings {
            SettingsView()
                .environmentObject(model)
        }
    }
}
