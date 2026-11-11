//! A careful, path-only guess at what a folder is, for showing findings
//! while a scan is still running. Never touches the filesystem.
//!
//! [`classify`](crate::classify) is the only thing that decides what an
//! item is, how big it is and how safe it is, and it needs the whole scan.
//! [`LiveMatcher`] answers a smaller question from a folder's path alone:
//! "will this folder very probably turn out to be an item of this rule?".
//! When it cannot be sure it says nothing:
//!
//! - a rule with `match.requires` needs to see inside the folder, so
//!   whatever its `match.path` matches is skipped;
//! - tied rules, and two equally specific rules, are skipped as in
//!   [`classify`](crate::classify);
//! - a rule whose own tier is `keep` is skipped: there is nothing to do
//!   about its items;
//! - anything the safety crate's name-based lists protect is skipped.
//!
//! The answer carries no tier and no path. The checks that need the scan
//! (git repositories, unreadable folders, what else is inside) only run
//! in [`classify`](crate::classify), which may still keep the folder.

use std::path::Path;

use rules::{Category, FoldedPath, Glob, Mode, Rule, Tier};
use safety::protected::{hard_block, review_only};

use crate::compiled::compile;

/// One rule, cut down to what a path-only match needs.
#[derive(Debug)]
struct LiveRule {
    glob: Glob,
    specificity: usize,
    /// The rule can only be decided by the full classifier.
    unsure: bool,
    keep: bool,
    title: String,
    category: Category,
}

/// What a folder very probably is, from its path alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveKind {
    /// The rule's title.
    pub title: String,
    /// The rule's category.
    pub category: Category,
}

/// What [`LiveMatcher::look`] says about one folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    /// No rule matches the folder. Something inside it may match.
    Descend,
    /// A rule matches, but only the full classifier can say what the
    /// folder is. Nothing inside it should be looked at either.
    Skip,
    /// The folder very probably becomes an item of this kind.
    Found(LiveKind),
}

/// Matches folder paths to rules while a scan runs. Owns what it needs, so
/// it can be handed to the scanner's worker threads.
#[derive(Debug)]
pub struct LiveMatcher {
    rules: Vec<LiveRule>,
}

impl LiveMatcher {
    /// Prepares the rules shown in `mode`. Use the same rules and mode as
    /// for [`classify`](crate::classify).
    pub fn new(rules: &[Rule], mode: Mode) -> Self {
        let rules = compile(rules, mode)
            .into_iter()
            .map(|c| LiveRule {
                specificity: c.specificity,
                unsure: c.tied || !c.requires.is_empty(),
                keep: c.rule.tier == Tier::Keep,
                title: c.rule.title.clone(),
                category: c.rule.category,
                glob: c.glob,
            })
            .collect();
        Self { rules }
    }

    /// Looks at one folder, given its path relative to the home folder.
    ///
    /// Ask from the top down, and do not ask about folders inside one that
    /// was answered [`Look::Skip`] or [`Look::Found`]: the first folder a
    /// rule matches is the item, as in [`classify`](crate::classify).
    pub fn look(&self, rel: &Path) -> Look {
        // No rule can be checked against a name that is not valid UTF-8,
        // and the classifier does not look inside such a folder.
        let Some(names) = names_of(rel) else {
            return Look::Skip;
        };
        let path = FoldedPath::from_names(&names);
        let matching: Vec<&LiveRule> = self
            .rules
            .iter()
            .filter(|r| r.glob.matches_folded(&path))
            .collect();
        if matching.is_empty() {
            return Look::Descend;
        }
        if matching.iter().any(|r| r.unsure) {
            return Look::Skip;
        }
        let top = matching.iter().map(|r| r.specificity).max().unwrap_or(0);
        let mut best = matching.iter().filter(|r| r.specificity == top);
        match (best.next(), best.next()) {
            (Some(rule), None) if !rule.keep && !protected(&names) => Look::Found(LiveKind {
                title: rule.title.clone(),
                category: rule.category,
            }),
            _ => Look::Skip,
        }
    }
}

/// The path's names, or `None` if it is empty or a name is not UTF-8.
fn names_of(rel: &Path) -> Option<Vec<String>> {
    let names: Option<Vec<String>> = rel
        .iter()
        .map(|name| name.to_str().map(str::to_string))
        .collect();
    names.filter(|n| !n.is_empty())
}

/// Whether the safety crate's name-based lists make the full classifier
/// keep whatever is at this path.
fn protected(names: &[String]) -> bool {
    let rel: Vec<&str> = names.iter().map(String::as_str).collect();
    hard_block(&rel).is_some() || review_only(&rel).is_some_and(|r| !r.deletable_in_v1())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn protected_places_are_recognised_by_name() {
        assert!(protected(&names(&[".ssh"])));
        assert!(protected(&names(&[".ssh", "cache"])));
        assert!(!protected(&names(&["Library", "Caches", "com.example"])));
    }

    #[test]
    fn an_empty_or_unreadable_name_is_skipped() {
        assert_eq!(names_of(Path::new("")), None);
        assert_eq!(
            names_of(Path::new("a/b")),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }
}
