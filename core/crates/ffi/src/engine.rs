//! The `RoomyCore` object: runs scans on background threads and answers the
//! read-only commands in `docs/command-contract.md`.
//!
//! Nothing here modifies the filesystem. The scanner reads folder listings
//! and metadata; everything after that works on the in-memory result. The
//! one write is History's own database, where each finished scan leaves a
//! short record (item ids, titles, categories and sizes, no paths) so the
//! next scan can say what changed.
//!
//! Faster rescans (F12): the core keeps the last scan's folder tree in
//! memory, with the macOS file event id from just before it started.
//! `start_rescan` asks the event log what changed since, and reads only
//! those folders again. Whenever that can't be trusted (no event log, a
//! different mode, the tree is over an hour old, events were dropped, hard
//! links in a changed folder...) it does a full scan instead.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rules::Rule;
use scanner::{RescanError, ScanError, ScanOptions, ScanResult};

use crate::cleanup::Pending;
use crate::run::LastCleanup;
use crate::types::{
    AndroidOverview, AppReview, AppSummary, Category, CleanupChoice, CleanupHandle, CleanupPreview,
    CleanupPreviewItem, CleanupResult, DuplicateSearch, FullDiskAccess, HistoryEntry, ItemDetail,
    ItemSummary, LargeFiles, LibrarySize, Mode, Overview, PlanHandle, PutBackResult, ScanChanges,
    ScanHandle, ScanPhase, ScanProgress, Sort, Tier,
};
use crate::views::Finished;
use crate::{embedded_rules, read_disk_summary, CoreError};

/// Shown when the home folder cannot be scanned at all.
const HOME_UNREADABLE: &str = concat!(brand::name!(), " could not read your home folder.");
/// Shown when the scan threads could not start.
const COULD_NOT_START: &str = concat!(brand::name!(), " could not start the scan. Please try again.");
/// Shown for anything unexpected, including a bug in the core.
const SOMETHING_WENT_WRONG: &str = "Something went wrong during the scan. Please try again.";

/// A kept folder tree older than this is not reused: the next rescan is a
/// full one, so sizes never drift for long if the event log missed
/// something.
const RESCAN_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// The History database, shared with the scan threads.
type HistorySlot = Arc<Mutex<Option<history::History>>>;

/// The last finished scan's folder tree, for the next quick rescan.
#[derive(Debug)]
struct KeptTree {
    /// The mode it was scanned in. The files recorded by name depend on it.
    mode: Mode,
    /// The tree.
    scan: Arc<ScanResult>,
    /// The file event id from just before the scan started.
    since: u64,
    /// When it was kept.
    kept_at: std::time::Instant,
}

/// Where the last scan's tree is kept between scans.
type TreeSlot = Arc<Mutex<Option<KeptTree>>>;

/// The core, as the app holds it. One per app run.
#[derive(uniffi::Object)]
pub struct RoomyCore {
    /// The folder every scan reads. Never comes from the app in production.
    pub(crate) home: PathBuf,
    /// The built-in rules, validated when the core was made.
    rules: Arc<Vec<Rule>>,
    /// The next handle number. Starts at 1.
    next_id: AtomicU64,
    /// Every scan that has not been closed.
    scans: Mutex<HashMap<u64, Arc<Slot>>>,
    /// The next cleanup plan number. Starts at 1.
    next_plan_id: AtomicU64,
    /// The one cleanup plan waiting for the user, if any. A new preview
    /// replaces it, so at most one plan exists at a time.
    pending_plan: Mutex<Option<Pending>>,
    /// The next cleanup number. Starts at 1.
    next_cleanup_id: AtomicU64,
    /// The most recent cleanup that moved anything, until it is put back.
    last_cleanup: Mutex<Option<LastCleanup>>,
    /// The History database, opened the first time it is needed.
    history: HistorySlot,
    /// The last scan's folder tree, for quick rescans.
    kept_tree: TreeSlot,
    /// The folders the user asked Roomy to keep. The app stores them and
    /// sends them at launch and on every change.
    protected_folders: Mutex<Vec<PathBuf>>,
    /// Test-only changes to the extra checks. Empty in the app.
    guard_overrides: Mutex<GuardOverrides>,
    /// The Docker screen's state: where `docker` is and the waiting plan.
    pub(crate) docker: crate::docker::DockerSlot,
    /// Where the System Data breakdown looks.
    pub(crate) system_data: crate::system_data::SystemDataSlot,
    /// The Tool clean-ups screen's waiting plan (Homebrew, simulators, SDK).
    pub(crate) tools: crate::tools::ToolSlot,
    /// The app uninstaller's list of apps and its latest review.
    apps: Mutex<crate::uninstall::AppsState>,
    /// The duplicate finder's latest search.
    duplicates: crate::duplicates::DuplicatesSlot,
    /// Test-only: the folders to look for apps in, instead of
    /// `/Applications` and `~/Applications`.
    app_folders: Mutex<Option<Vec<PathBuf>>>,
    /// The Android screen's latest overview.
    android: Mutex<crate::android::AndroidState>,
}

