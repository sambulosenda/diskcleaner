import SwiftUI

/// The Android screen: the Android SDK parts, emulators and Gradle downloads on this Mac, how big each one is,
/// and whether any Android project Roomy found still uses it.
///
/// Nothing is ticked for the user, even what no project uses. The Review button opens the same cleanup review
/// as every other screen, so nothing moves until the user confirms, everything goes to the Trash, and a move
/// can be put back. Developer mode only.
struct AndroidScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        content
            .sheet(isPresented: Binding(
                get: { model.cleanupState != .closed },
                set: { if !$0 { model.closeCleanupReview() } }
            )) {
                CleanupReviewSheet()
                    .environmentObject(model)
            }
            .onAppear { if model.androidState == .idle { model.loadAndroid() } }
    }

    @ViewBuilder
    private var content: some View {
        switch model.androidState {
        case .idle, .loading:
            ProgressView("Looking at the Android SDK and your projects\u{2026}")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .loaded(let overview):
            if overview.parts.isEmpty {
                CenteredMessage(symbol: "candybarphone", title: "No Android tools found",
                                message: overview.note ?? "Roomy didn't find the Android SDK in your home folder.") {
                    Button("Look again") { model.loadAndroid() }
                }
            } else {
                AndroidContent(overview: overview)
            }
        }
    }
}

/// The summary, the parts grouped by kind, the projects, and the Review button.
struct AndroidContent: View {
    @EnvironmentObject private var model: AppModel
    let overview: AndroidOverview

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
                .padding(Space.l)
            Divider()
            List {
                if let note = overview.note {
                    Label(note, systemImage: "info.circle")
                        .padding(.vertical, Space.xs)
                }
                ForEach(AndroidGroup.allCases) { group in
                    let parts = model.visibleAndroidParts.filter { group.kinds.contains($0.kind) }
                    if !parts.isEmpty {
                        Section(group.title) {
                            ForEach(parts) { part in
                                AndroidPartRow(part: part)
                            }
                        }
                    }
                }
                if !overview.projects.isEmpty {
                    Section("Android projects Roomy found") {
                        ForEach(overview.projects) { project in
                            AndroidProjectRow(project: project)
                        }
                    }
                }
            }
            .listStyle(.inset)
            .accessibilityLabel("Android tools")
            Divider()
            footer
                .padding(.horizontal, Space.l)
                .padding(.vertical, Space.s)
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: Space.m) {
            VStack(alignment: .leading, spacing: Space.xs) {
                Text("Android tools use \(Format.bytes(overview.totalBytes))")
                    .font(.title2.weight(.semibold))
                    .heading()
                Text(summary)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            Button {
                model.loadAndroid()
            } label: {
                Label("Look again", systemImage: "arrow.clockwise")
            }
            .help("Look at the Android SDK and your projects again")
        }
    }

    private var summary: String {
        let projects = overview.projects.count
        let found = projects == 1 ? "1 Android project" : "\(projects) Android projects"
        if overview.unusedBytes == 0 {
            return "Roomy found \(found). Everything here is used, or Roomy couldn't tell."
        }
        return "Roomy found \(found). \(Format.bytes(overview.unusedBytes)) is in parts none of them use. "
            + "Check each one before you choose it; nothing is chosen for you."
    }

    private var footer: some View {
        HStack(spacing: Space.m) {
            let count = overview.parts.filter { model.isAndroidChecked($0) }.count
            Text("\(count) \(count == 1 ? "item" : "items") chosen, \(Format.bytes(model.checkedAndroidBytes))")
                .font(.callout.monospacedDigit())
            Spacer()
            Button("Review\u{2026}") { model.reviewAndroidCleanup() }
                .buttonStyle(.borderedProminent)
                .disabled(count == 0)
                .accessibilityHint("Checks what would move to the Trash. Nothing is moved.")
        }
    }
}

/// How the screen groups the core's parts, in the order it shows them.
enum AndroidGroup: String, CaseIterable, Identifiable {
    case emulators
    case images
    case sdk
    case gradle

    var id: String { rawValue }

    var title: String {
        switch self {
        case .emulators: return "Emulators"
        case .images: return "Emulator images"
        case .sdk: return "SDK platforms, build tools and NDKs"
        case .gradle: return "Gradle"
        }
    }

    var kinds: [AndroidKind] {
        switch self {
        case .emulators: return [.emulator]
        case .images: return [.systemImage]
        case .sdk: return [.platform, .buildTools, .ndk]
        case .gradle: return [.gradleVersion, .gradleCache]
        }
    }
}

/// One part: tick box, title, whether anything uses it, what it is, and its size.
struct AndroidPartRow: View {
    @EnvironmentObject private var model: AppModel
    let part: AndroidPart

    var body: some View {
        HStack(alignment: .top, spacing: Space.s) {
            Toggle("Choose \(part.title)", isOn: Binding(
                get: { model.isAndroidChecked(part) },
                set: { model.setAndroidChecked($0, for: part) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .help("Choose this to review it for removal.")
            VStack(alignment: .leading, spacing: Space.xxs) {
                HStack(spacing: Space.s) {
                    Text(part.title)
                        .font(.body.weight(.medium))
                    if let badge = AndroidUsageBadge(usage: part.usage) {
                        Badge(text: badge.text, tint: badge.color)
                    }
                }
                if let detail = part.detail {
                    Text(detail)
                        .font(.callout)
                }
                if let line = usageLine {
                    Text(line)
                        .font(.callout)
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
        .accessibilityLabel("\(part.title), \(Format.bytes(part.sizeBytes))\(usageLine.map { ", \($0)" } ?? "")")
    }

    /// What uses it, or for an emulator, when it last ran.
    private var usageLine: String? {
        if part.kind == .emulator {
            guard let seconds = part.lastUsedUnixSeconds else { return nil }
            return "Last used \(Format.relative(Date(timeIntervalSince1970: TimeInterval(seconds))))"
        }
        return part.usageText.isEmpty ? nil : part.usageText
    }
}

/// The short label next to a part's title.
struct AndroidUsageBadge {
    let text: String
    let color: Color

    init?(usage: AndroidUsage) {
        switch usage {
        case .used: text = "In use"; color = .green
        case .probablyUsed: text = "Probably in use"; color = .green
        case .notSure: text = "Not sure"; color = .orange
        case .notUsed: text = "Not used"; color = .blue
        case .notApplicable: return nil
        }
    }
}

/// One project Roomy found, with when it was last worked on.
struct AndroidProjectRow: View {
    let project: AndroidProject

    var body: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: Space.xxs) {
                Text(project.name)
                    .font(.body.weight(.medium))
                if let path = project.path {
                    Text(path)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            Spacer(minLength: Space.m)
            if let seconds = project.lastActivityUnixSeconds {
                Text("Last worked on \(Format.relative(Date(timeIntervalSince1970: TimeInterval(seconds))))")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, Space.xxs)
        .accessibilityElement(children: .combine)
    }
}
