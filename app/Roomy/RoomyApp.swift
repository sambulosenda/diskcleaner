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
    /// Free space for the menu bar item and the low-space alert. Read only.
    @StateObject private var lowSpace = LowSpaceMonitor.live()

    /// Whether the menu bar item shows. SwiftUI writes this binding back on every scene update, so
    /// it only passes on real changes; setting the published value each time made an endless update loop.
    private var menuBarInserted: Binding<Bool> {
        Binding(
            get: { lowSpace.showInMenuBar },
            set: { newValue in
                if newValue != lowSpace.showInMenuBar { lowSpace.showInMenuBar = newValue }
            }
        )
    }

    var body: some Scene {
        Window("Roomy", id: "main") {
            ContentView()
                .environmentObject(model)
                .background(MainWindowOpener(monitor: lowSpace))
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
                Button("Search Roomy\u{2026}") { model.palette.toggle() }
                    .keyboardShortcut("k", modifiers: .command)
                    .disabled(!model.palette.isPresented && !model.palette.canOpen)
            }
        }

        Settings {
            SettingsView(lowSpace: lowSpace)
                .environmentObject(model)
        }

        MenuBarExtra(isInserted: menuBarInserted) {
            MenuBarMenu(monitor: lowSpace)
        } label: {
            MenuBarLabel(monitor: lowSpace)
        }
        .menuBarExtraStyle(.menu)
    }
}
