//! SQLite log of cleanups. Writes its own database only.
//!
//! What is kept, for the History screen: when each cleanup ran, and for each
//! item its title, size and what happened. **No paths are stored**: History
//! is for people to read, and nothing ever acts on what is in it. The undo
//! ("put back") works from the executor's own report, held in memory.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

/// Where the database lives, relative to the home folder. The last folder
/// is made if it is missing; the ones above it must already exist.
pub const FOLDER: &str = "Library/Application Support/Roomy";

/// The database file's name inside [`FOLDER`].
pub const FILE_NAME: &str = "history.sqlite";

/// The database file for `home`.
pub fn path_for(home: &Path) -> PathBuf {
    home.join(FOLDER).join(FILE_NAME)
}

/// The database layout. Bump [`SCHEMA_VERSION`] when it changes.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS cleanups (
    id INTEGER PRIMARY KEY,
    finished_at INTEGER NOT NULL,
    put_back_at INTEGER
);
CREATE TABLE IF NOT EXISTS cleanup_items (
    cleanup_id INTEGER NOT NULL REFERENCES cleanups(id),
    position INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    title TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    state TEXT NOT NULL,
    reason TEXT,
    PRIMARY KEY (cleanup_id, position)
);
";

/// The layout version, stored in `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 1;

/// Something went wrong reading or writing the History database.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// SQLite reported an error.
    #[error("the History database could not be used: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The database was made by a newer version of the app.
    #[error("the History database is from a newer version of Roomy")]
    NewerSchema,
    /// The database's folder could not be made.
    #[error("the History folder could not be made: {0}")]
    Folder(std::io::Error),
    /// A size or count does not fit in SQLite's signed 64-bit integers.
    #[error("a number is too large to record")]
    TooLarge,
}

/// What happened to one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemState {
    /// Moved to the Trash.
    Moved,
    /// Not moved, for this reason (plain words, no path).
    NotMoved {
        /// Why.
        reason: String,
    },
    /// Moved to the Trash, then put back.
    PutBack,
    /// Moved to the Trash; putting it back was refused, so it stayed there.
    StayedInTrash {
        /// Why (plain words, no path).
        reason: String,
    },
}

impl ItemState {
    /// The name stored in the database, and the reason if any.
    fn to_row(&self) -> (&'static str, Option<&str>) {
        match self {
            Self::Moved => ("moved", None),
            Self::NotMoved { reason } => ("not_moved", Some(reason)),
            Self::PutBack => ("put_back", None),
            Self::StayedInTrash { reason } => ("stayed_in_trash", Some(reason)),
        }
    }

    /// Reads a stored state. An unknown name reads as not moved, the safe
    /// side for a log.
    fn from_row(state: &str, reason: Option<String>) -> Self {
        let reason = reason.unwrap_or_default();
        match state {
            "moved" => Self::Moved,
            "put_back" => Self::PutBack,
            "stayed_in_trash" => Self::StayedInTrash { reason },
            _ => Self::NotMoved { reason },
        }
    }
}

/// One item of a cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The item's id from the scan.
    pub item_id: String,
    /// Its title.
    pub title: String,
    /// Allocated bytes.
    pub size_bytes: u64,
    /// What happened.
    pub state: ItemState,
}

/// One cleanup, as the History screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleanup {
    /// The cleanup's id in the database.
    pub id: i64,
    /// When it ran, in seconds since 1970 (UTC).
    pub finished_at: i64,
    /// When it was put back, if it was.
    pub put_back_at: Option<i64>,
    /// Its items, in the order they were cleaned.
    pub items: Vec<Item>,
}

impl Cleanup {
    /// Bytes that moved to the Trash and are still there.
    pub fn bytes_in_trash(&self) -> u64 {
        self.items
            .iter()
            .filter(|i| matches!(i.state, ItemState::Moved | ItemState::StayedInTrash { .. }))
            .fold(0u64, |sum, i| sum.saturating_add(i.size_bytes))
    }
}

