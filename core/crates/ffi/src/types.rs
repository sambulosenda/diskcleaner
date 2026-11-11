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
    /// Which one it is, when one rule finds many items with the same title:
    /// for example the project folder's name for a `node_modules` folder, or
    /// the emulator's name. A single folder or file name, never a path.
    /// Developer mode only; `None` in Simple mode and for one-of-a-kind items.
    #[uniffi(default = None)]
    pub place: Option<String>,
    /// Roughly when it last changed: the newest modification time of the
    /// item or anything inside it the scan saw, in seconds since 1970 (UTC).
    /// Shown only, as "Changed 3 days ago"; never decides what is safe.
    #[uniffi(default = None)]
    pub last_changed_unix_seconds: Option<i64>,
}

/// What the Overview screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Overview {
    /// Exact sum of the sizes of every `likely_safe` item (F2).
    pub likely_safe_bytes: u64,
    /// Exact sum of the sizes of every `review_first` item. Shown beside the
    /// headline as what more could be freed after the user has looked.
    #[uniffi(default = 0)]
    pub review_first_bytes: u64,
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
    /// The same folders, counted by the private area they belong to. Most
    /// folders first, ties in `PrivateArea` order. Adds up to
    /// `unreadable_count`.
    pub unreadable_areas: Vec<UnreadableArea>,
    /// The scan read again only the folders that changed since the
    /// previous scan in this mode, and reused the rest (F12).
    #[uniffi(default = false)]
    pub quick_rescan: bool,
}

/// What grew or shrank since the previous scan in the same mode (F11).
/// Holds no paths.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ScanChanges {
    /// When the previous scan finished, in seconds since 1970 (UTC).
    pub previous_scan_at: i64,
    /// Allocated bytes in the whole home folder then.
    pub scanned_bytes_before: u64,
    /// Allocated bytes in the whole home folder now.
    pub scanned_bytes_now: u64,
    /// Categories that changed by at least 10 MB, biggest change first.
    pub categories: Vec<CategoryChange>,
    /// Items that changed by at least 10 MB, biggest change first, at most
    /// 20.
    pub items: Vec<ItemChange>,
}

/// One category's size then and now.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CategoryChange {
    /// The category.
    pub category: Category,
    /// Allocated bytes at the previous scan.
    pub bytes_before: u64,
    /// Allocated bytes now.
    pub bytes_now: u64,
}

/// How an item changed since the previous scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ChangeKind {
    /// Not there at the previous scan.
    New,
    /// Bigger than at the previous scan.
    Grew,
    /// Smaller than at the previous scan.
    Shrank,
    /// There at the previous scan, not now (for example, cleaned up).
    Gone,
}

/// One item's size then and now.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ItemChange {
    /// The item's id in this scan, to open its detail. `None` for an item
    /// that is gone.
    pub item_id: Option<String>,
    /// Its title.
    pub title: String,
    /// Its category.
    pub category: Category,
    /// How it changed.
    pub kind: ChangeKind,
    /// Allocated bytes at the previous scan (0 if new).
    pub bytes_before: u64,
    /// Allocated bytes now (0 if gone).
    pub bytes_now: u64,
}

/// A part of the home folder that macOS may keep private. The app turns it
/// into words; the core sends no paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, uniffi::Enum)]
pub enum PrivateArea {
    /// Mail's messages and attachments.
    Mail,
    /// Messages' chats and attachments.
    Messages,
    /// Safari's data.
    Safari,
    /// iPhone and iPad backups.
    DeviceBackups,
    /// Photos libraries.
    Photos,
    /// Contacts, Calendars and Reminders.
    ContactsAndCalendars,
    /// The Trash.
    Trash,
    /// Other apps' private data.
    AppData,
    /// Any other folder.
    Other,
}

impl From<scanner::privacy::Area> for PrivateArea {
    fn from(area: scanner::privacy::Area) -> Self {
        use scanner::privacy::Area;
        match area {
            Area::Mail => Self::Mail,
            Area::Messages => Self::Messages,
            Area::Safari => Self::Safari,
            Area::DeviceBackups => Self::DeviceBackups,
            Area::Photos => Self::Photos,
            Area::ContactsAndCalendars => Self::ContactsAndCalendars,
            Area::Trash => Self::Trash,
            Area::AppData => Self::AppData,
            Area::Other => Self::Other,
        }
    }
}

/// How many folders in one private area the scan could not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct UnreadableArea {
    /// The area.
    pub area: PrivateArea,
    /// Folders in it that could not be read.
    pub folder_count: u64,
}

