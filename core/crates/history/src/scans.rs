//! A short record of each finished scan, for "what changed since the last
//! scan" (F11).
//!
//! Each record holds when the scan finished, its mode, the size of the whole
//! home folder and, per item, the item's id, title, category and size. **No
//! paths**: item ids are hashes, and titles are what the app already shows.
//! Nothing ever acts on these records; they are only compared with a newer
//! scan to say what grew or shrank.
//!
//! Only the latest [`KEEP_SCANS_PER_MODE`] records of each mode are kept.

use rusqlite::{params, OptionalExtension};

use crate::{History, HistoryError};

/// How many scan records of each mode are kept. Older ones are dropped
/// when a new one is recorded.
pub const KEEP_SCANS_PER_MODE: i64 = 5;

/// One item as a scan saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotItem {
    /// The item's id from the scan (a hash, never a path).
    pub item_id: String,
    /// Its title, as shown in the app.
    pub title: String,
    /// Its category's stored name, such as `cache`.
    pub category: String,
    /// Allocated bytes.
    pub size_bytes: u64,
}

/// One finished scan, as "what changed" compares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSnapshot {
    /// Its id in the database (ignored when recording).
    pub id: i64,
    /// When the scan finished, in seconds since 1970 (UTC).
    pub finished_at: i64,
    /// The mode it ran in, such as `simple`.
    pub mode: String,
    /// Allocated bytes of the whole home folder.
    pub scanned_bytes: u64,
    /// Every item it found.
    pub items: Vec<SnapshotItem>,
}

impl History {
    /// Records one finished scan and returns its id. Drops the oldest
    /// records of the same mode beyond [`KEEP_SCANS_PER_MODE`].
    pub fn record_scan(&mut self, scan: &ScanSnapshot) -> Result<i64, HistoryError> {
        let scanned = i64::try_from(scan.scanned_bytes).map_err(|_| HistoryError::TooLarge)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO scans (finished_at, mode, scanned_bytes) VALUES (?1, ?2, ?3)",
            params![scan.finished_at, scan.mode, scanned],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut insert = tx.prepare(
                "INSERT OR REPLACE INTO scan_items (scan_id, item_id, title, category, size_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for item in &scan.items {
                let size = i64::try_from(item.size_bytes).map_err(|_| HistoryError::TooLarge)?;
                insert.execute(params![id, item.item_id, item.title, item.category, size])?;
            }
        }
        // Rows of this database only: older scan records of the same mode.
        let old = "SELECT id FROM scans WHERE mode = ?1
                   ORDER BY finished_at DESC, id DESC LIMIT -1 OFFSET ?2";
        tx.execute(
            &format!("DELETE FROM scan_items WHERE scan_id IN ({old})"),
            params![scan.mode, KEEP_SCANS_PER_MODE],
        )?;
        tx.execute(
            &format!("DELETE FROM scans WHERE id IN ({old})"),
            params![scan.mode, KEEP_SCANS_PER_MODE],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// The most recent scan record of `mode`, if there is one.
    pub fn latest_scan(&self, mode: &str) -> Result<Option<ScanSnapshot>, HistoryError> {
        let head = self
            .conn
            .query_row(
                "SELECT id, finished_at, scanned_bytes FROM scans WHERE mode = ?1
                 ORDER BY finished_at DESC, id DESC LIMIT 1",
                params![mode],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, finished_at, scanned)) = head else {
            return Ok(None);
        };
        let mut stmt = self.conn.prepare(
            "SELECT item_id, title, category, size_bytes FROM scan_items
             WHERE scan_id = ?1 ORDER BY item_id",
        )?;
        let items = stmt
            .query_map(params![id], |r| {
                let size: i64 = r.get(3)?;
                Ok(SnapshotItem {
                    item_id: r.get(0)?,
                    title: r.get(1)?,
                    category: r.get(2)?,
                    size_bytes: u64::try_from(size).unwrap_or(0),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ScanSnapshot {
            id,
            finished_at,
            mode: mode.to_string(),
            scanned_bytes: u64::try_from(scanned).unwrap_or(0),
            items,
        }))
    }

    /// How many scan records of `mode` are kept. For tests.
    pub fn scan_count(&self, mode: &str) -> Result<i64, HistoryError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM scans WHERE mode = ?1",
            params![mode],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(at: i64, mode: &str, sizes: &[(&str, u64)]) -> ScanSnapshot {
        ScanSnapshot {
            id: 0,
            finished_at: at,
            mode: mode.into(),
            scanned_bytes: sizes.iter().map(|(_, s)| s).sum(),
            items: sizes
                .iter()
                .map(|(id, size)| SnapshotItem {
                    item_id: (*id).into(),
                    title: format!("Title {id}"),
                    category: "cache".into(),
                    size_bytes: *size,
                })
                .collect(),
        }
    }

    #[test]
    fn records_and_reads_back_the_latest_scan_of_a_mode() {
        let mut h = History::in_memory().unwrap();
        assert_eq!(h.latest_scan("simple").unwrap(), None);
        h.record_scan(&snapshot(10, "simple", &[("a", 5), ("b", 7)]))
            .unwrap();
        let id = h.record_scan(&snapshot(20, "simple", &[("a", 6)])).unwrap();
        h.record_scan(&snapshot(30, "developer", &[("c", 1)]))
            .unwrap();
        let got = h.latest_scan("simple").unwrap().unwrap();
        assert_eq!(
            got,
            ScanSnapshot {
                id,
                ..snapshot(20, "simple", &[("a", 6)])
            }
        );
        assert_eq!(h.latest_scan("developer").unwrap().unwrap().items.len(), 1);
    }

    #[test]
    fn only_the_latest_records_of_each_mode_are_kept() {
        let mut h = History::in_memory().unwrap();
        for at in 0..(KEEP_SCANS_PER_MODE + 3) {
            h.record_scan(&snapshot(at, "simple", &[("a", 1)])).unwrap();
        }
        h.record_scan(&snapshot(0, "developer", &[("a", 1)]))
            .unwrap();
        assert_eq!(h.scan_count("simple").unwrap(), KEEP_SCANS_PER_MODE);
        assert_eq!(h.scan_count("developer").unwrap(), 1);
        let items: i64 = h
            .conn
            .query_row("SELECT COUNT(*) FROM scan_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            items,
            KEEP_SCANS_PER_MODE + 1,
            "old items go with their scan"
        );
        assert_eq!(
            h.latest_scan("simple").unwrap().unwrap().finished_at,
            KEEP_SCANS_PER_MODE + 2
        );
    }

    #[test]
    fn scan_records_hold_no_paths() {
        let h = History::in_memory().unwrap();
        for table in ["scans", "scan_items"] {
            let cols: Vec<String> = h
                .conn
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert!(!cols.is_empty());
            assert!(!cols.iter().any(|c| c.contains("path")), "{cols:?}");
        }
    }
}