/// The History database.
pub struct History {
    conn: Connection,
}

impl History {
    /// Opens (or creates) the database file at `path`. The folder must exist.
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        Self::setup(Connection::open(path)?)
    }

    /// Opens (or creates) the database for `home`, making its own folder in
    /// `~/Library/Application Support` if needed. Never makes any other
    /// folder.
    pub fn open_for_home(home: &Path) -> Result<Self, HistoryError> {
        let folder = home.join(FOLDER);
        if !folder.is_dir() {
            std::fs::create_dir(&folder).map_err(HistoryError::Folder)?;
        }
        Self::open(&folder.join(FILE_NAME))
    }

    /// A database held in memory, for tests and when the file can't be used.
    pub fn in_memory() -> Result<Self, HistoryError> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(conn: Connection) -> Result<Self, HistoryError> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(HistoryError::NewerSchema);
        }
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// Records one cleanup and returns its id.
    pub fn record_cleanup(
        &mut self,
        finished_at: i64,
        items: &[Item],
    ) -> Result<i64, HistoryError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO cleanups (finished_at) VALUES (?1)",
            params![finished_at],
        )?;
        let id = tx.last_insert_rowid();
        for (position, item) in items.iter().enumerate() {
            let (state, reason) = item.state.to_row();
            let position = i64::try_from(position).map_err(|_| HistoryError::TooLarge)?;
            let size = i64::try_from(item.size_bytes).map_err(|_| HistoryError::TooLarge)?;
            tx.execute(
                "INSERT INTO cleanup_items (cleanup_id, position, item_id, title, size_bytes, state, reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, position, item.item_id, item.title, size, state, reason],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Records that a cleanup was put back. `results` gives, for each item
    /// id that was in the Trash, `None` if it was put back or the reason it
    /// stayed in the Trash.
    pub fn record_put_back(
        &mut self,
        cleanup_id: i64,
        at: i64,
        results: &[(String, Option<String>)],
    ) -> Result<(), HistoryError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE cleanups SET put_back_at = ?2 WHERE id = ?1",
            params![cleanup_id, at],
        )?;
        for (item_id, refused) in results {
            let state = match refused {
                None => ItemState::PutBack,
                Some(reason) => ItemState::StayedInTrash {
                    reason: reason.clone(),
                },
            };
            let (state, reason) = state.to_row();
            tx.execute(
                "UPDATE cleanup_items SET state = ?3, reason = ?4
                 WHERE cleanup_id = ?1 AND item_id = ?2 AND state = 'moved'",
                params![cleanup_id, item_id, state, reason],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The most recent cleanups, newest first, at most `limit`.
    pub fn recent(&self, limit: u32) -> Result<Vec<Cleanup>, HistoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, finished_at, put_back_at FROM cleanups ORDER BY finished_at DESC, id DESC LIMIT ?1",
        )?;
        let heads = stmt
            .query_map(params![limit], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        heads
            .into_iter()
            .map(|(id, finished_at, put_back_at)| {
                Ok(Cleanup {
                    id,
                    finished_at,
                    put_back_at,
                    items: self.items(id)?,
                })
            })
            .collect()
    }

    /// One cleanup, if it exists.
    pub fn get(&self, id: i64) -> Result<Option<Cleanup>, HistoryError> {
        let head = self
            .conn
            .query_row(
                "SELECT finished_at, put_back_at FROM cleanups WHERE id = ?1",
                params![id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?;
        match head {
            None => Ok(None),
            Some((finished_at, put_back_at)) => Ok(Some(Cleanup {
                id,
                finished_at,
                put_back_at,
                items: self.items(id)?,
            })),
        }
    }

    fn items(&self, cleanup_id: i64) -> Result<Vec<Item>, HistoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT item_id, title, size_bytes, state, reason FROM cleanup_items
             WHERE cleanup_id = ?1 ORDER BY position",
        )?;
        let rows = stmt
            .query_map(params![cleanup_id], |r| {
                let size: i64 = r.get(2)?;
                Ok(Item {
                    item_id: r.get(0)?,
                    title: r.get(1)?,
                    size_bytes: u64::try_from(size).unwrap_or(0),
                    state: ItemState::from_row(&r.get::<_, String>(3)?, r.get(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, size: u64, state: ItemState) -> Item {
        Item {
            item_id: id.into(),
            title: format!("Title {id}"),
            size_bytes: size,
            state,
        }
    }

    #[test]
    fn records_and_reads_back_a_cleanup() {
        let mut h = History::in_memory().unwrap();
        let items = vec![
            item("a", 100, ItemState::Moved),
            item(
                "b",
                50,
                ItemState::NotMoved {
                    reason: "This changed since you reviewed it.".into(),
                },
            ),
        ];
        let id = h.record_cleanup(1_800_000_000, &items).unwrap();
        let got = h.get(id).unwrap().unwrap();
        assert_eq!(got.items, items);
        assert_eq!(got.finished_at, 1_800_000_000);
        assert_eq!(got.put_back_at, None);
        assert_eq!(got.bytes_in_trash(), 100);
    }

    #[test]
    fn recent_is_newest_first_and_limited() {
        let mut h = History::in_memory().unwrap();
        for t in [10, 30, 20] {
            h.record_cleanup(t, &[item("a", 1, ItemState::Moved)])
                .unwrap();
        }
        let times: Vec<i64> = h.recent(2).unwrap().iter().map(|c| c.finished_at).collect();
        assert_eq!(times, vec![30, 20]);
    }

    #[test]
    fn put_back_updates_only_moved_items() {
        let mut h = History::in_memory().unwrap();
        let id = h
            .record_cleanup(
                5,
                &[
                    item("a", 100, ItemState::Moved),
                    item("b", 40, ItemState::Moved),
                    item(
                        "c",
                        7,
                        ItemState::NotMoved {
                            reason: "No.".into(),
                        },
                    ),
                ],
            )
            .unwrap();
        h.record_put_back(
            id,
            9,
            &[
                ("a".into(), None),
                (
                    "b".into(),
                    Some("Something new is in its old place.".into()),
                ),
                ("c".into(), None),
            ],
        )
        .unwrap();
        let got = h.get(id).unwrap().unwrap();
        assert_eq!(got.put_back_at, Some(9));
        assert_eq!(got.items[0].state, ItemState::PutBack);
        assert!(matches!(
            got.items[1].state,
            ItemState::StayedInTrash { .. }
        ));
        assert!(matches!(got.items[2].state, ItemState::NotMoved { .. }));
        assert_eq!(got.bytes_in_trash(), 40);
    }

    #[test]
    fn a_file_keeps_history_between_opens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        let id = History::open(&path)
            .unwrap()
            .record_cleanup(1, &[item("a", 1, ItemState::Moved)])
            .unwrap();
        let again = History::open(&path).unwrap();
        assert!(again.get(id).unwrap().is_some());
    }

    #[test]
    fn open_for_home_makes_only_its_own_folder() {
        let dir = tempfile::tempdir().unwrap();
        // No Library/Application Support: refused, nothing made.
        assert!(History::open_for_home(dir.path()).is_err());
        assert!(!dir.path().join("Library").exists());
        std::fs::create_dir_all(dir.path().join("Library/Application Support")).unwrap();
        History::open_for_home(dir.path()).unwrap();
        assert!(path_for(dir.path()).is_file());
    }

    #[test]
    fn a_newer_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        assert!(matches!(
            History::open(&path),
            Err(HistoryError::NewerSchema)
        ));
    }

    #[test]
    fn nothing_stored_looks_like_a_path() {
        let mut h = History::in_memory().unwrap();
        h.record_cleanup(1, &[item("a", 1, ItemState::Moved)])
            .unwrap();
        let cols: Vec<String> = h
            .conn
            .prepare("SELECT name FROM pragma_table_info('cleanup_items')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!cols.iter().any(|c| c.contains("path")), "{cols:?}");
    }
}
