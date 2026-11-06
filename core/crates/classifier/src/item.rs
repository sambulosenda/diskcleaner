//! What the classifier returns.

use std::fmt;
use std::path::PathBuf;

use rules::{Action, Category, Confidence, Tier};
use safety::protected::{BlockReason, ReviewReason};
use sha2::{Digest, Sha256};

/// The rule id given to space that no rule describes.
pub const UNKNOWN_RULE_ID: &str = "unknown";

/// The rule version given to space that no rule describes. Real rules start
/// at 1, so 0 always means "no rule".
pub const UNKNOWN_RULE_VERSION: u32 = 0;

/// A stable item id: the first 16 bytes of
/// `SHA-256(rule id, 0x00, path relative to home)`, in lowercase hex.
///
/// The same rule matching the same relative path always gives the same id,
/// whichever folder the home is in and however often it is scanned.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(String);

impl ItemId {
    /// Builds the id for `rule_id` matching `rel`, the path's bytes relative
    /// to home with `/` between names.
    pub fn new(rule_id: &str, rel: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(rule_id.as_bytes());
        hasher.update([0u8]);
        hasher.update(rel);
        let digest = hasher.finalize();
        let hex = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        Self(hex)
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether an item is a folder or a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A folder and everything in it.
    Folder,
    /// One plain file.
    File,
}

/// Why the classifier gave an item a more careful tier than its rule asks
/// for. The classifier only ever makes a tier more careful, never less.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lowered {
    /// The item is, or holds, a hard-blocked path. Tier becomes `keep`.
    Protected(BlockReason),
    /// The item is, or holds, a review-only path. Tier becomes at most
    /// `review_first`, or `keep` if the app may not remove it in v1.
    ReviewOnly(ReviewReason),
    /// Another rule matches something inside the item. Tier becomes at most
    /// that rule's tier.
    NestedRule {
        /// The other rule.
        rule_id: String,
    },
    /// Two equally specific rules both describe something inside the item,
    /// so nobody can say what it is. Tier becomes `keep`.
    RulesDisagree,
    /// The rule has low confidence, which is never `likely_safe`.
    LowConfidence,
    /// Part of the item could not be read during the scan, so what is in
    /// it is not known. Tier becomes at most `review_first`.
    Unreadable,
    /// A name inside the item is not valid UTF-8, so it could not be
    /// checked. Tier becomes at most `review_first`.
    UnusualName,
}

/// One classified item: a matched folder or file, or the unexplained rest of
/// a top-level folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedItem {
    /// Stable id. The UI refers to items only by this.
    pub id: ItemId,
    /// The rule that matched, or [`UNKNOWN_RULE_ID`].
    pub rule_id: String,
    /// That rule's version, or [`UNKNOWN_RULE_VERSION`].
    pub rule_version: u32,
    /// Short display name.
    pub title: String,
    /// What it is, in plain words.
    pub explain: String,
    /// What could go wrong if it is removed.
    pub risk: String,
    /// How it comes back, if it does.
    pub restore: Option<String>,
    /// What kind of item it is.
    pub category: Category,
    /// How the app presents it, after any [`Lowered`] adjustments.
    pub tier: Tier,
    /// How sure the rule is.
    pub confidence: Confidence,
    /// Whether it comes back by itself. Always `false` for `keep` items.
    pub regenerable: bool,
    /// What the app may do with it. Always `show_only` for `keep` items.
    pub action: Action,
    /// Allow-listed command id when `action` is `command`.
    pub command: Option<String>,
    /// Folder or file.
    pub kind: ItemKind,
    /// Allocated bytes on disk.
    pub size_bytes: u64,
    /// Absolute path. **Core-internal only**: never send it to the UI. The
    /// planner and executor must validate it again before acting.
    pub path: PathBuf,
    /// Why the tier is more careful than the rule's, if it is.
    pub lowered: Vec<Lowered>,
}

/// How careful a tier is: higher is more careful.
pub(crate) fn caution(tier: Tier) -> u8 {
    match tier {
        Tier::LikelySafe => 0,
        Tier::ReviewFirst => 1,
        Tier::Keep => 2,
    }
}
