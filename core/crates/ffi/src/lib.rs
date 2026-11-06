//! UniFFI surface between the Rust core and the SwiftUI app.
//!
//! Every function here is a typed command the app can call. Nothing in this crate modifies the
//! filesystem. `disk_summary` reads volume metadata only (via `statfs`).

use std::ffi::CString;
use std::mem::MaybeUninit;
use std::path::Path;

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
