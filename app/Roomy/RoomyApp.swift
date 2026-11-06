import SwiftUI

/// App entry point. The UI only; all real work happens in the Rust core.
@main
struct RoomyApp: App {
    // T10 runs on mock data. T11 swaps in the real core client here.
    @StateObject private var model = AppModel(client: MockCoreClient.fromLaunchArguments())

    var body: some Scene {
        Window("Roomy", id: "main") {
            ContentView()
                .environmentObject(model)
        }
        .defaultSize(width: 1120, height: 740)
        .commands {
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
