import SwiftUI

/// The JavaScript screen (Developer mode): each project's `node_modules`, with when the project was last
/// worked on, and the package managers' download caches.
///
/// Rows are ordinary scan items, so ticking one and pressing Review opens the same cleanup review as every
/// other list: nothing moves until the user confirms, everything goes to the Trash, it can be put back, and
/// it is recorded in History. Nothing is ticked for the user.
struct JavaScriptScreen: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        switch model.scanState {
        case .neverScanned:
            EmptyStateView()
        case .scanning(let progress):
            ScanningView(progress: progress)
        case .failed(let message):
            ErrorStateView(message: message)
        case .loaded(let overview, _):
            JavaScriptBrowser(unreadableCount: overview.unreadableCount)
        }
    }
}

/// The list, with the detail panel beside it once a row is selected, as on the other lists.
struct JavaScriptBrowser: View {
    @EnvironmentObject private var model: AppModel
    let unreadableCount: UInt64

    var body: some View {
        ListWithDetail {
            VStack(spacing: 0) {
                JavaScriptControls()
                Divider()
                JavaScriptListBody()
                CleanupBarSlot()
                Divider()
                UnmeasuredNote(unreadableCount: unreadableCount)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, Space.l)
                    .padding(.vertical, Space.s)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .sheet(isPresented: Binding(
            get: { model.cleanupState != .closed },
            set: { if !$0 { model.closeCleanupReview() } }
        )) {
            CleanupReviewSheet()
                .environmentObject(model)
        }
    }
}

/// The summary line and the sort order.
struct JavaScriptControls: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: Space.l) {
            Text(summary)
                .font(.callout)
                .foregroundStyle(.secondary)
            Spacer()
            Picker("Sort by:", selection: $model.javascriptSort) {
                ForEach(AppModel.JavaScriptSort.allCases, id: \.self) { sort in
                    Text(sort.label).tag(sort)
                }
            }
            .fixedSize()
        }
        .padding(.horizontal, Space.l)
        .padding(.vertical, Space.s)
    }

    private var summary: String {
        guard case .loaded(let overview) = model.javascriptState else { return "" }
        return JavaScriptText.summary(overview, now: Date())
    }
}

/// The projects and caches, or what to say when there are none.
struct JavaScriptListBody: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        switch model.javascriptState {
        case .idle, .loading:
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityLabel("Loading JavaScript projects")
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "Roomy couldn't list these", message: message) {
                Button("Scan again") { model.startScan() }
            }
        case .loaded:
            let projects = model.visibleJavaScriptProjects
            let caches = model.visibleJavaScriptCaches
            if projects.isEmpty && caches.isEmpty {
                CenteredMessage(symbol: SidebarItem.javascript.symbol, title: emptyTitle, message: emptyMessage) {
                    EmptyView()
                }
            } else {
                List(selection: $model.selectedItemID) {
                    if !projects.isEmpty {
                        Section("Projects") {
                            ForEach(projects, id: \.item.id) { project in
                                JavaScriptProjectRow(project: project)
                                    .tag(project.item.id)
                            }
                        }
                    }
                    if !caches.isEmpty {
                        Section("Package and tool caches") {
                            ForEach(caches) { cache in
                                ItemRow(item: cache)
                                    .tag(cache.id)
                            }
                        }
                    }
                }
                .listStyle(.inset)
                .accessibilityLabel("JavaScript")
            }
        }
    }

    private var emptyTitle: String {
        model.searchText.isEmpty ? "No JavaScript projects found" : "Nothing matches \u{201C}\(model.searchText)\u{201D}"
    }

    private var emptyMessage: String {
        model.searchText.isEmpty
            ? "Roomy looks for node_modules in projects, Developer, code, dev, src, repos, GitHub, Documents and folders in your home folder."
            : "Try a different search."
    }
}

/// One project: checkbox, name and folder, when it was last worked on, size and tier label.
struct JavaScriptProjectRow: View {
    @EnvironmentObject private var model: AppModel
    let project: JsProject

