import AppKit
import SwiftUI
import XCTest
@testable import Roomy

/// Renders each screen state in light and dark mode to PNG files for the pull request.
///
/// Skipped unless `ROOMY_SCREENSHOTS_DIR` is set. With xcodebuild, pass it as
/// `TEST_RUNNER_ROOMY_SCREENSHOTS_DIR=/some/folder xcodebuild ... test`.
@MainActor
final class ScreenshotTests: XCTestCase {
    private let suiteName = "RoomyTests.Screenshots"

    func testCaptureEveryState() throws {
        guard let path = ProcessInfo.processInfo.environment["ROOMY_SCREENSHOTS_DIR"], !path.isEmpty else {
            throw XCTSkip("Set ROOMY_SCREENSHOTS_DIR to capture screenshots")
        }
        let folder = URL(fileURLWithPath: path, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)

        let scannedAt = Date(timeIntervalSince1970: 1_793_000_000)
        let normal = MockData.overview(for: .normal)
        let states: [(String, Mode, AppModel.ScanState)] = [
            ("1-empty", .simple, .neverScanned),
            ("2-scanning", .simple, .scanning(ScanProgress(bytesSeen: 187_000_000_000, itemsSeen: 412_370, phase: .scanning))),
            ("3-overview-simple", .simple, .loaded(normal, scannedAt: scannedAt)),
            ("4-overview-developer", .developer, .loaded(normal, scannedAt: scannedAt)),
            ("5-nothing-found", .simple, .loaded(MockData.overview(for: .nothingFound), scannedAt: scannedAt)),
            ("6-error", .simple, .failed("Roomy couldn't finish reading your home folder.")),
        ]

        for appearance in [NSAppearance.Name.aqua, .darkAqua] {
            let suffix = appearance == .aqua ? "light" : "dark"
            for (name, mode, state) in states {
                let model = makeModel(mode: mode, state: state)
                try capture(ContentView().environmentObject(model), size: CGSize(width: 1120, height: 1080),
                            appearance: appearance, to: folder.appendingPathComponent("\(name)-\(suffix).png"))
            }
            for (name, mode) in [("8-list-simple", Mode.simple), ("9-list-developer", .developer)] {
                let model = makeListModel(mode: mode, overview: normal, scannedAt: scannedAt)
                try capture(ContentView().environmentObject(model), size: CGSize(width: 1120, height: 740),
                            appearance: appearance, to: folder.appendingPathComponent("\(name)-\(suffix).png"))
            }
            let paletteModel = makeListModel(mode: .simple, overview: normal, scannedAt: scannedAt)
            paletteModel.palette.open()
            try capture(ContentView().environmentObject(paletteModel), size: CGSize(width: 1120, height: 740),
                        appearance: appearance, to: folder.appendingPathComponent("13-palette-\(suffix).png"))
            let model = makeModel(mode: .developer, state: .neverScanned)
            try capture(SettingsView().environmentObject(model), size: CGSize(width: 440, height: 200),
                        appearance: appearance, to: folder.appendingPathComponent("7-settings-\(suffix).png"))
        }
    }

