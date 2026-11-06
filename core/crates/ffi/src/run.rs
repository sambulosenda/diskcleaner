//! Carrying out a cleanup plan, putting the last cleanup back, and History.
//!
//! The only file changes are the executor's: [`executor::execute`] moves the
//! plan's items to the Trash, and [`executor::put_back`] moves them back.
//! The app sends a plan handle or cleanup handle, never a path. Put back
//! works only from the executor's own report of the last cleanup, held in
//! memory, never from what History stored.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use executor::{ExecuteError, Outcome, PutBack, Report};

use crate::cleanup::{sentence, Pending};
use crate::types::{CleanupHandle, CleanupResult, CleanupResultItem, HistoryEntry, PutBackResult};
use crate::CoreError;

/// Shown when the plan is not the one waiting (already used, discarded or
/// replaced by a newer review).
const PLAN_GONE: &str = "This review is no longer open. Please review again.";
/// Shown when the review has expired.
const PLAN_EXPIRED: &str = "This review is more than ten minutes old. Please review again.";
/// Shown when the Trash can't be opened.
const TRASH_UNAVAILABLE: &str = "Roomy couldn't open your Trash, so nothing was moved.";
/// Shown when the home folder can't be opened.
const HOME_UNAVAILABLE: &str = "Roomy couldn't open your home folder, so nothing was moved.";
/// Shown for a move or removal that failed partway.
const MOVE_FAILED: &str = "The move to the Trash did not work.";
/// Shown when there is nothing to put back.
const NOTHING_TO_PUT_BACK: &str =
    "Roomy can only put back its most recent cleanup, until Roomy quits. You can still drag items back from the Trash.";

/// The last cleanup, kept so it can be put back once.
pub(crate) struct LastCleanup {
    /// Its handle.
    pub handle: CleanupHandle,
    /// Its id in History, if it was saved there.
    pub history_id: Option<i64>,
    /// The home and volume the plan was for.
    pub home: PathBuf,
    pub dev: u64,
    /// Every item that moved, from the executor's report.
    pub moved: Vec<PutBack>,
    /// Title and size of each moved item, by item id.
    pub rows: HashMap<String, (String, u64)>,
}

/// What [`run`] hands back: the result for the app, and the undo record.
pub(crate) struct Ran {
    pub result: CleanupResult,
    pub last: Option<LastCleanup>,
    /// What to write to History.
    pub history_items: Vec<history::Item>,
}

/// Carries out the waiting plan: moves its items to the Trash.
///
/// `cleanup_id` is the number the handle gets if anything moved.
pub(crate) fn run(pending: Pending, cleanup_id: u64, now: SystemTime) -> Result<Ran, CoreError> {
    let Pending { plan, titles } = pending;
    let home = plan.home().to_path_buf();
    let dev = plan.scan_root_dev();
    let report = executor::execute(plan, executor::Mode::Trash, now).map_err(execute_error)?;
    Ok(from_report(&report, &titles, home, dev, cleanup_id))
}

