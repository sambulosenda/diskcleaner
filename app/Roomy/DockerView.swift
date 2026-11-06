import SwiftUI

/// The Docker screen's state. Asks the core what Docker uses, and runs the two clean-ups the core allows
/// (unused images and build cache), each after a review. The app sends a clean-up kind, never a command.
@MainActor
final class DockerModel: ObservableObject {
    /// Where the screen has got to.
    enum Phase: Equatable {
        /// Asking Docker.
        case loading
        /// Docker's answer.
        case loaded(DockerOverview)
    }

    /// Where a review has got to.
    enum Review: Equatable {
        /// No review open.
        case closed
        /// Asking Docker what it would remove.
        case previewing(DockerCleanupKind)
        /// The list, waiting for the user.
        case ready(DockerCleanupPreview)
        /// Docker is removing it.
        case running(DockerCleanupPreview)
        /// Finished.
        case done(DockerCleanupResult)
        /// Could not preview or run, in plain words.
        case failed(String)

        var isOpen: Bool { self != .closed }
        var isBusy: Bool {
            switch self {
            case .previewing, .running: return true
            default: return false
            }
        }
    }

    @Published private(set) var phase: Phase = .loading
    @Published private(set) var review: Review = .closed

    private let client: CoreClient

    init(client: CoreClient) {
        self.client = client
    }

    /// Asks Docker again what it uses.
    func load() async {
        phase = .loading
        phase = .loaded(await client.dockerOverview())
    }

    /// Opens the review for one clean-up. Nothing is removed.
    func startReview(_ kind: DockerCleanupKind) {
        review = .previewing(kind)
        Task {
            do {
                review = .ready(try await client.previewDockerCleanup(kind: kind))
            } catch {
                review = .failed(DockerText.message(for: error))
            }
        }
    }

    /// Runs the reviewed clean-up. The core checks again first and refuses if anything changed.
    func confirm() {
        guard case .ready(let preview) = review, let plan = preview.plan else { return }
        review = .running(preview)
        Task {
            do {
                review = .done(try await client.runDockerCleanup(plan: plan))
            } catch {
                review = .failed(DockerText.message(for: error))
            }
            await load()
        }
    }

    /// Closes the review, dropping its plan if it was never run.
    func closeReview() {
        if case .ready(let preview) = review, let plan = preview.plan {
            Task { await client.discardDockerCleanup(plan: plan) }
        }
        review = .closed
    }
}

/// The words the Docker screen shows. Plain British English, no jargon beyond Docker's own names.
enum DockerText {
    static func title(_ kind: DockerCleanupKind) -> String {
        switch kind {
        case .unusedImages: return "Unused images"
        case .buildCache: return "Build cache"
        }
    }

    static func explain(_ kind: DockerCleanupKind) -> String {
        switch kind {
        case .unusedImages:
            return "Old image layers that no tag and no container uses, often left behind by rebuilding an image."
        case .buildCache:
            return "Steps Docker saved to make builds faster. The next build may take longer."
        }
    }

    /// Text for a thrown error, without a path.
    static func message(for error: Error) -> String {
        if let error = error as? CoreError {
            switch error {
            case .DockerUnavailable(let reason), .CleanupUnavailable(let reason):
                return reason
            default:
                break
            }
        }
        return "Something went wrong. Nothing was removed."
    }
}

/// The Docker screen: what Docker uses, and the two clean-ups Roomy can run for you.
struct DockerScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        DockerContent(client: model.client)
    }
}

/// Holds the screen's own model, made once from the app's core client.
private struct DockerContent: View {
    @StateObject private var docker: DockerModel

    init(client: CoreClient) {
        _docker = StateObject(wrappedValue: DockerModel(client: client))
    }

    var body: some View {
        Group {
            switch docker.phase {
            case .loading:
                ProgressView("Asking Docker\u{2026}")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .loaded(let overview):
                DockerOverviewView(overview: overview, docker: docker)
            }
        }
        .task { await docker.load() }
        .sheet(isPresented: Binding(
            get: { docker.review.isOpen },
            set: { if !$0 { docker.closeReview() } }
        )) {
            DockerReviewSheet(docker: docker)
        }
    }
}

/// Docker's answer, or why there isn't one.
private struct DockerOverviewView: View {
    let overview: DockerOverview
    @ObservedObject var docker: DockerModel

