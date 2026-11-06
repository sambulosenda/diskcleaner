//! Reads an item before acting on it: measures it the way the scanner does,
//! and checks everything inside it.
//!
//! `safety::validate` checks the item's own path and its parents. Removing a
//! folder also removes everything in it, so this walk applies the name-based
//! protected-path rules to every path inside the item as well. For example, a
//! folder that holds a `.photoslibrary`, a `.git` folder or a signing key is
//! refused as a whole.
//!
//! Read only. Never follows a symlink (`symlink_metadata` and `read_dir`
//! entries only) and never crosses onto another volume.

use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::SystemTime;

use safety::protected;

use crate::ItemRefusal;

/// What the walk learned about an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Survey {
    /// Allocated bytes, measured like the scanner's `total_bytes`.
    pub size_bytes: u64,
    /// Device id of the item itself.
    pub dev: u64,
    /// Inode number of the item itself.
    pub ino: u64,
    /// The item's own modification time.
    pub modified: SystemTime,
    /// Whether the item is a folder.
    pub is_dir: bool,
}

/// Measures `root` and checks every path inside it.
///
/// - `home`: the home path; `root` must be inside it.
/// - `scan_dev`: the device id of the scan root; nothing may be elsewhere.
/// - `selected_individually`: whether the user chose this item on its own,
///   which review-only paths inside it need.
pub fn survey(
    home: &Path,
    root: &Path,
    scan_dev: u64,
    selected_individually: bool,
) -> Result<Survey, ItemRefusal> {
    let root_rel = rel_parts(home, root)?;
    let meta = lstat(root)?;
    if meta.file_type().is_symlink() {
        return Err(ItemRefusal::ChangedSinceReview);
    }
    if meta.dev() != scan_dev {
        return Err(ItemRefusal::OtherVolumeInside {
            at: root.to_path_buf(),
        });
    }
    let modified = meta.modified().map_err(|_| ItemRefusal::Unreadable {
        at: root.to_path_buf(),
    })?;
    let mut seen_links = HashSet::new();
    let mut size = allocated(&meta, &mut seen_links);
    if meta.is_dir() {
        size += walk(
            root,
            root_rel,
            scan_dev,
            selected_individually,
            &mut seen_links,
        )?;
    }
    Ok(Survey {
        size_bytes: size,
        dev: meta.dev(),
        ino: meta.ino(),
        modified,
        is_dir: meta.is_dir(),
    })
}

/// Walks everything inside the folder `root` (not `root` itself) and returns
/// its allocated bytes. Iterative, so a deep tree cannot overflow the stack.
fn walk(
    root: &Path,
    root_rel: Vec<String>,
    scan_dev: u64,
    selected_individually: bool,
    seen_links: &mut HashSet<(u64, u64)>,
) -> Result<u64, ItemRefusal> {
    let mut size = 0u64;
    let mut stack = vec![(root.to_path_buf(), root_rel)];
    while let Some((dir, rel)) = stack.pop() {
        let entries =
            fs::read_dir(&dir).map_err(|_| ItemRefusal::Unreadable { at: dir.clone() })?;
        for entry in entries {
            let entry = entry.map_err(|_| ItemRefusal::Unreadable { at: dir.clone() })?;
            let path = entry.path();
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ItemRefusal::UnusualName { at: path.clone() })?;
            let mut child_rel = rel.clone();
            child_rel.push(name);
            check_protected(&path, &child_rel, selected_individually)?;
            // DirEntry::metadata is lstat: it never follows a symlink.
            let meta = entry
                .metadata()
                .map_err(|_| ItemRefusal::ChangedSinceReview)?;
            if meta.dev() != scan_dev {
                return Err(ItemRefusal::OtherVolumeInside { at: path });
            }
            size = size.saturating_add(allocated(&meta, seen_links));
            if meta.is_dir() {
                stack.push((path, child_rel));
            }
        }
    }
    Ok(size)
}

/// Applies the name-based protected-path rules to one path inside an item.
fn check_protected(path: &Path, rel: &[String], selected: bool) -> Result<(), ItemRefusal> {
    let parts: Vec<&str> = rel.iter().map(String::as_str).collect();
    if let Some(reason) = protected::hard_block(&parts) {
        return Err(ItemRefusal::ContainsProtected {
            at: path.to_path_buf(),
            reason,
        });
    }
    match protected::review_only(&parts) {
        Some(reason) if !reason.deletable_in_v1() => Err(ItemRefusal::ContainsNotRemovable {
            at: path.to_path_buf(),
            reason,
        }),
        Some(reason) if !selected => Err(ItemRefusal::ContainsReviewOnly {
            at: path.to_path_buf(),
            reason,
        }),
        _ => Ok(()),
    }
}

/// Allocated bytes of one entry, or 0 for a further name of a hard-linked
/// file already counted (any type with more than one link, as the scanner does).
fn allocated(meta: &Metadata, seen_links: &mut HashSet<(u64, u64)>) -> u64 {
    if !meta.is_dir() && meta.nlink() > 1 && !seen_links.insert((meta.dev(), meta.ino())) {
        return 0;
    }
    meta.blocks().saturating_mul(512)
}

/// `symlink_metadata`, with a missing path reported as "changed since review".
fn lstat(path: &Path) -> Result<Metadata, ItemRefusal> {
    path.symlink_metadata()
        .map_err(|_| ItemRefusal::ChangedSinceReview)
}

/// The parts of `path` below `home`, as UTF-8 names.
fn rel_parts(home: &Path, path: &Path) -> Result<Vec<String>, ItemRefusal> {
    let rest = path
        .strip_prefix(home)
        .map_err(|_| ItemRefusal::UnusualName {
            at: path.to_path_buf(),
        })?;
    rest.iter()
        .map(|part| {
            part.to_str()
                .map(str::to_string)
                .ok_or_else(|| ItemRefusal::UnusualName {
                    at: path.to_path_buf(),
                })
        })
        .collect()
}

/// Whether `actual` is within 1% of `expected` (validation step 7).
pub fn size_matches(expected: u64, actual: u64) -> bool {
    let diff = u128::from(expected.abs_diff(actual));
    diff * 100 <= u128::from(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_tolerance_is_one_percent() {
        assert!(size_matches(10_000, 10_000));
        assert!(size_matches(10_000, 10_100));
        assert!(size_matches(10_000, 9_900));
        assert!(!size_matches(10_000, 10_101));
        assert!(!size_matches(10_000, 9_899));
        assert!(size_matches(0, 0));
        assert!(!size_matches(0, 1));
        assert!(size_matches(u64::MAX, u64::MAX));
    }
}