/// Test-only changes to the extra checks, set by Rust tests only.
#[derive(Debug, Default)]
struct GuardOverrides {
    /// Replaces the recent-change window.
    recent_window: Option<std::time::Duration>,
    /// Replaces the list of running programs.
    running: Option<executor::Running>,
}

/// One scan's shared state, written by its thread and read by commands.
#[derive(Debug, Default)]
struct Slot {
    /// Set by `cancel_scan` or `close_scan`.
    cancel: Arc<AtomicBool>,
    /// Allocated bytes counted so far.
    bytes: AtomicU64,
    /// Files counted so far.
    files: AtomicU64,
    /// Where the scan has got to.
    state: Mutex<State>,
}

/// Where a scan has got to, with its result once there is one.
#[derive(Debug, Default)]
enum State {
    /// Walking the home folder.
    #[default]
    Scanning,
    /// Matching what was found to rules.
    Classifying,
    /// Done.
    Finished(Arc<Finished>),
    /// Stopped by the app.
    Cancelled,
    /// Could not run, with a plain-words reason.
    Failed(String),
}

/// Locks a mutex, carrying on if a thread panicked while holding it. Every
/// value behind these locks is valid at all times, so this is safe.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl RoomyCore {
    /// Makes a core that scans `home` with the built-in rules.
    ///
    /// Rust-only, for tests in this workspace: it is not exported to Swift.
    /// The Swift test target uses `with_home_for_testing`, which exists only
    /// with the `testing` feature.
    pub fn with_home(home: PathBuf) -> Result<Arc<Self>, CoreError> {
        Self::build(home)
    }

    /// Makes a core that scans `home` with the built-in rules.
    pub(crate) fn build(home: PathBuf) -> Result<Arc<Self>, CoreError> {
        let rules = embedded_rules::load()?;
        Ok(Arc::new(Self {
            rules: Arc::new(rules),
            next_id: AtomicU64::new(1),
            scans: Mutex::new(HashMap::new()),
            next_plan_id: AtomicU64::new(1),
            pending_plan: Mutex::new(None),
            next_cleanup_id: AtomicU64::new(1),
            last_cleanup: Mutex::new(None),
            history: Arc::new(Mutex::new(None)),
            kept_tree: Arc::new(Mutex::new(None)),
            protected_folders: Mutex::new(Vec::new()),
            guard_overrides: Mutex::new(GuardOverrides::default()),
            docker: crate::docker::DockerSlot::new(&home),
            system_data: crate::system_data::SystemDataSlot::new(&home),
            tools: crate::tools::ToolSlot::new(),
            home,
            apps: Mutex::new(crate::uninstall::AppsState::default()),
            duplicates: crate::duplicates::DuplicatesSlot::default(),
            app_folders: Mutex::new(None),
            android: Mutex::new(crate::android::AndroidState::default()),
        }))
    }

    /// Rust-only, for tests: changes how long ago an item must have last
    /// changed to be cleaned. Fixture homes are made just before a test, so
    /// tests that expect items to move set this to zero.
    pub fn set_recent_change_window_for_testing(&self, window: std::time::Duration) {
        lock(&self.guard_overrides).recent_window = Some(window);
    }

    /// Rust-only, for tests: pretends exactly these programs are running
    /// (executable paths), or with `None`, that the list can't be read.
    pub fn set_running_programs_for_testing(&self, programs: Option<Vec<PathBuf>>) {
        lock(&self.guard_overrides).running = Some(match programs {
            Some(programs) => executor::Running::Known(programs),
            None => executor::Running::Unknown,
        });
    }

    /// The home folder this core works on.
    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    /// Rust-only, for tests: looks for apps in exactly these folders,
    /// instead of `/Applications` and `~/Applications`.
    pub fn set_app_folders_for_testing(&self, folders: Vec<PathBuf>) {
        *lock(&self.app_folders) = Some(folders);
    }

    /// The extra checks for one preview or run, read just now.
    fn guards(&self) -> executor::Guards {
        let folders = lock(&self.protected_folders).clone();
        let overrides = lock(&self.guard_overrides);
        let running = match &overrides.running {
            Some(running) => running.clone(),
            None => executor::Running::now(),
        };
        executor::Guards {
            recent_window: overrides
                .recent_window
                .unwrap_or(executor::RECENT_CHANGE_WINDOW),
            protected_folders: folders,
            running,
        }
    }

    /// The scan behind `handle`, or `unknown_scan`.
    fn slot(&self, handle: ScanHandle) -> Result<Arc<Slot>, CoreError> {
        lock(&self.scans)
            .get(&handle.id)
            .cloned()
            .ok_or(CoreError::UnknownScan)
    }

    /// The finished result behind `handle`, or the error that says why
    /// there is none.
    fn finished(&self, handle: ScanHandle) -> Result<Arc<Finished>, CoreError> {
        let slot = self.slot(handle)?;
        let state = lock(&slot.state);
        match &*state {
            State::Finished(f) => Ok(Arc::clone(f)),
            State::Scanning | State::Classifying => Err(CoreError::ScanNotFinished),
            State::Cancelled => Err(CoreError::ScanCancelled),
            State::Failed(reason) => Err(CoreError::ScanFailed {
                reason: reason.clone(),
            }),
        }
    }
}

