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
            let model = makeModel(mode: .developer, state: .neverScanned)
            try capture(SettingsView().environmentObject(model), size: CGSize(width: 440, height: 200),
                        appearance: appearance, to: folder.appendingPathComponent("7-settings-\(suffix).png"))
        }
    }

    private func makeModel(mode: Mode, state: AppModel.ScanState) -> AppModel {
        let defaults = UserDefaults(suiteName: suiteName) ?? .standard
        defaults.removePersistentDomain(forName: suiteName)
        let model = AppModel(client: MockCoreClient(), defaults: defaults, pollInterval: 0)
        model.mode = mode
        model.showForPreview(state)
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
