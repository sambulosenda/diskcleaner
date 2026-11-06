//! Path validation and protected paths. Reads metadata only.
//!
//! [`validate`] runs the plan-time checks from the "Validation order" section
//! of `docs/safety/protected-paths.md`, in order. The executor must call it
//! again immediately before acting. Step 7 (size and modification time still
//! match the plan) belongs to the executor and is not here.
//!
//! This crate never follows symlinks: it only calls `symlink_metadata`.

pub mod protected;

use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use protected::{BlockReason, ReviewReason};

/// Why a path was refused. One variant per reason.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The home path given to the core is not usable: not absolute and
    /// canonical, the root folder, a symlink, or not a folder.
    #[error("the home folder setting is not valid")]
    InvalidHome,
    /// Step 1: the path does not start at the root.
    #[error("the path is not a full path")]
    NotAbsolute,
    /// Step 1: the path has `.` or `..` parts, doubled or trailing slashes,
    /// or a NUL byte.
    #[error("the path is written in an unusual way")]
    NotCanonical,
    /// Step 2: the path is not inside the home folder.
    #[error("the path is outside your home folder")]
    OutsideHome,
    /// Step 3: a part of the path is a symlink.
    #[error("part of the path is a link to somewhere else")]
    Symlink {
        /// The first part of the path found to be a symlink.
        at: PathBuf,
    },
    /// Step 3: a part of the path could not be read, or does not exist.
    #[error("part of the path could not be checked")]
    Unreadable {
        /// The part of the path that could not be read.
        at: PathBuf,
    },
    /// Step 4: the path is on a different disk from the scan.
    #[error("the path is on a different disk")]
    OtherVolume,
    /// Step 5: the path is hard-blocked. There is no override.
    #[error("this is {0}, which the app never removes")]
    HardBlocked(BlockReason),
    /// Step 6: the path is review-only and was not selected on its own.
    #[error("this is {0}, so you need to choose it yourself")]
    NeedsIndividualSelection(ReviewReason),
    /// Step 6: the path is review-only and may not be removed in v1 at all.
    #[error("this is {0}, which you can open in Finder but the app does not remove")]
    NotRemovableInV1(ReviewReason),
}

/// A path that passed every plan-time check. Only this crate can make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPath {
    path: PathBuf,
    review_only: Option<ReviewReason>,
}

impl ValidatedPath {
    /// The validated absolute path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Why the path is review-only, if it is.
    pub fn review_only(&self) -> Option<ReviewReason> {
        self.review_only
    }
}

/// What step 3 learned about one folder or file on the path.
#[derive(Debug, Clone, Copy)]
struct NodeInfo {
    dev: u64,
    ino: u64,
    is_dir: bool,
}

/// Validates `candidate` for removal.
///
/// - `home`: the home path passed to the core (never read from `$HOME`).
/// - `scan_root_dev`: the device id (`st_dev`) of the scan root.
/// - `selected_individually`: whether the user selected this item on its own.
///
/// Runs steps 1 to 6 of the validation order, stopping at the first refusal.
pub fn validate(
    home: &Path,
    scan_root_dev: u64,
    candidate: &Path,
    selected_individually: bool,
) -> Result<ValidatedPath, Refusal> {
    check_home(home)?;
    check_absolute_and_canonical(candidate)?; // step 1
    let rel = relative_to_home(home, candidate)?; // step 2
    let nodes = check_no_symlinks(home, &rel)?; // step 3
    check_same_volume(&nodes, scan_root_dev)?; // step 4
    check_not_hard_blocked(home, &rel, &nodes)?; // step 5
    let review_only = check_review_only(&rel, selected_individually)?; // step 6
    Ok(ValidatedPath {
        path: candidate.to_path_buf(),
        review_only,
    })
}

/// Refuses every Docker volume, by identifier. Every Docker action must call
/// this for each volume it would touch.
pub fn check_docker_volume(volume_id: &str) -> Result<(), Refusal> {
    Err(Refusal::HardBlocked(protected::docker_volume_block(
        volume_id,
    )))
}

/// The home path must be absolute, canonical, not `/`, and a real folder.
fn check_home(home: &Path) -> Result<(), Refusal> {
    if !is_absolute_and_canonical(home) || home.parent().is_none() {
        return Err(Refusal::InvalidHome);
    }
    match home.symlink_metadata() {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        _ => Err(Refusal::InvalidHome),
    }
}

/// Step 1: absolute and canonical.
fn check_absolute_and_canonical(candidate: &Path) -> Result<(), Refusal> {
    if !candidate.as_os_str().as_bytes().starts_with(b"/") {
        return Err(Refusal::NotAbsolute);
    }
    if !is_absolute_and_canonical(candidate) {
        return Err(Refusal::NotCanonical);
    }
    Ok(())
}

