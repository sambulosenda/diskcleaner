import XCTest
@testable import Roomy

/// Calls each T09 command (docs/command-contract.md) against a fixture home.
///
/// Needs the core built with the `testing` feature, which `./scripts/build-ffi.sh` does by default.
final class CommandContractTests: XCTestCase {
    private var fixtureDir: URL!
    private var core: RoomyCore!
    /// The fixture home's canonical path.
    private var homePath: String!

    override func setUpWithError() throws {
        fixtureDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("roomy-t09-\(UUID().uuidString)")
        let home = try makeFixtureHome(dir: fixtureDir.path)
        homePath = home
        core = try RoomyCore.withHomeForTesting(home: home)
    }

    // Fixture homes are left in the temporary directory on purpose: nothing in the app or its tests
    // deletes files except the Rust executor, and macOS clears the temporary directory itself.

    /// Polls until the scan stops scanning and classifying.
    private func waitForScan(_ handle: ScanHandle) throws -> ScanPhase {
        let deadline = Date().addingTimeInterval(120)
        while Date() < deadline {
            let phase = try core.scanProgress(handle: handle).phase
            switch phase {
            case .scanning, .classifying:
                Thread.sleep(forTimeInterval: 0.01)
            default:
                return phase
            }
        }
        XCTFail("The scan never ended")
        return .cancelled
    }

    private func finishedScan(_ mode: Mode) throws -> ScanHandle {
        let handle = core.startScan(mode: mode)
        XCTAssertEqual(try waitForScan(handle), .finished)
        return handle
    }

    func testStartScanAndProgressReachFinished() throws {
        let handle = core.startScan(mode: .simple)
        XCTAssertEqual(try waitForScan(handle), .finished)
        let progress = try core.scanProgress(handle: handle)
        XCTAssertGreaterThan(progress.bytesSeen, 0)
        XCTAssertGreaterThan(progress.itemsSeen, 0)
    }

    func testOverviewHeadlineIsTheExactSumOfLikelySafeItems() throws {
        let handle = try finishedScan(.developer)
        let overview = try core.overview(handle: handle)
        let likely = try core.listItems(handle: handle, category: nil, tier: .likelySafe, sort: .largestFirst)
        XCTAssertGreaterThan(overview.likelySafeBytes, 0)
        XCTAssertEqual(overview.likelySafeBytes, likely.reduce(0) { $0 + $1.sizeBytes })
        XCTAssertEqual(overview.likelySafeBytes, overview.categories.reduce(0) { $0 + $1.likelySafeBytes })
        XCTAssertGreaterThan(overview.diskTotal, 0)
        XCTAssertLessThanOrEqual(overview.diskFree, overview.diskTotal)
        XCTAssertLessThanOrEqual(overview.topItems.count, 8)
        XCTAssertFalse(overview.topItems.contains { $0.tier == .keep })
        XCTAssertEqual(overview.unreadableAreas.reduce(0) { $0 + $1.folderCount }, overview.unreadableCount)
    }

    func testFullDiskAccessIsOnForAFixtureHomeAnyAppCanRead() {
        // The fixture home has a Library/Messages folder made by the test, which macOS doesn't protect.
        XCTAssertEqual(core.fullDiskAccess(), .granted)
    }

    func testListItemsFiltersAndSorts() throws {
        let handle = try finishedScan(.developer)
        let all = try core.listItems(handle: handle, category: nil, tier: nil, sort: .largestFirst)
        XCTAssertFalse(all.isEmpty)
        XCTAssertEqual(all.map(\.sizeBytes), all.map(\.sizeBytes).sorted(by: >))
        let build = try core.listItems(handle: handle, category: .build, tier: nil, sort: .smallestFirst)
        XCTAssertFalse(build.isEmpty)
        XCTAssertTrue(build.allSatisfy { $0.category == .build })
        XCTAssertEqual(build.map(\.sizeBytes), build.map(\.sizeBytes).sorted())
        let byTitle = try core.listItems(handle: handle, category: nil, tier: nil, sort: .title)
        XCTAssertEqual(byTitle.count, all.count)
    }

    func testItemDetailHasNoPathInSimpleMode() throws {
        let handle = try finishedScan(.simple)
        for item in try core.listItems(handle: handle, category: nil, tier: nil, sort: .largestFirst) {
            let detail = try core.itemDetail(handle: handle, itemId: item.id)
            XCTAssertEqual(detail.id, item.id)
            XCTAssertNil(detail.path, "\(detail.ruleId) has a path in simple mode")
        }
    }

