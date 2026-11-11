import AppKit
import SwiftUI

/// The app uninstaller: installed apps on the left, and what the chosen one left behind on the right.
///
/// Choosing an app moves nothing. The Review button opens the same cleanup review as every other screen,
/// so nothing moves until the user confirms, and a move can be put back. Roomy can't move an app out of
/// the main Applications folder (its safety rules keep it inside your home folder), so for those it moves
/// what the app left behind and shows the app in Finder for you to drag to the Trash.
struct AppsScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        GeometryReader { geometry in
            HStack(spacing: 0) {
                AppList()
                    .frame(width: max(geometry.size.width * 0.38, 260))
                Divider()
                AppReviewPanel()
            }
        }
        .sheet(isPresented: Binding(
            get: { model.cleanupState != .closed },
            set: { if !$0 { model.closeCleanupReview() } }
        )) {
            CleanupReviewSheet()
                .environmentObject(model)
        }
        .onAppear { if model.appsState == .idle { model.loadApps() } }
    }
}

/// The installed apps, with their sizes. Apple's apps and Roomy itself are never listed.
struct AppList: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Apps")
                    .font(.headline)
                    .heading()
                Spacer()
                Button {
                    model.loadApps()
                } label: {
                    Label("Look again", systemImage: "arrow.clockwise")
                }
                .labelStyle(.iconOnly)
                .help("Look for apps again")
                .accessibilityLabel("Look for apps again")
                .disabled(model.appsState == .loading)
            }
            .padding(.horizontal, Space.l)
            .padding(.vertical, Space.s)
            Divider()
            content
        }
    }

    @ViewBuilder
    private var content: some View {
        switch model.appsState {
        case .idle, .loading:
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityLabel("Looking for apps")
        case .loaded:
            let apps = model.visibleApps
            if apps.isEmpty {
                CenteredMessage(symbol: "xmark.bin", title: "No apps to remove",
                                message: "Roomy didn't find any apps it can help remove. Apple's own apps aren't listed.") {
                    EmptyView()
                }
            } else {
                List(selection: $model.selectedAppID) {
                    ForEach(apps) { app in
                        AppRow(app: app)
                            .tag(app.appId)
                    }
                }
                .listStyle(.inset)
                .accessibilityLabel("Apps")
            }
        }
    }
}

/// Icon, name, version and size of one app.
struct AppRow: View {
    let app: AppSummary

    var body: some View {
        HStack(spacing: Space.s) {
            AppIcon(path: app.bundlePath)
                .frame(width: 28, height: 28)
            VStack(alignment: .leading, spacing: Space.xxs) {
                Text(app.name)
                    .font(.body.weight(.medium))
                if let version = app.version {
                    Text("Version \(version)")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: Space.s)
            if app.isOpen {
                Badge(text: "Open")
            }
            Text(Format.bytes(app.sizeBytes))
                .font(.body.monospacedDigit())
        }
        .padding(.vertical, Space.xs)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(app.name), \(Format.bytes(app.sizeBytes))\(app.isOpen ? ", open" : "")")
    }
}

/// The app's own icon, read by Finder. Reading it changes nothing.
struct AppIcon: View {
    let path: String

    var body: some View {
        Image(nsImage: NSWorkspace.shared.icon(forFile: path))
            .resizable()
            .aspectRatio(contentMode: .fit)
            .accessibilityHidden(true)
    }
}

/// What the chosen app left behind, with a tick box for each part.
struct AppReviewPanel: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        switch model.appReviewState {
        case .none:
            CenteredMessage(symbol: "xmark.bin", title: "Choose an app",
                            message: "Roomy shows the app and everything it keeps on your Mac. Nothing is removed until you review and confirm.") {
                EmptyView()
            }
        case .loading:
            ProgressView("Looking at what this app keeps\u{2026}")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "Roomy couldn't look at this app", message: message) {
                Button("Look again") { model.loadApps() }
            }
        case .loaded(let review):
            AppReviewContent(review: review)
        }
    }
}

/// The header, notes, parts and the Review button for one app.
struct AppReviewContent: View {
    @EnvironmentObject private var model: AppModel
    let review: AppReview

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
                .padding(Space.l)
            Divider()
            if review.parts.isEmpty {
                CenteredMessage(symbol: "checkmark.circle", title: "Nothing to remove",
                                message: review.note ?? "Roomy didn't find anything this app left in your home folder.") {
                    EmptyView()
                }
            } else {
                List {
                    if let note = review.note {
                        Label(note, systemImage: "info.circle")
                            .padding(.vertical, Space.xs)
                    }
                    ForEach(review.parts) { part in
                        AppPartRow(part: part)
                    }
                }
                .listStyle(.inset)
                .accessibilityLabel("What \(review.app.name) keeps")
                Divider()
                footer
                    .padding(.horizontal, Space.l)
                    .padding(.vertical, Space.s)
            }
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: Space.m) {
            AppIcon(path: review.app.bundlePath)
                .frame(width: 48, height: 48)
            VStack(alignment: .leading, spacing: Space.xs) {
                Text(review.app.name)
                    .font(.title2.weight(.semibold))
                    .heading()
                Text(locationText)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if review.app.isOpen {
                    Label("\(review.app.name) is open. Quit it before you remove anything.",
                          systemImage: "exclamationmark.circle")
                        .font(.callout)
                }
            }
            Spacer()
            Button("Show in Finder") {
                NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: review.app.bundlePath)])
            }
            .help("Shows the app in Finder. Nothing is moved.")
        }
    }

    private var locationText: String {
        switch review.app.location {
        case .applications:
            return "Roomy moves what \(review.app.name) keeps in your home folder. To remove the app itself, drag it from Applications to the Trash in Finder."
        case .homeApplications:
            return "This app is in the Applications folder in your home folder, so Roomy can move it too."
        }
    }

    private var footer: some View {
        HStack(spacing: Space.m) {
            let count = review.parts.filter { model.isPartChecked($0) }.count
            Text("\(count) \(count == 1 ? "item" : "items") chosen, \(Format.bytes(model.checkedPartBytes))")
                .font(.callout.monospacedDigit())
            Spacer()
            Button("Review\u{2026}") { model.reviewUninstall() }
                .buttonStyle(.borderedProminent)
                .disabled(count == 0 || review.app.isOpen)
                .accessibilityHint("Checks what would move to the Trash. Nothing is moved.")
        }
    }
}

/// One part of an app: tick box, title, what it is, and its size.
struct AppPartRow: View {
    @EnvironmentObject private var model: AppModel
    let part: AppPart

    var body: some View {
        HStack(alignment: .top, spacing: Space.s) {
            Toggle("Choose \(part.title)", isOn: Binding(
                get: { model.isPartChecked(part) },
                set: { model.setPartChecked($0, for: part) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .help("Choose this to review it for removal.")
            VStack(alignment: .leading, spacing: Space.xxs) {
                HStack(spacing: Space.s) {
                    Text(part.title)
                        .font(.body.weight(.medium))
                    if !part.sure {
                        Badge(text: "Check this", tint: .orange)
                    }
                }
                Text(part.explain)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if let path = part.path {
                    Text(path)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            Spacer(minLength: Space.m)
            Text(Format.bytes(part.sizeBytes))
                .font(.body.monospacedDigit())
        }
        .padding(.vertical, Space.xs)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(part.title), \(Format.bytes(part.sizeBytes))\(part.sure ? "" : ", check this")")
    }
}