#[uniffi::export]
impl RoomyCore {
    /// Makes the core for the real home folder (`dirs::home_dir`) and loads
    /// the built-in rules.
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>, CoreError> {
        let home = dirs::home_dir().ok_or(CoreError::HomeNotFound)?;
        if !home.is_absolute() {
            return Err(CoreError::HomeNotFound);
        }
        Self::build(home)
    }

    /// Starts a read-only scan of the whole home folder and returns at once.
    pub fn start_scan(&self, mode: Mode) -> ScanHandle {
        self.spawn_scan(mode, false)
    }

    /// Starts a read-only rescan and returns at once. Reads again only the
    /// folders that changed since the last scan in this mode, if the core
    /// can be sure which those are; otherwise scans everything, exactly
    /// like `start_scan`. `Overview.quick_rescan` says which it did.
    pub fn start_rescan(&self, mode: Mode) -> ScanHandle {
        self.spawn_scan(mode, true)
    }

    /// What grew or shrank since the previous scan in the same mode, or
    /// `None` if there was no previous scan (or History could not be used).
    pub fn scan_changes(&self, handle: ScanHandle) -> Result<Option<ScanChanges>, CoreError> {
        Ok(self.finished(handle)?.changes.clone())
    }
}

impl RoomyCore {
    /// Starts the scan thread. `quick` allows reusing the kept tree.
    fn spawn_scan(&self, mode: Mode, quick: bool) -> ScanHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(Slot::default());
        lock(&self.scans).insert(id, Arc::clone(&slot));
        let job = Job {
            home: self.home.clone(),
            rules: Arc::clone(&self.rules),
            mode,
            quick,
            history: Arc::clone(&self.history),
            kept_tree: Arc::clone(&self.kept_tree),
        };
        let worker = Arc::clone(&slot);
        let spawned = std::thread::Builder::new()
            .name(format!("roomy-scan-{id}"))
            .spawn(move || run_scan(&job, &worker));
        if let Err(err) = spawned {
            tracing::error!(%err, "could not start the scan thread");
            *lock(&slot.state) = State::Failed(COULD_NOT_START.to_string());
        }
        ScanHandle { id }
    }
}

#[uniffi::export]
impl RoomyCore {
    /// How far the scan has got.
    pub fn scan_progress(&self, handle: ScanHandle) -> Result<ScanProgress, CoreError> {
        let slot = self.slot(handle)?;
        let phase = match &*lock(&slot.state) {
            State::Scanning => ScanPhase::Scanning,
            State::Classifying => ScanPhase::Classifying,
            State::Finished(_) => ScanPhase::Finished,
            State::Cancelled => ScanPhase::Cancelled,
            State::Failed(reason) => ScanPhase::Failed {
                reason: reason.clone(),
            },
        };
        Ok(ScanProgress {
            bytes_seen: slot.bytes.load(Ordering::Relaxed),
            items_seen: slot.files.load(Ordering::Relaxed),
            phase,
        })
    }

