import XCTest
@testable import Roomy

/// The shared shapes, gaps and motion, and the words VoiceOver hears when a screen changes by itself.
@MainActor
final class DesignTests: XCTestCase {
    func testRadiiAndGapsStayOneSmallOrderedSet() {
        XCTAssertEqual([Radius.small, Radius.medium, Radius.large], [4, 8, 12])
        let gaps = [Space.xxs, Space.xs, Space.s, Space.m, Space.l, Space.xl, Space.xxl, Space.screen]
        XCTAssertEqual(gaps, gaps.sorted())
        XCTAssertEqual(Set(gaps).count, gaps.count)
        // Everything from 4 points up sits on the 4 point grid.
        XCTAssertTrue(gaps.dropFirst().allSatisfy { $0.truncatingRemainder(dividingBy: 4) == 0 })
    }

    func testDetailPanelIsAThirdOfTheWindowWithinLimits() {
        XCTAssertEqual(DetailPane.width(in: 600), DetailPane.minWidth)
        XCTAssertEqual(DetailPane.width(in: 960), 320)
        XCTAssertEqual(DetailPane.width(in: 2400), DetailPane.maxWidth)
    }

    func testSettingsHasThreeNamedTabs() {
        XCTAssertEqual(SettingsTab.allCases.map(\.title), ["General", "Alerts", "Folders to Keep"])
        XCTAssertTrue(SettingsTab.allCases.allSatisfy { !$0.symbol.isEmpty })
    }

    func testScanAnnouncementsSayWhatHappened() {
        let overview = MockData.overview(for: .normal)
        XCTAssertEqual(ScanText.spoken(.neverScanned), "")
        XCTAssertEqual(ScanText.spoken(.scanning(ScanProgress(bytesSeen: 1, itemsSeen: 1, phase: .scanning))),
                       "Scanning your Mac")
        XCTAssertEqual(ScanText.spoken(.loaded(overview, scannedAt: Date())),
                       "Scan finished. \(OverviewHeadline.headline(for: overview))")
        XCTAssertTrue(ScanText.spoken(.failed("No.")).contains("Nothing on your Mac was changed"))
    }

    func testCleanupAnnouncementsStayQuietUntilThereIsSomethingToSay() {
        XCTAssertEqual(CleanupText.spoken(.closed), "")
        XCTAssertEqual(CleanupText.spoken(.checking), "")
        XCTAssertEqual(CleanupText.spoken(.failed("The plan ran out.")), "Roomy couldn't finish this. The plan ran out.")
    }

    func testToolReviewsAlwaysSayTheyCannotBeUndone() {
        XCTAssertTrue(DockerText.cannotUndo.contains("can't be undone"))
        for kind in [ToolCleanupKind.homebrew, .unavailableSimulators, .androidSdkPackage] {
            XCTAssertTrue(ToolText.cannotUndo(kind).contains("can't be undone"), "\(kind)")
        }
    }

    func testFinishedToolCleanupsPromiseOnlyWhatTheToolReported() {
        let docker = DockerCleanupResult(kind: .buildCache, success: true, reclaimedBytes: nil, savedToHistory: true)
        XCTAssertEqual(DockerText.doneHeadline(docker), "Docker finished")
        let tool = ToolCleanupResult(kind: .homebrew, commandLine: "brew cleanup", success: true,
                                     reclaimedBytes: 2_000_000_000, savedToHistory: true)
        XCTAssertEqual(ToolText.doneHeadline(tool), "About \(Format.bytes(2_000_000_000)) freed")
    }
}
