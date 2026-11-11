//! What a running scan has found so far, for the scanning screen.
//!
//! The scanner tells us about a folder as soon as it has read everything
//! inside it, if the classifier's [`LiveMatcher`] is sure from the path
//! alone what the folder is. We keep only a tally per rule title: no
//! paths, no tiers and no item ids, so nothing here can be chosen for
//! cleanup. The finished scan is classified from scratch and is the only
//! source of items.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use classifier::{LiveKind, LiveMatcher, Look};
use rules::Rule;
use scanner::{Watch, Watched};

use crate::types::{Category, ScanFinding};

/// Folders of one kind, added up.
#[derive(Debug, Clone, Copy)]
struct Tally {
    category: Category,
    count: u32,
    bytes: u64,
}

/// The tallies of one scan, shared by its walk threads and the command
/// that reads them.
#[derive(Debug, Default, Clone)]
pub(crate) struct Findings {
    by_title: Arc<Mutex<HashMap<String, Tally>>>,
}

impl Findings {
    /// Adds one folder of `kind` holding `bytes`.
    fn add(&self, kind: &LiveKind, bytes: u64) {
        let mut tallies = lock(&self.by_title);
        let tally = tallies.entry(kind.title.clone()).or_insert(Tally {
            category: kind.category.into(),
            count: 0,
            bytes: 0,
        });
        tally.count = tally.count.saturating_add(1);
        tally.bytes = tally.bytes.saturating_add(bytes);
    }

    /// Everything found so far: largest first, then by title.
    pub(crate) fn list(&self) -> Vec<ScanFinding> {
        let mut list: Vec<ScanFinding> = lock(&self.by_title)
            .iter()
            .map(|(title, tally)| ScanFinding {
                title: title.clone(),
                category: tally.category,
                count: tally.count,
                size_bytes: tally.bytes,
            })
            .collect();
        list.sort_by(|a, b| {
            b.size_bytes
                .cmp(&a.size_bytes)
                .then_with(|| a.title.cmp(&b.title))
        });
        list
    }

    /// The scanner watch that fills these tallies during a full scan.
    pub(crate) fn watch(&self, rules: &[Rule], mode: rules::Mode) -> Watch {
        let matcher = LiveMatcher::new(rules, mode);
        // What each watched folder is, from when it was asked about until
        // the walk has finished reading it.
        let waiting: Arc<Mutex<HashMap<std::path::PathBuf, LiveKind>>> = Arc::default();
        let (asked, findings) = (Arc::clone(&waiting), self.clone());
        Watch {
            ask: Arc::new(move |rel: &Path| match matcher.look(rel) {
                Look::Descend => Watched::Descend,
                Look::Skip => Watched::Skip,
                Look::Found(kind) => {
                    lock(&asked).insert(rel.to_path_buf(), kind);
                    Watched::Report
                }
            }),
            found: Arc::new(move |rel: &Path, bytes: u64| {
                let kind = lock(&waiting).remove(rel);
                if let Some(kind) = kind {
                    findings.add(&kind, bytes);
                }
            }),
        }
    }
}

/// Locks a mutex, carrying on if a thread panicked while holding it: a
/// tally is valid at all times.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(title: &str) -> LiveKind {
        LiveKind {
            title: title.to_string(),
            category: rules::Category::Cache,
        }
    }

    #[test]
    fn folders_of_one_kind_are_added_up_and_the_largest_kind_comes_first() {
        let findings = Findings::default();
        findings.add(&kind("Small"), 10);
        findings.add(&kind("Big"), 300);
        findings.add(&kind("Small"), 15);
        let list = findings.list();
        let rows: Vec<(&str, u32, u64)> = list
            .iter()
            .map(|f| (f.title.as_str(), f.count, f.size_bytes))
            .collect();
        assert_eq!(rows, [("Big", 1, 300), ("Small", 2, 25)]);
        assert_eq!(list[0].category, Category::Cache);
    }

    #[test]
    fn nothing_found_is_an_empty_list() {
        assert!(Findings::default().list().is_empty());
    }
}
