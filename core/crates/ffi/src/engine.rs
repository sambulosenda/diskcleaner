//! The `RoomyCore` object: runs scans on background threads and answers the
//! read-only commands in `docs/command-contract.md`.
//!
//! Nothing here modifies the filesystem. The scanner reads folder listings
//! and metadata; everything after that works on the in-memory result.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rules::Rule;
use scanner::{ScanError, ScanOptions};

use crate::cleanup::Pending;
use crate::run::LastCleanup;
use crate::types::{
    Category, CleanupChoice, CleanupHandle, CleanupPreview, CleanupResult, FullDiskAccess,
    HistoryEntry, ItemDetail, ItemSummary, Mode, Overview, PlanHandle, PutBackResult, ScanHandle,
    ScanPhase, ScanProgress, Sort, Tier,
};
use crate::views::Finished;
use crate::{embedded_rules, read_disk_summary, CoreError};

/// Shown when the home folder cannot be scanned at all.
const HOME_UNREADABLE: &str = "Roomy could not read your home folder.";
/// Shown when the scan threads could not start.
const COULD_NOT_START: &str = "Roomy could not start the scan. Please try again.";
/// Shown for anything unexpected, including a bug in the core.
const SOMETHING_WENT_WRONG: &str = "Something went wrong during the scan. Please try again.";

/// The core, as the app holds it. One per app run.
#[derive(uniffi::Object)]
pub struct RoomyCore {
    /// The folder every scan reads. Never comes from the app in production.
    home: PathBuf,
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
    history: Mutex<Option<history::History>>,
    /// The folders the user asked Roomy to keep. The app stores them and
    /// sends them at launch and on every change.
    protected_folders: Mutex<Vec<PathBuf>>,
    /// Test-only changes to the extra checks. Empty in the app.
    guard_overrides: Mutex<GuardOverrides>,
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
            home,
            rules: Arc::new(rules),
            next_id: AtomicU64::new(1),
            scans: Mutex::new(HashMap::new()),
            next_plan_id: AtomicU64::new(1),
            pending_plan: Mutex::new(None),
            next_cleanup_id: AtomicU64::new(1),
            last_cleanup: Mutex::new(None),
            history: Mutex::new(None),
            protected_folders: Mutex::new(Vec::new()),
            guard_overrides: Mutex::new(GuardOverrides::default()),
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

    /// Starts a read-only scan of the home folder and returns at once.
    pub fn start_scan(&self, mode: Mode) -> ScanHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(Slot::default());
        lock(&self.scans).insert(id, Arc::clone(&slot));
        let home = self.home.clone();
        let rules = Arc::clone(&self.rules);
        let worker = Arc::clone(&slot);
        let spawned = std::thread::Builder::new()
            .name(format!("roomy-scan-{id}"))
            .spawn(move || run_scan(&home, &rules, mode, &worker));
        if let Err(err) = spawned {
            tracing::error!(%err, "could not start the scan thread");
            *lock(&slot.state) = State::Failed(COULD_NOT_START.to_string());
        }
        ScanHandle { id }
    }

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
            scanned_bytes: f.scanned_bytes,
            disk_total: disk.total_bytes,
            disk_free: disk.free_bytes,
            categories: f.categories(),
            top_items: f.top_items(),
            unreadable_count: f.unreadable_count,
            unreadable_areas: f.unreadable_areas.clone(),
        })
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
    fn with_history<T>(
        &self,
        create: bool,
        work: impl FnOnce(&mut history::History) -> Result<T, history::HistoryError>,
    ) -> Option<T> {
        let mut slot = lock(&self.history);
        if slot.is_none() {
            if !create && !history::path_for(&self.home).is_file() {
                return None;
            }
            match history::History::open_for_home(&self.home) {
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
}

/// The scan thread: walks, classifies and stores the result in `slot`.
/// Never panics out: a panic becomes a failed scan.
fn run_scan(home: &Path, rules: &[Rule], mode: Mode, slot: &Slot) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        scan_and_classify(home, rules, mode, slot)
    }));
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
fn scan_and_classify(home: &Path, rules: &[Rule], mode: Mode, slot: &Slot) -> State {
    let rule_mode = rules::Mode::from(mode);
    let opts = ScanOptions {
        cancel: Arc::clone(&slot.cancel),
        record_files: Some(classifier::record_files(rules, rule_mode)),
        ..ScanOptions::default()
    };
    let result = scanner::scan(home, opts, |p| {
        slot.bytes.store(p.bytes, Ordering::Relaxed);
        slot.files.store(p.files, Ordering::Relaxed);
    });
    let scan = match result {
        Ok(scan) => scan,
        Err(err) => return failed_state(&err),
    };
    *lock(&slot.state) = State::Classifying;
    if slot.cancel.load(Ordering::Relaxed) {
        return State::Cancelled;
    }
    let items = classifier::classify(&scan, rules, rule_mode);
    let unreadable =
        crate::views::unreadable_areas(home, scan.unreadable.iter().map(|u| u.path.as_path()));
    State::Finished(Arc::new(Finished::new(
        mode,
        home.to_path_buf(),
        scan.total_bytes(),
        scan.root_dev,
        unreadable,
        items,
    )))
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

    #[test]
    fn a_scan_cancelled_before_it_starts_ends_cancelled() {
        let home = fixtures::home();
        let root = fixtures::canonical_root(&home).unwrap();
        let rules = embedded_rules::load().unwrap();
        let slot = Slot::default();
        slot.cancel.store(true, Ordering::Relaxed);
        run_scan(&root, &rules, Mode::Simple, &slot);
        assert!(matches!(*lock(&slot.state), State::Cancelled));
    }

    #[test]
    fn a_scan_cancelled_after_classifying_ends_cancelled() {
        let home = fixtures::home();
        let root = fixtures::canonical_root(&home).unwrap();
        let rules = embedded_rules::load().unwrap();
        let slot = Slot::default();
        let state = scan_and_classify(&root, &rules, Mode::Simple, &slot);
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
