//! Writes fixture files safely inside one root folder.
//!
//! Every path is relative to the root, made only of plain names (no `..`,
//! no leading `/`), and created without following symlinks. Files are opened
//! with `create_new`, so nothing that already exists is ever overwritten.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::content;
use crate::manifest::{self, Manifest};
use crate::FixtureError;

/// The placeholder that [`Builder::text_with_home`] replaces with the root path.
pub const HOME_TOKEN: &str = "{HOME}";

/// What the builder knows about a path beyond what is on disk.
#[derive(Debug, Default, Clone)]
pub struct Note {
    /// The file's bytes include the absolute root path.
    pub contains_home_path: bool,
    /// The file is a hard link to this other fixture path.
    pub hard_link_of: Option<String>,
}

/// Creates fixture paths under one root and remembers notes for the manifest.
pub struct Builder {
    root: PathBuf,
    now: SystemTime,
    notes: BTreeMap<String, Note>,
}

/// Makes sure `root` is an empty plain folder and returns its canonical path.
pub fn prepare_root(root: &Path) -> Result<PathBuf, FixtureError> {
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => {
            let mut entries = fs::read_dir(root).map_err(|e| io_err(root, e))?;
            if entries.next().is_some() {
                return Err(FixtureError::NotEmpty(root.to_path_buf()));
            }
        }
        Ok(_) => return Err(FixtureError::NotAFolder(root.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(root).map_err(|e| io_err(root, e))?;
        }
        Err(e) => return Err(io_err(root, e)),
    }
    fs::canonicalize(root).map_err(|e| io_err(root, e))
}

impl Builder {
    /// A builder for an already prepared, canonical, empty root.
    pub fn new(root: PathBuf, now: SystemTime) -> Self {
        Self {
            root,
            now,
            notes: BTreeMap::new(),
        }
    }

    /// Creates the folder `rel` and any missing parents.
    pub fn dir(&mut self, rel: &str) -> Result<(), FixtureError> {
        let path = self.resolve(rel)?;
        self.make_dirs(&path)
    }

    /// Creates `rel` holding `size` deterministic bytes seeded by its path.
    pub fn bytes(&mut self, rel: &str, size: usize) -> Result<(), FixtureError> {
        let data = content::seeded_bytes(rel, size);
        self.write_new(rel, &data)
    }

    /// Creates `rel` holding exactly `text`.
    pub fn text(&mut self, rel: &str, text: &str) -> Result<(), FixtureError> {
        self.write_new(rel, text.as_bytes())
    }

    /// Creates `rel` holding `template` with every `{HOME}` replaced by the
    /// root path, and marks it in the manifest.
    pub fn text_with_home(&mut self, rel: &str, template: &str) -> Result<(), FixtureError> {
        let home = self.root.to_string_lossy().into_owned();
        let text = template.replace(HOME_TOKEN, &home);
        self.write_new(rel, text.as_bytes())?;
        self.note(rel).contains_home_path = true;
        Ok(())
    }

    /// Creates a symlink at `rel` pointing at `target` (stored as given).
    pub fn symlink(&mut self, rel: &str, target: &str) -> Result<(), FixtureError> {
        let path = self.resolve(rel)?;
        self.make_parent(&path)?;
        std::os::unix::fs::symlink(target, &path).map_err(|e| io_err(&path, e))
    }

    /// Creates `rel` as a hard link to the existing fixture file `existing`.
    pub fn hard_link(&mut self, rel: &str, existing: &str) -> Result<(), FixtureError> {
        let from = self.resolve(existing)?;
        self.check_no_symlink(&from)?;
        let path = self.resolve(rel)?;
        self.make_parent(&path)?;
        fs::hard_link(&from, &path).map_err(|e| io_err(&path, e))?;
        self.note(rel).hard_link_of = Some(existing.to_string());
        Ok(())
    }

    /// Sets the modification time of `rel` to `days` before the reference time.
    pub fn set_age_days(&mut self, rel: &str, days: u64) -> Result<(), FixtureError> {
        let path = self.resolve(rel)?;
        self.check_no_symlink(&path)?;
        let when = self
            .now
            .checked_sub(Duration::from_secs(days * 24 * 60 * 60))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let file = File::options()
            .write(true)
            .open(&path)
            .map_err(|e| io_err(&path, e))?;
        file.set_modified(when).map_err(|e| io_err(&path, e))
    }

    /// Writes the manifest into the root and returns it.
    pub fn finish(self) -> Result<Manifest, FixtureError> {
        let manifest = manifest::collect(&self.root, &self.notes)?;
        let json = manifest.to_json();
        let path = self.root.join(manifest::MANIFEST_FILE);
        create_new(&path, json.as_bytes())?;
        Ok(manifest)
    }

    fn note(&mut self, rel: &str) -> &mut Note {
        self.notes.entry(rel.to_string()).or_default()
    }

    /// Writes a new file at `rel`, refusing to overwrite anything.
    fn write_new(&mut self, rel: &str, data: &[u8]) -> Result<(), FixtureError> {
        let path = self.resolve(rel)?;
        self.make_parent(&path)?;
        create_new(&path, data)
    }

    /// Joins `rel` to the root after checking it is made of plain names only.
    fn resolve(&self, rel: &str) -> Result<PathBuf, FixtureError> {
        let rel_path = Path::new(rel);
        let plain = !rel.is_empty()
            && rel_path
                .components()
                .all(|c| matches!(c, Component::Normal(_)));
        if !plain {
            return Err(FixtureError::BadLayoutPath(rel.to_string()));
        }
        Ok(self.root.join(rel_path))
    }

    fn make_parent(&self, path: &Path) -> Result<(), FixtureError> {
        match path.parent() {
            Some(parent) if parent != self.root => self.make_dirs(parent),
            _ => Ok(()),
        }
    }

    /// `mkdir -p` below the root that refuses to go through a symlink.
    fn make_dirs(&self, path: &Path) -> Result<(), FixtureError> {
        let rel = path
            .strip_prefix(&self.root)
            .map_err(|_| FixtureError::BadLayoutPath(path.display().to_string()))?;
        let mut current = self.root.clone();
        for part in rel.components() {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(FixtureError::ThroughSymlink(current));
                }
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(FixtureError::BadLayoutPath(current.display().to_string())),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&current).map_err(|e| io_err(&current, e))?;
                }
                Err(e) => return Err(io_err(&current, e)),
            }
        }
        Ok(())
    }

    /// Refuses if any part of `path` below the root is a symlink.
    fn check_no_symlink(&self, path: &Path) -> Result<(), FixtureError> {
        let rel = path
            .strip_prefix(&self.root)
            .map_err(|_| FixtureError::BadLayoutPath(path.display().to_string()))?;
        let mut current = self.root.clone();
        for part in rel.components() {
            current.push(part);
            let meta = fs::symlink_metadata(&current).map_err(|e| io_err(&current, e))?;
            if meta.file_type().is_symlink() {
                return Err(FixtureError::ThroughSymlink(current));
            }
        }
        Ok(())
    }
}

/// Creates a new file holding `data`; fails if anything already exists there.
fn create_new(path: &Path, data: &[u8]) -> Result<(), FixtureError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| io_err(path, e))?;
    file.write_all(data).map_err(|e| io_err(path, e))
}

/// Wraps an I/O error with the path it happened on.
pub fn io_err(path: &Path, source: io::Error) -> FixtureError {
    FixtureError::Io {
        path: path.to_path_buf(),
        source,
    }
}
