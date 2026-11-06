import SwiftUI

/// The Tool clean-ups screen's state: Homebrew's `brew cleanup`, deleting unavailable simulators, and
/// uninstalling one Android SDK package. Each runs only after a review. The app sends a clean-up kind (and
/// for the Android SDK, one package id from the review), never a command or a path.
@MainActor
final class ToolCleanupModel: ObservableObject {
    /// Where a review has got to.
    enum Review: Equatable {
        /// No review open.
        case closed
        /// Asking the tool what it would remove.
        case previewing(ToolCleanupKind)
        /// The list, waiting for the user.
        case ready(ToolCleanupPreview)
        /// The tool is removing it.
        case running(ToolCleanupPreview)
        /// Finished.
        case done(ToolCleanupResult)
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

    @Published private(set) var review: Review = .closed
    /// For the Android SDK: the one package the user chose from the review.
    @Published var selectedPackage: String?

    private let client: CoreClient

    init(client: CoreClient) {
        self.client = client
    }

    /// Opens the review for one clean-up. Nothing is removed.
    func startReview(_ kind: ToolCleanupKind) {
        review = .previewing(kind)
        selectedPackage = nil
        Task {
            do {
                review = .ready(try await client.previewToolCleanup(kind: kind))
            } catch {
                review = .failed(ToolText.message(for: error))
            }
        }
    }

    /// Whether the review can be confirmed: it has a plan, and for the Android SDK a package from it is chosen.
    var canConfirm: Bool {
        guard case .ready(let preview) = review, preview.plan != nil else { return false }
        if preview.kind == .androidSdkPackage {
            return preview.items.contains { $0.id == selectedPackage }
        }
        return true
    }

    /// Runs the reviewed clean-up. The core checks again first and refuses if anything changed.
    func confirm() {
        guard canConfirm, case .ready(let preview) = review, let plan = preview.plan else { return }
        let package = preview.kind == .androidSdkPackage ? selectedPackage : nil
        review = .running(preview)
        Task {
            do {
                review = .done(try await client.runToolCleanup(plan: plan, package: package))
            } catch {
                review = .failed(ToolText.message(for: error))
            }
        }
    }

    /// Closes the review, dropping its plan if it was never run.
    func closeReview() {
        if case .ready(let preview) = review, let plan = preview.plan {
            Task { await client.discardToolCleanup(plan: plan) }
        }
        review = .closed
        selectedPackage = nil
    }
}

/// The words the Tool clean-ups screen shows. Plain British English.
enum ToolText {
    static func title(_ kind: ToolCleanupKind) -> String {
        switch kind {
        case .homebrew: return "Homebrew"
        case .unavailableSimulators: return "Unavailable simulators"
        case .androidSdkPackage: return "Android SDK packages"
        }
    }

    static func explain(_ kind: ToolCleanupKind) -> String {
        switch kind {
        case .homebrew:
            return "Older versions of things you installed with Homebrew, and downloads it kept after installing. Homebrew doesn't need them."
        case .unavailableSimulators:
            return "Simulators for iOS, watchOS or tvOS versions that are no longer installed in Xcode. They can't run any more."
        case .androidSdkPackage:
            return "Parts of the Android SDK, such as old build tools and system images. Choose one to uninstall. Android Studio can install it again if a project needs it."
        }
    }

    static func symbol(_ kind: ToolCleanupKind) -> String {
        switch kind {
        case .homebrew: return "mug"
        case .unavailableSimulators: return "iphone"
        case .androidSdkPackage: return "candybarphone"
        }
    }

    static func asking(_ kind: ToolCleanupKind) -> String {
        switch kind {
        case .homebrew: return "Asking Homebrew what it would remove\u{2026}"
        case .unavailableSimulators: return "Asking Xcode which simulators are unavailable\u{2026}"
        case .androidSdkPackage: return "Asking sdkmanager what is installed. This can take a minute\u{2026}"
        }
    }

    /// Text for a thrown error, without a path.
    static func message(for error: Error) -> String {
        if let error = error as? CoreError {
            switch error {
            case .ToolUnavailable(let reason), .CleanupUnavailable(let reason):
                return reason
            default:
                break
            }
        }
        return "Something went wrong. Nothing was removed."
    }
}

/// The Tool clean-ups screen: the three developer tool commands Roomy can run for you.
struct ToolCleanupsScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        ToolCleanupsContent(client: model.client)
    }
}

/// Holds the screen's own model, made once from the app's core client.
private struct ToolCleanupsContent: View {
    @StateObject private var tools: ToolCleanupModel

    init(client: CoreClient) {
        _tools = StateObject(wrappedValue: ToolCleanupModel(client: client))
    }

    var body: some View {
        List {
            Section {
                Text("Each tool lists what it would remove first. Nothing is removed until you confirm.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Section {
                row(.homebrew)
                row(.unavailableSimulators)
                row(.androidSdkPackage)
            }
        }
        .listStyle(.inset)
        .accessibilityLabel("Tool clean-ups")
        .sheet(isPresented: Binding(
            get: { tools.review.isOpen },
            set: { if !$0 { tools.closeReview() } }
        )) {
            ToolReviewSheet(tools: tools)
        }
    }

    private func row(_ kind: ToolCleanupKind) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Image(systemName: ToolText.symbol(kind))
                .frame(width: 20)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 4) {
                Text(ToolText.title(kind)).font(.headline)
                Text(ToolText.explain(kind)).font(.callout).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            Button("Review\u{2026}") { tools.startReview(kind) }
                .accessibilityHint("Lists what would be removed. Nothing is removed yet.")
        }
        .padding(.vertical, 4)
    }
}

