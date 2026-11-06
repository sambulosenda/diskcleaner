import Foundation

/// Everything the app asks of the Rust core, shaped like the `RoomyCore` commands in
/// `docs/command-contract.md` (T09) and using the same UniFFI-generated types.
///
/// T10 uses `MockCoreClient`; T11 adds a client that wraps `RoomyCore`. The protocol is main-actor
/// bound so `AppModel` can call it directly; a real client moves the core calls off the main thread.
@MainActor
protocol CoreClient: AnyObject {
    /// Starts a read-only scan of the home folder and returns its handle straight away.
    func startScan(mode: Mode) async -> ScanHandle

    /// How far the scan has got. Throws `CoreError.UnknownScan` for a handle the core doesn't know.
    func scanProgress(handle: ScanHandle) async throws -> ScanProgress

    /// Asks the scan to stop. Safe to call more than once, or after it has finished.
    func cancelScan(handle: ScanHandle) async

    /// Cancels the scan if it is running and frees its results. The handle is unknown afterwards.
    func closeScan(handle: ScanHandle) async

    /// The Overview for a finished scan. Throws `CoreError.ScanNotFinished` while it runs.
    func overview(handle: ScanHandle) async throws -> Overview
}
