//! What changed since the last scan (F11). Pure: compares a finished scan
//! with the short record History kept of the previous one. No paths.

use std::collections::{BTreeMap, HashMap};

use history::{ScanSnapshot, SnapshotItem};

use crate::types::{Category, CategoryChange, ChangeKind, ItemChange, Mode, ScanChanges};
use crate::views::Finished;

/// Changes smaller than this (10 MB) are left out, so small everyday churn
/// does not crowd out what matters.
pub const MIN_CHANGE_BYTES: u64 = 10_000_000;

/// At most this many items are listed.
pub const MAX_ITEM_CHANGES: usize = 20;

/// The name a mode is stored under.
pub fn mode_key(mode: Mode) -> &'static str {
    match mode {
        Mode::Simple => "simple",
        Mode::Developer => "developer",
    }
}

/// The name a category is stored under. Never change these: older records
/// use them.
fn category_key(category: Category) -> &'static str {
    match category {
        Category::Cache => "cache",
        Category::Build => "build",
        Category::DeveloperTools => "developer_tools",
        Category::Docker => "docker",
        Category::AppData => "app_data",
        Category::Logs => "logs",
        Category::Downloads => "downloads",
        Category::UserData => "user_data",
        Category::Backups => "backups",
        Category::System => "system",
        Category::Unknown => "unknown",
    }
}

/// Reads a stored category name. An unknown name reads as `Unknown`.
fn category_from_key(key: &str) -> Category {
    const ALL: [Category; 11] = [
        Category::Cache,
        Category::Build,
        Category::DeveloperTools,
        Category::Docker,
        Category::AppData,
        Category::Logs,
        Category::Downloads,
        Category::UserData,
        Category::Backups,
        Category::System,
        Category::Unknown,
    ];
    ALL.into_iter()
        .find(|c| category_key(*c) == key)
        .unwrap_or(Category::Unknown)
}

/// The record History keeps of `finished`.
pub fn snapshot(finished: &Finished, finished_at: i64) -> ScanSnapshot {
    ScanSnapshot {
        id: 0,
        finished_at,
        mode: mode_key(finished.mode).to_string(),
        scanned_bytes: finished.scanned_bytes,
        items: finished
            .items
            .iter()
            .map(|i| SnapshotItem {
                item_id: i.id.as_str().to_string(),
                title: i.title.clone(),
                category: category_key(i.category.into()).to_string(),
                size_bytes: i.size_bytes,
            })
            .collect(),
    }
}

/// What grew or shrank between `previous` and `finished`.
pub fn compare(previous: &ScanSnapshot, finished: &Finished) -> ScanChanges {
    ScanChanges {
        previous_scan_at: previous.finished_at,
        scanned_bytes_before: previous.scanned_bytes,
        scanned_bytes_now: finished.scanned_bytes,
        categories: category_changes(previous, finished),
        items: item_changes(previous, finished),
    }
}

/// Whether the change from `a` to `b` is big enough to show.
fn big_enough(a: u64, b: u64) -> bool {
    a.abs_diff(b) >= MIN_CHANGE_BYTES
}

fn category_changes(previous: &ScanSnapshot, finished: &Finished) -> Vec<CategoryChange> {
    let mut sizes: BTreeMap<Category, (u64, u64)> = BTreeMap::new();
    for item in &previous.items {
        let entry = sizes.entry(category_from_key(&item.category)).or_default();
        entry.0 = entry.0.saturating_add(item.size_bytes);
    }
    for item in &finished.items {
        let entry = sizes.entry(item.category.into()).or_default();
        entry.1 = entry.1.saturating_add(item.size_bytes);
    }
    let mut list: Vec<CategoryChange> = sizes
        .into_iter()
        .filter(|(_, (before, now))| big_enough(*before, *now))
        .map(|(category, (bytes_before, bytes_now))| CategoryChange {
            category,
            bytes_before,
            bytes_now,
        })
        .collect();
    // Stable sort keeps `Category` order for equal changes.
    list.sort_by_key(|c| std::cmp::Reverse(c.bytes_before.abs_diff(c.bytes_now)));
    list
}

fn item_changes(previous: &ScanSnapshot, finished: &Finished) -> Vec<ItemChange> {
    let before: HashMap<&str, &SnapshotItem> = previous
        .items
        .iter()
        .map(|i| (i.item_id.as_str(), i))
        .collect();
    let mut list = Vec::new();
    for item in &finished.items {
        let id = item.id.as_str();
        let was = before.get(id).map_or(0, |i| i.size_bytes);
        if !big_enough(was, item.size_bytes) {
            continue;
        }
        let kind = match before.get(id) {
            None => ChangeKind::New,
            Some(_) if item.size_bytes > was => ChangeKind::Grew,
            Some(_) => ChangeKind::Shrank,
        };
        list.push(ItemChange {
            item_id: Some(id.to_string()),
            title: item.title.clone(),
            category: item.category.into(),
            kind,
            bytes_before: was,
            bytes_now: item.size_bytes,
        });
    }
    for item in &previous.items {
        if finished.item(&item.item_id).is_some() || item.size_bytes < MIN_CHANGE_BYTES {
            continue;
        }
        list.push(ItemChange {
            item_id: None,
            title: item.title.clone(),
            category: category_from_key(&item.category),
            kind: ChangeKind::Gone,
            bytes_before: item.size_bytes,
            bytes_now: 0,
        });
    }
    list.sort_by(|a, b| {
        b.bytes_before
            .abs_diff(b.bytes_now)
            .cmp(&a.bytes_before.abs_diff(a.bytes_now))
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.item_id.cmp(&b.item_id))
    });
    list.truncate(MAX_ITEM_CHANGES);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_reads_back_from_its_stored_name() {
        for c in [
            Category::Cache,
            Category::Build,
            Category::DeveloperTools,
            Category::Docker,
            Category::AppData,
            Category::Logs,
            Category::Downloads,
            Category::UserData,
            Category::Backups,
            Category::System,
            Category::Unknown,
        ] {
            assert_eq!(category_from_key(category_key(c)), c);
        }
        assert_eq!(category_from_key("something_new"), Category::Unknown);
    }

    #[test]
    fn small_changes_are_left_out() {
        assert!(!big_enough(0, MIN_CHANGE_BYTES - 1));
        assert!(big_enough(MIN_CHANGE_BYTES, 0));
        assert!(big_enough(5, 5 + MIN_CHANGE_BYTES));
    }
}