/// Checks the raw bytes, because `Path::components` quietly hides doubled
/// slashes, trailing slashes and `.` parts.
fn is_absolute_and_canonical(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    let Some(rest) = bytes.strip_prefix(b"/") else {
        return false;
    };
    if bytes.contains(&0) {
        return false;
    }
    if rest.is_empty() {
        return true; // the root folder itself
    }
    rest.split(|b| *b == b'/')
        .all(|part| !part.is_empty() && part != b"." && part != b"..")
}

/// Step 2: the path starts with home. Returns the parts below home.
fn relative_to_home<'a>(home: &Path, candidate: &'a Path) -> Result<Vec<&'a str>, Refusal> {
    let rest = candidate
        .strip_prefix(home)
        .map_err(|_| Refusal::OutsideHome)?;
    rest.components()
        .map(|c| match c {
            Component::Normal(name) => name.to_str().ok_or(Refusal::NotCanonical),
            _ => Err(Refusal::NotCanonical),
        })
        .collect()
}

/// Step 3: no part of the path from home down to the candidate is a symlink.
/// Returns what was learned about each part, home first.
fn check_no_symlinks(home: &Path, rel: &[&str]) -> Result<Vec<NodeInfo>, Refusal> {
    let mut nodes = Vec::with_capacity(rel.len() + 1);
    let mut current = home.to_path_buf();
    nodes.push(node_info(&current)?);
    for part in rel {
        current.push(part);
        nodes.push(node_info(&current)?);
    }
    Ok(nodes)
}

/// Reads one part's metadata without following symlinks.
fn node_info(path: &Path) -> Result<NodeInfo, Refusal> {
    let meta = path.symlink_metadata().map_err(|_| Refusal::Unreadable {
        at: path.to_path_buf(),
    })?;
    if meta.file_type().is_symlink() {
        return Err(Refusal::Symlink {
            at: path.to_path_buf(),
        });
    }
    Ok(NodeInfo {
        dev: meta.dev(),
        ino: meta.ino(),
        is_dir: meta.is_dir(),
    })
}

/// Step 4: the candidate is on the same volume as the scan root.
fn check_same_volume(nodes: &[NodeInfo], scan_root_dev: u64) -> Result<(), Refusal> {
    match nodes.last() {
        Some(node) if node.dev == scan_root_dev => Ok(()),
        _ => Err(Refusal::OtherVolume),
    }
}

/// Step 5: the path is not hard-blocked by name, by the `.git` rule, or by
/// being the same file or folder as a hard-blocked path under another name.
fn check_not_hard_blocked(home: &Path, rel: &[&str], nodes: &[NodeInfo]) -> Result<(), Refusal> {
    if let Some(reason) = protected::hard_block(rel) {
        return Err(Refusal::HardBlocked(reason));
    }
    if let Some(reason) = protected::git_block(rel, |i| folder_holds_git(home, rel, nodes, i))? {
        return Err(Refusal::HardBlocked(reason));
    }
    if let Some(reason) = same_as_hard_blocked(home, nodes)? {
        return Err(Refusal::HardBlocked(reason));
    }
    Ok(())
}

/// Whether the folder made of home plus the first `i` parts holds `.git`.
/// Any error other than "not found" is a refusal, never a pass.
fn folder_holds_git(
    home: &Path,
    rel: &[&str],
    nodes: &[NodeInfo],
    i: usize,
) -> Result<bool, Refusal> {
    if !nodes.get(i).is_some_and(|n| n.is_dir) {
        return Ok(false);
    }
    let mut git = home.to_path_buf();
    git.extend(&rel[..i]);
    git.push(protected::GIT_DIR_NAME);
    match git.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Refusal::Unreadable { at: git }),
    }
}

/// Catches a hard-blocked path reached under another name, such as a
/// different letter case or a hard link to `~/.netrc`, by comparing device
/// and inode numbers with each fixed hard-blocked path that exists.
fn same_as_hard_blocked(home: &Path, nodes: &[NodeInfo]) -> Result<Option<BlockReason>, Refusal> {
    for entry in protected::HARD_BLOCKED {
        let mut path = home.to_path_buf();
        path.extend(entry.components);
        let meta = match path.symlink_metadata() {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(Refusal::Unreadable { at: path }),
        };
        let same = |n: &NodeInfo| n.dev == meta.dev() && n.ino == meta.ino();
        let hit = match entry.scope {
            protected::Scope::Itself => nodes.last().is_some_and(same),
            // Skip home itself (index 0): it is only blocked as "Itself".
            _ => nodes.iter().skip(1).any(same),
        };
        if hit {
            return Ok(Some(entry.reason));
        }
    }
    Ok(None)
}

/// Step 6: a review-only path needs individual selection, and some kinds may
/// not be removed in v1 at all.
fn check_review_only(
    rel: &[&str],
    selected_individually: bool,
) -> Result<Option<ReviewReason>, Refusal> {
    match protected::review_only(rel) {
        Some(reason) if !reason.deletable_in_v1() => Err(Refusal::NotRemovableInV1(reason)),
        Some(reason) if !selected_individually => Err(Refusal::NeedsIndividualSelection(reason)),
        other => Ok(other),
    }
}
