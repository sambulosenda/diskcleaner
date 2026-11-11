//! Keeping the last cleanup's undo record across restarts.
//!
//! After a cleanup moves anything, what the executor reported is written to
//! the undo file (see `history::undo`). The next time Roomy starts, that
//! record is read back so the cleanup can still be put back. Nothing here
//! moves files: the executor's put back checks every entry again, and an
//! entry whose Trash item is gone or changed stays where it is.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Component, Path};

use executor::{PutBack, TrashIdentity};
use history::{Undo, UndoItem, UndoRecord};

use crate::run::LastCleanup;
use crate::types::{CleanupHandle, CleanupResult, CleanupResultItem};

/// The Trash folder's name inside home.
const TRASH_DIR: &str = ".Trash";

/// Writes `last` to the undo file, or forgets the stored record when the
/// newest cleanup moved nothing. Errors go to the log; the undo file never
/// stops a cleanup.
pub(crate) fn remember(home: &Path, last: Option<&LastCleanup>, finished_at: i64) {
    let Some(record) = last.and_then(|l| record(l, finished_at)) else {
        forget(home);
        return;
    };
    let saved = Undo::open_for_home(home).and_then(|mut undo| undo.save(&record));
    if let Err(err) = saved {
        tracing::warn!(%err, "the undo record could not be saved");
        // The stored record is now for an older cleanup. Never offer that
        // one as the most recent after a restart.
        forget(home);
    }
}

/// Forgets the stored record. Makes no file if there is none.
pub(crate) fn forget(home: &Path) {
    let cleared = Undo::open_existing(home).and_then(|undo| match undo {
        Some(mut undo) => undo.clear(),
        None => Ok(()),
    });
    if let Err(err) = cleared {
        tracing::warn!(%err, "the undo record could not be cleared");
    }
}

/// The undo file could not be read just now. Says nothing about what it
/// holds, so the caller should try again later.
#[derive(Debug)]
pub(crate) struct Unreadable;

/// Reads the stored record back as the last cleanup, with `handle` as its
/// new handle. `Ok(None)` if there is none or any part of it is not
/// something Roomy could have written; a record like that is forgotten.
/// `Err` if the file could not be read, which leaves it as it is.
pub(crate) fn restore(
    home: &Path,
    handle: CleanupHandle,
) -> Result<Option<LastCleanup>, Unreadable> {
    let record = match Undo::open_existing(home).and_then(|undo| match undo {
        Some(undo) => undo.load(),
        None => Ok(None),
    }) {
        Ok(Some(record)) => record,
        Ok(None) => return Ok(None),
        Err(err) => {
            tracing::warn!(%err, "the undo record could not be read");
            return Err(Unreadable);
        }
    };
    let last = last_cleanup(home, handle, &record);
    if last.is_none() {
        tracing::warn!("the undo record is not usable, so it was dropped");
        forget(home);
    }
    Ok(last)
}

/// What the app is told about a cleanup that can still be put back: one row
/// per item that moved, `done` for those the Trash still holds.
/// `Ok(None)` if the Trash holds none of them any more. `Err` if the Trash
/// could not be looked at just now, which says nothing about what it holds.
pub(crate) fn summary(last: &LastCleanup) -> Result<Option<CleanupResult>, executor::ExecuteError> {
    let there = executor::still_in_trash(&last.home, last.dev, &last.moved)?;
    if !there.contains(&true) {
        return Ok(None);
    }
    let items: Vec<CleanupResultItem> = last
        .moved
        .iter()
        .zip(there)
        .map(|(entry, there)| {
            let (title, size) = last
                .rows
                .get(&entry.recommendation_id)
                .cloned()
                .unwrap_or_default();
            CleanupResultItem {
                item_id: entry.recommendation_id.clone(),
                title,
                size_bytes: if there { size } else { 0 },
                done: there,
                reason: (!there).then(|| "It is no longer in the Trash.".to_string()),
            }
        })
        .collect();
    let bytes_moved = items
        .iter()
        .fold(0u64, |sum, i| sum.saturating_add(i.size_bytes));
    Ok(Some(CleanupResult {
        cleanup: Some(last.handle),
        items,
        bytes_moved,
        saved_to_history: last.history_id.is_some(),
    }))
}

