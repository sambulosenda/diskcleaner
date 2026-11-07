import AppKit
import SwiftUI

/// Where System Settings shows Full Disk Access (Privacy & Security > Full Disk Access).
enum FullDiskAccessSettings {
    /// Opens System Settings at Privacy & Security > Full Disk Access.
    static let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")

    /// Opens the Full Disk Access list in System Settings.
    static func open() {
        guard let url else { return }
        NSWorkspace.shared.open(url)
    }
}

/// The first-run screen that asks for Full Disk Access. It explains why, opens System Settings at the
/// right place, notices by itself when access is switched on, and lets the user go on without it.
struct FullDiskAccessOnboarding: View {
    @EnvironmentObject private var model: AppModel
    /// Set once the user has opened System Settings from here, to show what to do if nothing changes.
    @State private var openedSettings = false

    /// How often the screen asks the core whether access is on yet, in nanoseconds.
    static let checkInterval: UInt64 = 1_000_000_000

    private var granted: Bool { model.fullDiskAccess == .granted }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            header
            if granted {
                GrantedNote(hasScanned: model.overview != nil)
            } else {
                reasons
                steps
            }
            Divider()
            footer
        }
        .padding(28)
        .frame(width: 520)
        .task { await watchForAccess() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            Task { await model.refreshFullDiskAccess() }
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 14) {
            Image(systemName: granted ? "checkmark.shield" : "lock.shield")
                .font(.system(size: 36))
                .foregroundStyle(granted ? Color.green : Color.accentColor)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 6) {
                Text(granted ? "Roomy can see your whole Mac" : "Let Roomy see your whole Mac")
                    .font(.title2.weight(.semibold))
                    .accessibilityAddTraits(.isHeader)
                Text(granted
                     ? "Full Disk Access is on."
                     : "macOS keeps some folders private until you say Roomy may look inside them.")
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var reasons: some View {
        VStack(alignment: .leading, spacing: 10) {
            ReasonRow(symbol: "chart.pie",
                      text: "Without it, Roomy can't measure Mail, Messages, Safari, iPhone backups and other apps' data, so the totals come out too low.")
            ReasonRow(symbol: "trash",
                      text: "macOS may also stop Roomy moving things to your Trash.")
            ReasonRow(symbol: "hand.raised",
                      text: "Roomy only reads sizes. Nothing leaves this Mac, and nothing is removed unless you confirm it.")
        }
    }

    private var steps: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("How to turn it on")
                .font(.headline)
            StepRow(number: 1, text: "Open Full Disk Access in System Settings.") {
                Button("Open System Settings") {
                    openedSettings = true
                    FullDiskAccessSettings.open()
                }
                .accessibilityHint("Opens Privacy & Security, Full Disk Access.")
            }
            StepRow(number: 2, text: "Switch on Roomy. If Roomy isn't in the list, drag this icon into it, or use the + button.") {
                AppIconToDrag()
            }
            StepRow(number: 3, text: "Come back here. Roomy notices by itself.") {
                EmptyView()
            }
            WaitingNote(openedSettings: openedSettings)
        }
    }

    @ViewBuilder
    private var footer: some View {
        HStack {
            if !granted {
                Text("You can turn it on later in Roomy's Settings.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if granted {
                Button("Not now") { model.finishAccessOnboarding(scan: false) }
                    .keyboardShortcut(.cancelAction)
                Button(model.overview == nil ? "Scan now" : "Scan again") { model.finishAccessOnboarding(scan: true) }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.isScanning)
            } else {
                Button("Continue without it") { model.finishAccessOnboarding(scan: false) }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityHint("Roomy scans what it can. Some folders won't be measured.")
            }
        }
    }

    /// Asks the core once a second until access is on, while the screen is open.
    private func watchForAccess() async {
        while !Task.isCancelled {
            await model.refreshFullDiskAccess()
            if model.fullDiskAccess == .granted { return }
            try? await Task.sleep(nanoseconds: Self.checkInterval)
        }
    }
}

/// One reason, with an icon.
private struct ReasonRow: View {
    let symbol: String
    let text: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: symbol)
                .foregroundStyle(.secondary)
                .frame(width: 20)
                .accessibilityHidden(true)
            Text(text)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// A numbered step, with an optional control under it.
