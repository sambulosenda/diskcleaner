//! The duplicate finder's last check: before a copy of a file moves to the
//! Trash, the copy the user keeps must still be there, unchanged, and the
//! same byte for byte as the copy that moves.
//!
//! Read only. Both files are opened relative to folders opened from home
//! down with `O_NOFOLLOW`, so no symlink is followed, and each must still be
//! the file the user reviewed (device, inode, length, modification time)
//! before and after the bytes are compared. If anything is off, the copy
//! stays where it is.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::fs::MetadataExt;
use std::time::SystemTime;

use planner::KeepCopy;

use crate::{sys, ItemRefusal};

/// How much of each file is read at a time.
const CHUNK_BYTES: usize = 1024 * 1024;

/// What a file looked like, to tell whether it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Look {
    dev: u64,
    ino: u64,
    len: u64,
    modified: Option<SystemTime>,
}

/// How a plain file looks now, or `None` if it isn't a plain file.
fn look(file: &File) -> Option<Look> {
    let meta = file.metadata().ok()?;
    meta.is_file().then(|| Look {
        dev: meta.dev(),
        ino: meta.ino(),
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// Checks the item `name` in `parent` (already surveyed as `item_dev`,
/// `item_ino`) against the copy the user keeps.
///
/// `keep_parent` and `keep_name` are the kept copy's folder, opened the same
/// safe way, and its name.
pub(crate) fn check(
    keep: &KeepCopy,
    keep_parent: &OwnedFd,
    keep_name: &OsStr,
    parent: &OwnedFd,
    name: &OsStr,
    item: (u64, u64),
) -> Result<(), ItemRefusal> {
    let reviewed = Look {
        dev: keep.dev,
        ino: keep.ino,
        len: keep.len,
        modified: Some(keep.modified),
    };
    let mut kept =
        sys::open_file_at(keep_parent, keep_name).map_err(|_| ItemRefusal::KeptCopyChanged)?;
    if look(&kept) != Some(reviewed) {
        return Err(ItemRefusal::KeptCopyChanged);
    }
    // Two names of one file are not two copies: moving one keeps nothing.
    if (keep.dev, keep.ino) == item {
        return Err(ItemRefusal::NotSameAsKeptCopy);
    }
    let mut moving =
        sys::open_file_at(parent, name).map_err(|_| ItemRefusal::ChangedSinceReview)?;
    let before = look(&moving).ok_or(ItemRefusal::ChangedSinceReview)?;
    if (before.dev, before.ino) != item {
        return Err(ItemRefusal::ChangedSinceReview);
    }
    if before.len != keep.len {
        return Err(ItemRefusal::NotSameAsKeptCopy);
    }
    let same =
        same_bytes(&mut kept, &mut moving, keep.len).map_err(|_| ItemRefusal::KeptCopyChanged)?;
    // Neither changed while they were read.
    if look(&kept) != Some(reviewed) {
        return Err(ItemRefusal::KeptCopyChanged);
    }
    if look(&moving) != Some(before) {
        return Err(ItemRefusal::ChangedSinceReview);
    }
    if !same {
        return Err(ItemRefusal::NotSameAsKeptCopy);
    }
    Ok(())
}

/// Whether the first `len` bytes of `a` and `b` are the same and both end
/// there.
fn same_bytes(a: &mut File, b: &mut File, len: u64) -> std::io::Result<bool> {
    let mut buf_a = vec![0u8; CHUNK_BYTES];
    let mut buf_b = vec![0u8; CHUNK_BYTES];
    let mut left = len;
    while left > 0 {
        let n = usize::try_from(left).map_or(CHUNK_BYTES, |l| l.min(CHUNK_BYTES));
        a.read_exact(&mut buf_a[..n])?;
        b.read_exact(&mut buf_b[..n])?;
        if buf_a[..n] != buf_b[..n] {
            return Ok(false);
        }
        left -= n as u64;
    }
    // Both must end where the review said they did.
    let mut one = [0u8; 1];
    Ok(a.read(&mut one)? == 0 && b.read(&mut one)? == 0)
}
