import AppKit
import SwiftUI

/// The right-hand panel for the selected row: what it is, why it's here, what removing it means and
/// how it comes back. Developer mode adds the place, the rule and Reveal in Finder.
///
/// No path is ever shown in simple mode: the core leaves `path` empty there, and this panel also
/// checks the mode itself before showing one.
struct ItemDetailPanel: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Details")
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                Spacer()
                Button {
                    model.selectedItemID = nil
                } label: {
                    Label("Hide Details", systemImage: "xmark")
                        .labelStyle(.iconOnly)
                }
                .buttonStyle(.borderless)
                .help("Hide details")
                .accessibilityHint("Closes this panel and clears the selection. Ticks stay as they are.")
            }
            .padding(.horizontal, Space.l)
            .padding(.vertical, Space.s)
            Divider()
            detail
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Details")
    }

    @ViewBuilder
    private var detail: some View {
        Group {
            switch model.detailState {
            case .none:
                CenteredMessage(symbol: "sidebar.right", title: "Choose an item",
                                message: "Select a row to see what it is and what removing it would mean.") { EmptyView() }
            case .loading:
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityLabel("Loading details")
            case .failed(let message):
                CenteredMessage(symbol: "exclamationmark.triangle", title: "No details", message: message) { EmptyView() }
            case .loaded(let detail):
                ItemDetailContent(detail: detail, mode: model.mode)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// A list with the detail panel beside it. The panel takes no room until a row is selected, then
/// slides in from the right; the list has the whole width the rest of the time.
struct ListWithDetail<Content: View>: View {
    @EnvironmentObject private var model: AppModel
    let list: () -> Content

    init(@ViewBuilder list: @escaping () -> Content) {
        self.list = list
    }

    /// Whether a row is selected, so there is something for the panel to show.
    private var showsDetail: Bool {
        if case .none = model.detailState { return false }
        return true
    }

    var body: some View {
        GeometryReader { geometry in
            HStack(spacing: 0) {
                list()
                if showsDetail {
                    HStack(spacing: 0) {
                        Divider()
                        ItemDetailPanel()
                    }
                    .frame(width: DetailPane.width(in: geometry.size.width))
                    .transition(.move(edge: .trailing))
                }
            }
            .motion(Motion.settle, value: showsDetail)
        }
        .clipped()
    }
}

/// The filled-in panel.
struct ItemDetailContent: View {
    let detail: ItemDetail
    let mode: Mode

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Space.l) {
                VStack(alignment: .leading, spacing: Space.xs) {
                    Text(detail.title)
                        .font(.title2.weight(.semibold))
                        .heading()
                    HStack(spacing: Space.s) {
                        Text(Format.bytes(detail.sizeBytes))
                            .font(.title3.monospacedDigit())
                        TierLabel(tier: detail.tier)
                    }
                }
                if let backup = detail.deviceBackup {
                    BackupWarningBox(backup: backup)
                }
                DetailSection(title: "What it is", text: detail.explain)
                DetailSection(title: "Why it's here", text: ItemDetailText.whyHere(detail))
                DetailSection(title: "If you remove it", text: detail.risk)
                DetailSection(title: "How it comes back", text: ItemDetailText.howItComesBack(detail))
                if mode == .developer {
                    DeveloperDetails(detail: detail)
                }
            }
            .padding(Space.xl)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// A heading and a paragraph.
private struct DetailSection: View {
    let title: String
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: Space.xs) {
            Text(title)
                .font(.headline)
                .heading()
            Text(text)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
        }
    }
}

/// Developer mode only: the place, the rule, how sure it is, any tool command, and Reveal in Finder.
private struct DeveloperDetails: View {
    let detail: ItemDetail

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            Divider()
            if let path = detail.path {
                LabeledRow(label: "Location", value: path, monospaced: true)
            }
            LabeledRow(label: "Rule", value: "\(detail.ruleId) (version \(detail.ruleVersion))", monospaced: true)
            LabeledRow(label: "Confidence", value: detail.confidence.label)
            if let command = detail.command {
                LabeledRow(label: "Command", value: command, monospaced: true)
            }
            if let path = detail.path, let url = Reveal.url(forDisplayPath: path) {
                Button(ItemRowMenuText.showInFinder) { Reveal.inFinder(url) }
                    .padding(.top, Space.xs)
                    .accessibilityHint("Opens a Finder window showing this item. Nothing is changed.")
            }
        }
    }
}

private struct LabeledRow: View {
    let label: String
    let value: String
    var monospaced = false

