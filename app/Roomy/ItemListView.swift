import AppKit
import QuickLook
import SwiftUI

/// A category screen: picks the right state, then shows the scan's items for this screen.
struct ItemListScreen: View {
    @EnvironmentObject private var model: AppModel
    let item: SidebarItem

    var body: some View {
        switch model.scanState {
        case .neverScanned:
            EmptyStateView()
        case .scanning(let progress):
            ScanningView(progress: progress)
        case .failed(let message):
            ErrorStateView(message: message)
        case .loaded(let overview, _):
            ItemBrowser(item: item, unreadableCount: overview.unreadableCount)
        }
    }
}

/// The list on the left two thirds and the detail panel on the right third.
struct ItemBrowser: View {
    @EnvironmentObject private var model: AppModel
    let item: SidebarItem
    let unreadableCount: UInt64

    var body: some View {
        GeometryReader { geometry in
            HStack(spacing: 0) {
                VStack(spacing: 0) {
                    ItemListControls()
                    Divider()
                    ItemListBody(item: item)
                    if !model.checkedItemIDs.isEmpty {
                        Divider()
                        CleanupBar()
                    }
                    Divider()
                    UnmeasuredNote(unreadableCount: unreadableCount)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                Divider()
                ItemDetailPanel()
                    .frame(width: max(geometry.size.width / 3, 280))
            }
        }
        .quickLookPreview($model.quickLookURL)
        .sheet(isPresented: Binding(
            get: { model.cleanupState != .closed },
            set: { if !$0 { model.closeCleanupReview() } }
        )) {
            CleanupReviewSheet()
                .environmentObject(model)
        }
    }
}

/// Sort order and tier filter. Both are applied by the core.
struct ItemListControls: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 16) {
            Picker("Show:", selection: $model.tierFilter) {
                Text("Everything").tag(Tier?.none)
                ForEach(Tier.allCases, id: \.self) { tier in
                    Text(tier.label).tag(Tier?.some(tier))
                }
            }
            .fixedSize()
            Picker("Sort by:", selection: $model.sort) {
                ForEach(Sort.allCases, id: \.self) { sort in
                    Text(sort.label).tag(sort)
                }
            }
            .fixedSize()
            Spacer()
            if !model.visibleLikelySafeItems.isEmpty {
                QuickSelectButton()
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

/// "Select all Likely safe": ticks every Likely safe row showing, and nothing else. Review first rows
/// are never ticked this way. Shown only when the list has Likely safe rows.
struct QuickSelectButton: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        let allChecked = model.allVisibleLikelySafeChecked
        Button(QuickSelectText.title(allChecked: allChecked)) {
            if allChecked { model.deselectAllLikelySafe() } else { model.selectAllLikelySafe() }
        }
        .help(QuickSelectText.help)
        .accessibilityHint(QuickSelectText.help)
    }
}

/// The words of the quick select button, kept together so tests can check them.
enum QuickSelectText {
    static func title(allChecked: Bool) -> String {
        allChecked ? "Deselect Likely safe" : "Select all Likely safe"
    }

    static let help =
        "Ticks only the Likely safe items in this list. Items to check first are left for you to choose. Nothing is moved."
}

/// The rows, or what to say when there are none.
struct ItemListBody: View {
    @EnvironmentObject private var model: AppModel
    let item: SidebarItem

    var body: some View {
        switch model.listState {
        case .idle, .loading:
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityLabel("Loading items")
        case .failed(let message):
            CenteredMessage(symbol: "exclamationmark.triangle", title: "Roomy couldn't list these", message: message) {
                Button("Scan again") { model.startScan() }
            }
        case .loaded:
            let items = model.visibleItems
            if items.isEmpty {
                CenteredMessage(symbol: item.symbol, title: emptyTitle, message: emptyMessage) { EmptyView() }
            } else {
                List(selection: $model.selectedItemID) {
                    ForEach(items) { row in
                        ItemRow(item: row)
                            .tag(row.id)
                            .contextMenu { ItemRowMenu(item: row).environmentObject(model) }
                    }
                }
                .listStyle(.inset)
                .accessibilityLabel(item.title)
                .modifier(ListKeys())
            }
        }
    }

    private var emptyTitle: String {
        model.searchText.isEmpty ? "Nothing here takes much space" : "No items match \u{201C}\(model.searchText)\u{201D}"
    }

    private var emptyMessage: String {
        model.tierFilter == nil ? "Roomy didn't find anything for this section." : "Try showing everything."
    }
}

