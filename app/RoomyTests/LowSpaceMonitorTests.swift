import XCTest
@testable import Roomy

/// Counts alerts instead of showing them, and answers the permission question as told.
@MainActor
final class FakeLowSpaceNotifier: LowSpaceNotifier {
    var allow = true
    private(set) var permissionRequests = 0
    private(set) var alerts: [UInt64] = []

    func requestPermission() async -> Bool {
        permissionRequests += 1
        return allow
    }

    func notifyLowSpace(freeBytes: UInt64) {
        alerts.append(freeBytes)
    }
}

/// Feeds the monitor made-up free-space readings.
@MainActor
final class FakeDisk {
    var free: UInt64 = 100 * gb
    var total: UInt64 = 500 * gb
    var fails = false

    func read() throws -> DiskSummary {
        if fails { throw CoreError.Io(reason: "test") }
        return DiskSummary(totalBytes: total, freeBytes: free)
    }
}

private let gb: UInt64 = 1_000_000_000

@MainActor
final class LowSpaceMonitorTests: XCTestCase {
    private let suiteName = "RoomyTests.LowSpace"
    private let disk = FakeDisk()
    private let notifier = FakeLowSpaceNotifier()

    private lazy var defaults: UserDefaults = {
        let store = UserDefaults(suiteName: suiteName) ?? .standard
        store.removePersistentDomain(forName: suiteName)
        return store
    }()

    private func makeMonitor() -> LowSpaceMonitor {
        let disk = disk
        return LowSpaceMonitor(readDisk: { try await MainActor.run { try disk.read() } },
                               notifier: notifier, defaults: defaults)
    }

    /// Lets the alert task the monitor starts run to the end.
    private func settle() async {
        for _ in 0..<20 { await Task.yield() }
    }

    func testDefaultsAreOnWithTheCoreThreshold() {
        let monitor = makeMonitor()
        XCTAssertTrue(monitor.showInMenuBar)
        XCTAssertTrue(monitor.alertsEnabled)
        XCTAssertEqual(monitor.thresholdBytes, lowSpaceDefaultThreshold())
        XCTAssertEqual(monitor.thresholdChoices, lowSpaceThresholdChoices())
        XCTAssertEqual(monitor.reading, .unknown)
        XCTAssertEqual(monitor.menuBarTitle, "")
    }

    func testPlentyOfSpaceDoesNotAlert() async {
        let monitor = makeMonitor()
        await monitor.refresh()
        await settle()
        XCTAssertEqual(monitor.reading, .read(free: 100 * gb, total: 500 * gb))
        XCTAssertFalse(monitor.isLow)
        XCTAssertNil(monitor.lowSpaceText)
        XCTAssertEqual(notifier.permissionRequests, 0, "no permission prompt unless an alert is due")
        XCTAssertTrue(notifier.alerts.isEmpty)
    }

    func testLowSpaceAlertsOnceUntilSpaceRecovers() async {
        let monitor = makeMonitor()
        disk.free = 4 * gb
        await monitor.refresh()
        await settle()
        XCTAssertTrue(monitor.isLow)
        XCTAssertNotNil(monitor.lowSpaceText)
        XCTAssertEqual(notifier.alerts, [4 * gb])

        disk.free = 3 * gb
        await monitor.refresh()
        await settle()
        XCTAssertEqual(notifier.alerts.count, 1, "still low: no second alert")

        disk.free = 50 * gb
        await monitor.refresh()
        disk.free = 2 * gb
        await monitor.refresh()
        await settle()
        XCTAssertEqual(notifier.alerts, [4 * gb, 2 * gb])
    }

    func testAlertsOffStillShowsLowButNeverNotifies() async {
        let monitor = makeMonitor()
        monitor.alertsEnabled = false
        disk.free = 4 * gb
        await monitor.refresh()
        await settle()
        XCTAssertTrue(monitor.isLow)
        XCTAssertTrue(notifier.alerts.isEmpty)
        XCTAssertEqual(notifier.permissionRequests, 0)

        // Turning alerts on while space is low warns straight away.
        monitor.alertsEnabled = true
        await settle()
        XCTAssertEqual(notifier.alerts, [4 * gb])
    }

    func testRaisingTheThresholdCanMakeSpaceLow() async {
        let monitor = makeMonitor()
        disk.free = 15 * gb
        await monitor.refresh()
        XCTAssertFalse(monitor.isLow)
        monitor.thresholdBytes = 20 * gb
        await settle()
        XCTAssertTrue(monitor.isLow)
        XCTAssertEqual(notifier.alerts, [15 * gb])
    }

