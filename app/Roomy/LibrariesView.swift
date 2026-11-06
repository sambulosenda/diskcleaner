import AppKit
import SwiftUI

/// The Photos, Mail and Music screen's state: how much each of the Mac's own libraries takes, from the
/// last scan. Shown only. Roomy never removes anything inside these libraries; it points to the app that
/// manages each one instead (PRD E6, E7, C1).
@MainActor
final class LibrariesModel: ObservableObject {
    /// Where the screen has got to.
    enum Phase: Equatable {
        /// No scan has finished yet.
        case noScan
        /// Asking the core.
        case loading
        /// The rows, in the core's order.
        case loaded([LibrarySize])
        /// The core couldn't answer, in plain words.
        case failed(String)
    }

    @Published private(set) var phase: Phase = .noScan

    private let client: CoreClient

    init(client: CoreClient) {
        self.client = client
    }

    /// Asks the core about the finished scan `handle`, or shows "scan first" when there is none.
    func load(handle: ScanHandle?) async {
        guard let handle else {
            phase = .noScan
            return
        }
        phase = .loading
        do {
            phase = .loaded(try await client.libraries(handle: handle))
        } catch {
            phase = .failed("Roomy couldn't measure your libraries. Please scan again.")
        }
    }

    /// The rows worth showing, largest first: those found, and those Roomy couldn't read. Ties keep the
    /// core's order.
    static func shown(_ rows: [LibrarySize]) -> [LibrarySize] {
        rows.enumerated()
            .filter { $0.element.found || $0.element.needsAccess }
            .sorted { $0.element.bytes != $1.element.bytes ? $0.element.bytes > $1.element.bytes : $0.offset < $1.offset }
            .map(\.element)
    }
}

/// Plain words and the owning app for each library. All of it lives here; the core sends only kinds
/// and sizes.
enum LibraryText {
    static func title(_ kind: LibraryKind) -> String {
        switch kind {
        case .photos: return "Photos"
        case .music: return "Music"
        case .tv: return "TV"
        case .mail: return "Mail"
        case .messagesAttachments: return "Messages attachments"
        case .finalCut: return "Final Cut Pro libraries"
        }
    }

    static func symbol(_ kind: LibraryKind) -> String {
        switch kind {
        case .photos: return "photo"
        case .music: return "music.note"
        case .tv: return "tv"
        case .mail: return "envelope"
        case .messagesAttachments: return "message"
        case .finalCut: return "film"
        }
    }

    static func explain(_ kind: LibraryKind) -> String {
        switch kind {
        case .photos:
            return "Your photos and videos, kept by the Photos app."
        case .music:
            return "Songs, music videos and podcasts you've downloaded or added to Music."
        case .tv:
            return "Films and TV programmes you've downloaded or added to the TV app."
        case .mail:
            return "Your email, and the attachments Mail has downloaded."
        case .messagesAttachments:
            return "Photos, videos and files sent or received in Messages."
        case .finalCut:
            return "Your Final Cut Pro projects with their media and render files."
        }
    }

    /// How to free space with the app's own tools.
    static func howToFree(_ kind: LibraryKind) -> String {
        switch kind {
        case .photos:
            return "In Photos, delete what you don't need, then empty Recently Deleted. If you use iCloud Photos, turning on Optimise Mac Storage in Photos Settings keeps smaller copies on this Mac."
        case .music:
            return "In Music, select songs you've downloaded and choose Remove Download. They stay in your library if they're in iCloud."
        case .tv:
            return "In the TV app, remove downloads you've already watched."
        case .mail:
            return "In Mail, delete large messages, then choose Mailbox > Erase Deleted Items. In Mail Settings > Accounts you can stop Mail downloading every attachment."
        case .messagesAttachments:
            return "To see the largest attachments, open System Settings > General > Storage and click the info button next to Messages. In Messages Settings you can also keep messages for one year or 30 days instead of for ever."
        case .finalCut:
            return "In Final Cut Pro, select a library and choose File > Delete Generated Library Files. Render and proxy files are also listed in Apps and caches."
        }
    }

    /// The app that manages the library.
    static func appBundleID(_ kind: LibraryKind) -> String {
        switch kind {
        case .photos: return "com.apple.Photos"
        case .music: return "com.apple.Music"
        case .tv: return "com.apple.TV"
        case .mail: return "com.apple.mail"
        case .messagesAttachments: return "com.apple.MobileSMS"
        case .finalCut: return "com.apple.FinalCut"
        }
    }

    static func appName(_ kind: LibraryKind) -> String {
        switch kind {
        case .photos: return "Photos"
        case .music: return "Music"
        case .tv: return "TV"
        case .mail: return "Mail"
        case .messagesAttachments: return "Messages"
        case .finalCut: return "Final Cut Pro"
        }
    }

