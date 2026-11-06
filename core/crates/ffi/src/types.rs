//! The records and enums the app sees. See `docs/command-contract.md`.
//!
//! These mirror the core's own types but carry no paths, except
//! [`ItemDetail::path`], which is a display string filled in only in
//! Developer mode.

use classifier::Lowered;

/// Opaque id for one scan. Starts at 1 and is never reused while the
/// [`crate::RoomyCore`] lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ScanHandle {
    /// The scan's number.
    pub id: u64,
}

/// Which view of the Mac the user wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Mode {
    /// Everyday items in plain words.
    Simple,
    /// Adds developer rules and shows paths in item details.
    Developer,
}

/// How the app presents an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Tier {
    /// Can be preselected for removal.
    LikelySafe,
    /// Shown, but the user must look first.
    ReviewFirst,
    /// Shown for information only.
    Keep,
}

/// What kind of item it is. The rule categories from the rule schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, uniffi::Enum)]
pub enum Category {
    /// App and system caches.
    Cache,
    /// Build output.
    Build,
    /// Developer tools data, such as simulators.
    DeveloperTools,
    /// Docker data.
    Docker,
    /// Data kept by an app.
    AppData,
    /// Log files.
    Logs,
    /// Downloaded files.
    Downloads,
    /// The user's own files.
    UserData,
    /// Backups.
    Backups,
    /// System data.
    System,
    /// Not known.
    Unknown,
}

/// How sure the rule is about what the item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Confidence {
    /// Very sure.
    High,
    /// Fairly sure.
    Medium,
    /// Not sure. Never `likely_safe`.
    Low,
}

/// What the app may later offer for the item. Nothing acts on it in T09.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Action {
    /// Move it to the Trash.
    Trash,
    /// Run one allow-listed command.
    Command,
    /// Show it only.
    ShowOnly,
}

/// Order for [`crate::RoomyCore::list_items`]. Ties are broken by item id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Sort {
    /// Size, largest first.
    LargestFirst,
    /// Size, smallest first.
    SmallestFirst,
    /// Title A to Z, ignoring case.
    Title,
}

/// Where a scan has got to.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ScanPhase {
    /// Walking the home folder and adding up sizes.
    Scanning,
    /// Matching what was found to rules.
    Classifying,
    /// Done. Results are ready.
    Finished,
    /// Stopped by `cancel_scan`.
    Cancelled,
    /// Could not run. `reason` is plain words with no path.
    Failed {
        /// Why, ready to show.
        reason: String,
    },
}

/// Live numbers while a scan runs.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ScanProgress {
    /// Allocated bytes counted so far.
    pub bytes_seen: u64,
    /// Files counted so far (hard links once).
    pub items_seen: u64,
    /// Where the scan has got to.
    pub phase: ScanPhase,
}

/// Size of one category, and how much of it is likely safe.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CategoryTotal {
    /// The category.
    pub category: Category,
    /// Sum of the sizes of every item in it.
    pub bytes: u64,
    /// The part of `bytes` that is `likely_safe`.
    pub likely_safe_bytes: u64,
    /// Number of items in it.
    pub item_count: u64,
}

/// One row in a list. Never carries a path.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ItemSummary {
    /// Stable opaque id (32 hex characters).
    pub id: String,
    /// Short display name.
    pub title: String,
    /// What it is, in plain words.
    pub explain: String,
    /// Allocated bytes on disk.
    pub size_bytes: u64,
    /// Tier after safety adjustments.
    pub tier: Tier,
    /// What kind of item it is.
    pub category: Category,
}

/// What the Overview screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Overview {
    /// Exact sum of the sizes of every `likely_safe` item (F2).
    pub likely_safe_bytes: u64,
    /// Allocated bytes in the whole home folder.
    pub scanned_bytes: u64,
    /// Size of the volume holding the home folder.
    pub disk_total: u64,
    /// Space available to the user on that volume.
    pub disk_free: u64,
    /// One entry per category with items, largest first.
    pub categories: Vec<CategoryTotal>,
    /// Up to 8 of the largest `likely_safe` and `review_first` items.
    pub top_items: Vec<ItemSummary>,
    /// Folders the scan could not read.
    pub unreadable_count: u64,
}

