import XCTest
@testable import Roomy

/// Tests for the placeholder screen and the Rust bridge (T02).
final class RoomyTests: XCTestCase {
    func testContentViewCanBeCreated() {
        _ = ContentView()
    }

    func testCoreVersionIsNotEmpty() {
        XCTAssertFalse(coreVersion().isEmpty)
    }

    func testDiskSummaryReadsTheTemporaryFolder() throws {
        let summary = try diskSummary(path: NSTemporaryDirectory())
        XCTAssertGreaterThan(summary.totalBytes, 0)
        XCTAssertLessThanOrEqual(summary.freeBytes, summary.totalBytes)
    }

    func testRustErrorsArriveAsSwiftThrows() {
        XCTAssertThrowsError(try diskSummary(path: "not/absolute")) { error in
            guard let coreError = error as? CoreError, case .InvalidPath = coreError else {
                return XCTFail("Expected CoreError.InvalidPath, got \(error)")
            }
        }
    }

    func testMissingPathThrowsInsteadOfCrashing() {
        let missing = NSTemporaryDirectory() + "roomy-t02-this-path-does-not-exist"
        XCTAssertThrowsError(try diskSummary(path: missing)) { error in
            guard let coreError = error as? CoreError, case .Io = coreError else {
                return XCTFail("Expected CoreError.Io, got \(error)")
            }
        }
    }
}