/// The review: what the tool would remove, and the button that removes it.
private struct ToolReviewSheet: View {
    @ObservedObject var tools: ToolCleanupModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            content
            Spacer(minLength: 0)
            Divider()
            footer
        }
        .padding(20)
        .frame(width: 560, height: 460)
        .interactiveDismissDisabled(tools.review.isBusy)
    }

    @ViewBuilder
    private var content: some View {
        switch tools.review {
        case .closed:
            EmptyView()
        case .previewing(let kind):
            Text(ToolText.title(kind)).font(.title2.weight(.semibold))
            ProgressView(ToolText.asking(kind))
        case .ready(let preview), .running(let preview):
            Text(ToolText.title(preview.kind)).font(.title2.weight(.semibold))
            if preview.items.isEmpty {
                Text("There is nothing to remove right now.")
            } else if preview.kind == .androidSdkPackage {
                packagePicker(preview)
            } else {
                Text(summary(preview))
                    .fixedSize(horizontal: false, vertical: true)
                List(preview.items, id: \.id) { item in
                    itemRow(item)
                }
                .listStyle(.bordered)
                Text("Roomy will run: \(preview.commandLine)")
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
        case .done(let result):
            Text(ToolText.title(result.kind)).font(.title2.weight(.semibold))
            if result.success {
                if let freed = result.reclaimedBytes {
                    Text("Done. About \(Format.bytes(freed)) freed.")
                } else {
                    Text("Done.")
                }
            } else {
                Text("The tool reported a problem, so it may not have removed everything. History has its full answer.")
            }
            Text(result.commandLine)
                .font(.system(.callout, design: .monospaced))
                .foregroundStyle(.secondary)
            if !result.savedToHistory {
                Text("This couldn't be saved to History.").foregroundStyle(.secondary)
            }
        case .failed(let message):
            Text("Nothing was removed").font(.title2.weight(.semibold))
            Text(message)
        }
    }

    private func summary(_ preview: ToolCleanupPreview) -> String {
        switch preview.kind {
        case .homebrew:
            return "Homebrew would remove \(Format.items(UInt64(preview.items.count))), about \(Format.bytes(preview.totalBytes)). This can't be undone, but Homebrew downloads them again if you install them."
        case .unavailableSimulators:
            return "Xcode would delete \(Format.items(UInt64(preview.items.count))), about \(Format.bytes(preview.totalBytes)), with their apps and data. This can't be undone."
        case .androidSdkPackage:
            return ""
        }
    }

    @ViewBuilder
    private func packagePicker(_ preview: ToolCleanupPreview) -> some View {
        Text("Choose one package to uninstall. This can't be undone, but Android Studio's SDK Manager can install it again.")
            .fixedSize(horizontal: false, vertical: true)
        List(preview.items, id: \.id, selection: $tools.selectedPackage) { item in
            itemRow(item).tag(item.id)
        }
        .listStyle(.bordered)
        .disabled(tools.review.isBusy)
        .accessibilityLabel("Installed packages")
        Text("Roomy will run: sdkmanager --uninstall \(tools.selectedPackage ?? "<package>")")
            .font(.system(.callout, design: .monospaced))
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .truncationMode(.middle)
    }

    private func itemRow(_ item: ToolPreviewItem) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).lineLimit(1).truncationMode(.middle)
                Text(item.detail).font(.caption).foregroundStyle(.secondary)
                    .lineLimit(1).truncationMode(.middle)
            }
            Spacer()
            Text(Format.bytes(item.sizeBytes)).font(.body.monospacedDigit())
        }
    }

    private var confirmTitle: String {
        guard case .ready(let preview) = tools.review else { return "Remove" }
        if preview.kind == .androidSdkPackage {
            let chosen = preview.items.first { $0.id == tools.selectedPackage }
            return chosen.map { "Uninstall \(Format.bytes($0.sizeBytes))" } ?? "Uninstall"
        }
        return "Remove \(Format.bytes(preview.totalBytes))"
    }

    @ViewBuilder
    private var footer: some View {
        HStack {
            if case .running = tools.review {
                ProgressView().controlSize(.small)
                Text("Removing\u{2026}")
            }
            Spacer()
            switch tools.review {
            case .ready(let preview) where preview.plan != nil:
                Button("Cancel", role: .cancel) { tools.closeReview() }
                    .keyboardShortcut(.cancelAction)
                Button(confirmTitle) { tools.confirm() }
                    .buttonStyle(.borderedProminent)
                    .disabled(!tools.canConfirm)
                    .accessibilityHint("Runs the command shown. This can't be undone.")
            case .previewing, .running:
                EmptyView()
            default:
                Button("Done") { tools.closeReview() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }
}
