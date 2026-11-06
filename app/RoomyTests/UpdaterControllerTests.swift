import XCTest
@testable import Roomy

/// Tests for the Sparkle wiring (task T13). They never start Sparkle or reach the network.
final class UpdaterControllerTests: XCTestCase {
    private let feed = "https://github.com/sambulosenda/diskcleaner/releases/latest/download/appcast.xml"

    func testConfiguredNeedsFeedAndKey() {
        XCTAssertTrue(UpdaterController.isConfigured(info: ["SUFeedURL": feed, "SUPublicEDKey": "abc123="]))
    }

    func testEmptyKeyIsNotConfigured() {
        XCTAssertFalse(UpdaterController.isConfigured(info: ["SUFeedURL": feed, "SUPublicEDKey": ""]))
        XCTAssertFalse(UpdaterController.isConfigured(info: ["SUFeedURL": feed, "SUPublicEDKey": "  "]))
        XCTAssertFalse(UpdaterController.isConfigured(info: ["SUFeedURL": feed]))
    }

    func testUnexpandedBuildSettingIsNotConfigured() {
        XCTAssertFalse(UpdaterController.isConfigured(
            info: ["SUFeedURL": feed, "SUPublicEDKey": "$(ROOMY_SPARKLE_PUBLIC_KEY)"]
        ))
    }

    func testMissingFeedIsNotConfigured() {
        XCTAssertFalse(UpdaterController.isConfigured(info: ["SUFeedURL": "", "SUPublicEDKey": "abc123="]))
    }

    /// The test host is a development build with no public key, so updates must be off.
    func testDevelopmentBuildHasUpdatesOff() {
        XCTAssertFalse(UpdaterController.isConfigured(info: Bundle.main.infoDictionary ?? [:]))
    }

    func testLoopbackFeedsAreAccepted() {
        for url in [
            "http://127.0.0.1:8000/appcast.xml",
            "http://localhost:8000/appcast.xml",
            "https://LOCALHOST/appcast.xml",
            "http://[::1]:8000/appcast.xml",
        ] {
            XCTAssertEqual(FeedDelegate.loopbackFeed(url), url, url)
        }
    }

    func testOtherFeedsAreRefused() {
        for url in [
            nil,
            "",
            "https://example.com/appcast.xml",
            "http://127.0.0.1.example.com/appcast.xml",
            "http://localhost.example.com/appcast.xml",
            "http://user:pw@127.0.0.1/appcast.xml",
            "file:///tmp/appcast.xml",
            "ftp://127.0.0.1/appcast.xml",
            "http://192.168.1.10/appcast.xml",
            "not a url",
        ] {
            XCTAssertNil(FeedDelegate.loopbackFeed(url), url ?? "nil")
        }
    }
}
