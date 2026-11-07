//! UniFFI surface between the Rust core and the SwiftUI app.
//!
//! Every function here is a typed command the app can call; the contract is
//! `docs/command-contract.md`. Nothing in this crate itself modifies the
//! filesystem (History writes only its own database, in its own crate):
//! scans are read only, `disk_summary` reads volume metadata only (via
//! `statfs`), the low-space check is pure arithmetic on a number the app
//! passes in, and the cleanup preview runs the executor's read-only checks.
//! The cleanup and put-back commands hand a plan to the `executor` crate,
//! which is the only code that moves files. The Docker commands do the same
//! with the executor's allow-listed Docker clean-ups. The System Data
//! breakdown only reads sizes, including of a few folders outside the home
//! folder, which nothing can clean.
//!
//! No path goes to the app in simple mode (except an installed app's own
//! bundle location, for "Show in Finder", which is not personal), and the
//! app never sends a path to
//! the core except in the T02 `disk_summary` command, the user's own
//! protected folders (`set_protected_folders`, which can only stop things
//! being cleaned) and the test-only commands behind the `testing` feature.

mod android;
mod backup;
mod changes;
mod cleanup;
mod docker;
mod embedded_rules;
mod engine;
mod files;
mod folders;
mod javascript;
mod low_space;
mod run;
mod system_data;
#[cfg(feature = "testing")]
mod testing;
mod tools;
mod types;
mod uninstall;
mod views;

use std::ffi::CString;
use std::mem::MaybeUninit;
use std::path::Path;

pub use docker::{
    DockerCleanupKind, DockerCleanupPreview, DockerCleanupResult, DockerOverview, DockerPlanHandle,
    DockerPreviewItem, DockerStatus, DockerUsageRow,
};
pub use engine::RoomyCore;
pub use javascript::{ActivitySource, JavaScriptOverview, JsProject, PackageManager};
pub use low_space::{
    check_low_space, low_space_default_threshold, low_space_threshold_choices, LowSpaceCheck,
};
pub use system_data::{
    LocalSnapshots, SnapshotStatus, SystemData, SystemDataKind, SystemDataOwner, SystemDataPart,
    SystemDataPlace, SystemDataPlaceState,
};
#[cfg(feature = "testing")]
pub use testing::make_fixture_home;
pub use tools::{
    ToolCleanupKind, ToolCleanupPreview, ToolCleanupResult, ToolPlanHandle, ToolPreviewItem,
};
pub use types::{
    Action, AndroidKind, AndroidOverview, AndroidPart, AndroidProject, AndroidUsage, AppLocation,
    AppPart, AppReview, AppSummary, Category, CategoryChange, CategoryTotal, Caution, ChangeKind,
    CleanupCheck, CleanupChoice, CleanupHandle, CleanupPreview, CleanupPreviewItem, CleanupResult,
    CleanupResultItem, Confidence, DeviceBackup, FileRow, FullDiskAccess, HistoryEntry,
    HistoryItem, HistoryItemState, ItemChange, ItemDetail, ItemSummary, LargeFiles, LibraryKind,
    LibrarySize, Mode, Overview, PlanHandle, PrivateArea, PutBackResult, ScanChanges, ScanHandle,
    ScanPhase, ScanProgress, Sort, Tier, UnreadableArea,
};

uniffi::setup_scaffolding!("roomy_core");

/// Errors returned to the app. In Swift these arrive as thrown errors, never as crashes.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// The path given by the app was not usable (empty, relative or containing a NUL byte).
    #[error("invalid path: {reason}")]
    InvalidPath { reason: String },
    /// The operating system refused the request, for example because the path does not exist.
    #[error("could not read disk information: {reason}")]
    Io { reason: String },
    /// The core could not find the home folder.
    #[error("Roomy could not find your home folder.")]
    HomeNotFound,
    /// A built-in rule failed validation. A bug caught by tests.
    #[error("the built-in rules are not valid: {reason}")]
    RulesInvalid { reason: String },
    /// The scan handle was never issued or has been closed.
    #[error("Roomy lost track of this scan. Please scan again.")]
    UnknownScan,
    /// The scan is still running.
    #[error("The scan hasn't finished yet.")]
    ScanNotFinished,
    /// The scan was cancelled.
    #[error("The scan was cancelled.")]
    ScanCancelled,
    /// The scan could not run. `reason` is plain words with no path.
    #[error("{reason}")]
    ScanFailed { reason: String },
    /// The scan has no item with that id.
    #[error("Roomy could not find that item. Please scan again.")]
    UnknownItem,
    /// A cleanup preview was asked for with no items.
    #[error("Nothing was chosen.")]
    NothingSelected,
    /// The cleanup preview could not check anything, for example because
    /// the Trash could not be opened. `reason` is plain words with no path.
    #[error("{reason}")]
    CleanupUnavailable { reason: String },
    /// Put back was asked for a cleanup the core no longer holds: not the
    /// most recent one, already put back, or from before Roomy started.
    /// Trying again will not help, unlike `CleanupUnavailable`.
    #[error("Roomy can only put back its most recent cleanup, until Roomy quits. You can still drag items back from the Trash.")]
    NothingToPutBack,
    /// A Docker clean-up could not be previewed or was refused before it
    /// ran: Docker is missing or stopped, the review expired, or Docker's
    /// data changed. Nothing was removed. `reason` is plain words, no path.
    #[error("{reason}")]
    DockerUnavailable { reason: String },
    /// The app uninstaller was given an app id that is not from the latest
    /// `list_apps` (or, for `preview_uninstall`, not the app last reviewed).
    #[error("Roomy lost track of this app. Please look again.")]
    UnknownApp,
    /// A tool clean-up (Homebrew, simulators, Android SDK) could not be
    /// previewed or was refused before it ran: the tool is missing, the
    /// review expired, the package was not in the review, or something
    /// changed. Nothing was removed. `reason` is plain words, no path.
    #[error("{reason}")]
    ToolUnavailable { reason: String },
}

