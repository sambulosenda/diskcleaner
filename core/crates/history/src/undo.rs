//! The record that lets the last cleanup be put back after Roomy restarts.
//!
//! This is its own small file, `undo.sqlite`, beside the History database.
//! History itself still holds no paths. This file holds, for the most recent
//! cleanup only, where each moved item was (relative to the home folder),
//! its name in the Trash, and what the Trash entry looked like right after
//! the move. It is replaced by the next cleanup and removed once the cleanup
//! is put back or the Trash no longer holds any of it.
//!
//! Nothing here moves files. The executor's put back reads a record made
//! from this file and checks every entry again before acting: the Trash
//! entry must still be exactly what Roomy left (inode, size and change
//! time), and nothing is ever replaced. Paths are stored relative to home
//! and Trash entries as one plain name, so a record cannot name anything
//! outside the home folder or the Trash.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::{HistoryError, FOLDER};

/// The undo file's name inside [`FOLDER`].
pub const UNDO_FILE_NAME: &str = "undo.sqlite";

/// The undo file for `home`.
pub fn undo_path_for(home: &Path) -> PathBuf {
    home.join(FOLDER).join(UNDO_FILE_NAME)
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS undo (
    only_row INTEGER PRIMARY KEY CHECK (only_row = 1),
    history_id INTEGER,
    finished_at INTEGER NOT NULL,
    dev INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS undo_items (
    position INTEGER PRIMARY KEY,
    item_id TEXT NOT NULL,
    title TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    original BLOB NOT NULL,
    trash_name BLOB NOT NULL,
    ino INTEGER NOT NULL,
    is_dir INTEGER NOT NULL,
    st_size INTEGER NOT NULL,
    ctime_s INTEGER NOT NULL,
    ctime_ns INTEGER NOT NULL
);
";

/// One moved item of the last cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoItem {
    /// The item's id.
    pub item_id: String,
    /// Its title, for the result shown after a put back.
    pub title: String,
    /// Allocated bytes that moved.
    pub size_bytes: u64,
    /// Where it was, relative to the home folder.
    pub original: PathBuf,
    /// Its one plain name directly inside `~/.Trash`.
    pub trash_name: OsString,
    /// The Trash entry's inode number right after the move.
    pub ino: u64,
    /// Whether it is a folder.
    pub is_dir: bool,
    /// The Trash entry's size as the system reported it (`st_size`).
    pub st_size: i64,
    /// The Trash entry's status-change time, seconds and nanoseconds.
    pub ctime: (i64, i64),
}

/// The last cleanup, as much as is needed to put it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoRecord {
    /// Its id in History, if it was saved there.
    pub history_id: Option<i64>,
    /// When it ran, in seconds since 1970 (UTC).
    pub finished_at: i64,
    /// The device id of the volume it ran on.
    pub dev: u64,
    /// Every item that moved, in order.
    pub items: Vec<UndoItem>,
}

/// The undo file.
pub struct Undo {
    conn: Connection,
}

