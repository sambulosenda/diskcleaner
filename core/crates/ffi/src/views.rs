//! Builds what the app sees from a finished scan. Pure: no filesystem access.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use classifier::ClassifiedItem;

use crate::types::{
    Category, CategoryTotal, Caution, ItemDetail, ItemSummary, Mode, PrivateArea, Sort, Tier,
    UnreadableArea,
};
use crate::CoreError;

/// How many items `Overview.top_items` holds at most.
pub const TOP_ITEMS: usize = 8;

/// Counts unreadable folders by the private area each belongs to. Most
/// folders first, ties in [`PrivateArea`] order. Pure: no filesystem access.
pub fn unreadable_areas<'a>(
    home: &Path,
    unreadable: impl IntoIterator<Item = &'a Path>,
) -> Vec<UnreadableArea> {
    let mut counts: BTreeMap<PrivateArea, u64> = BTreeMap::new();
    for path in unreadable {
        let area = PrivateArea::from(scanner::privacy::area_of(home, path));
        let count = counts.entry(area).or_insert(0);
        *count = count.saturating_add(1);
    }
    let mut list: Vec<UnreadableArea> = counts
        .into_iter()
        .map(|(area, folder_count)| UnreadableArea { area, folder_count })
        .collect();
    // Stable sort keeps `PrivateArea` order for equal counts.
    list.sort_by_key(|a| std::cmp::Reverse(a.folder_count));
    list
}

/// A finished scan, held in memory until the app closes it.
#[derive(Debug)]
pub struct Finished {
    /// The mode the scan ran in.
    pub mode: Mode,
    /// The home folder that was scanned. Core-internal only.
    pub home: PathBuf,
    /// Allocated bytes of the whole home folder.
    pub scanned_bytes: u64,
    /// The device id of the home folder's volume, from the scan.
    pub root_dev: u64,
    /// Folders the scan could not read.
    pub unreadable_count: u64,
    /// The same folders, counted by private area. See [`unreadable_areas`].
    pub unreadable_areas: Vec<UnreadableArea>,
    /// Classified items, largest first, then by id (the classifier's order).
    pub items: Vec<ClassifiedItem>,
    /// Item id to index in `items`.
    pub by_id: HashMap<String, usize>,
}

impl Finished {
    /// Wraps classified items and indexes them by id.
    pub fn new(
        mode: Mode,
        home: PathBuf,
        scanned_bytes: u64,
        root_dev: u64,
        unreadable_areas: Vec<UnreadableArea>,
        items: Vec<ClassifiedItem>,
    ) -> Self {
        let unreadable_count = unreadable_areas
            .iter()
            .fold(0u64, |sum, a| sum.saturating_add(a.folder_count));
        let by_id = items
            .iter()
            .enumerate()
            .map(|(i, item)| (item.id.as_str().to_string(), i))
            .collect();
        Self {
            mode,
            home,
            scanned_bytes,
            root_dev,
            unreadable_count,
            unreadable_areas,
            items,
            by_id,
        }
    }

    /// Exact sum of the sizes of every `likely_safe` item (F2).
    pub fn likely_safe_bytes(&self) -> u64 {
        self.items
            .iter()
            .filter(|i| Tier::from(i.tier) == Tier::LikelySafe)
            .fold(0u64, |sum, i| sum.saturating_add(i.size_bytes))
    }

    /// One total per category that has items, largest first, ties in
    /// `Category` order.
    pub fn categories(&self) -> Vec<CategoryTotal> {
        let mut totals: BTreeMap<Category, CategoryTotal> = BTreeMap::new();
        for item in &self.items {
            let category = Category::from(item.category);
            let t = totals.entry(category).or_insert(CategoryTotal {
                category,
                bytes: 0,
                likely_safe_bytes: 0,
                item_count: 0,
            });
            t.bytes = t.bytes.saturating_add(item.size_bytes);
            t.item_count += 1;
            if Tier::from(item.tier) == Tier::LikelySafe {
                t.likely_safe_bytes = t.likely_safe_bytes.saturating_add(item.size_bytes);
            }
        }
        let mut list: Vec<CategoryTotal> = totals.into_values().collect();
        // Stable sort keeps `Category` order for equal sizes.
        list.sort_by_key(|t| std::cmp::Reverse(t.bytes));
        list
    }

    /// Up to [`TOP_ITEMS`] of the largest items that are not `keep`.
    pub fn top_items(&self) -> Vec<ItemSummary> {
        let mut top: Vec<&ClassifiedItem> = self
            .items
            .iter()
            .filter(|i| Tier::from(i.tier) != Tier::Keep)
            .collect();
        sort_items(&mut top, Sort::LargestFirst);
        top.into_iter().take(TOP_ITEMS).map(summary).collect()
    }

