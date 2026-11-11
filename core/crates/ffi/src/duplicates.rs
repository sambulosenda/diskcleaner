//! The duplicate finder: looks for files with the same content in the
//! user's own folders, and turns the copies the user ticked into a cleanup
//! plan that always leaves one copy.
//!
//! Nothing here changes a file. Files are found and compared by
//! `analysers::duplicates` (read only, by content: size, then SHA-256). The
//! app sends copy ids, never paths. The plan this builds is checked by the
//! executor's own preview and carried out by the same `run_cleanup` as any
//! other cleanup, so the Trash, put back and History work the same way.
//!
//! Safety, on top of everything every cleanup gets:
//! - Nothing is ticked for the user: the app starts every copy unticked.
//! - At least one copy of each file stays: a review that ticks every copy
//!   of a file is refused, and the planner refuses a plan whose kept copy
//!   would also move.
//! - Each copy that moves names the copy it keeps. Just before moving, the
//!   executor checks that copy is still there, unchanged, and the same byte
//!   for byte, or leaves both alone.
//! - Every copy is chosen on its own, so the review-only rule for personal
//!   files (Downloads, Desktop, Documents) is met, and copies the safety
//!   rules would never let go are shown as staying put.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use analysers::duplicates::{self as finder, Copy, Group, Progress};
use planner::{Expected, ItemRequest, KeepCopy};

use crate::cleanup::{check_entries, sentence, Entry, Prepared};
use crate::types::{DuplicateCopy, DuplicateGroup, DuplicateReport, DuplicateSearch, Mode};
use crate::views::display_path;
use crate::CoreError;

/// The folders searched, inside home. The user's own files live here;
/// Library and hidden folders hold app data, never offered.
pub(crate) const FOLDERS: &[&str] = &[
    "Desktop",
    "Documents",
    "Downloads",
    "Movies",
    "Music",
    "Pictures",
];

/// Shown when the search thread can't start.
const COULD_NOT_START: &str = "Roomy couldn't start looking for copies. Please try again.";
/// Shown for anything unexpected, including a bug in the core.
const SOMETHING_WENT_WRONG: &str =
    "Something went wrong while looking for copies. Please try again.";
/// Shown when no search has finished.
const NOT_SEARCHED: &str = "Look for copies first.";
/// Shown for a copy the app can never move. Never holds a path.
const STAYS: &str = "Roomy can't move this copy";

/// The duplicate finder's state, shared with the search thread.
#[derive(Debug, Default)]
pub(crate) struct DuplicatesSlot {
    inner: Mutex<Search>,
}

/// The latest search.
#[derive(Debug, Default)]
struct Search {
    /// Bumped by every search, so ids from an older one are refused.
    generation: u64,
    /// Stops this search.
    cancel: Arc<AtomicBool>,
    /// How far it has got.
    progress: Arc<Progress>,
    /// Where it is, with its result once there is one.
    status: Arc<Mutex<Status>>,
}

/// Where a search is.
#[derive(Debug, Default)]
enum Status {
    #[default]
    NotStarted,
    Running,
    Finished(Arc<Found>),
    Cancelled,
    Failed(String),
}

/// A finished search: what the app was shown, and what the core needs to
/// build a plan from it.
#[derive(Debug)]
struct Found {
    report: DuplicateReport,
    groups: Vec<FoundGroup>,
}

/// One group, as the core keeps it.
#[derive(Debug)]
struct FoundGroup {
    name: String,
    copies: Vec<FoundCopy>,
}

/// One copy, as the core keeps it.
#[derive(Debug)]
struct FoundCopy {
    copy_id: String,
    copy: Copy,
    /// Why the copy can't move, if the safety rules say so.
    stays: Option<String>,
}