impl Undo {
    /// Opens (or creates) the undo file at `path`. The folder must exist.
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        Self::setup(Connection::open(path)?)
    }

    /// Opens (or creates) the undo file for `home`, making Roomy's own
    /// folder in `~/Library/Application Support` if needed.
    pub fn open_for_home(home: &Path) -> Result<Self, HistoryError> {
        let folder = home.join(FOLDER);
        if !folder.is_dir() {
            std::fs::create_dir(&folder).map_err(HistoryError::Folder)?;
        }
        Self::open(&folder.join(UNDO_FILE_NAME))
    }

    /// Opens the undo file for `home` only if it already exists. Never
    /// makes a file or folder.
    pub fn open_existing(home: &Path) -> Result<Option<Self>, HistoryError> {
        let path = undo_path_for(home);
        if !path.is_file() {
            return Ok(None);
        }
        Self::open(&path).map(Some)
    }

    /// An undo store held in memory, for tests.
    pub fn in_memory() -> Result<Self, HistoryError> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(conn: Connection) -> Result<Self, HistoryError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Stores `record`, replacing any earlier one.
    pub fn save(&mut self, record: &UndoRecord) -> Result<(), HistoryError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM undo_items; DELETE FROM undo;")?;
        tx.execute(
            "INSERT INTO undo (only_row, history_id, finished_at, dev) VALUES (1, ?1, ?2, ?3)",
            params![record.history_id, record.finished_at, bits(record.dev)],
        )?;
        for (position, item) in record.items.iter().enumerate() {
            let position = i64::try_from(position).map_err(|_| HistoryError::TooLarge)?;
            let size = i64::try_from(item.size_bytes).map_err(|_| HistoryError::TooLarge)?;
            tx.execute(
                "INSERT INTO undo_items (position, item_id, title, size_bytes, original,
                     trash_name, ino, is_dir, st_size, ctime_s, ctime_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    position,
                    item.item_id,
                    item.title,
                    size,
                    item.original.as_os_str().as_bytes(),
                    item.trash_name.as_bytes(),
                    bits(item.ino),
                    item.is_dir,
                    item.st_size,
                    item.ctime.0,
                    item.ctime.1
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The stored record, if there is one.
    pub fn load(&self) -> Result<Option<UndoRecord>, HistoryError> {
        let head = self
            .conn
            .query_row("SELECT history_id, finished_at, dev FROM undo", [], |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            });
        let (history_id, finished_at, dev) = match head {
            Ok(head) => head,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mut stmt = self.conn.prepare(
            "SELECT item_id, title, size_bytes, original, trash_name, ino, is_dir, st_size,
                    ctime_s, ctime_ns
             FROM undo_items ORDER BY position",
        )?;
        let items = stmt
            .query_map([], |r| {
                let size: i64 = r.get(2)?;
                Ok(UndoItem {
                    item_id: r.get(0)?,
                    title: r.get(1)?,
                    size_bytes: u64::try_from(size).unwrap_or(0),
                    original: PathBuf::from(OsString::from_vec(r.get(3)?)),
                    trash_name: OsString::from_vec(r.get(4)?),
                    ino: unbits(r.get(5)?),
                    is_dir: r.get(6)?,
                    st_size: r.get(7)?,
                    ctime: (r.get(8)?, r.get(9)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(UndoRecord {
            history_id,
            finished_at,
            dev: unbits(dev),
            items,
        }))
    }

    /// Forgets the stored record.
    pub fn clear(&mut self) -> Result<(), HistoryError> {
        self.conn
            .execute_batch("DELETE FROM undo_items; DELETE FROM undo;")?;
        Ok(())
    }
}

/// A `u64` kept bit for bit in SQLite's signed integer.
fn bits(n: u64) -> i64 {
    i64::from_ne_bytes(n.to_ne_bytes())
}

/// The reverse of [`bits`].
fn unbits(n: i64) -> u64 {
    u64::from_ne_bytes(n.to_ne_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> UndoRecord {
        UndoRecord {
            history_id: Some(7),
            finished_at: 100,
            dev: u64::MAX - 3,
            items: vec![UndoItem {
                item_id: "a".into(),
                title: "Xcode build files".into(),
                size_bytes: 4096,
                original: PathBuf::from("Library/Developer/Xcode/DerivedData"),
                trash_name: OsString::from("DerivedData 2"),
                ino: u64::MAX,
                is_dir: true,
                st_size: 96,
                ctime: (5, 6),
            }],
        }
    }

    #[test]
    fn an_empty_store_has_no_record() {
        assert_eq!(Undo::in_memory().unwrap().load().unwrap(), None);
    }

    #[test]
    fn a_record_comes_back_exactly() {
        let mut undo = Undo::in_memory().unwrap();
        undo.save(&record()).unwrap();
        assert_eq!(undo.load().unwrap(), Some(record()));
    }

    #[test]
    fn a_new_record_replaces_the_old_one() {
        let mut undo = Undo::in_memory().unwrap();
        undo.save(&record()).unwrap();
        let mut newer = record();
        newer.history_id = None;
        newer.items.clear();
        undo.save(&newer).unwrap();
        assert_eq!(undo.load().unwrap(), Some(newer));
    }

    #[test]
    fn clear_forgets_the_record() {
        let mut undo = Undo::in_memory().unwrap();
        undo.save(&record()).unwrap();
        undo.clear().unwrap();
        assert_eq!(undo.load().unwrap(), None);
    }

    #[test]
    fn open_existing_makes_nothing() {
        let home = Path::new("/roomy-test-home-that-does-not-exist");
        assert!(Undo::open_existing(home).unwrap().is_none());
        assert!(!home.exists());
    }
}