    var body: some View {
        switch overview.status {
        case .notInstalled:
            CenteredMessage(symbol: "shippingbox", title: "Docker isn't installed",
                            message: "Roomy looked in the usual places and didn't find Docker.") {
                retry
            }
        case .notRunning:
            CenteredMessage(symbol: "shippingbox", title: "Docker isn't running",
                            message: "Open Docker, then try again. Roomy needs Docker running to see what it uses.") {
                retry
            }
        case .unavailable(let reason):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "Roomy couldn't ask Docker",
                            message: reason) {
                retry
            }
        case .ready:
            ready
        }
    }

    private var retry: some View {
        Button("Try again") { Task { await docker.load() } }
    }

    private var ready: some View {
        List {
            if let disk = overview.diskImageBytes {
                Section {
                    LabeledContent("Docker disk image", value: Format.bytes(disk))
                    Text("Docker Desktop keeps everything below inside this one file on your Mac.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
            Section(header: Text("Roomy can clean these up")) {
                cleanupRow(.unusedImages, row: overview.images,
                           detail: "\(Format.count(overview.images.count)) images, \(Format.count(overview.images.active)) in use")
                cleanupRow(.buildCache, row: overview.buildCache,
                           detail: "\(Format.count(overview.buildCache.count)) cache records")
            }
            Section(header: Text("Shown only")) {
                infoRow(title: "Stopped containers",
                        size: overview.containers.reclaimableBytes,
                        explain: "A stopped container can hold work that isn't saved anywhere else, so Roomy doesn't remove them. To remove them yourself, run:",
                        command: "docker container prune")
                infoRow(title: "Volumes",
                        size: overview.volumes.sizeBytes,
                        explain: "Volumes hold your databases and other data. Roomy never removes them.",
                        command: nil)
            }
            Section {
                Button("Check again") { Task { await docker.load() } }
            }
        }
        .listStyle(.inset)
        .accessibilityLabel("Docker")
    }

    private func cleanupRow(_ kind: DockerCleanupKind, row: DockerUsageRow, detail: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(DockerText.title(kind)).font(.headline)
                Text(DockerText.explain(kind)).font(.callout).foregroundStyle(.secondary)
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            Text(Format.bytes(row.reclaimableBytes))
                .font(.body.monospacedDigit())
            Button("Review\u{2026}") { docker.startReview(kind) }
                .disabled(row.reclaimableBytes == 0)
                .accessibilityHint("Lists what Docker would remove. Nothing is removed yet.")
        }
        .padding(.vertical, 4)
    }

    private func infoRow(title: String, size: UInt64, explain: String, command: String?) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(title).font(.headline)
                Spacer()
                Text(Format.bytes(size)).font(.body.monospacedDigit())
            }
            Text(explain).font(.callout).foregroundStyle(.secondary)
            if let command {
                Text(command)
                    .font(.system(.callout, design: .monospaced))
                    .textSelection(.enabled)
            }
        }
        .padding(.vertical, 4)
    }
}

/// The review: what Docker would remove, and the button that removes it.
private struct DockerReviewSheet: View {
    @ObservedObject var docker: DockerModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            content
            Spacer(minLength: 0)
            Divider()
            footer
        }
        .padding(20)
        .frame(width: 520, height: 440)
        .interactiveDismissDisabled(docker.review.isBusy)
    }

    @ViewBuilder
    private var content: some View {
        switch docker.review {
        case .closed:
            EmptyView()
        case .previewing(let kind):
            Text(DockerText.title(kind)).font(.title2.weight(.semibold))
            ProgressView("Asking Docker what it would remove\u{2026}")
        case .ready(let preview), .running(let preview):
            Text(DockerText.title(preview.kind)).font(.title2.weight(.semibold))
            if preview.items.isEmpty {
                Text("There is nothing to remove right now.")
            } else {
                Text("Docker would remove \(Format.items(UInt64(preview.items.count))), about \(Format.bytes(preview.totalBytes)). This can't be undone, but Docker downloads or rebuilds these when they are needed again.")
                    .fixedSize(horizontal: false, vertical: true)
                List(preview.items, id: \.id) { item in
                    HStack {
                        Text(item.id).font(.system(.body, design: .monospaced))
                        Spacer()
                        Text(item.age).foregroundStyle(.secondary)
                        Text(Format.bytes(item.sizeBytes)).font(.body.monospacedDigit())
                    }
                }
                .listStyle(.bordered)
                Text("Roomy will run: \(preview.commandLine)")
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
        case .done(let result):
            Text(DockerText.title(result.kind)).font(.title2.weight(.semibold))
            if result.success {
                if let freed = result.reclaimedBytes {
                    Text("Docker freed \(Format.bytes(freed)).")
                } else {
                    Text("Docker finished.")
                }
            } else {
                Text("Docker reported a problem, so it may not have removed everything.")
            }
            if !result.savedToHistory {
                Text("This couldn't be saved to History.").foregroundStyle(.secondary)
            }
        case .failed(let message):
            Text("Nothing was removed").font(.title2.weight(.semibold))
            Text(message)
        }
    }

    @ViewBuilder
    private var footer: some View {
        HStack {
            if case .running = docker.review {
                ProgressView().controlSize(.small)
                Text("Docker is removing these\u{2026}")
            }
            Spacer()
            switch docker.review {
            case .ready(let preview) where preview.plan != nil:
                Button("Cancel", role: .cancel) { docker.closeReview() }
                    .keyboardShortcut(.cancelAction)
                Button("Remove \(Format.bytes(preview.totalBytes))") { docker.confirm() }
                    .buttonStyle(.borderedProminent)
                    .accessibilityHint("Runs \(preview.commandLine). This can't be undone.")
            case .previewing, .running:
                EmptyView()
            default:
                Button("Done") { docker.closeReview() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }
}
