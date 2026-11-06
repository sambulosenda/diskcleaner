import XCTest
@testable import Roomy

/// The System Data screen's model and words, against the mock. Nothing here reads the disk.
@MainActor
final class SystemDataTests: XCTestCase {
    private func loaded(_ model: SystemDataModel) -> SystemData? {
        if case .loaded(let data) = model.phase { return data }
        return nil
    }

    func testLoadAsksTheCoreInTheCurrentMode() async {
        let mock = MockCoreClient()
        let model = SystemDataModel(client: mock)
        XCTAssertEqual(model.phase, .loading)
        await model.load(mode: .simple)
        await model.load(mode: .developer)
        XCTAssertEqual(mock.systemDataCalls, [.simple, .developer])
        XCTAssertNotNil(loaded(model))
    }

    func testSimpleModeHasNoPaths() async {
        let model = SystemDataModel(client: MockCoreClient())
        await model.load(mode: .simple)
        guard let data = loaded(model) else { return XCTFail("\(model.phase)") }
        XCTAssertTrue(data.parts.flatMap(\.places).allSatisfy { $0.path == nil })
        await model.load(mode: .developer)
        guard let dev = loaded(model) else { return XCTFail("\(model.phase)") }
        XCTAssertTrue(dev.parts.flatMap(\.places).allSatisfy { $0.path != nil })
    }

    func testTotalIsTheSumOfTheParts() {
        let data = MockData.systemData(mode: .simple)
        XCTAssertEqual(data.totalBytes, data.parts.reduce(UInt64(0)) { $0 + $1.sizeBytes })
    }

    func testPartsAreListedLargestFirst() {
        let sorted = SystemDataText.sorted(MockData.systemData(mode: .simple).parts)
        XCTAssertEqual(sorted.map(\.sizeBytes), sorted.map(\.sizeBytes).sorted(by: >))
        XCTAssertEqual(sorted.first?.kind, .caches)
    }

    func testEveryPlaceHasPlainWords() {
        for place in MockData.systemData(mode: .simple).parts.flatMap(\.places) {
            XCTAssertNotEqual(SystemDataText.place(place.id), place.id, "no words for \(place.id)")
        }
        let kinds: [SystemDataKind] = [.caches, .logs, .softwareUpdates, .temporaryFiles, .swap, .sleepImage]
        for kind in kinds {
            XCTAssertFalse(SystemDataText.title(kind).isEmpty)
            XCTAssertFalse(SystemDataText.explain(kind).lowercased().contains("junk"))
            XCTAssertFalse(SystemDataText.advice(kind).lowercased().contains("junk"))
        }
    }

    func testOnlyPartsWithItemsLinkSomewhere() {
        XCTAssertEqual(SystemDataText.destination(.caches), .appsAndCaches)
        XCTAssertEqual(SystemDataText.destination(.softwareUpdates), .downloads)
        XCTAssertEqual(SystemDataText.actionTitle(.logs), "Show items to clean")
        XCTAssertEqual(SystemDataText.actionTitle(.caches), "Show in Apps and caches")
        XCTAssertNil(SystemDataText.actionTitle(.sleepImage))
        XCTAssertNil(SystemDataText.actionTitle(.swap))
        XCTAssertNil(SystemDataText.actionTitle(.temporaryFiles))
    }

    func testSnapshotWords() {
        XCTAssertEqual(SystemDataText.snapshotCount(0), "None")
        XCTAssertEqual(SystemDataText.snapshotCount(1), "1 snapshot")
        XCTAssertEqual(SystemDataText.snapshotCount(3), "3 snapshots")
        let data = MockData.systemData(mode: .simple)
        let dates = SystemDataText.snapshotDates(data.snapshots)
        XCTAssertEqual(dates, "Taken between \(SystemDataText.snapshotDay(1_793_779_200)) and \(SystemDataText.snapshotDay(1_793_957_412))")
        // Read as UTC, a snapshot named 4 Nov at 08:00 is 4 Nov wherever the Mac is.
        XCTAssertEqual(SystemDataText.snapshotDay(1_793_779_200), SystemDataText.snapshotDay(1_793_750_400))
    }

    func testUnlistedSnapshotsHaveNoDates() async {
        let mock = MockCoreClient()
        mock.snapshotsUnavailable = true
        let model = SystemDataModel(client: mock)
        await model.load(mode: .simple)
        guard let data = loaded(model) else { return XCTFail("\(model.phase)") }
        guard case .unavailable(let reason) = data.snapshots.status else { return XCTFail("\(data.snapshots)") }
        XCTAssertFalse(reason.isEmpty)
        XCTAssertNil(SystemDataText.snapshotDates(data.snapshots))
    }
}
