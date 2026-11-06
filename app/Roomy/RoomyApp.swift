import SwiftUI

/// App entry point. The UI only; all real work happens in the Rust core.
@main
struct RoomyApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