/// Why the core made an item's tier more careful than its rule's.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Caution {
    /// The item is, or holds, a protected place.
    Protected,
    /// The item is, or holds, something the user must look at first.
    ReviewOnly,
    /// Another rule matches something inside it.
    NestedRule {
        /// The other rule's id.
        rule_id: String,
    },
    /// Two rules equally describe something inside it.
    RulesDisagree,
    /// The rule is not sure.
    LowConfidence,
    /// Part of it could not be read during the scan.
    Unreadable,
    /// A name inside it could not be checked.
    UnusualName,
}

/// Everything the detail panel shows for one item.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ItemDetail {
    /// Stable opaque id.
    pub id: String,
    /// Short display name.
    pub title: String,
    /// What it is and why it exists.
    pub explain: String,
    /// What could go wrong if it is removed.
    pub risk: String,
    /// How it comes back, if it does.
    pub restore: Option<String>,
    /// The rule that matched, or `unknown`.
    pub rule_id: String,
    /// That rule's version; 0 for `unknown`.
    pub rule_version: u32,
    /// How sure the rule is.
    pub confidence: Confidence,
    /// Tier after safety adjustments.
    pub tier: Tier,
    /// What kind of item it is.
    pub category: Category,
    /// Allocated bytes on disk.
    pub size_bytes: u64,
    /// Whether it comes back by itself. Always `false` for `keep`.
    pub regenerable: bool,
    /// What the app may later offer. Always `show_only` for `keep`.
    pub action: Action,
    /// Allow-listed command id when `action` is `command`.
    pub command: Option<String>,
    /// Why the tier is more careful than the rule's.
    pub cautions: Vec<Caution>,
    /// Developer mode only: the place as a display string starting `~`.
    /// Always `None` in simple mode. Display only; never sent back.
    pub path: Option<String>,
}

impl From<Mode> for rules::Mode {
    fn from(m: Mode) -> Self {
        match m {
            Mode::Simple => rules::Mode::Simple,
            Mode::Developer => rules::Mode::Developer,
        }
    }
}

impl From<rules::Tier> for Tier {
    fn from(t: rules::Tier) -> Self {
        match t {
            rules::Tier::LikelySafe => Tier::LikelySafe,
            rules::Tier::ReviewFirst => Tier::ReviewFirst,
            rules::Tier::Keep => Tier::Keep,
        }
    }
}

impl From<rules::Category> for Category {
    fn from(c: rules::Category) -> Self {
        use rules::Category as R;
        match c {
            R::Cache => Category::Cache,
            R::Build => Category::Build,
            R::DeveloperTools => Category::DeveloperTools,
            R::Docker => Category::Docker,
            R::AppData => Category::AppData,
            R::Logs => Category::Logs,
            R::Downloads => Category::Downloads,
            R::UserData => Category::UserData,
            R::Backups => Category::Backups,
            R::System => Category::System,
            R::Unknown => Category::Unknown,
        }
    }
}

impl From<rules::Confidence> for Confidence {
    fn from(c: rules::Confidence) -> Self {
        match c {
            rules::Confidence::High => Confidence::High,
            rules::Confidence::Medium => Confidence::Medium,
            rules::Confidence::Low => Confidence::Low,
        }
    }
}

impl From<rules::Action> for Action {
    fn from(a: rules::Action) -> Self {
        match a {
            rules::Action::Trash => Action::Trash,
            rules::Action::Command => Action::Command,
            rules::Action::ShowOnly => Action::ShowOnly,
        }
    }
}

impl From<&Lowered> for Caution {
    fn from(l: &Lowered) -> Self {
        match l {
            Lowered::Protected(_) => Caution::Protected,
            Lowered::ReviewOnly(_) => Caution::ReviewOnly,
            Lowered::NestedRule { rule_id } => Caution::NestedRule {
                rule_id: rule_id.clone(),
            },
            Lowered::RulesDisagree => Caution::RulesDisagree,
            Lowered::LowConfidence => Caution::LowConfidence,
            Lowered::Unreadable => Caution::Unreadable,
            Lowered::UnusualName => Caution::UnusualName,
        }
    }
}
