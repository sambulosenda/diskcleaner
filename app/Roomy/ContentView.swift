import AppKit
import SwiftUI

/// The main window: sidebar, content and toolbar.
struct ContentView: View {
    @EnvironmentObject private var model: AppModel
    @State private var columnVisibility = NavigationSplitViewVisibility.all

    var body: some View {
        NavigationSplitView(columnVisibility: $columnVisibility) {
            SidebarView()
                .navigationSplitViewColumnWidth(min: 200, ideal: 230, max: 300)
        } detail: {
            DetailView()
        }
        .searchable(text: $model.searchText, placement: .toolbar, prompt: "Search items")
        .toolbar { MainToolbar(model: model) }
        .frame(minWidth: 960, minHeight: 640)
        .overlay { CommandPaletteOverlay().environmentObject(model.palette) }
        .sheet(isPresented: $model.showsAccessOnboarding) {
            FullDiskAccessOnboarding()
                .environmentObject(model)
        }
        .task { await model.windowDidAppear() }
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
            } else if item == .history {
                HistoryScreen()
            } else if item == .systemData {
                SystemDataScreen()
            } else if item == .libraries {
                LibrariesScreen()
            } else if item == .docker {
                DockerScreen()
            } else if item == .javascript {
                JavaScriptScreen()
            } else if item == .apps {
                AppsScreen()
            } else if item.itemFilter != nil {
                ItemListScreen(item: item)
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
        ToolbarItem(placement: .automatic) {
            ModeIndicator(mode: model.mode)
                .fixedSize()
        }
    }
}

/// Shows which mode is on, in words. Clicking it opens Settings, where the mode is changed.
struct ModeIndicator: View {
    let mode: Mode

    var body: some View {
        opener
            .help("Change the mode in Settings (⌘,)")
            .accessibilityLabel("\(mode.label) mode")
            .accessibilityHint("Opens Settings, where you can change the mode.")
    }

    @ViewBuilder
    private var opener: some View {
        if #available(macOS 14.0, *) {
            SettingsLink { label }
        } else {
            Button {
                // macOS 13's name for the Settings window action. macOS 14 and later use SettingsLink above.
                NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
            } label: {
                label
            }
        }
    }

    private var label: some View {
        Label("\(mode.label) mode", systemImage: mode == .developer ? "hammer" : "person")
            .labelStyle(.titleAndIcon)
    }
}
