//! What a scan returns: a flat tree of folders with sizes, and the largest files.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Index of a folder in [`ScanResult::dirs`]. The root is always 0.
pub type DirId = usize;

/// One folder and its sizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dir {
    /// The folder's own name. Empty for the root.
    pub name: OsString,
    /// The parent folder. `None` for the root.
    pub parent: Option<DirId>,
    /// Sub-folders, sorted by name.
    pub children: Vec<DirId>,
    /// The folder's own modification time (`lstat`), if the system reported
    /// one. The executor checks it is unchanged before acting.
    pub modified: Option<SystemTime>,
    /// Allocated bytes of the folder itself and the files directly in it.
    pub own_bytes: u64,
    /// Allocated bytes of everything in the folder, sub-folders included.
    pub total_bytes: u64,
    /// File lengths (not allocated size) of everything in the folder.
    pub total_logical_bytes: u64,
    /// Number of files in the folder and its sub-folders.
    pub total_files: u64,
    /// The folder could not be listed, so its contents are not counted.
    pub unreadable: bool,
    /// The folder directly holds an entry named `.git` (any type, any
    /// case), so it is probably a git repository.
    pub has_git: bool,
    /// The plain files directly in this folder, sorted by name. Only filled
    /// in for folders that [`crate::ScanOptions::record_files`] asked for;
    /// empty otherwise. Symlinks and repeat names of hard-linked files are
    /// never listed.
    pub files: Vec<FileEntry>,
}

/// One plain file, recorded only in folders the caller asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// The file's name.
    pub name: OsString,
    /// Space on disk (`st_blocks * 512`).
    pub allocated_bytes: u64,
    /// File length.
    pub logical_bytes: u64,
    /// Last modification time (`lstat`), if the system reported one.
    pub modified: Option<SystemTime>,
}

/// A large file kept for E2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargeFile {
    /// Absolute path, built from the scan root.
    pub path: PathBuf,
    /// Space on disk (`st_blocks * 512`).
    pub allocated_bytes: u64,
    /// File length.
    pub logical_bytes: u64,
    /// Last modification time, if the system reported one.
    pub modified: Option<SystemTime>,
}

/// Why a folder could not be listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnreadableReason {
    /// No permission (including macOS privacy protection).
    PermissionDenied,
    /// The folder went away during the scan.
    Vanished,
    /// Any other read error, with its description.
    Other(String),
}

/// A folder that could not be listed. Its own entry is still counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// Absolute path of the folder.
    pub path: PathBuf,
    /// Why it could not be read.
    pub reason: UnreadableReason,
}

/// The result of one scan.
#[derive(Debug, Clone)]
pub struct ScanResult {
    /// The folder that was scanned.
    pub root: PathBuf,
    /// The device id of the scanned folder's volume. The planner and
    /// executor refuse anything on another volume.
    pub root_dev: u64,
    /// Every folder reached. Index 0 is the root; parents come before children.
    pub dirs: Vec<Dir>,
    /// The largest files, largest first.
    pub largest_files: Vec<LargeFile>,
    /// Folders that could not be listed, sorted by path.
    pub unreadable: Vec<Unreadable>,
    /// Mount points of other volumes that were skipped, sorted.
    pub other_volumes: Vec<PathBuf>,
    /// Number of symlinks seen (counted, never followed).
    pub symlinks: u64,
    /// Extra names of already counted hard-linked files that were skipped.
    pub hard_links_skipped: u64,
    /// Entries that disappeared or could not be read between listing and `lstat`.
    pub entries_skipped: u64,
}

impl ScanResult {
    /// The root folder's record.
    pub fn root_dir(&self) -> &Dir {
        // The walk always creates the root record first.
        &self.dirs[0]
    }

    /// Allocated bytes of the whole tree.
    pub fn total_bytes(&self) -> u64 {
        self.root_dir().total_bytes
    }

    /// Number of files in the whole tree (hard links counted once).
    pub fn total_files(&self) -> u64 {
        self.root_dir().total_files
    }

    /// The absolute path of folder `id`, or `None` if there is no such folder.
    pub fn path_of(&self, id: DirId) -> Option<PathBuf> {
        let mut names: Vec<&OsStr> = Vec::new();
        let mut current = Some(id);
        while let Some(i) = current {
            let dir = self.dirs.get(i)?;
            if dir.parent.is_some() {
                names.push(&dir.name);
            }
            current = dir.parent;
        }
        let mut path = self.root.clone();
        path.extend(names.iter().rev());
        Some(path)
    }

    /// Finds the folder at `rel`, a path relative to the root.
    pub fn find(&self, rel: &Path) -> Option<DirId> {
        let mut id = 0;
        for part in rel.components() {
            let name = part.as_os_str();
            let dir = self.dirs.get(id)?;
            id = *dir
                .children
                .iter()
                .find(|&&child| self.dirs.get(child).is_some_and(|c| c.name == name))?;
        }
        Some(id)
    }
}