/// Locks a mutex, carrying on if a thread panicked while holding it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl DuplicatesSlot {
    /// Starts a new search of `home`'s own folders, stopping any earlier one.
    pub(crate) fn start(&self, home: &Path, mode: Mode) {
        let mut search = lock(&self.inner);
        search.cancel.store(true, Ordering::Relaxed);
        search.generation = search.generation.wrapping_add(1);
        search.cancel = Arc::new(AtomicBool::new(false));
        search.progress = Arc::new(Progress::default());
        search.status = Arc::new(Mutex::new(Status::Running));
        let generation = search.generation;
        let cancel = Arc::clone(&search.cancel);
        let progress = Arc::clone(&search.progress);
        let status = Arc::clone(&search.status);
        let home = home.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name(format!("roomy-duplicates-{generation}"))
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    search_now(&home, mode, generation, &progress, &cancel)
                }));
                let state = outcome.unwrap_or_else(|_| {
                    tracing::error!("the duplicate search panicked");
                    Status::Failed(SOMETHING_WENT_WRONG.to_string())
                });
                let mut current = lock(&status);
                // A result that arrives after the app asked to stop is dropped.
                *current = if cancel.load(Ordering::Relaxed) {
                    Status::Cancelled
                } else {
                    state
                };
            });
        if let Err(err) = spawned {
            tracing::error!(%err, "could not start the duplicate search");
            *lock(&search.status) = Status::Failed(COULD_NOT_START.to_string());
        }
    }

    /// Stops the search, if one is running.
    pub(crate) fn cancel(&self) {
        let search = lock(&self.inner);
        search.cancel.store(true, Ordering::Relaxed);
        let mut status = lock(&search.status);
        if matches!(*status, Status::Running) {
            *status = Status::Cancelled;
        }
    }

    /// Where the latest search is.
    pub(crate) fn progress(&self) -> DuplicateSearch {
        let search = lock(&self.inner);
        let status = lock(&search.status);
        match &*status {
            Status::NotStarted => DuplicateSearch::NotStarted,
            Status::Running => {
                let p = &search.progress;
                let files_seen = p.files_seen.load(Ordering::Relaxed);
                if p.comparing.load(Ordering::Relaxed) {
                    DuplicateSearch::Comparing {
                        files_seen,
                        bytes_compared: p.bytes_compared.load(Ordering::Relaxed),
                        bytes_to_compare: p.bytes_to_compare.load(Ordering::Relaxed),
                    }
                } else {
                    DuplicateSearch::Looking { files_seen }
                }
            }
            Status::Finished(found) => DuplicateSearch::Finished {
                report: found.report.clone(),
            },
            Status::Cancelled => DuplicateSearch::Cancelled,
            Status::Failed(reason) => DuplicateSearch::Failed {
                reason: reason.clone(),
            },
        }
    }

    /// The finished search, or why there is none.
    fn found(&self) -> Result<Arc<Found>, CoreError> {
        let search = lock(&self.inner);
        let status = lock(&search.status);
        match &*status {
            Status::Finished(found) => Ok(Arc::clone(found)),
            _ => Err(CoreError::CleanupUnavailable {
                reason: NOT_SEARCHED.to_string(),
            }),
        }
    }

    /// Builds the dry run for the ticked copies, exactly as a cleanup would
    /// check them, and keeps the plan of what would move.
    pub(crate) fn prepare(
        &self,
        home: &Path,
        copy_ids: &[String],
        plan_id: u64,
        guards: &executor::Guards,
        now: SystemTime,
    ) -> Result<Prepared, CoreError> {
        let found = self.found()?;
        let dev = home_dev(home)?;
        let entries = entries_for(&found, copy_ids)?;
        check_entries(home, dev, entries, plan_id, guards, now)
    }
}

/// The search thread's work: finds the groups and words them for the app.
fn search_now(
    home: &Path,
    mode: Mode,
    generation: u64,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Status {
    let Ok(dev) = home_dev(home) else {
        return Status::Failed(SOMETHING_WENT_WRONG.to_string());
    };
    let folders: Vec<PathBuf> = FOLDERS.iter().map(|f| home.join(f)).collect();
    match finder::find(&folders, dev, progress, cancel) {
        Ok(groups) => Status::Finished(Arc::new(found(home, dev, mode, generation, groups))),
        Err(finder::Cancelled) => Status::Cancelled,
    }
}

/// Turns the finder's groups into what the app shows and the core keeps.
fn found(home: &Path, dev: u64, mode: Mode, generation: u64, groups: Vec<Group>) -> Found {
    let mut shown = Vec::new();
    let mut kept = Vec::new();
    for (g, group) in groups.into_iter().enumerate() {
        let name = file_name(&group.copies[0].path);
        let copies: Vec<FoundCopy> = group
            .copies
            .iter()
            .enumerate()
            .map(|(c, copy)| FoundCopy {
                copy_id: format!("dup-{generation}-{g}-{c}"),
                copy: copy.clone(),
                stays: stays(home, dev, &copy.path),
            })
            .collect();
        // Nothing to offer if no copy could move.
        if copies.iter().all(|c| c.stays.is_some()) {
            continue;
        }
        shown.push(DuplicateGroup {
            group_id: format!("dupg-{generation}-{g}"),
            name: name.clone(),
            size_bytes: group.copies[0].allocated_bytes,
            could_free_bytes: could_free(&copies),
            copies: copies.iter().map(|c| shown_copy(home, mode, c)).collect(),
        });
        kept.push(FoundGroup { name, copies });
    }
    let could_free_bytes = shown
        .iter()
        .fold(0u64, |sum, g| sum.saturating_add(g.could_free_bytes));
    Found {
        report: DuplicateReport {
            groups: shown,
            could_free_bytes,
            folders: FOLDERS.iter().map(|f| f.to_string()).collect(),
        },
        groups: kept,
    }
}

/// The most removing copies could free while keeping one: every movable
/// copy, less the one that frees least if every copy could move.
fn could_free(copies: &[FoundCopy]) -> u64 {
    let frees: Vec<u64> = copies
        .iter()
        .filter(|c| c.stays.is_none())
        .map(|c| c.copy.frees_bytes())
        .collect();
    let sum = frees.iter().fold(0u64, |s, f| s.saturating_add(*f));
    if frees.len() == copies.len() {
        sum.saturating_sub(frees.iter().copied().min().unwrap_or(0))
    } else {
        sum
    }
}

/// Why the safety rules would never let this copy go, or `None` if the
/// user may choose it.
fn stays(home: &Path, dev: u64, path: &Path) -> Option<String> {
    match safety::validate(home, dev, path, true) {
        Ok(valid) => match valid.review_only() {
            Some(reason) if !reason.deletable_in_v1() => Some(format!("{STAYS}.")),
            _ => None,
        },
        Err(refusal) => Some(format!("{STAYS}: {}", sentence(&refusal.to_string()))),
    }
}

/// What the app shows for one copy.
fn shown_copy(home: &Path, mode: Mode, c: &FoundCopy) -> DuplicateCopy {
    DuplicateCopy {
        copy_id: c.copy_id.clone(),
        name: file_name(&c.copy.path),
        location: location(home, &c.copy.path),
        modified_unix_seconds: crate::run::unix_seconds(c.copy.modified),
        frees_bytes: c.copy.frees_bytes(),
        shares_space: c.copy.shares_space(),
        can_move: c.stays.is_none(),
        stays_reason: c.stays.clone(),
        path: match mode {
            Mode::Developer => display_path(home, &c.copy.path),
            Mode::Simple => None,
        },
    }
}

/// The folder a copy is in, in plain words: "Documents › Trips".
fn location(home: &Path, path: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    match parent.strip_prefix(home) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel
            .iter()
            .map(|p| clean(&p.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" \u{203a} "),
        _ => "Home".to_string(),
    }
}

/// The file's name, safe to show.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| clean(&n.to_string_lossy()))
        .unwrap_or_default()
}

