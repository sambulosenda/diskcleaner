import XCTest
@testable import Roomy

private let gb: UInt64 = 1_000_000_000
private let day: TimeInterval = 24 * 60 * 60

/// A clock the test can move.
private final class ClockBox: @unchecked Sendable {
    var now: Date
    init(_ now: Date) { self.now = now }
}

@MainActor
final class WeeklyNoteTests: XCTestCase {
    private let suiteName = "RoomyTests.WeeklyNote"
    private var clock = Date(timeIntervalSince1970: 1_800_000_000)

    private lazy var defaults: UserDefaults = {
        let store = UserDefaults(suiteName: suiteName) ?? .standard
        store.removePersistentDomain(forName: suiteName)
        return store
    }()

    /// A model whose clock is fixed at the test's current time; `advance` moves it.
    private func makeModel() -> WeeklyNoteModel {
        let box = ClockBox(clock)
        boxes.append(box)
        return WeeklyNoteModel(defaults: defaults, now: { box.now })
    }

    private var boxes: [ClockBox] = []

    /// Moves every model's clock forward.
    private func advance(_ seconds: TimeInterval) {
        clock = clock.addingTimeInterval(seconds)
        boxes.forEach { $0.now = clock }
    }

    private func saveScan(likelySafe: UInt64, reviewFirst: UInt64 = 0) {
        LastScanSummary(likelySafeBytes: likelySafe, reviewFirstBytes: reviewFirst, scannedAt: clock)
            .save(to: defaults)
    }

    func testANoteIsDueOnlyAfterAWeekWithSpaceWorthFreeing() {
        let now = clock
        let scan = LastScanSummary(likelySafeBytes: 5 * gb, reviewFirstBytes: 0, scannedAt: now)
        let weekAgo = now.addingTimeInterval(-7 * day)
        XCTAssertTrue(WeeklyNote.isDue(summary: scan, enabled: true, answeredAt: weekAgo, now: now))
        XCTAssertFalse(WeeklyNote.isDue(summary: scan, enabled: false, answeredAt: weekAgo, now: now))
        XCTAssertFalse(WeeklyNote.isDue(summary: scan, enabled: true, answeredAt: now.addingTimeInterval(-6 * day), now: now))
        XCTAssertFalse(WeeklyNote.isDue(summary: scan, enabled: true, answeredAt: nil, now: now))
        XCTAssertFalse(WeeklyNote.isDue(summary: nil, enabled: true, answeredAt: weekAgo, now: now))
    }

    func testReviewFirstSpaceNeverPrompts() {
        let scan = LastScanSummary(likelySafeBytes: gb / 2, reviewFirstBytes: 80 * gb, scannedAt: clock)
        XCTAssertFalse(WeeklyNote.isDue(summary: scan, enabled: true,
                                        answeredAt: clock.addingTimeInterval(-30 * day), now: clock))
    }

    func testANewUserIsNotPromptedInTheFirstWeek() {
        saveScan(likelySafe: 9 * gb)
        let model = makeModel()
        XCTAssertNil(model.note)
        advance(6 * day)
        model.refresh()
        XCTAssertNil(model.note)
        advance(2 * day)
        model.refresh()
        XCTAssertNotNil(model.note)
    }

    func testAnsweringKeepsRoomyQuietForAWeek() {
        saveScan(likelySafe: 9 * gb)
        let model = makeModel()
        advance(8 * day)
        model.refresh()
        XCTAssertNotNil(model.note)
        model.answer()
        XCTAssertNil(model.note)
        advance(6 * day)
        model.refresh()
        XCTAssertNil(model.note)
        advance(1 * day)
        model.refresh()
        XCTAssertNotNil(model.note)
    }

    func testTurningItOffHidesTheNoteAndIsRemembered() {
        saveScan(likelySafe: 9 * gb)
        let model = makeModel()
        advance(8 * day)
        model.refresh()
        XCTAssertNotNil(model.note)
        model.isEnabled = false
        XCTAssertNil(model.note)
        XCTAssertFalse(makeModel().isEnabled)
    }

    func testTheNoteNamesTheSpaceAndNeverSoundsUrgent() {
        let scan = LastScanSummary(likelySafeBytes: 5 * gb, reviewFirstBytes: 0, scannedAt: clock)
        let text = WeeklyNote.text(for: scan, now: clock)
        XCTAssertTrue(text.hasPrefix("\(Format.bytes(5 * gb)) is ready to clean"))
        XCTAssertFalse(text.lowercased().contains("junk"))
        XCTAssertFalse(text.contains("!"))
    }
}
