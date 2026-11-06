import SwiftUI

/// The main window: sidebar, content and toolbar.
struct ContentView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationSplitView {
            SidebarView()
                .navigationSplitViewColumnWidth(min: 200, ideal: 230, max: 300)
        } detail: {
            DetailView()
        }
        .searchable(text: $model.searchText, placement: .toolbar, prompt: "Search items")
        .toolbar { MainToolbar(model: model) }
        .frame(minWidth: 960, minHeight: 640)
    }
}

/// The sidebar. Arrow keys move through it; the Developer group appears only in Developer mode.
struct SidebarView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        List(selection: $model.selection) {
            ForEach(model.sidebarSections, id: \.self) { items in
                Section(header: sectionHeader(for: items)) {
                    ForEach(items) { item in
                        Label(item.title, systemImage: item.symbol)
                            .tag(item)
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .accessibilityLabel("Sections")
    }

    @ViewBuilder
    private func sectionHeader(for items: [SidebarItem]) -> some View {
        if items == SidebarItem.developerItems {
            Text("Developer")
        } else if items == SidebarItem.actionItems {
            Text("Clean up")
        } else {
            EmptyView()
        }
    }
}

/// The content area for the selected sidebar entry.
struct DetailView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        let item = model.selection ?? .overview
        Group {
            if item == .overview {
                OverviewScreen()
            } else {
                PlaceholderView(item: item)
            }
        }
        .navigationTitle(item.title)
    }
}

/// Toolbar: Scan, the mode indicator, and (added by `.searchable`) the search field.
struct MainToolbar: ToolbarContent {
    @ObservedObject var model: AppModel

    var body: some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            Button {
                model.startScan()
            } label: {
                Label("Scan", systemImage: "arrow.clockwise")
            }
            .labelStyle(.titleAndIcon)
            .disabled(model.isScanning)
            .help("Scan this Mac (⌘R)")
            .accessibilityLabel("Scan")
            .accessibilityHint("Measures what is filling your Mac. Nothing is removed.")
        }
        ToolbarItem(placement: .primaryAction) {
            ModeIndicator(mode: model.mode)
        }
    }
}

/// Shows which mode is on, in words. The mode is changed in Settings.
struct ModeIndicator: View {
    let mode: Mode

    var body: some View {
        Text("\(mode.label) mode")
            .font(.callout)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(Capsule().strokeBorder(Color.secondary.opacity(0.5)))
            .help("Change the mode in Settings (⌘,)")
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("\(mode.label) mode")
            .accessibilityHint("Change the mode in Settings.")
    }
}
