//! Test-only commands, compiled in only with the `testing` feature. The
//! release app is built without it (see `docs/command-contract.md`), so a
//! shipped app can only ever scan the real home folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{validate_path, CoreError, RoomyCore};

#[uniffi::export]
impl RoomyCore {
    /// Like `new()`, but scans `home` instead of the real home folder.
    /// `home` must be an absolute path.
    #[uniffi::constructor]
    pub fn with_home_for_testing(home: String) -> Result<Arc<Self>, CoreError> {
        validate_path(&home)?;
        Self::build(PathBuf::from(home))
    }
}

/// Builds the T04 fixture home in `dir` (empty or not there yet) and returns
/// its canonical path, ready for `RoomyCore.with_home_for_testing`.
#[uniffi::export]
pub fn make_fixture_home(dir: String) -> Result<String, CoreError> {
    validate_path(&dir)?;
    let dir = Path::new(&dir);
    fixtures::build(dir, std::time::SystemTime::now()).map_err(|e| CoreError::Io {
        reason: e.to_string(),
    })?;
    let root = std::fs::canonicalize(dir).map_err(|e| CoreError::Io {
        reason: e.to_string(),
    })?;
    root.into_os_string()
        .into_string()
        .map_err(|_| CoreError::InvalidPath {
            reason: "the path is not valid UTF-8".to_string(),
        })
}
