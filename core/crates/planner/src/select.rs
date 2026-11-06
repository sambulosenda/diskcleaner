//! Turns the items the user ticked into [`ItemRequest`]s. Pure: the path,
//! size and modification time all come from the engine's own scan, never
//! from the UI, and nothing here touches the filesystem.

use classifier::{ClassifiedItem, UNKNOWN_RULE_ID};
use rules::{Action, Tier};

use crate::{Expected, ItemRequest};

/// Why an item cannot go into a plan at all. Checked before any path check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotOffered {
    /// The item's tier is `keep`.
    #[error("the app keeps this, so it is not offered for cleanup")]
    Keep,
    /// No rule describes the item.
    #[error("the app does not know what this is, so it is not offered for cleanup")]
    Unknown,
    /// The item's action is a tool command or show only, not the Trash.
    /// Tool commands are not part of cleanup yet.
    #[error("this is not something the app moves to the Trash")]
    NotTrash,
    /// The scan did not record the item's modification time, so the
    /// executor could not check it is unchanged.
    #[error("the app could not check this properly during the scan")]
    NoModifiedTime,
}

/// The request for one ticked item, or why it cannot be offered.
///
/// `selected_individually` is whether the user ticked this item on its own
/// (not as part of a group). It decides whether review-only paths may be
/// included (validation step 6).
pub fn request_for(
    item: &ClassifiedItem,
    selected_individually: bool,
) -> Result<ItemRequest, NotOffered> {
    if item.rule_id == UNKNOWN_RULE_ID {
        return Err(NotOffered::Unknown);
    }
    if item.tier == Tier::Keep {
        return Err(NotOffered::Keep);
    }
    if item.action != Action::Trash {
        return Err(NotOffered::NotTrash);
    }
    let modified = item.modified.ok_or(NotOffered::NoModifiedTime)?;
    Ok(ItemRequest {
        recommendation_id: item.id.as_str().to_string(),
        path: item.path.clone(),
        expected: Expected {
            size_bytes: item.size_bytes,
            modified,
        },
        regenerable: item.regenerable,
        selected_individually,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use classifier::{ItemId, ItemKind};
    use rules::{Category, Confidence};
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn item() -> ClassifiedItem {
        ClassifiedItem {
            id: ItemId::new("cache", b"Library/Caches/x"),
            rule_id: "cache".into(),
            rule_version: 1,
            title: "Cache".into(),
            explain: String::new(),
            risk: String::new(),
            restore: None,
            category: Category::Cache,
            tier: Tier::LikelySafe,
            confidence: Confidence::High,
            regenerable: true,
            action: Action::Trash,
            command: None,
            kind: ItemKind::Folder,
            size_bytes: 4096,
            modified: Some(SystemTime::UNIX_EPOCH),
            path: PathBuf::from("/Users/sam/Library/Caches/x"),
            lowered: Vec::new(),
        }
    }

    #[test]
    fn a_trash_item_becomes_a_request_from_the_scan() {
        let item = item();
        let req = request_for(&item, true).unwrap();
        assert_eq!(req.recommendation_id, item.id.as_str());
        assert_eq!(req.path, item.path);
        assert_eq!(req.expected.size_bytes, 4096);
        assert_eq!(req.expected.modified, SystemTime::UNIX_EPOCH);
        assert!(req.regenerable);
        assert!(req.selected_individually);
    }

    #[test]
    fn keep_items_are_never_offered() {
        let mut item = item();
        item.tier = Tier::Keep;
        assert_eq!(request_for(&item, true), Err(NotOffered::Keep));
    }

    #[test]
    fn unknown_space_is_never_offered() {
        let mut item = item();
        item.rule_id = UNKNOWN_RULE_ID.into();
        assert_eq!(request_for(&item, true), Err(NotOffered::Unknown));
    }

    #[test]
    fn command_and_show_only_items_are_not_offered() {
        for action in [Action::Command, Action::ShowOnly] {
            let mut item = item();
            item.action = action;
            assert_eq!(request_for(&item, true), Err(NotOffered::NotTrash));
        }
    }

    #[test]
    fn an_item_without_a_modification_time_is_not_offered() {
        let mut item = item();
        item.modified = None;
        assert_eq!(request_for(&item, true), Err(NotOffered::NoModifiedTime));
    }
}
