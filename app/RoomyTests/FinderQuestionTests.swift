import XCTest
@testable import Roomy

@MainActor
final class FinderQuestionTests: XCTestCase {
    private let suiteName = "RoomyTests.FinderQuestion"

    private lazy var defaults: UserDefaults = {
        let store = UserDefaults(suiteName: suiteName) ?? .standard
        store.removePersistentDomain(forName: suiteName)
        return store
    }()

    private func makeModel() -> (AppModel, MockCoreClient) {
        let client = MockCoreClient(scenario: .normal, totalSteps: 5)
        return (AppModel(client: client, defaults: defaults, pollInterval: 0), client)
    }

    private func answer(_ matched: PlaceMatch, item: ItemSummary? = nil, count: UInt64 = 0,
                        safe: UInt64 = 0, review: UInt64 = 0) -> PlaceAnswer {
        PlaceAnswer(matched: matched, item: item, itemCount: count, likelySafeBytes: safe, reviewFirstBytes: review)
    }

    func testAQuestionAfterAScanIsAnsweredFromThatScan() async {
        let (model, client) = makeModel()
        model.startScan()
        await model.waitForScan()
        let item = MockData.items[0]
        client.placeAnswer = answer(.item, item: item)

        model.askAbout(URL(fileURLWithPath: "/Users/sam/Library/Caches/Thing"))
        XCTAssertEqual(model.placeQuestion?.name, "Thing")
        XCTAssertEqual(model.placeQuestion?.state, .checking)
        await model.waitForPlaceAnswer()

        XCTAssertEqual(model.placeQuestion?.state, .answered(client.placeAnswer))
        XCTAssertEqual(client.placesAsked, ["/Users/sam/Library/Caches/Thing"])
        XCTAssertTrue(model.checkedItemIDs.isEmpty, "asking never ticks anything")
    }

    func testAQuestionBeforeAnyScanScansFirstThenAnswers() async {
        let (model, client) = makeModel()
        model.askAbout(URL(fileURLWithPath: "/Users/sam/Movies"))
        XCTAssertEqual(model.placeQuestion?.state, .checking)
        XCTAssertTrue(model.isScanning)
        await model.waitForScan()
        XCTAssertEqual(model.placeQuestion?.state, .answered(client.placeAnswer))
        XCTAssertEqual(client.placesAsked, ["/Users/sam/Movies"])
    }

    func testAFailedScanSaysRoomyCannotAnswerYet() async {
        let client = MockCoreClient(scenario: .failure, totalSteps: 5)
        let model = AppModel(client: client, defaults: defaults, pollInterval: 0)
        model.askAbout(URL(fileURLWithPath: "/Users/sam/Movies"))
        await model.waitForScan()
        XCTAssertEqual(model.placeQuestion?.state, .unavailable)
        XCTAssertTrue(client.placesAsked.isEmpty)
    }

    func testClosingTheSheetBeforeTheAnswerArrivesKeepsItClosed() async {
        let (model, _) = makeModel()
        model.askAbout(URL(fileURLWithPath: "/Users/sam/Movies"))
        model.placeQuestion = nil
        await model.waitForScan()
        XCTAssertNil(model.placeQuestion)
    }

    func testTheWordsForEachKindOfAnswer() {
        let item = MockData.items[0]
        XCTAssertEqual(PlaceText.lead(for: answer(.item, item: item)), item.explain)
        XCTAssertTrue(PlaceText.lead(for: answer(.insideItem, item: item)).hasPrefix("This is part of \(item.title)."))
        let both = PlaceText.lead(for: answer(.holdsItems, count: 3, safe: 2_000_000_000, review: 1_000_000_000))
        XCTAssertTrue(both.hasPrefix("This folder holds 3 things Roomy knows about."))
        XCTAssertTrue(both.contains("likely safe to remove"))
        XCTAssertTrue(both.contains("worth checking first"))
        XCTAssertTrue(PlaceText.lead(for: answer(.holdsItems, count: 1)).contains("1 thing Roomy knows about. Roomy keeps all of it."))
        XCTAssertEqual(PlaceText.lead(for: answer(.notListed)), PlaceText.notListed)
        XCTAssertTrue(PlaceText.lead(for: answer(.outsideHome)).contains("outside your home folder"))
        XCTAssertEqual(PlaceText.title(name: "Movies"), "What is \u{201C}Movies\u{201D}?")
    }
}
