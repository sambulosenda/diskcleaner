//! Checks a rule's `match.path` against the protected lists in the `safety`
//! crate. The lists themselves live only in `safety::protected`; nothing
//! here adds to them or loosens them.
//!
//! Two checks run for each list, and either one refusing is enough:
//!
//! 1. **Location check.** For every fixed protected location, could the glob
//!    match that location, anything inside it (when the whole subtree is
//!    protected), or a folder that holds it? Wildcards are taken at their
//!    widest, so `Library/*` is refused because it could match
//!    `Library/Keychains`.
//! 2. **Plain-reading check.** The plainest path the glob matches (see
//!    [`Glob::witness`]) is passed to the safety crate's own name checks.
//!    This catches rules aimed at protected *kinds* of item wherever they
//!    are, such as `Projects/*.p12` or `Pictures/*.photoslibrary`.
//!
//! The second check uses one reading on purpose: taken at its widest, `*`
//! could match a file called `x.key`, which would refuse every rule with a
//! wildcard. Protected kinds of item matched only by a wildcard are still
//! refused at clean-up time by `safety::validate`, which checks every real
//! path, so this is a first line of defence rather than the only one.
//!
//! What cannot be checked here: "any directory that contains a `.git`
//! directory" depends on what is on disk, so only `safety::validate` can
//! enforce it.

use safety::protected::{
    self, BlockReason, Entry, ReviewReason, Scope, HARD_BLOCKED, MESSAGES_DB_MARKER,
    MESSAGES_FOLDER, REVIEW_ONLY,
};

use crate::glob::{CharTok, Glob, PathTok};

/// The hard-block reason the glob could run into, if any.
pub(crate) fn hard_block(glob: &Glob) -> Option<BlockReason> {
    for entry in HARD_BLOCKED {
        if let Some(reason) = entry_hit(glob, entry) {
            return Some(reason);
        }
    }
    if glob.could_overlap(&messages_database_target()) {
        return Some(BlockReason::MessagesDatabase);
    }
    let witness = glob.witness();
    let names: Vec<&str> = witness.iter().map(String::as_str).collect();
    protected::hard_block(&names)
}

/// The review-only reason the glob could run into, if any.
pub(crate) fn review_only(glob: &Glob) -> Option<ReviewReason> {
    for entry in REVIEW_ONLY {
        if let Some(reason) = entry_hit(glob, entry) {
            return Some(reason);
        }
    }
    let witness = glob.witness();
    let names: Vec<&str> = witness.iter().map(String::as_str).collect();
    protected::review_only(&names)
}

/// Whether the glob could match the entry's location, anything the entry
/// covers, or a folder that holds the entry.
fn entry_hit<R: Copy>(glob: &Glob, entry: &Entry<R>) -> Option<R> {
    let base: Vec<PathTok> = entry
        .components
        .iter()
        .map(|c| PathTok::literal(c))
        .collect();
    let mut covered = vec![base.clone()];
    match entry.scope {
        Scope::Itself => {}
        Scope::Subtree => covered.push(with(&base, PathTok::AnyDepth)),
        Scope::ItselfAndChildren => {
            covered.push(with(&base, PathTok::Name(vec![CharTok::Star])));
        }
    }
    if covered.iter().any(|t| glob.could_overlap(t)) {
        return Some(entry.reason);
    }
    // A folder that holds the entry: removing it would remove the entry.
    let holds = (0..base.len()).any(|k| glob.could_overlap(&base[..k]));
    holds.then_some(entry.reason)
}

/// `base` followed by one more token.
fn with(base: &[PathTok], last: PathTok) -> Vec<PathTok> {
    let mut v = base.to_vec();
    v.push(last);
    v
}

/// `Library/Messages/**/*<marker>*/**`: any name containing `.db` at any
/// depth under Messages, the way `safety::protected::hard_block` reads it.
fn messages_database_target() -> Vec<PathTok> {
    let mut name = vec![CharTok::Star];
    name.extend(
        protected::fold(MESSAGES_DB_MARKER)
            .chars()
            .map(CharTok::Lit),
    );
    name.push(CharTok::Star);
    let mut target: Vec<PathTok> = MESSAGES_FOLDER
        .iter()
        .map(|c| PathTok::literal(c))
        .collect();
    target.push(PathTok::AnyDepth);
    target.push(PathTok::Name(name));
    target.push(PathTok::AnyDepth);
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hard(p: &str) -> Option<BlockReason> {
        hard_block(&Glob::parse(p).unwrap())
    }

    fn review(p: &str) -> Option<ReviewReason> {
        review_only(&Glob::parse(p).unwrap())
    }

    #[test]
    fn schema_examples_are_not_blocked() {
        assert_eq!(hard("Library/Developer/Xcode/DerivedData/*"), None);
        assert_eq!(
            hard("Library/Application Support/MobileSync/Backup/*"),
            None
        );
        assert_eq!(hard("Library/Developer/CoreSimulator/Devices/*"), None);
        assert_eq!(review("Library/Developer/Xcode/DerivedData/*"), None);
        assert_eq!(review("Library/Developer/CoreSimulator/Devices/*"), None);
    }

    #[test]
    fn fixed_locations_are_blocked() {
        assert_eq!(hard("Library/Keychains/*"), Some(BlockReason::Keychains));
        assert_eq!(hard("Library/*"), Some(BlockReason::Keychains));
        assert_eq!(hard("*"), Some(BlockReason::MainFolder));
        assert_eq!(hard("Library"), Some(BlockReason::MainFolder));
        assert_eq!(hard("**"), Some(BlockReason::HomeFolder));
        assert_eq!(hard("**/node_modules"), Some(BlockReason::Keychains));
        assert_eq!(hard(".ssh"), Some(BlockReason::Credentials));
        assert_eq!(hard(".docker"), Some(BlockReason::Credentials));
        assert_eq!(hard(".config/*"), Some(BlockReason::Credentials));
        assert_eq!(hard("library/mail/v10"), Some(BlockReason::Mail));
    }

    #[test]
    fn named_kinds_are_blocked() {
        assert_eq!(
            hard("Pictures/*.photoslibrary"),
            Some(BlockReason::MediaLibrary)
        );
        assert_eq!(hard("Projects/*.p12"), Some(BlockReason::SigningMaterial));
        assert_eq!(hard("Projects/*/.git"), Some(BlockReason::GitRepository));
        assert_eq!(
            hard("Library/Messages/*"),
            Some(BlockReason::MessagesDatabase)
        );
    }

    #[test]
    fn review_only_locations() {
        assert_eq!(review("Downloads/*.dmg"), Some(ReviewReason::PersonalFile));
        assert_eq!(
            review("Library/Containers/*"),
            Some(ReviewReason::AppLeftovers)
        );
        assert_eq!(
            review("Library/Application Support/MobileSync/Backup/*"),
            Some(ReviewReason::DeviceBackup)
        );
        assert_eq!(review("Projects/*/.env"), Some(ReviewReason::EnvFile));
        assert_eq!(review("Library/Caches/com.apple.Safari"), None);
        assert_eq!(review("Library/Application Support/Slack/Cache"), None);
    }
}