/// Space on the volume that holds a given path, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct DiskSummary {
    /// Total size of the volume.
    pub total_bytes: u64,
    /// Space available to the current user (not counting space reserved for the system).
    pub free_bytes: u64,
}

/// The version of the Rust core, taken from Cargo.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Total and free space on the volume holding `path`. Read only.
///
/// `path` must be absolute. The home folder is passed in by the app rather than read from
/// `$HOME`, so tests can point this at any directory.
#[uniffi::export]
pub fn disk_summary(path: String) -> Result<DiskSummary, CoreError> {
    let c_path = validate_path(&path)?;
    let stats = statfs(&c_path)?;
    Ok(summary_from_stats(&stats))
}

/// Total and free space on the volume holding `path`, for core-internal callers.
pub(crate) fn read_disk_summary(path: &Path) -> Result<DiskSummary, CoreError> {
    let text = path.to_str().ok_or_else(|| CoreError::InvalidPath {
        reason: "the path is not valid UTF-8".to_string(),
    })?;
    let c_path = validate_path(text)?;
    let stats = statfs(&c_path)?;
    Ok(summary_from_stats(&stats))
}

/// Checks that `path` is absolute and turns it into a C string for `statfs`.
fn validate_path(path: &str) -> Result<CString, CoreError> {
    if path.is_empty() {
        return Err(CoreError::InvalidPath {
            reason: "the path is empty".to_string(),
        });
    }
    if !Path::new(path).is_absolute() {
        return Err(CoreError::InvalidPath {
            reason: "the path must be absolute".to_string(),
        });
    }
    CString::new(path).map_err(|_| CoreError::InvalidPath {
        reason: "the path contains a NUL byte".to_string(),
    })
}

/// Calls `statfs(2)` on `path`. Reads metadata only.
fn statfs(path: &CString) -> Result<libc::statfs, CoreError> {
    let mut stats = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `path` is a valid NUL-terminated string and `stats` points to writable memory
    // large enough for a `statfs` struct. The struct is only read after a zero return,
    // which means the kernel filled it in.
    let result = unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(CoreError::Io {
            reason: std::io::Error::last_os_error().to_string(),
        });
    }
    // SAFETY: statfs returned 0, so the struct is initialised.
    Ok(unsafe { stats.assume_init() })
}

/// Turns raw `statfs` block counts into byte totals.
///
/// Field types differ between macOS and Linux, hence the casts. Multiplication saturates
/// rather than overflowing.
#[allow(clippy::unnecessary_cast)]
fn summary_from_stats(stats: &libc::statfs) -> DiskSummary {
    let block_size = stats.f_bsize as u64;
    DiskSummary {
        total_bytes: (stats.f_blocks as u64).saturating_mul(block_size),
        free_bytes: (stats.f_bavail as u64).saturating_mul(block_size),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_version_is_not_empty() {
        assert!(!core_version().is_empty());
    }

    #[test]
    fn disk_summary_reads_a_temp_directory() {
        let dir = std::env::temp_dir();
        let summary = disk_summary(dir.to_string_lossy().into_owned()).unwrap();
        assert!(summary.total_bytes > 0);
        assert!(summary.free_bytes <= summary.total_bytes);
    }

    #[test]
    fn disk_summary_rejects_an_empty_path() {
        assert!(matches!(
            disk_summary(String::new()),
            Err(CoreError::InvalidPath { .. })
        ));
    }

    #[test]
    fn disk_summary_rejects_a_relative_path() {
        assert!(matches!(
            disk_summary("relative/path".to_string()),
            Err(CoreError::InvalidPath { .. })
        ));
    }

    #[test]
    fn disk_summary_rejects_a_nul_byte() {
        assert!(matches!(
            disk_summary("/tmp/\0bad".to_string()),
            Err(CoreError::InvalidPath { .. })
        ));
    }

    #[test]
    fn disk_summary_reports_a_missing_path_as_an_error() {
        let missing = std::env::temp_dir().join("roomy-t02-this-path-does-not-exist");
        assert!(matches!(
            disk_summary(missing.to_string_lossy().into_owned()),
            Err(CoreError::Io { .. })
        ));
    }
}