    /// Asks the scan to stop. Does nothing for a finished or unknown scan.
    pub fn cancel_scan(&self, handle: ScanHandle) {
        if let Ok(slot) = self.slot(handle) {
            slot.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Cancels the scan if it is running and frees its results.
    pub fn close_scan(&self, handle: ScanHandle) {
        if let Some(slot) = lock(&self.scans).remove(&handle.id) {
            slot.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// What the Overview screen shows for a finished scan.
    pub fn overview(&self, handle: ScanHandle) -> Result<Overview, CoreError> {
        let f = self.finished(handle)?;
        let disk = read_disk_summary(&f.home)?;
        Ok(Overview {
            likely_safe_bytes: f.likely_safe_bytes(),
            review_first_bytes: f.review_first_bytes(),
            scanned_bytes: f.scanned_bytes,
            disk_total: disk.total_bytes,
            disk_free: disk.free_bytes,
            categories: f.categories(),
            top_items: f.top_items(),
            unreadable_count: f.unreadable_count,
            unreadable_areas: f.unreadable_areas.clone(),
            quick_rescan: f.quick_rescan,
        })
    }

    /// How much the Mac's own libraries take in a finished scan: Photos,
    /// Music, TV, Mail, Messages attachments and Final Cut Pro. One row per
    /// kind, always in the same order. Shown only: Roomy never cleans inside
    /// a library, and no row holds a path.
    pub fn libraries(&self, handle: ScanHandle) -> Result<Vec<LibrarySize>, CoreError> {
        Ok(self.finished(handle)?.libraries.clone())
    }

    /// Every item of a finished scan, optionally one category and/or tier.
    pub fn list_items(
        &self,
        handle: ScanHandle,
        category: Option<Category>,
        tier: Option<Tier>,
        sort: Sort,
    ) -> Result<Vec<ItemSummary>, CoreError> {
        Ok(self.finished(handle)?.list(category, tier, sort))
    }

    /// What the Large files screen lists (E2): up to 500 of the largest
    /// files, and files and folders in Downloads not opened for a while.
    /// Read when the scan finished; paths only in Developer mode. Nothing
    /// is ticked for the user. The row ids go to `preview_cleanup` like
    /// any item id.
    pub fn large_files(&self, handle: ScanHandle) -> Result<LargeFiles, CoreError> {
        let f = self.finished(handle)?;
        Ok(f.loose.view(&f.home, f.mode))
    }

    /// Everything the detail panel shows for one item.
    pub fn item_detail(
        &self,
        handle: ScanHandle,
        item_id: String,
    ) -> Result<ItemDetail, CoreError> {
        let f = self.finished(handle)?;
        let mut detail = f.detail(&item_id)?;
        detail.device_backup = f
            .item(&item_id)
            .and_then(|item| crate::backup::device_backup(&f, item));
        Ok(detail)
    }

    /// What the JavaScript screen shows: each project's `node_modules` with
    /// when the project was last worked on, and the package managers'
    /// caches. Reads only the modified times of each project's
    /// `package.json`, lock files and git files.
    pub fn javascript_projects(
        &self,
        handle: ScanHandle,
    ) -> Result<crate::javascript::JavaScriptOverview, CoreError> {
        let f = self.finished(handle)?;
        Ok(crate::javascript::overview(&f))
    }

    /// The dry run: checks the ticked items exactly as a cleanup would, just
    /// before it acts, and says what would move to the Trash. Moves nothing.
    ///
    /// Keeps a single-use plan of the items that would move, replacing any
    /// earlier plan. Nothing in this version carries a plan out.
    pub fn preview_cleanup(
        &self,
        handle: ScanHandle,
        choices: Vec<CleanupChoice>,
    ) -> Result<CleanupPreview, CoreError> {
        let f = self.finished(handle)?;
        // Any earlier plan is dropped first, so a failed preview never leaves
        // an older plan behind.
        *lock(&self.pending_plan) = None;
        let id = self.next_plan_id.fetch_add(1, Ordering::Relaxed);
        let guards = self.guards();
        let prepared =
            crate::cleanup::prepare(&f, &choices, id, &guards, std::time::SystemTime::now())?;
        *lock(&self.pending_plan) = prepared.plan;
        Ok(prepared.preview)
    }

    /// Checks items the way `preview_cleanup` would, for the menu bar panel's
    /// locked rows, and says which would move and why the others would not
    /// (an app is open, it changed recently, a folder you keep). Moves
    /// nothing and keeps no plan: the waiting plan, if any, is left alone,
    /// so a review open in the main window still works. Each id counts as
    /// chosen on its own, as a ticked row is.
    pub fn check_items(
        &self,
        handle: ScanHandle,
        item_ids: Vec<String>,
    ) -> Result<Vec<CleanupPreviewItem>, CoreError> {
        let f = self.finished(handle)?;
        let choices: Vec<CleanupChoice> = item_ids
            .into_iter()
            .map(|item_id| CleanupChoice {
                item_id,
                chosen_alone: true,
            })
            .collect();
        // The plan id is never handed out: the plan is dropped here.
        let id = self.next_plan_id.fetch_add(1, Ordering::Relaxed);
        let guards = self.guards();
        let prepared =
            crate::cleanup::prepare(&f, &choices, id, &guards, std::time::SystemTime::now())?;
        Ok(prepared.preview.items)
    }

    /// Drops the waiting plan, if it is `plan`. Safe to call more than once
    /// or with an unknown plan.
    pub fn discard_cleanup(&self, plan: PlanHandle) {
        let mut pending = lock(&self.pending_plan);
        if pending.as_ref().is_some_and(|p| p.plan.id() == plan.id) {
            *pending = None;
        }
    }

    /// Carries out the waiting plan: moves its items to the Trash. The
    /// executor checks every item again first; an item that changed since
    /// the review stays where it is. The plan is used up.
    ///
    /// Throws `cleanup_unavailable` if `plan` is not the waiting plan, it
    /// has expired, or the Trash can't be opened. Nothing moves then.
    pub fn run_cleanup(&self, plan: PlanHandle) -> Result<CleanupResult, CoreError> {
        let pending = {
            let mut slot = lock(&self.pending_plan);
            match slot.take() {
                Some(p) if p.plan.id() == plan.id => p,
                other => {
                    *slot = other;
                    return Err(crate::run::plan_gone());
                }
            }
        };
        let id = self.next_cleanup_id.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now();
        let mut ran = crate::run::run(pending, id, &self.guards(), now)?;
        let history_id = self.with_history(true, |h| {
            h.record_cleanup(crate::run::unix_seconds(now), &ran.history_items)
        });
        ran.result.saved_to_history = history_id.is_some();
        if let Some(last) = ran.last.as_mut() {
            last.history_id = history_id;
        }
        // A newer cleanup replaces the older undo, even if nothing moved.
        *lock(&self.last_cleanup) = ran.last;
        Ok(ran.result)
    }

    /// The app uninstaller: every app in `/Applications` and
    /// `~/Applications`, except Apple's and Roomy itself, sorted by name,
    /// each with its size. Reads folder listings, each app's `Info.plist`
    /// and metadata only. Each call gives new app ids, and forgets the
    /// earlier list and review.
    pub fn list_apps(&self) -> Vec<AppSummary> {
        let folders = lock(&self.app_folders)
            .clone()
            .unwrap_or_else(|| crate::uninstall::app_folders(&self.home));
        let running = self.guards().running;
        crate::uninstall::list(&mut lock(&self.apps), &self.home, &folders, &running)
    }

    /// The app uninstaller: what the app `app_id` (from `list_apps`) left in
    /// the home folder, measured just now, plus the app itself when it is in
    /// `~/Applications`. Read only. Replaces the earlier review.
    ///
    /// Throws `unknown_app` for an id that is not from the latest list.
    pub fn review_app(&self, app_id: String, mode: Mode) -> Result<AppReview, CoreError> {
        let running = self.guards().running;
        crate::uninstall::review(&mut lock(&self.apps), &self.home, &app_id, mode, &running)
    }

    /// The app uninstaller's dry run: checks the ticked parts of the latest
    /// review (ids from `review_app`) exactly as a cleanup would, and says
    /// what would move to the Trash. Moves nothing. The plan it keeps is
    /// carried out by `run_cleanup` and can be put back with
    /// `put_back_cleanup`, like any cleanup. Replaces any earlier plan.
    ///
    /// Throws `unknown_app` if `app_id` is not the app last reviewed,
    /// `unknown_item` for a part id that is not in that review,
    /// `nothing_selected` for no parts, and `cleanup_unavailable` while the
    /// app is open.
    pub fn preview_uninstall(
        &self,
        app_id: String,
        part_ids: Vec<String>,
    ) -> Result<CleanupPreview, CoreError> {
        *lock(&self.pending_plan) = None;
        let id = self.next_plan_id.fetch_add(1, Ordering::Relaxed);
        let guards = self.guards();
        let prepared = crate::uninstall::prepare(
            &lock(&self.apps),
            &self.home,
            &app_id,
            &part_ids,
            id,
            &guards,
            std::time::SystemTime::now(),
        )?;
        *lock(&self.pending_plan) = prepared.plan;
        Ok(prepared.preview)
    }

    /// The Android screen: every Android SDK part (emulator system images,
    /// platforms, build tools, NDKs), every emulator and every Gradle
    /// download in the home folder, each measured just now and marked with
    /// whether any Android project in the home folder (or, for a system
    /// image, any emulator) uses it. Read only: reads folder listings,
    /// metadata, `source.properties`, emulator `config.ini` files and Gradle
    /// build files. Can take some seconds. Each call gives new item ids.
    pub fn android_overview(&self, mode: Mode) -> AndroidOverview {
        crate::android::overview(&mut lock(&self.android), &self.home, mode)
    }

    /// The Android screen's dry run: checks the ticked items of the latest
    /// `android_overview` exactly as a cleanup would, and says what would
    /// move to the Trash. Moves nothing. The plan it keeps is carried out by
    /// `run_cleanup` and can be put back with `put_back_cleanup`, like any
    /// cleanup. Replaces any earlier plan.
    ///
    /// Throws `unknown_item` for an id that is not from the latest overview
    /// and `nothing_selected` for no ids.
    pub fn preview_android_cleanup(
        &self,
        item_ids: Vec<String>,
    ) -> Result<CleanupPreview, CoreError> {
        *lock(&self.pending_plan) = None;
        let id = self.next_plan_id.fetch_add(1, Ordering::Relaxed);
        let guards = self.guards();
        let prepared = crate::android::prepare(
            &lock(&self.android),
            &self.home,
            &item_ids,
            id,
            &guards,
            std::time::SystemTime::now(),
        )?;
        *lock(&self.pending_plan) = prepared.plan;
        Ok(prepared.preview)
    }

    /// The duplicate finder: starts looking, on a background thread, for
    /// files with exactly the same content in Desktop, Documents, Downloads,
    /// Movies, Music and Pictures. Reads only: files are compared by size,
    /// then SHA-256. Returns at once; ask `duplicate_search_progress` how it
    /// is going. Stops and replaces any earlier search, and its copy ids.
    pub fn start_duplicate_search(&self, mode: Mode) {
        self.duplicates.start(&self.home, mode);
    }

    /// Where the duplicate search is, with what it found once finished.
    pub fn duplicate_search_progress(&self) -> DuplicateSearch {
        self.duplicates.progress()
    }

    /// Stops the duplicate search. Safe to call when none is running.
    pub fn cancel_duplicate_search(&self) {
        self.duplicates.cancel();
    }

    /// The duplicate finder's dry run: checks the ticked copies (ids from
    /// the finished search) exactly as a cleanup would, including that the
    /// copy that stays is still the same byte for byte, and says what would
    /// move to the Trash. Moves nothing. The plan it keeps is carried out by
    /// `run_cleanup` and can be put back with `put_back_cleanup`, like any
    /// cleanup. Replaces any earlier plan.
    ///
    /// Throws `unknown_item` for a copy id not from the latest search,
    /// `nothing_selected` for no copies, and `cleanup_unavailable` if every
    /// copy of a file was ticked (one must stay) or no search has finished.
    pub fn preview_duplicate_cleanup(
        &self,
        copy_ids: Vec<String>,
    ) -> Result<CleanupPreview, CoreError> {
        *lock(&self.pending_plan) = None;
        let id = self.next_plan_id.fetch_add(1, Ordering::Relaxed);
        let guards = self.guards();
        let prepared = self.duplicates.prepare(
            &self.home,
            &copy_ids,
            id,
            &guards,
            std::time::SystemTime::now(),
        )?;
        *lock(&self.pending_plan) = prepared.plan;
        Ok(prepared.preview)
    }

    /// Sets the folders the user asked Roomy to keep. Nothing inside one of
    /// them, and nothing that holds one, is ever cleaned. Replaces the
    /// earlier list and returns the new one, as the core will use it.
    ///
    /// Each folder must be a full path inside the home folder. A folder that
    /// exists is stored as its real location, with any symlinks resolved, so
    /// keeping a link also keeps what it points to. Throws `invalid_path`
    /// (and keeps the earlier list) if any folder is not usable.
    pub fn set_protected_folders(&self, folders: Vec<String>) -> Result<Vec<String>, CoreError> {
        let mut checked: Vec<PathBuf> = Vec::with_capacity(folders.len());
        for folder in &folders {
            let path = crate::folders::checked(&self.home, folder)?;
            if !checked.contains(&path) {
                checked.push(path);
            }
        }
        let shown = crate::folders::shown(&checked);
        *lock(&self.protected_folders) = checked;
        Ok(shown)
    }

    /// The folders the user asked Roomy to keep, as the core uses them.
    pub fn protected_folders(&self) -> Vec<String> {
        crate::folders::shown(&lock(&self.protected_folders))
    }

    /// Puts the last cleanup back: moves what it moved from the Trash to
    /// where it was. Works once, for the most recent cleanup since the core
    /// was made. An item stays in the Trash if it changed there, or if
    /// something new is in its old place.
    pub fn put_back_cleanup(&self, cleanup: CleanupHandle) -> Result<PutBackResult, CoreError> {
        let last = {
            let mut slot = lock(&self.last_cleanup);
            match slot.take() {
                Some(l) if l.handle == cleanup => l,
                other => {
                    *slot = other;
                    return Err(crate::run::nothing_to_put_back());
                }
            }
        };
        // Taken out while it runs, so a second request cannot put back
        // twice. An error here comes before anything moved, so the record
        // goes back and the user can try again.
        let (result, rows) = match crate::run::put_back(&last) {
            Ok(done) => done,
            Err(err) => {
                let mut slot = lock(&self.last_cleanup);
                if slot.is_none() {
                    *slot = Some(last);
                }
                return Err(err);
            }
        };
        if let Some(id) = last.history_id {
            let at = crate::run::unix_seconds(std::time::SystemTime::now());
            self.with_history(true, |h| h.record_put_back(id, at, &rows));
        }
        Ok(result)
    }

    /// The most recent cleanups, newest first, at most `limit`. Empty if
    /// nothing has been cleaned yet. Holds no paths.
    pub fn cleanup_history(&self, limit: u32) -> Vec<HistoryEntry> {
        self.with_history(false, |h| h.recent(limit))
            .map(crate::run::entries)
            .unwrap_or_default()
    }

    /// Whether Roomy can read the folders macOS keeps private, right now.
    /// Lists a few of them inside home and reads nothing else. Cheap enough
    /// for the app to ask once a second while it waits for the user to
    /// turn Full Disk Access on.
    pub fn full_disk_access(&self) -> FullDiskAccess {
        scanner::privacy::full_disk_access(&self.home).into()
    }
}

impl RoomyCore {
    /// Runs `work` on the History database, opening it first if needed.
    /// With `create` false, a database that does not exist yet is not
    /// made. Errors go to the log; History never stops a cleanup.
    pub(crate) fn with_history<T>(
        &self,
        create: bool,
        work: impl FnOnce(&mut history::History) -> Result<T, history::HistoryError>,
    ) -> Option<T> {
        with_history(&self.history, &self.home, create, work)
    }
}

/// Runs `work` on the History database in `slot`, opening it for `home`
/// first if needed. With `create` false, a database that does not exist
/// yet is not made. Errors go to the log; History never stops a cleanup
/// or a scan.
fn with_history<T>(
    slot: &Mutex<Option<history::History>>,
    home: &Path,
    create: bool,
    work: impl FnOnce(&mut history::History) -> Result<T, history::HistoryError>,
) -> Option<T> {
    let mut slot = lock(slot);
    if slot.is_none() {
        if !create && !history::path_for(home).is_file() {
            return None;
        }
        match history::History::open_for_home(home) {
            Ok(h) => *slot = Some(h),
            Err(err) => {
                tracing::warn!(%err, "History could not be opened");
                return None;
            }
        }
    }
    let h = slot.as_mut()?;
    match work(h) {
        Ok(value) => Some(value),
        Err(err) => {
            tracing::warn!(%err, "History could not be used");
            None
        }
    }
}

/// Everything one scan thread needs.
struct Job {
    home: PathBuf,
    rules: Arc<Vec<Rule>>,
    mode: Mode,
    /// Whether the kept tree may be reused.
    quick: bool,
    history: HistorySlot,
    kept_tree: TreeSlot,
}

/// The scan thread: walks, classifies and stores the result in `slot`.
/// Never panics out: a panic becomes a failed scan.
fn run_scan(job: &Job, slot: &Slot) {
    let outcome = catch_unwind(AssertUnwindSafe(|| scan_and_classify(job, slot)));
    let state = match outcome {
        Ok(state) => state,
        Err(_) => {
            tracing::error!("the scan thread panicked");
            State::Failed(SOMETHING_WENT_WRONG.to_string())
        }
    };
    store_final(slot, state);
}

/// Stores a scan's final state. A result that arrives after the app asked
/// to cancel is dropped, so a cancelled scan never turns into a finished one.
fn store_final(slot: &Slot, state: State) {
    let mut current = lock(&slot.state);
    let cancelled = slot.cancel.load(Ordering::Relaxed);
    *current = match state {
        State::Finished(_) if cancelled => State::Cancelled,
        other => other,
    };
}

/// Does the work of one scan and returns its final state.
fn scan_and_classify(job: &Job, slot: &Slot) -> State {
    let (home, rules, mode) = (job.home.as_path(), job.rules.as_slice(), job.mode);
    let rule_mode = rules::Mode::from(mode);
    let opts = ScanOptions {
        cancel: Arc::clone(&slot.cancel),
        record_files: Some(crate::files::record_files(classifier::record_files(
            rules, rule_mode,
        ))),
        // A full scan keeps the largest files for the Large files screen. A
        // quick rescan can't (see `scanner::rescan`), so it carries the last
        // full scan's list forward instead; each row is checked again.
        largest_files: crate::files::SCANNED_LARGE_FILES,
        largest_in: Some(crate::files::largest_in()),
        ..ScanOptions::default()
    };
    // Noted before anything is read, so whatever changes during the walk
    // is in the next rescan's list too.
    let since = scanner::fsevents::current_event_id();
    let progress = |p: scanner::ScanProgress| {
        slot.bytes.store(p.bytes, Ordering::Relaxed);
        slot.files.store(p.files, Ordering::Relaxed);
    };
    let quick = if job.quick {
        let quick_opts = ScanOptions {
            largest_files: 0,
            largest_in: None,
            ..opts.clone()
        };
        quick_rescan(job, &quick_opts, &progress)
    } else {
        None
    };
    let (scan, quick_rescan) = match quick {
        Some(Ok(mut scan)) => {
            scan.largest_files = kept_largest_files(job);
            (scan, true)
        }
        Some(Err(err)) => return failed_state(&err),
        None => match scanner::scan(home, opts, progress) {
            Ok(scan) => (scan, false),
            Err(err) => return failed_state(&err),
        },
    };
    *lock(&slot.state) = State::Classifying;
    if slot.cancel.load(Ordering::Relaxed) {
        return State::Cancelled;
    }
    let items = classifier::classify(&scan, rules, rule_mode);
    if slot.cancel.load(Ordering::Relaxed) {
        return State::Cancelled;
    }
    let loose = crate::files::find(&scan, &items, home, std::time::SystemTime::now());
    let unreadable =
        crate::views::unreadable_areas(home, scan.unreadable.iter().map(|u| u.path.as_path()));
    let mut finished = Finished::new(
        mode,
        home.to_path_buf(),
        scan.total_bytes(),
        scan.root_dev,
        unreadable,
        items,
        loose,
    )
    .with_places(crate::place::place_sources(rules))
    .with_libraries(
        analysers::libraries::library_sizes(&scan)
            .iter()
            .map(LibrarySize::from)
            .collect(),
    );
    finished.quick_rescan = quick_rescan;
    keep_tree(job, scan, since);
    if slot.cancel.load(Ordering::Relaxed) {
        return State::Cancelled;
    }
    finished.changes = record_and_compare(job, &finished);
    State::Finished(Arc::new(finished))
}

/// Tries a quick rescan from the kept tree. `None` means "do a full scan":
/// there is no usable tree, or the event log or the rescan said it can't
/// be sure. `Some(Err)` is a real failure, such as being cancelled.
fn quick_rescan(
    job: &Job,
    opts: &ScanOptions,
    progress: &impl Fn(scanner::ScanProgress),
) -> Option<Result<ScanResult, ScanError>> {
    let kept = {
        let slot = lock(&job.kept_tree);
        let kept = slot.as_ref()?;
        if kept.mode != job.mode || kept.kept_at.elapsed() > RESCAN_MAX_AGE {
            return None;
        }
        (Arc::clone(&kept.scan), kept.since)
    };
    let (tree, since) = kept;
    let changes =
        match scanner::fsevents::changes_since(&job.home, since, scanner::fsevents::REPLAY_TIMEOUT)
        {
            Ok(changes) => changes,
            Err(why) => {
                tracing::info!(%why, "full scan instead of a quick rescan");
                return None;
            }
        };
    tracing::info!(
        folders = changes.folders.len(),
        subtrees = changes.subtrees.len(),
        "quick rescan"
    );
    match scanner::rescan(&tree, &changes, opts.clone(), progress) {
        Ok(scan) => Some(Ok(scan)),
        Err(RescanError::NeedsFullScan(why)) => {
            tracing::info!(%why, "full scan instead of a quick rescan");
            None
        }
        Err(RescanError::Scan(err)) => Some(Err(err)),
    }
}

/// The kept tree's largest files, for a quick rescan, which can't find
/// them itself. `crate::files::find` checks each again, so one that
/// changed or went away is left out; a file new since the last full scan
/// shows after the next one.
fn kept_largest_files(job: &Job) -> Vec<scanner::LargeFile> {
    lock(&job.kept_tree)
        .as_ref()
        .map(|kept| kept.scan.largest_files.clone())
        .unwrap_or_default()
}

/// Keeps `scan` for the next quick rescan, replacing the older tree. Only
/// where there is an event log to say what changed (`since`); elsewhere
/// the tree is dropped, so it uses no memory.
fn keep_tree(job: &Job, scan: ScanResult, since: Option<u64>) {
    *lock(&job.kept_tree) = since.map(|since| KeptTree {
        mode: job.mode,
        scan: Arc::new(scan),
        since,
        kept_at: std::time::Instant::now(),
    });
}

/// Records `finished` in History and compares it with the previous scan in
/// the same mode. `None` if there was none or History can't be used.
fn record_and_compare(job: &Job, finished: &Finished) -> Option<ScanChanges> {
    let at = crate::run::unix_seconds(std::time::SystemTime::now());
    let snapshot = crate::changes::snapshot(finished, at);
    let previous = with_history(&job.history, &job.home, true, |h| {
        let previous = h.latest_scan(&snapshot.mode)?;
        h.record_scan(&snapshot)?;
        Ok(previous)
    })??;
    Some(crate::changes::compare(&previous, finished))
}

/// Turns a scanner error into a final state with a plain-words reason. The
/// error itself (which may name a path) goes to the log only.
fn failed_state(err: &ScanError) -> State {
    let reason = match err {
        ScanError::Cancelled => return State::Cancelled,
        ScanError::RootNotAbsolute(_)
        | ScanError::RootIsSymlink(_)
        | ScanError::RootNotAFolder(_)
        | ScanError::Root { .. } => HOME_UNREADABLE,
        ScanError::Threads(_) => COULD_NOT_START,
        ScanError::Internal => SOMETHING_WENT_WRONG,
    };
    tracing::warn!(error = %err, "scan failed");
    State::Failed(reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(home: PathBuf) -> Job {
        Job {
            home,
            rules: Arc::new(embedded_rules::load().unwrap()),
            mode: Mode::Simple,
            quick: false,
            history: Arc::new(Mutex::new(None)),
            kept_tree: Arc::new(Mutex::new(None)),
        }
    }

    #[test]
    fn a_scan_cancelled_before_it_starts_ends_cancelled() {
        let home = fixtures::home();
        let root = fixtures::canonical_root(&home).unwrap();
        let slot = Slot::default();
        slot.cancel.store(true, Ordering::Relaxed);
        run_scan(&job(root), &slot);
        assert!(matches!(*lock(&slot.state), State::Cancelled));
    }

    #[test]
    fn a_scan_cancelled_after_classifying_ends_cancelled() {
        let home = fixtures::home();
        let root = fixtures::canonical_root(&home).unwrap();
        let slot = Slot::default();
        let state = scan_and_classify(&job(root), &slot);
        assert!(matches!(state, State::Finished(_)));
        // The cancel flag is checked again before the result is stored.
        slot.cancel.store(true, Ordering::Relaxed);
        store_final(&slot, state);
        assert!(matches!(*lock(&slot.state), State::Cancelled));
    }

    #[test]
    fn scanner_errors_never_carry_a_path() {
        let errors = [
            ScanError::RootNotAbsolute(PathBuf::from("relative/secret")),
            ScanError::RootIsSymlink(PathBuf::from("/Users/sam/secret")),
            ScanError::RootNotAFolder(PathBuf::from("/Users/sam/secret")),
            ScanError::Threads("pool /Users/sam/secret".to_string()),
            ScanError::Internal,
        ];
        for err in &errors {
            match failed_state(err) {
                State::Failed(reason) => assert!(!reason.contains("secret"), "{reason}"),
                other => panic!("expected a failure, got {other:?}"),
            }
        }
        assert!(matches!(
            failed_state(&ScanError::Cancelled),
            State::Cancelled
        ));
    }
}
