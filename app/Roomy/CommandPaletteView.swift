import AppKit
import SwiftUI

/// The ⌘K palette, drawn over the window while it is open. Clicking outside it or pressing Escape
/// closes it.
struct CommandPaletteOverlay: View {
    @EnvironmentObject private var palette: CommandPaletteModel

    var body: some View {
        if palette.isPresented {
            ZStack(alignment: .top) {
                Color.black.opacity(0.18)
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture { palette.close() }
                    .accessibilityHidden(true)
                PaletteSettingsBridge()
                    .padding(.top, 64)
                    .padding(.horizontal, Space.xxl)
            }
        }
    }
}

/// Hands the panel a way to open Settings. macOS 14 and later need SwiftUI's `openSettings`; macOS 13
/// uses its own action name, as `ModeIndicator` does.
private struct PaletteSettingsBridge: View {
    var body: some View {
        if #available(macOS 14.0, *) {
            ModernSettingsBridge()
        } else {
            CommandPalettePanel {
                NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
            }
        }
    }
}

@available(macOS 14.0, *)
private struct ModernSettingsBridge: View {
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        CommandPalettePanel { openSettings() }
    }
}

/// The search field and the results. Up and Down move the highlight, Return opens it.
struct CommandPalettePanel: View {
    @EnvironmentObject private var palette: CommandPaletteModel
    /// Watched so the results follow the scan, the ticks and the mode.
    @EnvironmentObject private var model: AppModel
    let openSettings: () -> Void

    var body: some View {
        let results = palette.results
        let highlighted = palette.clampedHighlight(count: results.count)
        ScrollViewReader { proxy in
            VStack(spacing: 0) {
                HStack(spacing: Space.s) {
                    Image(systemName: "magnifyingglass")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                        .accessibilityHidden(true)
                    PaletteSearchField(
                        text: $palette.query,
                        placeholder: "Go to a screen or item, or run an action",
                        onMove: { offset in
                            palette.moveHighlight(by: offset)
                            let now = palette.results
                            if !now.isEmpty {
                                proxy.scrollTo(now[palette.clampedHighlight(count: now.count)].id)
                            }
                        },
                        onSubmit: { palette.runHighlighted(openSettings: openSettings) },
                        onCancel: { palette.close() }
                    )
                    .frame(height: 28)
                }
                .padding(.horizontal, Space.l)
                .padding(.vertical, Space.m)
                Divider()
                if results.isEmpty {
                    Text("Nothing matches \u{201C}\(palette.query)\u{201D}.")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(Space.l)
                } else {
                    ScrollView {
                        LazyVStack(spacing: Space.xxs) {
                            ForEach(Array(results.enumerated()), id: \.element.id) { index, command in
                                Button {
                                    palette.run(command, openSettings: openSettings)
                                } label: {
                                    PaletteRow(command: command, isHighlighted: index == highlighted)
                                }
                                .buttonStyle(.plain)
                                .id(command.id)
                                .accessibilityLabel(command.subtitle.isEmpty
                                    ? command.title : "\(command.title), \(command.subtitle)")
                                .accessibilityHint(command.kind.label)
                                .accessibilityAddTraits(index == highlighted ? .isSelected : [])
                            }
                        }
                        .padding(Space.s)
                    }
                    .frame(maxHeight: 360)
                }
                Divider()
                Text("\u{2191}\u{2193} to move, Return to open, Esc to close")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, Space.l)
                    .padding(.vertical, Space.s)
            }
        }
        .frame(width: 600)
        .fixedSize(horizontal: false, vertical: true)
        .background(.regularMaterial, in: Radius.shape(Radius.large))
        .overlay(
            Radius.shape(Radius.large)
                .strokeBorder(Color.primary.opacity(0.1))
        )
        .shadow(color: .black.opacity(0.25), radius: 24, y: 8)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Search Roomy")
    }
}

/// One result: symbol, title, a short line under it, and what kind of entry it is.
private struct PaletteRow: View {
    let command: PaletteCommand
    let isHighlighted: Bool

    var body: some View {
        HStack(spacing: Space.m) {
            Image(systemName: command.symbol)
                .frame(width: 22)
                .foregroundStyle(isHighlighted ? Color.white : Color.secondary)
            VStack(alignment: .leading, spacing: Space.xxs) {
                Text(command.title)
                    .lineLimit(1)
                if !command.subtitle.isEmpty {
                    Text(command.subtitle)
                        .font(.caption)
                        .lineLimit(1)
                        .foregroundStyle(isHighlighted ? Color.white.opacity(0.85) : Color.secondary)
                }
            }
            Spacer(minLength: Space.s)
            Text(command.kind.label)
                .font(.caption)
                .foregroundStyle(isHighlighted ? Color.white.opacity(0.85) : Color.secondary)
        }
        .foregroundStyle(isHighlighted ? Color.white : Color.primary)
        .padding(.horizontal, Space.s)
        .padding(.vertical, Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            Radius.shape(Radius.medium)
                .fill(isHighlighted ? Color.accentColor : Color.clear)
        )
        .contentShape(Rectangle())
    }
}

/// A plain text field that passes Up, Down, Return and Escape to the palette. SwiftUI's own key
/// handling for these needs macOS 14, and the app supports macOS 13.
struct PaletteSearchField: NSViewRepresentable {
    @Binding var text: String
    let placeholder: String
    let onMove: (Int) -> Void
    let onSubmit: () -> Void
    let onCancel: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField()
        field.placeholderString = placeholder
        field.isBordered = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.font = .systemFont(ofSize: 18)
        field.lineBreakMode = .byTruncatingTail
        field.cell?.usesSingleLineMode = true
        field.delegate = context.coordinator
        field.setAccessibilityLabel("Search Roomy")
        // Typing should go straight to the field once it is in the window.
        DispatchQueue.main.async { field.window?.makeFirstResponder(field) }
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        if field.stringValue != text { field.stringValue = text }
    }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: PaletteSearchField

        init(_ parent: PaletteSearchField) {
            self.parent = parent
        }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            parent.text = field.stringValue
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            switch selector {
            case #selector(NSResponder.moveUp(_:)):
                parent.onMove(-1)
            case #selector(NSResponder.moveDown(_:)):
                parent.onMove(1)
            case #selector(NSResponder.insertNewline(_:)):
                parent.onSubmit()
            case #selector(NSResponder.cancelOperation(_:)):
                parent.onCancel()
            default:
                return false
            }
            return true
        }
    }
}