private struct StepRow<Control: View>: View {
    let number: Int
    let text: String
    @ViewBuilder let control: () -> Control

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Text("\(number)")
                .font(.callout.weight(.semibold).monospacedDigit())
                .frame(width: 22, height: 22)
                .background(Circle().fill(Color.accentColor.opacity(0.15)))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 8) {
                Text(text)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel("Step \(number): \(text)")
                control()
            }
        }
    }
}

/// Roomy's own icon, which can be dragged into the Full Disk Access list in System Settings.
private struct AppIconToDrag: View {
    var body: some View {
        HStack(spacing: 8) {
            Image(nsImage: NSApp.applicationIconImage)
                .resizable()
                .frame(width: 40, height: 40)
            Text("Roomy")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
        .padding(6)
        .background(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(nsColor: .separatorColor)))
        .onDrag { NSItemProvider(object: Bundle.main.bundleURL as NSURL) }
        .help("Drag into the Full Disk Access list")
        .accessibilityLabel("Roomy app icon")
        .accessibilityHint("Drag into the Full Disk Access list in System Settings.")
    }
}

/// "Waiting for Full Disk Access", and what to do if switching it on seems to do nothing.
private struct WaitingNote: View {
    let openedSettings: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Waiting for Full Disk Access…")
                    .foregroundStyle(.secondary)
            }
            if openedSettings {
                Text("Switched it on and nothing changed? If System Settings offers to quit and reopen Roomy, choose that. Otherwise quit Roomy and open it again.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// Shown once access is on.
private struct GrantedNote: View {
    let hasScanned: Bool

    var body: some View {
        Text(hasScanned
             ? "Scan again so Roomy can measure the folders it couldn't see before."
             : "Roomy can now measure every folder in your home folder.")
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// "What wasn't scanned": the private areas Roomy couldn't read, with a way to turn on Full Disk Access.
/// Shown from the Overview footer's "Why?" when some folders couldn't be read and access is not on.
struct UnscannedAreasBox: View {
    @EnvironmentObject private var model: AppModel
    let overview: Overview
    /// In a popover the box drops its own background and border.
    var inPopover = false
    /// How many areas show before "and more".
    static let shownAreas = 5

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: "lock.shield")
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                Text("What Roomy couldn't scan")
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)
            }
            Text(Self.summary(for: overview.unreadableCount))
                .fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Self.rows(for: overview.unreadableAreas), id: \.area) { row in
                    HStack(spacing: 8) {
                        Image(systemName: row.area.symbol)
                            .frame(width: 18)
                            .foregroundStyle(.secondary)
                            .accessibilityHidden(true)
                        Text(row.area.label)
                        Spacer()
                        Text(row.countText)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                    }
                    .accessibilityElement(children: .combine)
                }
            }
            .font(.callout)
            HStack(spacing: 12) {
                Button("Turn on Full Disk Access") { model.showAccessOnboarding() }
                    .accessibilityHint("Shows how to let Roomy measure these folders.")
                Text("Then scan again.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 10)
            .fill(inPopover ? Color.clear : Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 10)
            .strokeBorder(inPopover ? Color.clear : Color(nsColor: .separatorColor)))
    }

    /// "Roomy couldn't open 212 folders, most often because macOS keeps them private. …"
    static func summary(for count: UInt64) -> String {
        let folders = count == 1 ? "1 folder" : "\(Format.count(count)) folders"
        return "Roomy couldn't open \(folders), most often because macOS keeps them private. "
            + "Their sizes aren't in these results. Everything else here is right."
    }

    /// The first few areas, with the rest folded into one "Other folders" row so the box stays short.
    static func rows(for areas: [UnreadableArea]) -> [UnreadableArea] {
        guard areas.count > shownAreas else { return areas }
        let named = areas.filter { $0.area != .other }
        let shown = Array(named.prefix(shownAreas - 1))
        let rest = areas.filter { area in !shown.contains(where: { $0.area == area.area }) }
        let restCount = rest.reduce(UInt64(0)) { $0 + $1.folderCount }
        return shown + [UnreadableArea(area: .other, folderCount: restCount)]
    }
}
