//! Match scan results to rules. Requirement F4. Never touches the filesystem.
//!
//! [`classify`] turns a [`ScanResult`] and the loaded rules into
//! [`ClassifiedItem`]s:
//!
//! - **Rule items.** Walking down from the home folder, the first folder (or
//!   recorded file) whose path matches a rule's `match.path`, and holds what
//!   its `match.requires` names, becomes one item. Everything inside belongs
//!   to it; the walk does not match rules inside an item.
//! - **Most specific rule wins**: the one whose `match.path` has the longest
//!   literal prefix. Two rules that are equally specific and could match the
//!   same path *tie*. Ties are an error ([`check_ties`] logs them at load
//!   time) and tied rules never classify anything: what they match is left
//!   as unknown with tier `keep`.
//! - **Unknown items.** Space no rule describes is grouped by top-level
//!   folder (`Documents`, `Library`, ...) as one `unknown` item each, with
//!   tier `keep`. Files directly in the home folder are not grouped.
//! - **Mode.** In simple mode, developer-mode rules are left out, so what
//!   they describe counts as unknown.
//! - **Only ever more careful.** Each item is checked against the safety
//!   crate's protected and review-only lists, for git repositories, for
//!   folders the scan could not read, and for other rules matching inside
//!   it. Any of these can make the tier more careful than the rule's (see
//!   [`Lowered`]); nothing makes it less careful. A `keep` item always has
//!   action `show_only` and `regenerable = false`.
//!
//! The output order is fixed: size descending, then id.
//!
//! The scan only lists files by name in folders it was asked about. Pass
//! [`record_files`] for the same rules and mode as
//! [`scanner::ScanOptions::record_files`], or rules that match files (such
//! as `Downloads/*.dmg`) and rules with `match.requires` match nothing.

mod compiled;
mod item;
mod live;
mod walk;

use std::collections::HashMap;
use std::sync::Arc;

use rules::{Action, Category, Confidence, FoldedPath, Glob, Mode, Rule, Tier};
use scanner::{DirId, RecordFiles, ScanResult};

pub use compiled::{check_ties, RuleTie};
pub use item::{ClassifiedItem, ItemId, ItemKind, Lowered, UNKNOWN_RULE_ID, UNKNOWN_RULE_VERSION};
pub use live::{LiveKind, LiveMatcher, Look};

/// Classifies everything in `scan` with `rules`, as shown in `mode`.
///
/// Pure: reads only `scan` and `rules`. Returns items sorted by size
/// (largest first), then by id.
pub fn classify(scan: &ScanResult, rules: &[Rule], mode: Mode) -> Vec<ClassifiedItem> {
    let compiled = compiled::compile(rules, mode);
    let ctx = walk::Ctx {
        scan,
        rules: &compiled,
    };
    let found = walk::find_items(&ctx);
    let mut claimed: HashMap<DirId, u64> = HashMap::new();
    for f in &found {
        if let Some(top) = f.top {
            *claimed.entry(top).or_default() += f.item.size_bytes;
        }
    }
    let mut items: Vec<ClassifiedItem> = found.into_iter().map(|f| f.item).collect();
    items.extend(unknown_items(scan, &claimed));
    items.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| a.id.cmp(&b.id))
    });
    items
}

/// One `unknown` item per top-level folder, for the space that no rule item
/// inside it accounts for. Folders with nothing left over are skipped.
fn unknown_items(scan: &ScanResult, claimed: &HashMap<DirId, u64>) -> Vec<ClassifiedItem> {
    let root = scan.root_dir();
    let mut items = Vec::new();
    for &top in &root.children {
        let Some(dir) = scan.dirs.get(top) else {
            continue;
        };
        let rest = dir
            .total_bytes
            .saturating_sub(claimed.get(&top).copied().unwrap_or(0));
        if rest == 0 {
            continue;
        }
        let Some(path) = scan.path_of(top) else {
            continue;
        };
        let name = dir.name.to_string_lossy();
        items.push(ClassifiedItem {
            id: ItemId::new(UNKNOWN_RULE_ID, dir.name.as_encoded_bytes()),
            rule_id: UNKNOWN_RULE_ID.to_string(),
            rule_version: UNKNOWN_RULE_VERSION,
            title: format!("Other items in {name}"),
            explain: "Files and folders the app does not recognise yet.".to_string(),
            risk: "These may be your own files, so the app does not offer to remove them."
                .to_string(),
            restore: None,
            category: Category::Unknown,
            tier: Tier::Keep,
            confidence: Confidence::Low,
            regenerable: false,
            action: Action::ShowOnly,
            command: None,
            kind: ItemKind::Folder,
            size_bytes: rest,
            modified: dir.modified,
            last_changed: dir.modified,
            path,
            lowered: Vec::new(),
        });
    }
    items
}

/// Tells the scanner which folders' files the rules need to see by name:
/// the folders a rule's `match.path` can name a file in, and the folders
/// inside a matched item that its `match.requires` can name a file in.
///
/// Use the same rules and mode as for [`classify`].
pub fn record_files(rules: &[Rule], mode: Mode) -> RecordFiles {
    let mut globs = Vec::new();
    let mut root = false;
    for c in compiled::compile(rules, mode) {
        let path = &c.rule.matcher.path;
        match parent_pattern(path) {
            Some(parent) => globs.extend(Glob::parse(&parent).ok()),
            None => root = true,
        }
        for req in &c.rule.matcher.requires {
            let folder = match parent_pattern(req) {
                Some(parent) => format!("{path}/{parent}"),
                None => path.clone(),
            };
            globs.extend(Glob::parse(&folder).ok());
        }
    }
    RecordFiles(Arc::new(move |rel: &std::path::Path| {
        if rel.as_os_str().is_empty() {
            return root;
        }
        let Some(text) = rel.to_str() else {
            return false;
        };
        let folded = FoldedPath::new(text);
        globs.iter().any(|g| g.matches_folded(&folded))
    }))
}

/// The folder part of a pattern: everything before the last `/`. A pattern
/// ending in `**` can name a file in any folder it reaches, so it is its own
/// folder part. `None` means the pattern names something directly in its
/// base folder.
fn parent_pattern(pattern: &str) -> Option<String> {
    if pattern == "**" || pattern.ends_with("/**") {
        return Some(pattern.to_string());
    }
    pattern
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_patterns() {
        assert_eq!(
            parent_pattern("Downloads/*.dmg").as_deref(),
            Some("Downloads")
        );
        assert_eq!(parent_pattern("a/**").as_deref(), Some("a/**"));
        assert_eq!(parent_pattern("Info.plist"), None);
    }

    #[test]
    fn ids_depend_on_rule_and_path_only() {
        let a = ItemId::new("xcode.derived_data", b"Library/x");
        assert_eq!(a, ItemId::new("xcode.derived_data", b"Library/x"));
        assert_ne!(a, ItemId::new("xcode.derived_data", b"Library/y"));
        assert_ne!(a, ItemId::new("other", b"Library/x"));
        assert_eq!(a.as_str().len(), 32);
    }
}
