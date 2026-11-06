//! The folders the user asked Roomy to keep. Checking them only reads
//! metadata (to resolve symlinks); the list itself lives in the app's
//! settings and is sent to the core at launch and on every change.

use std::path::{Component, Path, PathBuf};

use crate::CoreError;

/// Shown when a folder is not inside the home folder.
const NOT_IN_HOME: &str = "Choose a folder inside your home folder.";
/// Shown when a folder is written in an unusual way.
const UNUSUAL: &str = "That folder's location is written in an unusual way.";

/// Checks one folder from the app and returns the path the core will use:
/// its real location if it exists (symlinks resolved), otherwise as given.
pub(crate) fn checked(home: &Path, folder: &str) -> Result<PathBuf, CoreError> {
    crate::validate_path(folder)?;
    let path = Path::new(folder);
    let plain = path
        .components()
        .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    if !plain {
        return Err(invalid(UNUSUAL));
    }
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let home_real = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let inside = |p: &Path| {
        p != home && p != home_real && (p.starts_with(home) || p.starts_with(&home_real))
    };
    if !inside(&real) {
        return Err(invalid(NOT_IN_HOME));
    }
    // Keep the spelling under the core's own home path, which is what scan
    // paths start with.
    match real.strip_prefix(&home_real) {
        Ok(rest) if home_real != home => Ok(home.join(rest)),
        _ => Ok(real),
    }
}

/// The folders as text for the app.
pub(crate) fn shown(folders: &[PathBuf]) -> Vec<String> {
    folders
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

fn invalid(reason: &str) -> CoreError {
    CoreError::InvalidPath {
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_must_be_plain_paths_inside_home() {
        let dir = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir(home.join("keep")).unwrap();
        let s = |p: &Path| p.to_str().unwrap().to_string();
        assert_eq!(
            checked(&home, &s(&home.join("keep"))).unwrap(),
            home.join("keep")
        );
        assert_eq!(
            checked(&home, &s(&home.join("not-yet"))).unwrap(),
            home.join("not-yet")
        );
        assert!(checked(&home, &s(&home)).is_err());
        assert!(checked(&home, "/etc").is_err());
        assert!(checked(&home, "relative/path").is_err());
        assert!(checked(&home, &format!("{}/keep/../..", s(&home))).is_err());
    }

    #[test]
    fn a_kept_link_keeps_what_it_points_to() {
        let dir = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(home.join("Developer/app")).unwrap();
        std::os::unix::fs::symlink(home.join("Developer/app"), home.join("app")).unwrap();
        let link = home.join("app");
        assert_eq!(
            checked(&home, link.to_str().unwrap()).unwrap(),
            home.join("Developer/app")
        );
    }
}