/// Whether Roomy can read the folders macOS keeps private.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FullDiskAccess {
    /// Full Disk Access is on.
    Granted,
    /// Full Disk Access is off: some private folders could not be listed.
    NotGranted,
    /// None of the folders checked exist, so the core can't tell.
    Unknown,
}

impl From<scanner::privacy::Access> for FullDiskAccess {
    fn from(access: scanner::privacy::Access) -> Self {
        use scanner::privacy::Access;
        match access {
            Access::Granted => Self::Granted,
            Access::NotGranted => Self::NotGranted,
            Access::Unknown => Self::Unknown,
        }
    }
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
    /// For an iPhone or iPad backup only: which device it belongs to.
    /// Always `Some` for a backup, even if the details couldn't be read,
    /// so the app always shows its warning. `None` for everything else.
    pub device_backup: Option<DeviceBackup>,
}

/// What an iPhone or iPad backup's `Info.plist` says about the device, for
/// the warning before it goes to the Trash. Either part may be missing.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DeviceBackup {
    /// The device's name, cleaned to be safe to show.
    pub device_name: Option<String>,
    /// When the backup last finished, in seconds since 1970. Formatting
    /// happens in the app.
    pub last_backup_unix_seconds: Option<i64>,
}

/// Opaque id for one cleanup plan. Single use; expires ten minutes after
/// the preview that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct PlanHandle {
    /// The plan's number.
    pub id: u64,
}

/// One item the user ticked, as the app sends it. An id, never a path.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CleanupChoice {
    /// The item's id from `list_items` or `overview`.
    pub item_id: String,
    /// Whether the user ticked this item on its own, rather than as part of
    /// a group (such as a future "choose all"). Some items, and items holding
    /// things like an `.env` file, may only move when chosen on their own.
    pub chosen_alone: bool,
}

/// Whether one item would move.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum CleanupCheck {
    /// It passed every check and would move to the Trash.
    WillMove,
    /// It would not move. `reason` is plain words with no path.
    WontMove {
        /// Why, ready to show.
        reason: String,
    },
}

/// One row of the cleanup preview.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CleanupPreviewItem {
    /// The item's id.
    pub item_id: String,
    /// Short display name.
    pub title: String,
    /// Allocated bytes. For an item that would move, measured just now;
    /// otherwise the size from the scan.
    pub size_bytes: u64,
    /// Whether it would move, and if not, why.
    pub check: CleanupCheck,
    /// Developer mode only: the place as a display string starting `~`.
    /// Always `None` in simple mode. Display only; never sent back.
    pub path: Option<String>,
    /// For an iPhone or iPad backup only: which device it belongs to, as in
    /// [`ItemDetail::device_backup`].
    pub device_backup: Option<DeviceBackup>,
    /// What happens after it is removed: how it comes back, in the rule's
    /// own words, as in [`ItemDetail::restore`]. `None` if it does not come
    /// back by itself (it can still be put back from the Trash).
    pub restore: Option<String>,
}

/// The dry run: exactly what would move to the Trash. Nothing has moved.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CleanupPreview {
    /// The plan holding only the items that would move, or `None` if
    /// nothing would.
    pub plan: Option<PlanHandle>,
    /// One row per distinct ticked item, in the order sent.
    pub items: Vec<CleanupPreviewItem>,
    /// Sum of `size_bytes` of the items that would move.
    pub bytes_to_move: u64,
    /// How long the plan stays usable, in seconds.
    pub expires_in_seconds: u64,
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

/// Opaque id for one finished cleanup, used to put it back. Only the last
/// cleanup since the core was made can be put back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct CleanupHandle {
    /// The cleanup's number.
    pub id: u64,
}

/// What happened to one item of a cleanup, or of putting one back.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CleanupResultItem {
    /// The item's id.
    pub item_id: String,
    /// Short display name.
    pub title: String,
    /// Allocated bytes that moved (0 if it did not).
    pub size_bytes: u64,
    /// Whether it moved.
    pub done: bool,
    /// Why it did not, in plain words with no path. `None` when `done`.
    pub reason: Option<String>,
}

/// What a cleanup did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CleanupResult {
    /// The handle for putting this cleanup back, if anything moved.
    pub cleanup: Option<CleanupHandle>,
    /// One per item of the plan, in order.
    pub items: Vec<CleanupResultItem>,
    /// Sum of `size_bytes` of the items that moved.
    pub bytes_moved: u64,
    /// Whether the cleanup was saved to History. A cleanup still happens if
    /// History can't be written.
    pub saved_to_history: bool,
}