/// Checkbox, icon, title, explanation, size and tier label.
///
/// Ticked rows go into the cleanup review. `keep` items have no checkbox, and nothing is ticked for
/// the user, so Review first items are never preselected.
struct ItemRow: View {
    @EnvironmentObject private var model: AppModel
    let item: ItemSummary

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            checkbox
                .frame(width: 20)
            CategoryIcon(category: item.category)
            VStack(alignment: .leading, spacing: 2) {
                ItemTitle(item: item)
                Text(item.explain)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 12)
            Text(Format.bytes(item.sizeBytes))
                .font(.body.monospacedDigit())
            TierLabel(tier: item.tier)
                .frame(width: 110, alignment: .trailing)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(item.accessibleTitle), \(Format.bytes(item.sizeBytes)), \(item.tier.label)")
    }

    @ViewBuilder
    private var checkbox: some View {
        if item.tier == .keep {
            Color.clear
                .frame(width: 16, height: 16)
                .accessibilityHidden(true)
        } else {
            Toggle("Choose \(item.accessibleTitle)", isOn: Binding(
                get: { model.isChecked(item) },
                set: { model.setChecked($0, for: item) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .help("Choose this to review it for cleanup.")
        }
    }
}

/// The right-click menu on an item row. Choosing changes only the tick; Show in Finder, Quick Look
/// and Copy Path only look. Nothing here changes anything on disk.
struct ItemRowMenu: View {
    @EnvironmentObject private var model: AppModel
    let item: ItemSummary

    var body: some View {
        if item.tier != .keep {
            let checked = model.isChecked(item)
            Button(ItemRowMenuText.choose(checked: checked)) { model.setChecked(!checked, for: item) }
        }
        if model.canShowFiles {
            Divider()
            Button(ItemRowMenuText.showInFinder) { model.showInFinder(item.id) }
            if model.selectedItemID == item.id, model.selectedQuickLookURL != nil {
                Button(ItemRowMenuText.quickLook) { model.toggleQuickLook() }
            }
            Button(ItemRowMenuText.copyPath) { model.copyPath(item.id) }
        }
    }
}

/// The words of the row menu, kept together so tests can check them.
enum ItemRowMenuText {
    static func choose(checked: Bool) -> String {
        checked ? "Leave Out of Cleanup" : "Choose for Cleanup"
    }

    static let showInFinder = "Show in Finder"
    static let quickLook = "Quick Look"
    static let copyPath = "Copy Path"
}

/// A key the item list handles itself.
enum ListKey: Equatable {
    /// ⌘A: tick the Likely safe rows showing (never Review first or Keep rows).
    case selectAll
    /// Space: Quick Look the selected row when it is a single file.
    case quickLook

    /// Space's key code on every Mac keyboard layout.
    static let spaceKeyCode: UInt16 = 49

    /// Which list key a key press is, or `nil` for any other key. Shift, Option and Control turn
    /// either key into something else, so they are left alone.
    static func from(keyCode: UInt16, characters: String?, modifiers: NSEvent.ModifierFlags) -> ListKey? {
        let flags = modifiers.intersection([.command, .shift, .option, .control])
        if flags == .command, characters?.lowercased() == "a" { return .selectAll }
        if flags.isEmpty, keyCode == spaceKeyCode { return .quickLook }
        return nil
    }
}

/// ⌘A and Space for the item list, on macOS 13 and later.
struct ListKeys: ViewModifier {
    @EnvironmentObject private var model: AppModel
    @StateObject private var keys = ListKeyMonitor()

    func body(content: Content) -> some View {
        content
            .onAppear { keys.start(model: model) }
            .onDisappear { keys.stop() }
    }
}

/// Watches key presses while an item list shows.
///
/// A key goes to the list only when the app's main window is the key window, no sheet is open, and no
/// text field is being typed in (so ⌘A and Space still work in the search field and the ⌘K palette).
/// Every other key is passed on untouched.
@MainActor
final class ListKeyMonitor: ObservableObject {
    private var monitor: Any?

    func start(model: AppModel) {
        guard monitor == nil else { return }
        // AppKit calls local monitors on the main thread, while it handles the event.
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak model] event in
            MainActor.assumeIsolated { () -> NSEvent? in
                guard let model, Self.listMayTake(event),
                      let key = ListKey.from(keyCode: event.keyCode,
                                             characters: event.charactersIgnoringModifiers,
                                             modifiers: event.modifierFlags) else { return event }
                switch key {
                case .selectAll:
                    model.selectAllShortcut()
                    return nil
                case .quickLook:
                    return model.toggleQuickLook() ? nil : event
                }
            }
        }
    }

    func stop() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
    }

    /// Whether the key press is meant for the list rather than a text field, a sheet or another window.
    private static func listMayTake(_ event: NSEvent) -> Bool {
        guard let window = event.window, window.isKeyWindow, window.isMainWindow,
              window.attachedSheet == nil else { return false }
        return !(window.firstResponder is NSText)
    }
}