    var body: some View {
        HStack(alignment: .center, spacing: Space.s) {
            checkbox
                .frame(width: 20)
            Image(systemName: "shippingbox")
                .font(.title3)
                .foregroundStyle(.secondary)
                .frame(width: 24)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: Space.xxs) {
                Text(project.name)
                    .font(.body.weight(.medium))
                Text(JavaScriptText.subtitle(project, now: Date()))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .help(JavaScriptText.sourceHelp(project))
            }
            Spacer(minLength: Space.m)
            Text(Format.bytes(project.item.sizeBytes))
                .font(.body.monospacedDigit())
            TierLabel(tier: project.item.tier)
                .frame(width: 110, alignment: .trailing)
        }
        .padding(.vertical, Space.xs)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(
            "\(project.name), \(JavaScriptText.lastWorkedOn(project, now: Date())), \(Format.bytes(project.item.sizeBytes))"
        )
    }

    @ViewBuilder
    private var checkbox: some View {
        if project.item.tier == .keep {
            Color.clear
                .frame(width: 16, height: 16)
                .accessibilityHidden(true)
        } else {
            Toggle("Choose \(project.name)", isOn: Binding(
                get: { model.isChecked(project.item) },
                set: { model.setChecked($0, for: project.item) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .help("Choose this to review it for cleanup.")
        }
    }
}

/// The JavaScript screen's words. Kept apart from the views so tests can check them.
enum JavaScriptText {
    /// A project counts as resting once nobody has worked on it for this long.
    static let restingAfter: TimeInterval = 90 * 86_400

    /// "Last worked on 3 months ago", or what to say when there is no date.
    static func lastWorkedOn(_ project: JsProject, now: Date) -> String {
        guard let seconds = project.lastActivityUnixSeconds else { return "No date found" }
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        if now.timeIntervalSince(date) < 86_400 { return "Worked on today" }
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .full
        formatter.dateTimeStyle = .named
        return "Last worked on \(formatter.localizedString(for: date, relativeTo: now))"
    }

    /// The row's second line: when, which tool, and the folder.
    static func subtitle(_ project: JsProject, now: Date) -> String {
        var parts = [lastWorkedOn(project, now: now)]
        if let manager = project.packageManager { parts.append(name(of: manager)) }
        if let folder = project.folder { parts.append(folder) }
        return parts.joined(separator: " \u{00B7} ")
    }

    /// Where the date came from, for the row's tooltip.
    static func sourceHelp(_ project: JsProject) -> String {
        var text: String
        switch project.activitySource {
        case .git: text = "The date is the project's last commit, staged change or branch switch."
        case .lockFile: text = "The date is when the project's packages last changed."
        case .manifest: text = "The date is when package.json last changed."
        case nil: text = "Roomy couldn't find package.json, a lock file or git in this project."
        }
        if let command = project.installCommand {
            text += " To get the packages back, run \(command) in the project folder."
        } else if !project.hasManifest {
            text += " This folder has no package.json, so the packages may not come back by themselves."
        }
        return text
    }

    /// "4 projects, 3.6 GB. 2 not worked on for 3 months or more, 1.6 GB."
    static func summary(_ overview: JavaScriptOverview, now: Date) -> String {
        let projects = overview.projects
        guard !projects.isEmpty else { return "" }
        let noun = projects.count == 1 ? "project" : "projects"
        let bytes = projects.reduce(UInt64(0)) { $0 + $1.item.sizeBytes }
        var text = "\(projects.count) \(noun), \(Format.bytes(bytes))."
        let resting = projects.filter { project in
            guard let seconds = project.lastActivityUnixSeconds else { return false }
            return now.timeIntervalSince1970 - TimeInterval(seconds) >= restingAfter
        }
        if !resting.isEmpty {
            let restingBytes = resting.reduce(UInt64(0)) { $0 + $1.item.sizeBytes }
            text += " \(resting.count) not worked on for 3 months or more, \(Format.bytes(restingBytes))."
        }
        return text
    }

    static func name(of manager: PackageManager) -> String {
        switch manager {
        case .npm: return "npm"
        case .pnpm: return "pnpm"
        case .yarn: return "Yarn"
        case .bun: return "Bun"
        }
    }
}
