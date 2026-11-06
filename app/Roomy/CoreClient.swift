import Foundation

/// Everything the app asks of the Rust core, as T09 sketches it.
///
/// T10 uses `MockCoreClient`; T11 adds a real client that calls the UniFFI commands.
/// It is main-actor bound so views and `AppModel` can call it directly; a real client
/// moves heavy work off the main thread inside its own implementation.
@MainActor
protocol CoreClient: AnyObject {
    /// Starts a scan of the home folder and returns its handle straight away.
    func startScan(mode: Mode) async throws -> ScanHandle

    /// Reports how far the scan has got. Phase `.done` means `overview` is ready.
    func scanProgress(_ handle: ScanHandle) async throws -> ScanProgress

    /// Stops a scan. Safe to call more than once, or after the scan has finished.
    func cancelScan(_ handle: ScanHandle) async

    /// The Overview for a finished scan.
    func overview(_ handle: ScanHandle) async throws -> Overview
}

/// Errors a core client can report. Messages are plain words, ready to show.
enum CoreClientError: Error, Equatable, LocalizedError {
    case scanFailed(String)
    case cancelled
    case unknownScan
    case scanNotFinished

    var errorDescription: String? {
        switch self {
        case .scanFailed(let reason): return reason
        case .cancelled: return "The scan was cancelled."
        case .unknownScan: return "Roomy lost track of this scan. Please scan again."
        case .scanNotFinished: return "The scan hasn't finished yet."
        }
    }
}