/// Text with control characters (such as new lines) taken out.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// One entry per distinct ticked copy, each naming a copy that stays.
fn entries_for(found: &Found, copy_ids: &[String]) -> Result<Vec<Entry>, CoreError> {
    let mut ticked: Vec<(usize, usize)> = Vec::new();
    for id in copy_ids {
        let at = find_copy(found, id).ok_or(CoreError::UnknownItem)?;
        if !ticked.contains(&at) {
            ticked.push(at);
        }
    }
    let mut entries = Vec::with_capacity(ticked.len());
    for &(g, c) in &ticked {
        let group = &found.groups[g];
        // The oldest copy the user didn't tick stays.
        let keep = group
            .copies
            .iter()
            .enumerate()
            .find(|(i, _)| !ticked.contains(&(g, *i)))
            .map(|(_, k)| k)
            .ok_or_else(|| CoreError::CleanupUnavailable {
                reason: format!(
                    "Keep at least one copy of \u{201c}{}\u{201d}. Untick one copy, then review again.",
                    group.name
                ),
            })?;
        let copy = &group.copies[c];
        let (request, wont_move) = match &copy.stays {
            Some(why) => (None, Some(why.clone())),
            None => (Some(request(copy, keep)), None),
        };
        entries.push(Entry {
            item_id: copy.copy_id.clone(),
            title: group.name.clone(),
            history_title: Some(format!("Copy of {}", group.name)),
            size_bytes: copy.copy.allocated_bytes,
            path: None,
            device_backup: None,
            restore: None,
            request,
            wont_move,
        });
    }
    Ok(entries)
}

/// Where the copy `id` is in the finished search.
fn find_copy(found: &Found, id: &str) -> Option<(usize, usize)> {
    found.groups.iter().enumerate().find_map(|(g, group)| {
        group
            .copies
            .iter()
            .position(|c| c.copy_id == id)
            .map(|c| (g, c))
    })
}

/// The request to move `copy`, keeping `keep`.
fn request(copy: &FoundCopy, keep: &FoundCopy) -> ItemRequest {
    ItemRequest {
        recommendation_id: copy.copy_id.clone(),
        path: copy.copy.path.clone(),
        expected: Expected {
            size_bytes: copy.copy.allocated_bytes,
            modified: copy.copy.modified,
        },
        // Trash only: the user's own files are never rebuilt by a tool.
        regenerable: false,
        // Every copy is its own tick box, and none is ticked for the user.
        selected_individually: true,
        owners: &[],
        belongs_to: None,
        keep_copy: Some(KeepCopy {
            path: keep.copy.path.clone(),
            dev: keep.copy.dev,
            ino: keep.copy.ino,
            len: keep.copy.len,
            modified: keep.copy.modified,
        }),
    }
}

/// The device id of the home folder.
fn home_dev(home: &Path) -> Result<u64, CoreError> {
    std::fs::symlink_metadata(home)
        .map(|m| m.dev())
        .map_err(|_| CoreError::HomeNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_are_plain_words() {
        let home = Path::new("/Users/sam");
        assert_eq!(
            location(home, Path::new("/Users/sam/Documents/Trips/a.mov")),
            "Documents \u{203a} Trips"
        );
        assert_eq!(location(home, Path::new("/Users/sam/a.mov")), "Home");
        assert_eq!(file_name(Path::new("/x/bad\nname.txt")), "badname.txt");
    }
}
