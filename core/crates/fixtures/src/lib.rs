//! Reproducible fake home folders for tests.
//!
//! [`build`] fills an empty folder with a small, deterministic copy of the
//! things a real Mac home holds: everyday files, Xcode, Android and
//! JavaScript developer data, and a set of traps (symlinks, hard links, a git
//! repository, secrets, signing keys) that the engine must never trip over.
//! It writes a `fixtures-manifest.json` at the root that lists every path it
//! made, with its kind, size and SHA-256 checksum.
//!
//! [`home`] builds one in a fresh temp folder for a test.
//!
//! This crate is test infrastructure. It only ever creates files, and only
//! inside the folder it is given: it refuses a folder that is not empty, never
//! overwrites, never removes, and never writes through a symlink. It is not
//! linked into the app.

mod builder;
mod content;
mod layout;
mod manifest;
pub mod paths;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub use manifest::{Entry, EntryKind, Manifest, MANIFEST_FILE};

/// Why a fixture home could not be built.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    /// The target folder already has something in it.
    #[error("the target folder is not empty: {0}")]
    NotEmpty(PathBuf),
    /// The target exists but is not a plain folder (for example a symlink).
    #[error("the target is not a plain folder: {0}")]
    NotAFolder(PathBuf),
    /// A layout path was absolute or tried to leave the root. A bug in this crate.
    #[error("layout path escapes the fixture root: {0}")]
    BadLayoutPath(String),
    /// A layout path would be written through a symlink. A bug in this crate.
    #[error("layout path goes through a symlink: {0}")]
    ThroughSymlink(PathBuf),
    /// Reading or writing failed.
    #[error("could not write {path}: {source}")]
    Io {
        /// The path being worked on.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Builds a fixture home in `root`, which must be empty or not exist yet.
///
/// `now` is the reference time for files that must look old (for example the
/// stale JavaScript project, whose `package.json` is set 120 days before
/// `now`). File contents never depend on `now`, so checksums do not either.
///
/// A few files hold the home path itself, because the real tools write
/// absolute paths (Xcode's `WorkspacePath`, the Android AVD `.ini` files).
/// Those entries are marked `contains_home_path` in the manifest. Building
/// twice into the same path gives byte-identical trees; building into two
/// different paths differs only in those marked files.
///
/// Returns the manifest, which is also written to `root/fixtures-manifest.json`.
pub fn build(root: &Path, now: SystemTime) -> Result<Manifest, FixtureError> {
    let root = builder::prepare_root(root)?;
    let mut b = builder::Builder::new(root, now);
    layout::everyday::build(&mut b)?;
    layout::xcode::build(&mut b)?;
    layout::android::build(&mut b)?;
    layout::js::build(&mut b)?;
    layout::devtools::build(&mut b)?;
    layout::traps::build(&mut b)?;
    b.finish()
}

/// Builds a fixture home in a fresh temp folder, for a test.
///
/// The fixture root is the temp folder itself. On macOS the temp folder sits
/// behind the `/var -> /private/var` symlink, so call
/// [`canonical_root`] before handing the path to code that refuses symlinks.
///
/// # Panics
///
/// Panics if the fixture cannot be built. Use [`try_home`] to get the error.
pub fn home() -> tempfile::TempDir {
    match try_home() {
        Ok(dir) => dir,
        Err(err) => panic!("could not build fixture home: {err}"),
    }
}

/// Like [`home`], but returns the error instead of panicking.
pub fn try_home() -> Result<tempfile::TempDir, FixtureError> {
    let dir = tempfile::tempdir().map_err(|source| FixtureError::Io {
        path: std::env::temp_dir(),
        source,
    })?;
    build(dir.path(), SystemTime::now())?;
    Ok(dir)
}

/// The canonical path of a fixture home (no symlinks in it), as used in the
/// files that hold the home path.
pub fn canonical_root(dir: &tempfile::TempDir) -> Result<PathBuf, FixtureError> {
    std::fs::canonicalize(dir.path()).map_err(|source| FixtureError::Io {
        path: dir.path().to_path_buf(),
        source,
    })
}