    func testItemDetailShowsDisplayPathsInDeveloperMode() throws {
        let handle = try finishedScan(.developer)
        let items = try core.listItems(handle: handle, category: nil, tier: nil, sort: .largestFirst)
        let first = try XCTUnwrap(items.first)
        let detail = try core.itemDetail(handle: handle, itemId: first.id)
        let path = try XCTUnwrap(detail.path)
        XCTAssertTrue(path.hasPrefix("~/"), path)
        XCTAssertFalse(detail.ruleId.isEmpty)
    }

    func testCancelScanEndsTheScan() throws {
        let handle = core.startScan(mode: .developer)
        core.cancelScan(handle: handle)
        let phase = try waitForScan(handle)
        // The fixture is small, so the scan may finish before the cancel lands.
        XCTAssertTrue(phase == .cancelled || phase == .finished, "\(phase)")
        if phase == .cancelled {
            XCTAssertThrowsError(try core.overview(handle: handle)) { error in
                guard case CoreError.ScanCancelled = error else {
                    return XCTFail("Expected ScanCancelled, got \(error)")
                }
            }
        }
    }

    func testUnknownHandlesAndItemsThrow() throws {
        let unknown = ScanHandle(id: 999)
        XCTAssertThrowsError(try core.overview(handle: unknown)) { error in
            guard case CoreError.UnknownScan = error else {
                return XCTFail("Expected UnknownScan, got \(error)")
            }
        }
        let handle = try finishedScan(.simple)
        XCTAssertThrowsError(try core.itemDetail(handle: handle, itemId: String(repeating: "0", count: 32))) { error in
            guard case CoreError.UnknownItem = error else {
                return XCTFail("Expected UnknownItem, got \(error)")
            }
        }
        core.closeScan(handle: handle)
        XCTAssertThrowsError(try core.scanProgress(handle: handle))
    }

    // MARK: - Cleanup preview (dry run)

    /// Every path under `root` with its size and modification time. Never follows a symlink.
    private func snapshot(_ root: URL) throws -> [String] {
        let keys: [URLResourceKey] = [.fileSizeKey, .contentModificationDateKey]
        let walker = FileManager.default.enumerator(at: root, includingPropertiesForKeys: keys, options: [])
        var out: [String] = []
        while let url = walker?.nextObject() as? URL {
            let values = try url.resourceValues(forKeys: Set(keys))
            out.append("\(url.path) \(values.fileSize ?? -1) \(values.contentModificationDate?.timeIntervalSince1970 ?? 0)")
        }
        return out.sorted()
    }