/// What putting a cleanup back did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PutBackResult {
    /// One per item that had moved, in order. `done` means it is back.
    pub items: Vec<CleanupResultItem>,
    /// Sum of `size_bytes` of the items put back.
    pub bytes_put_back: u64,
}

/// What happened to one item, as History shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum HistoryItemState {
    /// Moved to the Trash.
    Moved,
    /// Not moved.
    NotMoved {
        /// Why, in plain words.
        reason: String,
    },
    /// Moved, then put back.
    PutBack,
    /// Moved; putting it back was refused, so it is still in the Trash.
    StayedInTrash {
        /// Why, in plain words.
        reason: String,
    },
}

/// One item of a past cleanup. No path.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HistoryItem {
    /// Short display name.
    pub title: String,
    /// Allocated bytes.
    pub size_bytes: u64,
    /// What happened.
    pub state: HistoryItemState,
}

/// One past cleanup, as the History screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HistoryEntry {
    /// The entry's id in History.
    pub id: i64,
    /// When it ran, in seconds since 1970 (UTC).
    pub finished_at: i64,
    /// When it was put back, if it was.
    pub put_back_at: Option<i64>,
    /// Bytes that went to the Trash and were not put back.
    pub bytes_in_trash: u64,
    /// Its items, in the order they were cleaned.
    pub items: Vec<HistoryItem>,
}

impl From<history::ItemState> for HistoryItemState {
    fn from(s: history::ItemState) -> Self {
        match s {
            history::ItemState::Moved => Self::Moved,
            history::ItemState::NotMoved { reason } => Self::NotMoved { reason },
            history::ItemState::PutBack => Self::PutBack,
            history::ItemState::StayedInTrash { reason } => Self::StayedInTrash { reason },
        }
    }
}

impl From<history::Cleanup> for HistoryEntry {
    fn from(c: history::Cleanup) -> Self {
        let bytes_in_trash = c.bytes_in_trash();
        Self {
            id: c.id,
            finished_at: c.finished_at,
            put_back_at: c.put_back_at,
            bytes_in_trash,
            items: c
                .items
                .into_iter()
                .map(|i| HistoryItem {
                    title: i.title,
                    size_bytes: i.size_bytes,
                    state: i.state.into(),
                })
                .collect(),
        }
    }
}

/// Where an installed app is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AppLocation {
    /// The main `/Applications` folder. Roomy's safety rules keep it out of
    /// everything outside the home folder, so the user moves the app itself
    /// to the Trash in Finder; Roomy moves what it left behind.
    Applications,
    /// `~/Applications`. Roomy can move the app itself too.
    HomeApplications,
}

/// One installed app, as the uninstaller lists it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AppSummary {
    /// Id to send back to `review_app` and `preview_uninstall`. Valid until
    /// the next `list_apps`.
    pub app_id: String,
    /// The app's name.
    pub name: String,
    /// The app's version, if it says.
    pub version: Option<String>,
    /// The app's bundle id, for example `com.tinyspeck.slackmacgap`.
    pub bundle_id: String,
    /// Allocated bytes of the app itself.
    pub size_bytes: u64,
    /// Where the app is.
    pub location: AppLocation,
    /// The app bundle's location, for "Show in Finder". An app in an
    /// Applications folder is not personal, so this is given in both modes.
    /// Display only; never sent back.
    pub bundle_path: String,
    /// Whether the app (or anything inside it) is running now.
    pub is_open: bool,
}

/// One thing the uninstaller can move to the Trash for an app.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AppPart {
    /// Id to send to `preview_uninstall`.
    pub item_id: String,
    /// Short name, for example "Caches".
    pub title: String,
    /// What it is, in plain words.
    pub explain: String,
    /// Allocated bytes, measured just now.
    pub size_bytes: u64,
    /// Whether the app should tick it to begin with. Never true for data the
    /// app keeps for itself (Application Support, containers) or for
    /// anything matched less surely than by the bundle id.
    pub preselected: bool,
    /// Whether it was matched by the app's bundle id. When false, it was
    /// matched by a similar name and the user should check it.
    pub sure: bool,
    /// Developer mode only: the place as a display string starting `~`.
    pub path: Option<String>,
}

/// What the uninstaller found for one app.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AppReview {
    /// The app.
    pub app: AppSummary,
    /// The app itself (when it is in `~/Applications`) and what it left in
    /// the home folder, the app first, then largest first.
    pub parts: Vec<AppPart>,
    /// Plain words to show above the list, if there is something to say,
    /// for example that another copy of the app is installed.
    pub note: Option<String>,
}