    func testDeniedPermissionShowsNothingAndSaysSo() async {
        notifier.allow = false
        let monitor = makeMonitor()
        disk.free = 1 * gb
        await monitor.refresh()
        await settle()
        XCTAssertTrue(notifier.alerts.isEmpty)
        XCTAssertTrue(monitor.notificationsDenied)
    }

    func testAFailedReadIsNeverLow() async {
        disk.fails = true
        let monitor = makeMonitor()
        await monitor.refresh()
        await settle()
        XCTAssertEqual(monitor.reading, .failed)
        XCTAssertFalse(monitor.isLow)
        XCTAssertTrue(notifier.alerts.isEmpty)
        XCTAssertEqual(monitor.menuBarTitle, "")
    }

    func testSettingsAreRemembered() {
        let first = makeMonitor()
        first.showInMenuBar = false
        first.alertsEnabled = false
        first.thresholdBytes = 50 * gb

        let second = makeMonitor()
        XCTAssertFalse(second.showInMenuBar)
        XCTAssertFalse(second.alertsEnabled)
        XCTAssertEqual(second.thresholdBytes, 50 * gb)
    }

    func testAStoredThresholdSettingsDoesNotOfferFallsBackToTheDefault() {
        defaults.set(NSNumber(value: UInt64(3)), forKey: LowSpaceMonitor.thresholdKey)
        XCTAssertEqual(makeMonitor().thresholdBytes, lowSpaceDefaultThreshold())
    }

    func testTheRealReaderReadsTheHomeVolume() async throws {
        let summary = try await LowSpaceMonitor.readHomeVolume()
        XCTAssertGreaterThan(summary.totalBytes, 0)
        XCTAssertLessThanOrEqual(summary.freeBytes, summary.totalBytes)
    }
}

/// The menu bar menu's summary of the last scan. Its button only opens Roomy or starts a scan.
@MainActor
final class MenuBarScanSummaryTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)

    func testBeforeAnyScanItOffersToScan() {
        let summary = MenuBarScanSummary.make(for: .neverScanned, now: now)
        XCTAssertEqual(summary.headline, "Scan to see what you can free")
        XCTAssertNil(summary.detail)
        XCTAssertEqual(summary.action, .scan)
    }

    func testWhileScanningThereIsNothingToPress() {
        let progress = ScanProgress(bytesSeen: 1, itemsSeen: 1, phase: .scanning)
        let summary = MenuBarScanSummary.make(for: .scanning(progress), now: now)
        XCTAssertEqual(summary.action, .none)
        XCTAssertTrue(summary.buttonTitle.isEmpty)
    }

    func testAFailedScanOffersToScanAgain() {
        let summary = MenuBarScanSummary.make(for: .failed("Something went wrong."), now: now)
        XCTAssertEqual(summary.action, .scan)
        XCTAssertEqual(summary.buttonTitle, "Scan Again\u{2026}")
    }

    func testAFinishedScanShowsTheOverviewHeadlineAndOpensReview() {
        let overview = MockData.overview(for: .normal)
        let scannedAt = now.addingTimeInterval(-2 * 60 * 60)
        let summary = MenuBarScanSummary.make(for: .loaded(overview, scannedAt: scannedAt), now: now)
        XCTAssertEqual(summary.headline, OverviewHeadline.headline(for: overview))
        XCTAssertTrue(summary.headline.contains(Format.bytes(overview.likelySafeBytes)))
        XCTAssertEqual(summary.detail, "Scanned \(Format.relative(scannedAt, now: now))")
        XCTAssertEqual(summary.action, .review)
        XCTAssertEqual(summary.buttonTitle, "Review in Diskwell\u{2026}")
    }

    func testNoWordingSoundsAlarming() {
        let states: [AppModel.ScanState] = [
            .neverScanned, .failed("x"),
            .loaded(MockData.overview(for: .normal), scannedAt: now),
        ]
        for state in states {
            let s = MenuBarScanSummary.make(for: state, now: now)
            for text in [s.headline, s.detail ?? "", s.buttonTitle] {
                XCTAssertFalse(text.lowercased().contains("junk"), text)
                XCTAssertFalse(text.lowercased().contains("delete"), text)
            }
        }
    }
}