/// Turns the executor's report into the app's result, History's rows and
/// the undo record.
fn from_report(
    report: &Report,
    titles: &HashMap<String, String>,
    home: PathBuf,
    dev: u64,
    cleanup_id: u64,
) -> Ran {
    let mut items = Vec::new();
    let mut history_items = Vec::new();
    let mut moved = Vec::new();
    let mut rows = HashMap::new();
    for item in &report.items {
        let title = titles
            .get(&item.recommendation_id)
            .cloned()
            .unwrap_or_default();
        let reason = match (&item.outcome, item.moved_ino) {
            (Outcome::MovedToTrash { to }, Some(ino)) => {
                moved.push(PutBack {
                    recommendation_id: item.recommendation_id.clone(),
                    original: item.path.clone(),
                    in_trash: to.clone(),
                    ino,
                });
                rows.insert(item.recommendation_id.clone(), (title.clone(), item.bytes));
                None
            }
            (Outcome::Refused(refusal), _) => Some(sentence(&refusal.to_string())),
            // Trash mode never deletes; any other outcome is a failure.
            _ => Some(MOVE_FAILED.to_string()),
        };
        history_items.push(history::Item {
            item_id: item.recommendation_id.clone(),
            title: title.clone(),
            size_bytes: item.bytes,
            state: match &reason {
                None => history::ItemState::Moved,
                Some(reason) => history::ItemState::NotMoved {
                    reason: reason.clone(),
                },
            },
        });
        items.push(CleanupResultItem {
            item_id: item.recommendation_id.clone(),
            title,
            size_bytes: item.bytes,
            done: reason.is_none(),
            reason,
        });
    }
    let handle = CleanupHandle { id: cleanup_id };
    let last = (!moved.is_empty()).then_some(LastCleanup {
        handle,
        history_id: None,
        home,
        dev,
        moved,
        rows,
    });
    Ran {
        result: CleanupResult {
            cleanup: last.as_ref().map(|l| l.handle),
            items,
            bytes_moved: report.total_bytes(),
            saved_to_history: false,
        },
        last,
        history_items,
    }
}

/// For each item that had moved: `None` if it was put back, or why it
/// stayed in the Trash. What History records.
pub(crate) type PutBackRows = Vec<(String, Option<String>)>;

/// Puts the last cleanup back. Returns the app's result and the rows for
/// History (`None` = put back, or the reason it stayed in the Trash).
pub(crate) fn put_back(last: &LastCleanup) -> Result<(PutBackResult, PutBackRows), CoreError> {
    let results = executor::put_back(&last.home, last.dev, &last.moved).map_err(execute_error)?;
    let mut items = Vec::new();
    let mut rows = Vec::new();
    for (entry, result) in last.moved.iter().zip(results) {
        let (title, size) = last
            .rows
            .get(&entry.recommendation_id)
            .cloned()
            .unwrap_or_default();
        let reason = result.err().map(|r| sentence(&r.to_string()));
        rows.push((entry.recommendation_id.clone(), reason.clone()));
        items.push(CleanupResultItem {
            item_id: entry.recommendation_id.clone(),
            title,
            size_bytes: size,
            done: reason.is_none(),
            reason,
        });
    }
    let bytes_put_back = items
        .iter()
        .filter(|i| i.done)
        .fold(0u64, |sum, i| sum.saturating_add(i.size_bytes));
    Ok((
        PutBackResult {
            items,
            bytes_put_back,
        },
        rows,
    ))
}

/// The error for a plan that is not the waiting one.
pub(crate) fn plan_gone() -> CoreError {
    CoreError::CleanupUnavailable {
        reason: PLAN_GONE.to_string(),
    }
}

/// The error for a cleanup that can't be put back.
pub(crate) fn nothing_to_put_back() -> CoreError {
    CoreError::CleanupUnavailable {
        reason: NOTHING_TO_PUT_BACK.to_string(),
    }
}

/// Seconds since 1970, for History. A clock before 1970 reads as 0.
pub(crate) fn unix_seconds(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// History entries for the app.
pub(crate) fn entries(cleanups: Vec<history::Cleanup>) -> Vec<HistoryEntry> {
    cleanups.into_iter().map(HistoryEntry::from).collect()
}

/// Plain words for an error that stops the whole cleanup or put back.
fn execute_error(err: ExecuteError) -> CoreError {
    tracing::warn!(%err, "cleanup refused");
    let reason = match err {
        ExecuteError::Plan(planner::PlanError::Expired)
        | ExecuteError::Plan(planner::PlanError::ClockWentBack) => PLAN_EXPIRED,
        ExecuteError::TrashUnavailable => TRASH_UNAVAILABLE,
        ExecuteError::HomeUnavailable => HOME_UNAVAILABLE,
        ExecuteError::Plan(_) | ExecuteError::DeleteNeedsRegenerable => PLAN_GONE,
    };
    CoreError::CleanupUnavailable {
        reason: reason.to_string(),
    }
}
