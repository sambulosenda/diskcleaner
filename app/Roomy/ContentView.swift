import SwiftUI

/// Placeholder main window until the Overview screen arrives (T10).
/// Shows the Rust core version and the free space on the home folder's disk, to prove the bridge works.
struct ContentView: View {
    @State private var spaceText = "Checking free space…"

    var body: some View {
        VStack(spacing: 12) {
            Text("Roomy")
                .font(.title)
            Text("Core version \(coreVersion())")
                .foregroundStyle(.secondary)
            Text(spaceText)
        }
        .padding()
        .frame(minWidth: 480, minHeight: 320)
        .task {
            spaceText = Self.describeHomeDisk()
        }
    }

    /// Asks the core for the space on the disk that holds the home folder, as a short sentence.
    static func describeHomeDisk() -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        do {
            let summary = try diskSummary(path: home)
            return "\(formatBytes(summary.freeBytes)) free of \(formatBytes(summary.totalBytes))"
        } catch {
            return "Free space is not available right now."
        }
    }

    /// Formats a byte count the way Finder does. The core always works in bytes.
    static func formatBytes(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }
}