/// What kind of Android part something is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AndroidKind {
    /// An emulator system image, for one Android version.
    SystemImage,
    /// One Android emulator (AVD), with what is inside it.
    Emulator,
    /// An SDK platform, what projects build against.
    Platform,
    /// One version of the build tools.
    BuildTools,
    /// One version of the NDK.
    Ndk,
    /// One Gradle version the Gradle wrapper downloaded.
    GradleVersion,
    /// Gradle's download and build cache.
    GradleCache,
}

/// Whether any Android project (or, for a system image, any emulator) that
/// Roomy found uses a part.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AndroidUsage {
    /// Named by these projects, or run by these emulators.
    Used { by: Vec<String> },
    /// No project names it, but these leave the choice to Android Studio and
    /// this is the newest one installed, so they probably use it.
    ProbablyUsed { by: Vec<String> },
    /// Roomy couldn't read a setting of these projects or emulators.
    NotSure { by: Vec<String> },
    /// Nothing Roomy found uses it.
    NotUsed,
    /// Not something projects name: an emulator, or the Gradle cache.
    NotApplicable,
}

/// One thing the Android screen can move to the Trash.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AndroidPart {
    /// Id to send to `preview_android_cleanup`. Valid until the next
    /// `android_overview`.
    pub item_id: String,
    /// What it is.
    pub kind: AndroidKind,
    /// Short name, for example "Android 14 (API 34) emulator image".
    pub title: String,
    /// A line under the title, for example "Google Play, for Apple silicon Macs".
    pub detail: Option<String>,
    /// What it is and what happens if it goes, in plain words.
    pub explain: String,
    /// Allocated bytes, measured just now.
    pub size_bytes: u64,
    /// Whether anything uses it.
    pub usage: AndroidUsage,
    /// `usage` in plain words, for example "Used by Shop". Empty for an emulator.
    pub usage_text: String,
    /// For an emulator: when it last ran (roughly), in seconds since 1970.
    pub last_used_unix_seconds: Option<i64>,
    /// Developer mode only: the place as a display string starting `~`.
    /// Display only; never sent back.
    pub path: Option<String>,
}

/// One Android project Roomy found.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AndroidProject {
    /// Its name (for a React Native or Flutter app, the app's folder name).
    pub name: String,
    /// Developer mode only: where it is, starting `~`. Display only.
    pub path: Option<String>,
    /// When its build files or top folder last changed, in seconds since 1970.
    pub last_activity_unix_seconds: Option<i64>,
}

/// Everything the Android screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AndroidOverview {
    /// Whether `~/Library/Android/sdk` exists.
    pub sdk_found: bool,
    /// Every part, emulator and Gradle download, by kind then newest first.
    /// Nothing is ever ticked for the user.
    pub parts: Vec<AndroidPart>,
    /// The Android projects found in the home folder, by name.
    pub projects: Vec<AndroidProject>,
    /// Allocated bytes of every part.
    pub total_bytes: u64,
    /// Allocated bytes of the parts nothing uses (`AndroidUsage::NotUsed`).
    pub unused_bytes: u64,
    /// Plain words to show above the list, if there is something to say.
    pub note: Option<String>,
}

/// Which of the Mac's own libraries a [`LibrarySize`] row is about. The
/// app names each one and points to the app that manages it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum LibraryKind {
    /// Photos libraries.
    Photos,
    /// The Music library and its media folder, and the older iTunes folder.
    Music,
    /// The TV library and its media folder.
    Tv,
    /// Mail's mailboxes and downloaded attachments.
    Mail,
    /// Photos, videos and files from Messages conversations.
    MessagesAttachments,
    /// Final Cut Pro libraries.
    FinalCut,
}

impl From<analysers::libraries::LibraryKind> for LibraryKind {
    fn from(k: analysers::libraries::LibraryKind) -> Self {
        use analysers::libraries::LibraryKind as A;
        match k {
            A::Photos => Self::Photos,
            A::Music => Self::Music,
            A::Tv => Self::Tv,
            A::Mail => Self::Mail,
            A::MessagesAttachments => Self::MessagesAttachments,
            A::FinalCut => Self::FinalCut,
        }
    }
}

/// How much space one kind of library takes, from the last scan. Shown
/// only: Roomy never cleans inside these libraries, and the row holds no
/// path.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibrarySize {
    /// Which library.
    pub kind: LibraryKind,
    /// Allocated bytes, counted once.
    pub bytes: u64,
    /// How many separate libraries were found (Photos, Music, TV, Final
    /// Cut). 0 for Mail and Messages attachments.
    pub library_count: u32,
    /// Whether anything of this kind was found.
    pub found: bool,
    /// Whether part of it couldn't be read (usually Full Disk Access is
    /// off), so it may really be larger, or exist when `found` is false.
    pub needs_access: bool,
}

