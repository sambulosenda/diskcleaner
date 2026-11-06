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
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
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
                    }
                }
                .listStyle(.inset)
                .accessibilityLabel(item.title)
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
            Image(systemName: item.category.symbol)
                .font(.title3)
                .foregroundStyle(.secondary)
                .frame(width: 24)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title)
                    .font(.body.weight(.medium))
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
        .accessibilityLabel("\(item.title), \(Format.bytes(item.sizeBytes)), \(item.tier.label)")
    }

    @ViewBuilder
    private var checkbox: some View {
        if item.tier == .keep {
            Color.clear
                .frame(width: 16, height: 16)
                .accessibilityHidden(true)
        } else {
            Toggle("Choose \(item.title)", isOn: Binding(
                get: { model.isChecked(item) },
                set: { model.setChecked($0, for: item) }
            ))
            .toggleStyle(.checkbox)
            .labelsHidden()
            .help("Choose this to review it for cleanup.")
        }
    }
}