    /// The Full Disk Access screen (waiting and switched on) and the Overview's "What Roomy couldn't scan".
    func testCaptureFullDiskAccessScreens() async throws {
        guard let path = ProcessInfo.processInfo.environment["ROOMY_SCREENSHOTS_DIR"], !path.isEmpty else {
            throw XCTSkip("Set ROOMY_SCREENSHOTS_DIR to capture screenshots")
        }
        let folder = URL(fileURLWithPath: path, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let scannedAt = Date(timeIntervalSince1970: 1_793_000_000)

        for appearance in [NSAppearance.Name.aqua, .darkAqua] {
            let suffix = appearance == .aqua ? "light" : "dark"
            for (name, access) in [("10-access-waiting", FullDiskAccess.notGranted), ("11-access-on", .granted)] {
                let model = makeModel(mode: .simple, state: .neverScanned, access: access)
                await model.refreshFullDiskAccess()
                try capture(FullDiskAccessOnboarding().environmentObject(model), size: CGSize(width: 520, height: 640),
                            appearance: appearance, to: folder.appendingPathComponent("\(name)-\(suffix).png"))
            }
            let model = makeModel(mode: .simple, state: .loaded(MockData.overview(for: .normal), scannedAt: scannedAt),
                                  access: .notGranted)
            await model.refreshFullDiskAccess()
            try capture(ContentView().environmentObject(model), size: CGSize(width: 1120, height: 1300),
                        appearance: appearance, to: folder.appendingPathComponent("12-overview-unscanned-\(suffix).png"))
        }
    }

    private func makeModel(mode: Mode, state: AppModel.ScanState, access: FullDiskAccess = .granted) -> AppModel {
        let defaults = UserDefaults(suiteName: suiteName) ?? .standard
        defaults.removePersistentDomain(forName: suiteName)
        // The Full Disk Access screen has been seen, so it never opens over a screenshot by itself.
        defaults.set(true, forKey: AppModel.accessOnboardingKey)
        let client = MockCoreClient()
        client.access = access
        let model = AppModel(client: client, defaults: defaults, pollInterval: 0)
        model.mode = mode
        model.showForPreview(state)
        return model
    }

    /// Apps and caches, with the biggest item's detail open.
    private func makeListModel(mode: Mode, overview: Overview, scannedAt: Date) -> AppModel {
        let model = makeModel(mode: mode, state: .neverScanned)
        model.selection = .appsAndCaches
        let items = MockData.items
            .filter { [.cache, .appData].contains($0.category) }
            .sorted(by: AppModel.order(for: .largestFirst))
        let selected = items[0]
        model.showForPreview(.loaded(overview, scannedAt: scannedAt), list: .loaded(items),
                             detail: .loaded(MockData.detail(for: selected, mode: mode)), selectedItemID: selected.id)
        return model
    }

    /// Puts the view in a real window (so sidebars, lists and toolbars draw), then saves the window as PNG.
    private func capture<V: View>(_ view: V, size: CGSize, appearance: NSAppearance.Name, to url: URL) throws {
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.appearance = NSAppearance(named: appearance)
        window.title = "Roomy"
        let host = NSHostingView(rootView: view)
        if #available(macOS 14.0, *) {
            host.sceneBridgingOptions = [.toolbars, .title]
        }
        window.contentView = host
        window.setContentSize(size)
        window.orderFrontRegardless()
        defer { window.close() }

        RunLoop.main.run(until: Date().addingTimeInterval(1.5))
        window.displayIfNeeded()

        // A real window-server capture draws sidebars and glass materials exactly as on screen.
        // It needs Screen Recording permission for Terminal; without it, fall back to drawing the views.
        if Self.screencapture(windowNumber: window.windowNumber, to: url) { return }

        guard let frameView = window.contentView?.superview ?? window.contentView else {
            throw CaptureError.noView
        }
        guard let rep = frameView.bitmapImageRepForCachingDisplay(in: frameView.bounds) else {
            throw CaptureError.noBitmap
        }
        frameView.cacheDisplay(in: frameView.bounds, to: rep)
        guard let data = rep.representation(using: .png, properties: [:]) else { throw CaptureError.noPNG }
        try data.write(to: url)
    }

    /// Captures one window with the system `screencapture` tool. True if a usable PNG was written.
    ///
    /// `screencapture` overwrites `url`. An older file left there is not deleted first (the safety
    /// grep allows no file deletion in the app or its tests); it is ignored by the date check below.
    private static func screencapture(windowNumber: Int, to url: URL) -> Bool {
        let started = Date().addingTimeInterval(-1)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        process.arguments = ["-x", "-o", "-l", String(windowNumber), url.path]
        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return false
        }
        let attributes = (try? FileManager.default.attributesOfItem(atPath: url.path)) ?? [:]
        let size = attributes[.size] as? Int ?? 0
        let written = (attributes[.modificationDate] as? Date).map { $0 >= started } ?? false
        return process.terminationStatus == 0 && written && size > 10_000
    }

    private enum CaptureError: Error { case noView, noBitmap, noPNG }
}
