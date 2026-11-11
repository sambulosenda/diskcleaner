//! Parallel, read-only walk of one folder tree. Requirement F1.
//!
//! [`scan`] walks a root folder and reports how much disk space each folder
//! takes, plus the largest files (for E2). It follows these rules:
//!
//! - **Read only.** The crate only ever reads folder listings and metadata.
//!   It never opens a file for writing, renames, deletes or runs a process.
//! - **Allocated size.** Sizes are `st_blocks * 512`, the space really used on
//!   disk, which is what `du` reports. Sparse files, compressed files and
//!   iCloud placeholders therefore count for what they take, not their length.
//! - **No symlinks followed.** Every entry is read with `lstat`. A symlink is
//!   counted as the small entry it is and never entered.
//! - **One volume.** A folder whose `st_dev` differs from the root's is a
//!   mount point; it is listed in [`ScanResult::other_volumes`] and skipped.
//! - **Hard links once.** A file with more than one link is counted once per
//!   `(st_dev, st_ino)`, under the name whose full path sorts first, so the
//!   same tree always gives the same folder sizes. Its other names are
//!   skipped.
//! - **Unreadable folders are not errors.** A folder that cannot be listed is
//!   recorded in [`ScanResult::unreadable`] with its path and the walk goes on.
//!
//! The result keeps one record per folder, not per file, so memory grows with
//! the number of folders. Only the largest files are kept by path.

pub mod fsevents;
mod largest;
pub mod privacy;
mod rescan;
mod result;
mod walk;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

pub use rescan::{rescan, Changes, RescanError};
pub use result::{Dir, DirId, FileEntry, LargeFile, ScanResult, Unreadable, UnreadableReason};

/// How many of the largest files E2 asks for.
pub const DEFAULT_LARGEST_FILES: usize = 500;

/// Settings for one scan.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Set to `true` from any thread to stop the scan early. The scan then
    /// returns [`ScanError::Cancelled`].
    pub cancel: Arc<AtomicBool>,
    /// How many of the largest files to keep. Default 500.
    pub largest_files: usize,
    /// Worker threads. `None` uses one per CPU core.
    pub threads: Option<usize>,
    /// How often the progress callback is called while the scan runs.
    pub progress_interval: Duration,
    /// Folders whose files should be listed by name in [`Dir::files`].
    /// `None` (the default) lists none.
    pub record_files: Option<RecordFiles>,
    /// Folders whose files may go in [`ScanResult::largest_files`], asked
    /// the same way as `record_files` (path relative to the root). `None`
    /// (the default) lets every folder's files in. Files in other folders
    /// are still counted, just never kept by path.
    pub largest_in: Option<RecordFiles>,
    /// Folders to report, with their size, as soon as the walk has read
    /// everything inside them. `None` (the default) reports none.
    pub watch: Option<Watch>,
}

/// Hears about a reported folder: its path relative to the scan root and the
/// bytes counted inside it.
pub type WatchFound = dyn Fn(&Path, u64) + Send + Sync;

/// What a [`Watch`] says about one folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watched {
    /// Not of interest itself. Ask about the folders inside it.
    Descend,
    /// Not of interest, and neither is anything inside it.
    Skip,
    /// Report this folder once everything inside it has been read. The
    /// folders inside it are not asked about.
    Report,
}

/// Lets the caller hear about chosen folders while the walk is still
/// running, instead of waiting for the whole result.
///
/// `ask` is called once per folder, from a worker thread, with the folder's
/// path relative to the scan root. The root itself is never asked about. A
/// folder answered with [`Watched::Report`] is passed to `found` with the
/// bytes counted inside it, once the walk has read all of it.
///
/// The size given to `found` is a lower bound: files with more than one
/// hard link are only counted when the whole walk ends (see the crate
/// docs), so they are missing from it. A folder that could not be listed,
/// or a walk that was cancelled, is never reported. A rescan never reports.
#[derive(Clone)]
pub struct Watch {
    /// Says whether a folder should be reported.
    pub ask: Arc<dyn Fn(&Path) -> Watched + Send + Sync>,
    /// Hears about a reported folder and the bytes counted inside it.
    pub found: Arc<WatchFound>,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Watch(..)")
    }
}

/// Says which folders should have their files listed by name.
///
/// The function is called once per folder with its path relative to the
/// scan root (empty for the root itself) and returns `true` to list the
/// plain files directly in it. The classifier uses this to see the few
/// files its rules name, without the scan keeping every file name in
/// memory.
#[derive(Clone)]
pub struct RecordFiles(pub Arc<dyn Fn(&Path) -> bool + Send + Sync>);

impl std::fmt::Debug for RecordFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecordFiles(..)")
    }
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            largest_files: DEFAULT_LARGEST_FILES,
            threads: None,
            progress_interval: Duration::from_millis(100),
            record_files: None,
            largest_in: None,
            watch: None,
        }
    }
}

/// A snapshot of how far a scan has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanProgress {
    /// Files counted so far (hard links counted once).
    pub files: u64,
    /// Folders listed so far.
    pub dirs: u64,
    /// Allocated bytes counted so far.
    pub bytes: u64,
    /// `true` only for the last call, made once the walk has finished.
    pub finished: bool,
}

/// Why a scan could not run or did not finish.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The root must be an absolute path.
    #[error("the scan root is not an absolute path: {0}")]
    RootNotAbsolute(PathBuf),
    /// The root itself is a symlink. Symlinks are never followed.
    #[error("the scan root is a symlink: {0}")]
    RootIsSymlink(PathBuf),
    /// The root is not a folder.
    #[error("the scan root is not a folder: {0}")]
    RootNotAFolder(PathBuf),
    /// The root's metadata could not be read.
    #[error("could not read the scan root {path}: {source}")]
    Root {
        /// The root.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The worker threads could not be started.
    #[error("could not start the scan threads: {0}")]
    Threads(String),
    /// The cancel flag was set before the walk finished.
    #[error("the scan was cancelled")]
    Cancelled,
    /// The walk stopped without a result. A bug in this crate.
    #[error("the scan stopped unexpectedly")]
    Internal,
}

/// Scans `root` and returns per-folder sizes and the largest files.
///
/// `progress` is called on the calling thread about every
/// `opts.progress_interval` while the walk runs, then once more with
/// `finished: true`. The walk itself runs on a separate thread pool.
///
/// `root` must be an absolute path to a folder and must not itself be a
/// symlink. On macOS a temp folder sits behind `/var -> /private/var`, so
/// canonicalise it first.
pub fn scan(
    root: &Path,
    opts: ScanOptions,
    progress: impl Fn(ScanProgress),
) -> Result<ScanResult, ScanError> {
    let root_meta = check_root(root)?;
    walk::run(root, &root_meta, &opts, &progress)
}

/// Checks the root is an absolute path to a real folder, without following it.
fn check_root(root: &Path) -> Result<std::fs::Metadata, ScanError> {
    if !root.is_absolute() {
        return Err(ScanError::RootNotAbsolute(root.to_path_buf()));
    }
    let meta = std::fs::symlink_metadata(root).map_err(|source| ScanError::Root {
        path: root.to_path_buf(),
        source,
    })?;
    if meta.file_type().is_symlink() {
        return Err(ScanError::RootIsSymlink(root.to_path_buf()));
    }
    if !meta.is_dir() {
        return Err(ScanError::RootNotAFolder(root.to_path_buf()));
    }
    Ok(meta)
}
