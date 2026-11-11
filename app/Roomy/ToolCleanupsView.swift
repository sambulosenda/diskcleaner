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

    /// What the tool would remove, in one line. The Android SDK review has its own heading.
    static func reviewHeadline(_ preview: ToolCleanupPreview) -> String {
        let amount = "\(Format.items(UInt64(preview.items.count))), about \(Format.bytes(preview.totalBytes))"
        switch preview.kind {
        case .homebrew: return "Homebrew would remove \(amount)"
        case .unavailableSimulators: return "Xcode would delete \(amount), with their apps and data"
        case .androidSdkPackage: return "Choose one package to uninstall"
        }
    }

    /// Said under the heading of every tool review: none of these go to the Trash.
    static func cannotUndo(_ kind: ToolCleanupKind) -> String {
        switch kind {
        case .homebrew: return "This can't be undone, but Homebrew downloads them again if you install them."
        case .unavailableSimulators: return "This can't be undone."
        case .androidSdkPackage:
            return "This can't be undone, but Android Studio's SDK Manager can install it again."
        }
    }

    /// "About 1.2 GB freed", or "Done" when the tool didn't say how much.
    static func doneHeadline(_ result: ToolCleanupResult) -> String {
        result.reclaimedBytes.map { "About \(Format.bytes($0)) freed" } ?? "Done"
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
        HStack(alignment: .firstTextBaseline, spacing: Space.m) {
            Image(systemName: ToolText.symbol(kind))
                .frame(width: 20)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: Space.xs) {
                Text(ToolText.title(kind)).font(.headline)
                Text(ToolText.explain(kind)).font(.callout).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            Button("Review\u{2026}") { tools.startReview(kind) }
                .accessibilityHint("Lists what would be removed. Nothing is removed yet.")
        }
        .padding(.vertical, Space.xs)
    }
}

/// The review: what the tool would remove, and the button that removes it. It sits in the same frame
/// as the main cleanup review, so reviewing looks the same everywhere.
private struct ToolReviewSheet: View {
    @ObservedObject var tools: ToolCleanupModel

    var body: some View {
        ReviewSheet(title: title) {
            content
        } footer: {
            footer
        }
        .interactiveDismissDisabled(tools.review.isBusy)
    }

    private var title: String {
        switch tools.review {
        case .previewing(let kind): return ToolText.title(kind)
        case .ready(let preview), .running(let preview): return ToolText.title(preview.kind)
        case .done(let result): return ToolText.title(result.kind)
        case .closed, .failed: return "Tool clean-ups"
        }
    }

    @ViewBuilder
    private var content: some View {
        switch tools.review {
        case .closed:
            EmptyView()
        case .previewing(let kind):
            ReviewWorking(text: ToolText.asking(kind))
        case .ready(let preview), .running(let preview):
            if preview.items.isEmpty {
                CenteredMessage(symbol: "checkmark.circle", title: "Nothing to remove",
                                message: "There is nothing to remove right now.") { EmptyView() }
            } else if preview.kind == .androidSdkPackage {
                packagePicker(preview)
            } else {
                VStack(alignment: .leading, spacing: Space.s) {
                    ReviewSummary(headline: ToolText.reviewHeadline(preview), warning: ToolText.cannotUndo(preview.kind))
                    List(preview.items, id: \.id) { item in
                        itemRow(item, marker: .included)
                    }
                    .listStyle(.inset)
                    .accessibilityLabel(ToolText.reviewHeadline(preview))
                    ReviewCommandLine(text: "Roomy will run: \(preview.commandLine)")
                }
            }
        case .done(let result):
            VStack(alignment: .leading, spacing: Space.m) {
                if result.success {
                    FinishPanel(headline: ToolText.doneHeadline(result))
                } else {
                    Label("The tool reported a problem, so it may not have removed everything. History has its full answer.",
                          systemImage: "exclamationmark.triangle")
                }
                Text(result.commandLine)
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                if !result.savedToHistory {
                    Text("This couldn't be saved to History.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, Space.xl)
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "Nothing was removed",
                            message: message) { EmptyView() }
        }
    }

    private func packagePicker(_ preview: ToolCleanupPreview) -> some View {
        VStack(alignment: .leading, spacing: Space.s) {
            ReviewSummary(headline: "Choose one package to uninstall",
                          warning: ToolText.cannotUndo(.androidSdkPackage))
            List(preview.items, id: \.id, selection: $tools.selectedPackage) { item in
                itemRow(item, marker: .none).tag(item.id)
            }
            .listStyle(.inset)
            .disabled(tools.review.isBusy)
            .accessibilityLabel("Installed packages")
            ReviewCommandLine(text: "Roomy will run: sdkmanager --uninstall \(tools.selectedPackage ?? "<package>")")
        }
    }

    private func itemRow(_ item: ToolPreviewItem, marker: ReviewMarker) -> some View {
        ReviewRow(marker: marker, title: item.title, sizeBytes: item.sizeBytes) {
            ReviewDetailLine(item.detail)
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
        if case .running = tools.review {
            ProgressView().controlSize(.small)
            Text("Removing\u{2026}")
                .font(.callout)
                .foregroundStyle(.secondary)
        } else if case .ready(let preview) = tools.review, preview.plan != nil {
            Text("Nothing has been removed yet.")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
        Spacer()
        switch tools.review {
        case .ready(let preview) where preview.plan != nil:
            Button("Cancel", role: .cancel) { tools.closeReview() }
                .keyboardShortcut(.cancelAction)
            // No Return key for this button: what the tool removes doesn't go to the Trash.
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