    /// The size line, such as "41.3 GB in 2 libraries".
    static func size(_ row: LibrarySize) -> String {
        if !row.found { return "Not measured" }
        let bytes = Format.bytes(row.bytes)
        if row.libraryCount > 1 { return "\(bytes) in \(row.libraryCount) libraries" }
        return bytes
    }
}

/// Opens apps and the one folder this screen shows in Finder. Nothing here changes a file.
enum LibraryActions {
    /// Where the app is installed, or `nil` if it isn't.
    static func appURL(for kind: LibraryKind) -> URL? {
        NSWorkspace.shared.urlForApplication(withBundleIdentifier: LibraryText.appBundleID(kind))
    }

    /// Opens the app that manages the library.
    static func openApp(for kind: LibraryKind) {
        guard let url = appURL(for: kind) else { return }
        NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration(), completionHandler: nil)
    }

    /// The Messages attachments folder. A fixed place inside the home folder, built here rather than sent
    /// by the core, and only ever opened in Finder (PRD E6). Roomy never removes anything in it.
    static var messagesAttachmentsFolder: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Messages/Attachments", isDirectory: true)
    }

    /// Opens the Messages attachments folder in Finder.
    static func showMessagesAttachments() {
        NSWorkspace.shared.open(messagesAttachmentsFolder)
    }
}

/// The Photos, Mail and Music screen.
struct LibrariesScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        LibrariesContent(client: model.client, handle: model.finishedScan)
    }
}

/// Holds the screen's own model, made once from the app's core client.
private struct LibrariesContent: View {
    let handle: ScanHandle?
    @StateObject private var libraries: LibrariesModel
    @EnvironmentObject private var model: AppModel

    init(client: CoreClient, handle: ScanHandle?) {
        self.handle = handle
        _libraries = StateObject(wrappedValue: LibrariesModel(client: client))
    }

    var body: some View {
        Group {
            switch libraries.phase {
            case .noScan:
                CenteredMessage(symbol: "photo.on.rectangle", title: "Scan to see your libraries",
                                message: "Roomy measures Photos, Mail, Music and Messages as part of a scan. Nothing is removed.") {
                    Button("Scan") { model.startScan() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(model.isScanning)
                }
            case .loading:
                ProgressView("Measuring\u{2026}")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .failed(let message):
                CenteredMessage(symbol: "exclamationmark.triangle", title: "Roomy couldn't measure your libraries",
                                message: message) {
                    Button("Scan again") { model.startScan() }
                }
            case .loaded(let rows):
                LibrariesList(rows: LibrariesModel.shown(rows))
            }
        }
        .task(id: handle?.id) { await libraries.load(handle: handle) }
    }
}

/// The libraries that were found, largest first.
private struct LibrariesList: View {
    let rows: [LibrarySize]
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if rows.isEmpty {
            CenteredMessage(symbol: "photo.on.rectangle", title: "No libraries found",
                            message: "Roomy didn't find Photos, Music, Mail or Messages files in your home folder.") {
                EmptyView()
            }
        } else {
            Form {
                Section {
                    Text("These belong to the apps that manage them, so Roomy doesn't remove anything inside them. Use each app's own tools, and nothing you need is lost.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                Section {
                    ForEach(rows, id: \.kind) { row in
                        LibraryRow(row: row)
                    }
                }
                if rows.contains(where: \.needsAccess) {
                    Section {
                        HStack {
                            Text("Some of these are private to macOS. Turn on Full Disk Access so Roomy can measure them.")
                                .font(.callout)
                            Spacer()
                            Button("Full Disk Access\u{2026}") { model.showAccessOnboarding() }
                        }
                    }
                }
            }
            .formStyle(.grouped)
        }
    }
}

/// One library: its size, what it is, how to free space, and a button to open its app.
private struct LibraryRow: View {
    let row: LibrarySize

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Label(LibraryText.title(row.kind), systemImage: LibraryText.symbol(row.kind))
                    .font(.headline)
                Spacer()
                Text(LibraryText.size(row))
                    .font(.body.monospacedDigit())
            }
            Text(LibraryText.explain(row.kind))
                .font(.callout)
                .foregroundStyle(.secondary)
            if row.needsAccess {
                Label(accessNote, systemImage: "lock")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Text(LibraryText.howToFree(row.kind))
                .font(.callout)
            HStack {
                if LibraryActions.appURL(for: row.kind) != nil {
                    Button("Open \(LibraryText.appName(row.kind))") { LibraryActions.openApp(for: row.kind) }
                }
                if row.kind == .messagesAttachments && row.found {
                    Button("Show in Finder") { LibraryActions.showMessagesAttachments() }
                }
            }
            .padding(.top, 2)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .contain)
    }

    private var accessNote: String {
        row.found
            ? "Some of it couldn't be read, so it may be larger."
            : "Roomy couldn't read this without Full Disk Access."
    }
}
