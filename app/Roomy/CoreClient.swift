import Foundation

/// Everything the app asks of the Rust core, shaped like the `RoomyCore` commands in
/// `docs/command-contract.md` (T09) and using the same UniFFI-generated types.
///
/// `RustCoreClient` wraps the real core; `MockCoreClient` stands in for previews and tests. The
/// protocol is main-actor bound so `AppModel` can call it directly; `RustCoreClient` moves every core
/// call off the main thread.
@MainActor
protocol CoreClient: AnyObject {
    /// Starts a read-only scan of the home folder and returns its handle straight away. The real core reads
    /// again only the folders that changed since the last scan when it can be sure which those are
    /// (`Overview.quickRescan`), and otherwise scans everything.
    func startScan(mode: Mode) async -> ScanHandle

    /// How far the scan has got. Throws `CoreError.UnknownScan` for a handle the core doesn't know.
    func scanProgress(handle: ScanHandle) async throws -> ScanProgress

    /// Asks the scan to stop. Safe to call more than once, or after it has finished.
    func cancelScan(handle: ScanHandle) async

    /// Cancels the scan if it is running and frees its results. The handle is unknown afterwards.
    func closeScan(handle: ScanHandle) async

    /// The Overview for a finished scan. Throws `CoreError.ScanNotFinished` while it runs.
    func overview(handle: ScanHandle) async throws -> Overview

    /// What grew or shrank since the previous scan in the same mode, or `nil` if there was none (or the core
    /// can't say). Holds no paths.
    func scanChanges(handle: ScanHandle) async -> ScanChanges?

    /// Every item of a finished scan, optionally one category and/or one tier (`nil` means any).
    func listItems(handle: ScanHandle, category: Category?, tier: Tier?, sort: Sort) async throws -> [ItemSummary]

    /// How much Photos, Music, TV, Mail, Messages attachments and Final Cut Pro take in a finished scan.
    /// Shown only: Roomy never cleans inside these libraries. One row per kind, in the same order every time.
    func libraries(handle: ScanHandle) async throws -> [LibrarySize]

    /// Everything the detail panel shows for one item. Throws `CoreError.UnknownItem` for an unknown id.
    func itemDetail(handle: ScanHandle, itemId: String) async throws -> ItemDetail

    /// The JavaScript screen: each project's `node_modules` with when the project was last worked on, and
    /// the package managers' caches, from a finished scan. Reads file dates only. Rows carry item ids, so
    /// they are cleaned with `previewCleanup` like any item.
    func javascriptProjects(handle: ScanHandle) async throws -> JavaScriptOverview

    /// The dry run: checks the ticked items the way a cleanup would and says what would move to the
    /// Trash. Moves nothing. Sends item ids only, never paths.
    func previewCleanup(handle: ScanHandle, choices: [CleanupChoice]) async throws -> CleanupPreview

    /// Drops the plan a preview made, when the user closes the review without cleaning up.
    func discardCleanup(plan: PlanHandle) async

    /// Moves the plan's items to the Trash. The core checks every item again first. The plan is used up.
    func runCleanup(plan: PlanHandle) async throws -> CleanupResult

    /// Puts the most recent cleanup back where it was. Works once, until the app quits.
    func putBackCleanup(cleanup: CleanupHandle) async throws -> PutBackResult

    /// Past cleanups, newest first. Holds no paths.
    func cleanupHistory(limit: UInt32) async -> [HistoryEntry]

    /// Whether Roomy can read the folders macOS keeps private, right now. Reads a few folder listings only.
    func fullDiskAccess() async -> FullDiskAccess

    /// Tells the core which folders the user asked Roomy to keep. Nothing inside one, and nothing that
    /// holds one, is ever cleaned. Returns the list as the core will use it (symlinks resolved).
    /// Throws `CoreError.InvalidPath` for a folder outside the home folder; the earlier list stays.
    func setProtectedFolders(_ folders: [String]) async throws -> [String]

    /// What Docker uses, from the read-only `docker system df`. Never throws: a missing or stopped Docker is
    /// a `DockerStatus`. Can take several seconds.
    func dockerOverview() async -> DockerOverview

    /// The dry run of one Docker clean-up: what Docker would remove. Removes nothing. Sends a kind, never a
    /// command. Throws `CoreError.DockerUnavailable` if Docker is missing or stopped.
    func previewDockerCleanup(kind: DockerCleanupKind) async throws -> DockerCleanupPreview

    /// Drops the plan a Docker preview made, when the user closes the review without removing anything.
    func discardDockerCleanup(plan: DockerPlanHandle) async

    /// Runs the reviewed Docker clean-up. The core checks again first and refuses, removing nothing, if
    /// anything new appeared. The plan is used up.
    func runDockerCleanup(plan: DockerPlanHandle) async throws -> DockerCleanupResult

    /// What macOS's System Data holds, measured just now: snapshots, caches, logs, updates, temporary
    /// files, swap and the sleep image. Reads only; never throws. Paths come back in Developer mode only.
    /// Can take several seconds.
    func systemData(mode: Mode) async -> SystemData
    /// The dry run of one tool clean-up (`brew cleanup`, deleting unavailable simulators, or one Android SDK
    /// package): what the tool would remove. Removes nothing. Sends a kind, never a command. Throws
    /// `CoreError.ToolUnavailable` if the tool is missing. `sdkmanager` can take a minute.
    func previewToolCleanup(kind: ToolCleanupKind) async throws -> ToolCleanupPreview

    /// Drops the plan a tool preview made, when the user closes the review without removing anything.
    func discardToolCleanup(plan: ToolPlanHandle) async

    /// Runs the reviewed tool clean-up. `package` is the id of one package from the review for the Android
    /// SDK, and `nil` otherwise. The core checks again first and refuses, removing nothing, if anything
    /// changed. The plan is used up.
    func runToolCleanup(plan: ToolPlanHandle, package: String?) async throws -> ToolCleanupResult

    /// The app uninstaller: every app in /Applications and ~/Applications, except Apple's and Roomy, with
    /// its size. Reads only. Each call gives new app ids.
    func listApps() async -> [AppSummary]

    /// What one app left in the home folder, measured just now, plus the app itself when it is in
    /// ~/Applications. Reads only. Throws `CoreError.UnknownApp` for an id from an older list.
    func reviewApp(appId: String, mode: Mode) async throws -> AppReview

    /// The uninstaller's dry run: checks the ticked parts of the last review the way a cleanup would and
    /// says what would move to the Trash. Moves nothing. Its plan runs with `runCleanup`, like any cleanup.
    func previewUninstall(appId: String, partIds: [String]) async throws -> CleanupPreview

    /// The Android screen: SDK parts, emulators and Gradle downloads, each with its size and whether any
    /// Android project (or emulator) uses it. Reads only; can take some seconds. Each call gives new ids.
    func androidOverview(mode: Mode) async -> AndroidOverview

    /// The Android screen's dry run: checks the ticked rows the way a cleanup would and says what would
    /// move to the Trash. Moves nothing. Its plan runs with `runCleanup`, like any cleanup.
    func previewAndroidCleanup(itemIds: [String]) async throws -> CleanupPreview
}