impl From<&analysers::libraries::LibrarySize> for LibrarySize {
    fn from(s: &analysers::libraries::LibrarySize) -> Self {
        Self {
            kind: s.kind.into(),
            bytes: s.bytes,
            library_count: s.bundle_count,
            found: s.found,
            needs_access: s.needs_access,
        }
    }
}

/// One copy of a file the duplicate finder found.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DuplicateCopy {
    /// Id to send to `preview_duplicate_cleanup`.
    pub copy_id: String,
    /// The file's name.
    pub name: String,
    /// The folder it is in, in plain words, for example "Documents › Trips".
    pub location: String,
    /// When it was last changed, in seconds since 1970.
    pub modified_unix_seconds: i64,
    /// About how much moving it to the Trash and emptying the Trash frees.
    /// Less than the file's size for an APFS clone.
    pub frees_bytes: u64,
    /// Whether it shares most of its space with another file (an APFS
    /// clone, as Finder's Duplicate makes), so removing it frees little.
    pub shares_space: bool,
    /// Whether the user may choose it. When false, it can only be kept.
    pub can_move: bool,
    /// Why it can't move, when `can_move` is false. Never holds a path.
    pub stays_reason: Option<String>,
    /// Developer mode only: the place as a display string starting `~`.
    pub path: Option<String>,
}

/// Files with exactly the same content.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DuplicateGroup {
    /// Stable for this search, for the app's lists.
    pub group_id: String,
    /// The name of the oldest copy.
    pub name: String,
    /// Allocated bytes of one copy.
    pub size_bytes: u64,
    /// The most that removing copies could free while keeping one.
    pub could_free_bytes: u64,
    /// Two or more copies, oldest first. None is ever ticked to begin with.
    pub copies: Vec<DuplicateCopy>,
}

/// What a finished duplicate search found.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DuplicateReport {
    /// Groups with the most space to free first.
    pub groups: Vec<DuplicateGroup>,
    /// Sum of every group's `could_free_bytes`.
    pub could_free_bytes: u64,
    /// The folders in the home folder that were searched, by name.
    pub folders: Vec<String>,
}

/// Where the duplicate search is.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum DuplicateSearch {
    /// No search has been started.
    NotStarted,
    /// Listing files.
    Looking {
        /// Files looked at so far.
        files_seen: u64,
    },
    /// Comparing files of the same size, by content.
    Comparing {
        /// Files looked at.
        files_seen: u64,
        /// Bytes read so far.
        bytes_compared: u64,
        /// About how many bytes need reading. Can be more than end up read.
        bytes_to_compare: u64,
    },
    /// Done.
    Finished {
        /// What was found.
        report: DuplicateReport,
    },
    /// Stopped by the app.
    Cancelled,
    /// Could not run. `reason` is plain words with no path.
    Failed {
        /// Why.
        reason: String,
    },
}

/// One row of the Large files screen (E2): a file, or for old Downloads a
/// file or folder directly in Downloads. Never ticked for the user, and
/// always Review first.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FileRow {
    /// Send this back in a [`CleanupChoice`] to review it for cleanup.
    pub item_id: String,
    /// The file's or folder's own name, such as `Holiday.mov`.
    pub name: String,
    /// The folder in home it is in, such as `Movies`, or `Home folder`.
    pub place: String,
    /// Space it takes on disk.
    pub size_bytes: u64,
    /// `true` for a folder in Downloads.
    pub is_folder: bool,
    /// When macOS last recorded it being opened, in seconds since 1970.
    /// `None` when there is no record, which is not the same as never.
    pub last_opened_unix_seconds: Option<i64>,
    /// When it last changed, in seconds since 1970.
    pub modified_unix_seconds: i64,
    /// Where it is, starting `~`. Developer mode only; `None` in simple mode.
    pub path: Option<String>,
}

/// What the Large files screen lists for a finished scan.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LargeFiles {
    /// Up to 500 of the largest files, largest first.
    pub large_files: Vec<FileRow>,
    /// Files and folders directly in Downloads not opened (or, with no
    /// record of being opened, not changed) for `old_after_days`, largest
    /// first. A file can be in both lists, with the same id.
    pub old_downloads: Vec<FileRow>,
    /// How many days count as old.
    pub old_after_days: u32,
}