    func testPreviewCleanupSaysWhatWouldMoveAndMovesNothing() throws {
        let home = URL(fileURLWithPath: try makeFixtureHome(dir: fixtureDir.path + "-cleanup"))
        let trash = home.appendingPathComponent(".Trash")
        try FileManager.default.createDirectory(at: trash, withIntermediateDirectories: false)
        core = try RoomyCore.withHomeForTesting(home: home.path)
        // The fixture was made just now: without this every item changed "in the last few minutes".
        core.relaxInUseChecksForTesting()
        let handle = try finishedScan(.simple)
        let likely = try core.listItems(handle: handle, category: nil, tier: .likelySafe, sort: .largestFirst)
        XCTAssertFalse(likely.isEmpty)
        let before = try snapshot(home)

        let preview = try core.previewCleanup(handle: handle, choices: likely.map {
            CleanupChoice(itemId: $0.id, chosenAlone: true)
        })

        XCTAssertEqual(try snapshot(home), before, "the preview changed something")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: trash.path), [])
        XCTAssertEqual(preview.items.count, likely.count)
        XCTAssertTrue(preview.items.contains { $0.check == .willMove })
        XCTAssertTrue(preview.items.allSatisfy { $0.path == nil }, "no path in simple mode")
        XCTAssertEqual(preview.expiresInSeconds, 600)
        let plan = try XCTUnwrap(preview.plan)
        core.discardCleanup(plan: plan)
        core.discardCleanup(plan: plan)
    }

    func testPreviewCleanupWithoutATrashSaysSoInPlainWords() throws {
        let handle = try finishedScan(.simple)
        let likely = try core.listItems(handle: handle, category: nil, tier: .likelySafe, sort: .largestFirst)
        XCTAssertThrowsError(try core.previewCleanup(handle: handle, choices: likely.map {
            CleanupChoice(itemId: $0.id, chosenAlone: true)
        })) { error in
            guard case CoreError.CleanupUnavailable(let reason) = error else {
                return XCTFail("Expected CleanupUnavailable, got \(error)")
            }
            XCTAssertFalse(reason.contains("/"), reason)
        }
    }

    func testRunCleanupMovesToTheTrashAndPutBackRestores() throws {
        let home = URL(fileURLWithPath: try makeFixtureHome(dir: fixtureDir.path + "-run"))
        let trash = home.appendingPathComponent(".Trash")
        try FileManager.default.createDirectory(at: trash, withIntermediateDirectories: false)
        core = try RoomyCore.withHomeForTesting(home: home.path)
        // The fixture was made just now: without this every item changed "in the last few minutes".
        core.relaxInUseChecksForTesting()
        let handle = try finishedScan(.simple)
        let likely = try core.listItems(handle: handle, category: nil, tier: .likelySafe, sort: .largestFirst)
        let preview = try core.previewCleanup(handle: handle, choices: likely.map {
            CleanupChoice(itemId: $0.id, chosenAlone: true)
        })
        let plan = try XCTUnwrap(preview.plan)

        let result = try core.runCleanup(plan: plan)
        XCTAssertEqual(result.bytesMoved, preview.bytesToMove)
        XCTAssertTrue(result.items.allSatisfy(\.done))
        XCTAssertFalse(try FileManager.default.contentsOfDirectory(atPath: trash.path).isEmpty)
        XCTAssertThrowsError(try core.runCleanup(plan: plan), "a plan is single use")

        let back = try core.putBackCleanup(cleanup: try XCTUnwrap(result.cleanup))
        XCTAssertEqual(back.bytesPutBack, result.bytesMoved)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: trash.path), [])
        let history = core.cleanupHistory(limit: 10)
        XCTAssertEqual(history.count, 1)
        XCTAssertNotNil(history.first?.putBackAt)
    }

    // MARK: - App uninstaller

    /// Writes a small app bundle into `folder`, the way an installer would. Writes only.
    private func makeApp(in folder: URL, name: String, bundleId: String) throws {
        let contents = folder.appendingPathComponent("\(name).app/Contents/MacOS", isDirectory: true)
        try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
        let info: [String: Any] = ["CFBundleIdentifier": bundleId, "CFBundleName": name]
        let data = try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        try data.write(to: folder.appendingPathComponent("\(name).app/Contents/Info.plist"))
        try Data(repeating: 7, count: 20_000).write(to: contents.appendingPathComponent(name))
    }

    func testUninstallMovesAnAppAndItsCachesAndCanPutThemBack() throws {
        core.useHomeApplicationsOnlyForTesting()
        core.relaxInUseChecksForTesting()
        let home = URL(fileURLWithPath: homePath)
        try makeApp(in: home.appendingPathComponent("Applications"), name: "Tiny", bundleId: "com.example.tiny")
        try FileManager.default.createDirectory(at: home.appendingPathComponent(".Trash"),
                                                withIntermediateDirectories: false)
        let cache = home.appendingPathComponent("Library/Caches/com.example.tiny", isDirectory: true)
        try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: true)
        try Data(repeating: 1, count: 5_000).write(to: cache.appendingPathComponent("c"))

        let apps = core.listApps()
        let tiny = try XCTUnwrap(apps.first { $0.name == "Tiny" })
        XCTAssertEqual(tiny.location, .homeApplications)
        XCTAssertFalse(apps.contains { $0.bundleId.hasPrefix("com.apple.") })
        let review = try core.reviewApp(appId: tiny.appId, mode: .simple)
        XCTAssertTrue(review.parts.contains { $0.itemId == "app" })
        XCTAssertTrue(review.parts.allSatisfy { $0.path == nil })
        let preview = try core.previewUninstall(appId: tiny.appId, partIds: review.parts.map(\.itemId))
        XCTAssertTrue(preview.items.allSatisfy { $0.check == .willMove }, "\(preview.items)")
        let result = try core.runCleanup(plan: try XCTUnwrap(preview.plan))
        XCTAssertTrue(result.items.allSatisfy(\.done), "\(result.items)")
        XCTAssertFalse(FileManager.default.fileExists(atPath: cache.path))
        _ = try core.putBackCleanup(cleanup: try XCTUnwrap(result.cleanup))
        XCTAssertTrue(FileManager.default.fileExists(atPath: cache.appendingPathComponent("c").path))
    }

    func testUninstallRefusesIdsFromAnOlderList() throws {
        core.useHomeApplicationsOnlyForTesting()
        let home = URL(fileURLWithPath: homePath)
        try makeApp(in: home.appendingPathComponent("Applications"), name: "Tiny", bundleId: "com.example.tiny")
        let old = try XCTUnwrap(core.listApps().first)
        _ = core.listApps()
        XCTAssertThrowsError(try core.reviewApp(appId: old.appId, mode: .simple)) { error in
            guard case CoreError.UnknownApp = error else { return XCTFail("Expected UnknownApp, got \(error)") }
        }
    }

    func testLargeFilesListsNoPathInSimpleModeAndNothingIsTicked() throws {
        let handle = try finishedScan(.simple)
        let files = try core.largeFiles(handle: handle)
        XCTAssertEqual(files.oldAfterDays, 90)
        XCTAssertLessThanOrEqual(files.largeFiles.count, 500)
        for row in files.largeFiles + files.oldDownloads {
            XCTAssertNil(row.path)
            XCTAssertFalse(row.name.contains("/"))
        }
        let sizes = files.largeFiles.map(\.sizeBytes)
        XCTAssertEqual(sizes, sizes.sorted(by: >))
    }

    // MARK: - Duplicate finder

    /// Polls until the duplicate search ends.
    private func waitForDuplicates() -> DuplicateSearch {
        let deadline = Date().addingTimeInterval(120)
        while Date() < deadline {
            let progress = core.duplicateSearchProgress()
            switch progress {
            case .looking, .comparing:
                Thread.sleep(forTimeInterval: 0.01)
            default:
                return progress
            }
        }
        XCTFail("The duplicate search never ended")
        return .cancelled
    }

    func testDuplicateFinderMovesOneCopyKeepsTheOtherAndCanPutItBack() throws {
        core.relaxInUseChecksForTesting()
        let home = URL(fileURLWithPath: homePath)
        let trash = home.appendingPathComponent(".Trash")
        if !FileManager.default.fileExists(atPath: trash.path) {
            try FileManager.default.createDirectory(at: trash, withIntermediateDirectories: false)
        }
        let folder = home.appendingPathComponent("Documents/Roomy duplicates test", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let data = Data((0..<(2 * 1024 * 1024)).map { UInt8(truncatingIfNeeded: $0 &* 7) })
        let first = folder.appendingPathComponent("song.m4a")
        let second = folder.appendingPathComponent("song copy.m4a")
        try data.write(to: first)
        try data.write(to: second)

        core.startDuplicateSearch(mode: .simple)
        guard case .finished(let report) = waitForDuplicates() else { return XCTFail("the search didn't finish") }
        let group = try XCTUnwrap(report.groups.first { $0.copies.contains { $0.name == "song copy.m4a" } })
        XCTAssertEqual(group.copies.count, 2)
        XCTAssertTrue(group.copies.allSatisfy { $0.path == nil })
        // Ticking every copy is refused.
        XCTAssertThrowsError(try core.previewDuplicateCleanup(copyIds: group.copies.map(\.copyId))) { error in
            guard case CoreError.CleanupUnavailable = error else { return XCTFail("\(error)") }
        }
        let preview = try core.previewDuplicateCleanup(copyIds: [group.copies[1].copyId])
        XCTAssertTrue(preview.items.allSatisfy { $0.check == .willMove }, "\(preview.items)")
        let result = try core.runCleanup(plan: try XCTUnwrap(preview.plan))
        XCTAssertTrue(result.items.allSatisfy(\.done), "\(result.items)")
        let left = [first, second].filter { FileManager.default.fileExists(atPath: $0.path) }
        XCTAssertEqual(left.count, 1, "exactly one copy stays")
        _ = try core.putBackCleanup(cleanup: try XCTUnwrap(result.cleanup))
        XCTAssertTrue(FileManager.default.fileExists(atPath: first.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: second.path))
    }
}