    var body: some View {
        VStack(alignment: .leading, spacing: Space.xxs) {
            Text(label)
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            Text(value)
                .font(monospaced ? .callout.monospaced() : .callout)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
    }
}

/// The warning for an iPhone or iPad backup: which device it is, when it was last backed up, and to
/// check iCloud first. Shown in the detail panel and the cleanup review whenever the core says an item
/// is a device backup, even if it couldn't read the device's details.
struct BackupWarningBox: View {
    let backup: DeviceBackup
    var compact = false

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Space.s) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: Space.xs) {
                Text(BackupWarning.headline(backup))
                    .font(compact ? .callout.weight(.semibold) : .body.weight(.semibold))
                Text(compact ? BackupWarning.shortDetails(backup) : BackupWarning.details(backup))
                    .font(compact ? .callout : .body)
                    .foregroundStyle(.secondary)
            }
            .fixedSize(horizontal: false, vertical: true)
        }
        .padding(compact ? Space.s : Space.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.orange.opacity(0.12), in: Radius.shape(Radius.medium))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Warning. \(BackupWarning.headline(backup)) \(BackupWarning.details(backup))")
    }
}

/// The words of the device backup warning, kept together so tests can check them.
enum BackupWarning {
    /// The device's name, or a plain description when the core couldn't read it.
    static func device(_ backup: DeviceBackup) -> String {
        backup.deviceName ?? "this iPhone or iPad"
    }

    /// "This may be your only copy of Sam's iPhone."
    static func headline(_ backup: DeviceBackup) -> String {
        "This may be your only copy of \(device(backup))."
    }

    /// When it was last backed up, then what to check.
    static func details(
        _ backup: DeviceBackup,
        locale: Locale = .current,
        timeZone: TimeZone = .current
    ) -> String {
        [lastBackedUp(backup, locale: locale, timeZone: timeZone), checkICloud].joined(separator: " ")
    }

    /// The same, without the steps, for a row in the cleanup review.
    static func shortDetails(
        _ backup: DeviceBackup,
        locale: Locale = .current,
        timeZone: TimeZone = .current
    ) -> String {
        [lastBackedUp(backup, locale: locale, timeZone: timeZone), "Check it's backed up to iCloud first."]
            .joined(separator: " ")
    }

    static let checkICloud =
        "Check it's backed up to iCloud first: on the device, open Settings, tap your name, "
        + "then iCloud, then iCloud Backup."

    /// "Last backed up on 30 October 2026." or a plain line when the date is unknown.
    static func lastBackedUp(
        _ backup: DeviceBackup,
        locale: Locale = .current,
        timeZone: TimeZone = .current
    ) -> String {
        guard let seconds = backup.lastBackupUnixSeconds else {
            return "Roomy couldn't tell when it was last backed up to this Mac."
        }
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.timeZone = timeZone
        formatter.dateStyle = .long
        formatter.timeStyle = .none
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        return "Last backed up to this Mac on \(formatter.string(from: date))."
    }
}

/// Plain words the panel builds from the core's fields.
enum ItemDetailText {
    /// Why the item is listed with its tier, plus every reason the core gave for being careful.
    static func whyHere(_ detail: ItemDetail) -> String {
        let lead: String
        switch detail.tier {
        case .likelySafe:
            lead = detail.regenerable
                ? "Roomy recognises this, and it comes back by itself if it is removed."
                : "Roomy recognises this, and it is usually safe to remove."
        case .reviewFirst:
            lead = "It may hold something you want, so look at it before removing it."
        case .keep:
            lead = "It is shown so you know where the space goes. Roomy won't offer to remove it."
        }
        let reasons = detail.cautions.map(\.explanation)
        return ([lead] + reasons).joined(separator: " ")
    }

    static func howItComesBack(_ detail: ItemDetail) -> String {
        if let restore = detail.restore, !restore.isEmpty { return restore }
        return "It doesn't come back by itself."
    }
}

/// Reveal in Finder, Quick Look and Copy Path, in Developer mode only (Sam's choice for T11).
///
/// The core gives Developer mode a display path starting `~/`. Swift turns it back into a place inside
/// the home folder for Finder only; it is never sent to the core, and nothing is changed on disk.
enum Reveal {
    /// The place a `~/…` display path names inside `home`, or `nil` if it doesn't look like one.
    static func url(
        forDisplayPath path: String,
        home: URL = FileManager.default.homeDirectoryForCurrentUser
    ) -> URL? {
        if path == "~" { return home }
        guard path.hasPrefix("~/") else { return nil }
        let parts = path.dropFirst(2).split(separator: "/", omittingEmptySubsequences: false)
        guard !parts.isEmpty, !parts.contains(where: { $0.isEmpty || $0 == "." || $0 == ".." }) else { return nil }
        return parts.reduce(home) { $0.appendingPathComponent(String($1)) }
    }

    /// Opens a Finder window with the item selected.
    static func inFinder(_ url: URL) {
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }

    /// Puts the full path on the clipboard, as plain text.
    static func copyPath(_ url: URL) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(url.path, forType: .string)
    }

    /// Whether `url` is one ordinary file: not a folder, not a package, and not a symbolic link.
    /// Reads metadata only; the link itself is checked, never followed.
    static func isSingleFile(_ url: URL) -> Bool {
        guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey]) else {
            return false
        }
        return values.isRegularFile == true && values.isSymbolicLink != true
    }
}
