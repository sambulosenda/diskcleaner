//! Shared helpers: a fake home folder in a temp directory.
//!
//! Tests never touch the real home folder. Every home here is a fresh temp
//! directory, canonicalised so that macOS's `/var -> /private/var` symlink
//! is not inside the path (that would make every test fail step 3).

#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use safety::{validate, Refusal, ValidatedPath};

/// A fake home folder that is deleted when dropped.
pub struct FakeHome {
    _dir: tempfile::TempDir,
    pub home: PathBuf,
    pub dev: u64,
}

impl FakeHome {
    /// Makes an empty fake home.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("make temp dir");
        let home = fs::canonicalize(dir.path())
            .expect("canonicalise temp dir")
            .join("home");
        fs::create_dir(&home).expect("make home");
        let dev = fs::symlink_metadata(&home).expect("stat home").dev();
        Self {
            _dir: dir,
            home,
            dev,
        }
    }

    /// The absolute path for a path relative to home, written with `/`.
    pub fn path(&self, rel: &str) -> PathBuf {
        if rel.is_empty() {
            self.home.clone()
        } else {
            self.home.join(rel)
        }
    }

    /// Creates `rel` as a file, making its parent folders.
    pub fn file(&self, rel: &str) -> PathBuf {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("make parents");
        }
        fs::write(&path, b"data").expect("write file");
        path
    }

    /// Creates `rel` as a folder, making its parent folders.
    pub fn dir(&self, rel: &str) -> PathBuf {
        let path = self.path(rel);
        fs::create_dir_all(&path).expect("make folder");
        path
    }

    /// Makes `rel` a Git repository root (a folder holding a `.git` folder).
    pub fn repo(&self, rel: &str) -> PathBuf {
        self.dir(&format!("{rel}/.git"));
        self.path(rel)
    }

    /// Validates `path` on the home's own volume.
    pub fn check(
        &self,
        path: &Path,
        selected_individually: bool,
    ) -> Result<ValidatedPath, Refusal> {
        validate(&self.home, self.dev, path, selected_individually)
    }

    /// Validates `path` with and without individual selection and returns
    /// both results.
    pub fn check_both(&self, path: &Path) -> [Result<ValidatedPath, Refusal>; 2] {
        [self.check(path, false), self.check(path, true)]
    }
}