    /// Items filtered by category and tier (`None` means any), in `sort`
    /// order.
    pub fn list(
        &self,
        category: Option<Category>,
        tier: Option<Tier>,
        sort: Sort,
    ) -> Vec<ItemSummary> {
        let mut list: Vec<&ClassifiedItem> = self
            .items
            .iter()
            .filter(|i| category.is_none_or(|c| Category::from(i.category) == c))
            .filter(|i| tier.is_none_or(|t| Tier::from(i.tier) == t))
            .collect();
        sort_items(&mut list, sort);
        list.into_iter().map(summary).collect()
    }

    /// The classified item with this id, if the scan has one.
    pub fn item(&self, item_id: &str) -> Option<&ClassifiedItem> {
        self.by_id.get(item_id).and_then(|&i| self.items.get(i))
    }

    /// Full detail for one item. `path` is filled in only in Developer mode.
    /// `device_backup` is left empty here, because filling it reads a file;
    /// `RoomyCore::item_detail` fills it in.
    pub fn detail(&self, item_id: &str) -> Result<ItemDetail, CoreError> {
        let item = self
            .by_id
            .get(item_id)
            .and_then(|&i| self.items.get(i))
            .ok_or(CoreError::UnknownItem)?;
        let path = match self.mode {
            Mode::Developer => display_path(&self.home, &item.path),
            Mode::Simple => None,
        };
        Ok(ItemDetail {
            id: item.id.as_str().to_string(),
            title: item.title.clone(),
            explain: item.explain.clone(),
            risk: item.risk.clone(),
            restore: item.restore.clone(),
            rule_id: item.rule_id.clone(),
            rule_version: item.rule_version,
            confidence: item.confidence.into(),
            tier: item.tier.into(),
            category: item.category.into(),
            size_bytes: item.size_bytes,
            regenerable: item.regenerable,
            action: item.action.into(),
            command: item.command.clone(),
            cautions: item.lowered.iter().map(Caution::from).collect(),
            path,
            device_backup: None,
        })
    }
}

/// The list row for an item. Never holds a path.
pub(crate) fn summary(item: &ClassifiedItem) -> ItemSummary {
    ItemSummary {
        id: item.id.as_str().to_string(),
        title: item.title.clone(),
        explain: item.explain.clone(),
        size_bytes: item.size_bytes,
        tier: item.tier.into(),
        category: item.category.into(),
    }
}

/// Sorts items in place. Ties are always broken by id.
fn sort_items(items: &mut [&ClassifiedItem], sort: Sort) {
    match sort {
        Sort::LargestFirst => items.sort_by(|a, b| {
            b.size_bytes
                .cmp(&a.size_bytes)
                .then_with(|| a.id.cmp(&b.id))
        }),
        Sort::SmallestFirst => items.sort_by(|a, b| {
            a.size_bytes
                .cmp(&b.size_bytes)
                .then_with(|| a.id.cmp(&b.id))
        }),
        Sort::Title => items.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        }),
    }
}

/// `path` as a display string starting `~`, for Developer mode only. A
/// path outside `home` (which the classifier never produces) gives `None`,
/// so nothing outside the home folder is ever shown.
pub(crate) fn display_path(home: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(home).ok()?;
    if rel.as_os_str().is_empty() {
        return Some("~".to_string());
    }
    Some(format!("~/{}", rel.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_paths_start_with_a_tilde() {
        let home = Path::new("/Users/sam");
        assert_eq!(
            display_path(home, Path::new("/Users/sam/Library/Caches")).as_deref(),
            Some("~/Library/Caches")
        );
        assert_eq!(display_path(home, home).as_deref(), Some("~"));
        assert_eq!(display_path(home, Path::new("/etc/passwd")), None);
    }

    #[test]
    fn unreadable_folders_are_counted_by_area_most_first() {
        let home = Path::new("/Users/sam");
        let paths: Vec<PathBuf> = [
            "Library/Containers/com.apple.stocks",
            "Library/Containers/com.apple.news",
            "Library/Group Containers/group.com.apple.notes",
            "Library/Mail",
            "Library/Messages",
            "Library/Caches/locked",
        ]
        .iter()
        .map(|p| home.join(p))
        .collect();
        let areas = unreadable_areas(home, paths.iter().map(PathBuf::as_path));
        let got: Vec<(PrivateArea, u64)> = areas.iter().map(|a| (a.area, a.folder_count)).collect();
        assert_eq!(
            got,
            vec![
                (PrivateArea::AppData, 3),
                (PrivateArea::Mail, 1),
                (PrivateArea::Messages, 1),
                (PrivateArea::Other, 1),
            ]
        );
    }

    #[test]
    fn no_unreadable_folders_means_no_areas() {
        assert!(unreadable_areas(Path::new("/Users/sam"), []).is_empty());
    }
}