/// The record to store for `last`. `None` if a path is not inside its home
/// or the Trash, which the executor never reports.
fn record(last: &LastCleanup, finished_at: i64) -> Option<UndoRecord> {
    let trash = last.home.join(TRASH_DIR);
    let items = last
        .moved
        .iter()
        .map(|entry| {
            let (title, size_bytes) = last
                .rows
                .get(&entry.recommendation_id)
                .cloned()
                .unwrap_or_default();
            let original = entry.original.strip_prefix(&last.home).ok()?;
            let trash_name = entry.in_trash.strip_prefix(&trash).ok()?;
            (is_plain(original) && is_one_name(trash_name)).then(|| UndoItem {
                item_id: entry.recommendation_id.clone(),
                title,
                size_bytes,
                original: original.to_path_buf(),
                trash_name: trash_name.as_os_str().to_os_string(),
                ino: entry.identity.ino,
                is_dir: entry.identity.is_dir,
                st_size: entry.identity.size,
                ctime: entry.identity.ctime,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(UndoRecord {
        history_id: last.history_id,
        finished_at,
        dev: last.dev,
        items,
    })
}

/// The last cleanup a stored record describes. `None` if it has no items or
/// any path in it is not plain.
fn last_cleanup(home: &Path, handle: CleanupHandle, record: &UndoRecord) -> Option<LastCleanup> {
    if record.items.is_empty() {
        return None;
    }
    let trash = home.join(TRASH_DIR);
    let mut moved = Vec::with_capacity(record.items.len());
    let mut rows = HashMap::new();
    for item in &record.items {
        let trash_name = Path::new(&item.trash_name);
        if !is_plain(&item.original) || !is_one_name(trash_name) {
            return None;
        }
        moved.push(PutBack {
            recommendation_id: item.item_id.clone(),
            original: home.join(&item.original),
            in_trash: trash.join(trash_name),
            identity: TrashIdentity {
                ino: item.ino,
                is_dir: item.is_dir,
                size: item.st_size,
                ctime: item.ctime,
            },
        });
        rows.insert(item.item_id.clone(), (item.title.clone(), item.size_bytes));
    }
    Some(LastCleanup {
        handle,
        history_id: record.history_id,
        home: home.to_path_buf(),
        dev: record.dev,
        moved,
        rows,
    })
}

/// Whether `path` is relative, not empty, and made of plain names only (no
/// `.`, `..` or root).
fn is_plain(path: &Path) -> bool {
    path.components().count() > 0 && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Whether `path` is exactly one plain name.
fn is_one_name(path: &Path) -> bool {
    is_plain(path) && path.components().count() == 1 && path.as_os_str() != OsStr::new(TRASH_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn item(original: &str, trash_name: &str) -> UndoItem {
        UndoItem {
            item_id: "a".into(),
            title: "A".into(),
            size_bytes: 10,
            original: PathBuf::from(original),
            trash_name: trash_name.into(),
            ino: 1,
            is_dir: false,
            st_size: 10,
            ctime: (1, 2),
        }
    }

    fn restored(items: Vec<UndoItem>) -> Option<LastCleanup> {
        let record = UndoRecord {
            history_id: None,
            finished_at: 0,
            dev: 1,
            items,
        };
        last_cleanup(Path::new("/Users/sam"), CleanupHandle { id: 1 }, &record)
    }

    #[test]
    fn a_plain_record_is_restored_inside_home_and_the_trash() {
        let last = restored(vec![item("Library/Caches/x", "x 2")]).unwrap();
        assert_eq!(
            last.moved[0].original,
            Path::new("/Users/sam/Library/Caches/x")
        );
        assert_eq!(last.moved[0].in_trash, Path::new("/Users/sam/.Trash/x 2"));
    }

    #[test]
    fn a_record_that_points_anywhere_else_is_dropped_whole() {
        let good = || item("Library/Caches/x", "x");
        for bad in [
            item("/etc/passwd", "x"),
            item("../eve/x", "x"),
            item("Library/../../x", "x"),
            item("", "x"),
            item("Library/Caches/x", "a/b"),
            item("Library/Caches/x", ".."),
            item("Library/Caches/x", "/tmp/x"),
            item("Library/Caches/x", ""),
        ] {
            assert!(restored(vec![good(), bad.clone()]).is_none(), "{bad:?}");
        }
        assert!(restored(Vec::new()).is_none());
    }
}
