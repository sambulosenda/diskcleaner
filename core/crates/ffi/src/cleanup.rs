//! The cleanup preview (dry run): turns the items the user ticked into a
//! single-use plan and reports exactly what would move to the Trash.
//!
//! Nothing here changes a file. The app sends item ids only; every path,
//! size and modification time comes from the engine's own scan. The checks
//! are the executor's own (`executor::preview`), so what the preview says
//! would move is what the executor would move if nothing changes meanwhile.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;

use executor::ItemRefusal;
use planner::{ItemRequest, Plan, PlanError};

use crate::types::{
    CleanupCheck, CleanupChoice, CleanupPreview, CleanupPreviewItem, DeviceBackup, PlanHandle,
};
use crate::views::{display_path, Finished};
use crate::CoreError;

/// Shown when the Trash cannot be opened, so nothing could move.
/// Shown when macOS refuses to let Roomy open the Trash (privacy protection).
const TRASH_NOT_ALLOWED: &str = "macOS didn't let Roomy open your Trash, so nothing can be moved. Turn on Full Disk Access for Roomy in System Settings, then try again.";
const TRASH_UNAVAILABLE: &str = "Roomy couldn't open your Trash, so nothing can be moved.";
/// Shown when the home folder cannot be opened, so nothing could move.
const HOME_UNAVAILABLE: &str = "Roomy couldn't open your home folder, so nothing can be moved.";
/// Shown for anything else that stops the whole preview.
const PREVIEW_FAILED: &str = "Roomy couldn't check these items. Please scan again.";
/// Shown for an item inside another chosen item. The classifier never makes
/// such items, so this is a guard, not something users should see.
const INSIDE_ANOTHER: &str = "It is inside another item you chose.";
/// Shown for a duplicate whose kept copy was also chosen. The duplicate
/// finder never builds such a plan, so this is a guard.
const KEEP_COPY_MOVES: &str = "The copy you're keeping was chosen too, so this one stays.";

/// What one ticked item looks like on its way through the preview.
pub(crate) struct Entry {
    pub item_id: String,
    pub title: String,
    /// The name History and the result screen use, if not `title`.
    pub history_title: Option<String>,
    pub size_bytes: u64,
    pub path: Option<String>,
    /// Which device it belongs to, for an iPhone or iPad backup.
    pub device_backup: Option<DeviceBackup>,
    /// How it comes back after removal, in the rule's words, if it does.
    pub restore: Option<String>,
    /// The request, while the item is still in the running.
    pub request: Option<ItemRequest>,
    /// Why it would not move, once that is known.
    pub wont_move: Option<String>,
}

/// A preview, plus the plan to keep for the run (if anything would move).
pub(crate) struct Prepared {
    pub preview: CleanupPreview,
    pub plan: Option<Pending>,
}

/// The plan waiting for the user, with what the History and the result
/// screen need to name each item.
pub(crate) struct Pending {
    /// The single-use plan of the items that would move.
    pub plan: Plan,
    /// Title of each item in the plan, by item id.
    pub titles: HashMap<String, String>,
}

/// Builds the preview for `choices` against the finished scan `f`.
///
/// `plan_id` is the id the plan gets if anything would move.
pub(crate) fn prepare(
    f: &Finished,
    choices: &[CleanupChoice],
    plan_id: u64,
    guards: &executor::Guards,
    now: SystemTime,
) -> Result<Prepared, CoreError> {
    let entries = entries_for(f, choices)?;
    check_entries(&f.home, f.root_dev, entries, plan_id, guards, now)
}

/// Checks `entries` exactly as a cleanup would just before acting, and
/// keeps a plan of the ones that would move. Shared by the cleanup preview
/// and the app uninstaller.
///
/// `home` and `dev` are the home folder and its volume.
pub(crate) fn check_entries(
    home: &Path,
    dev: u64,
    mut entries: Vec<Entry>,
    plan_id: u64,
    guards: &executor::Guards,
    now: SystemTime,
) -> Result<Prepared, CoreError> {
    if entries.is_empty() {
        return Err(CoreError::NothingSelected);
    }
    // Plan-time path checks (safety steps 1 to 6). One refused item would
    // refuse the whole plan, so drop it, note why, and build again.
    let Some(plan) = build_dropping_refused(home, dev, &mut entries, plan_id, now)? else {
        return Ok(finish(entries, None));
    };
    // The executor's checks just before acting (steps 1 to 7, then the
    // extra checks: folders you keep, apps open, git work, recent changes),
    // touching nothing.
    let checks =
        executor::preview(&plan, executor::Mode::Trash, guards, now).map_err(execute_error)?;
    drop(plan);
    for check in checks {
        let Some(entry) = entries
            .iter_mut()
            .find(|e| e.item_id == check.recommendation_id)
        else {
            continue;
        };
        match check.result {
            Ok(bytes) => entry.size_bytes = bytes,
            Err(refusal) => {
                entry.request = None;
                entry.wont_move = Some(item_refusal(&refusal));
            }
        }
    }
    // The plan kept for the run holds only the items that passed.
    let plan = build_dropping_refused(home, dev, &mut entries, plan_id, now)?;
    Ok(finish(entries, plan))
}

/// One entry per distinct ticked id, in the order ticked. Items that can't
/// be offered at all (`keep`, unknown, not a Trash item) are marked here.
fn entries_for(f: &Finished, choices: &[CleanupChoice]) -> Result<Vec<Entry>, CoreError> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for choice in choices {
        if !seen.insert(choice.item_id.as_str()) {
            continue;
        }
        let Some(item) = f.item(&choice.item_id) else {
            let file = f.loose.get(&choice.item_id).ok_or(CoreError::UnknownItem)?;
            entries.push(file_entry(f, file, choice.chosen_alone));
            continue;
        };
        let (request, wont_move) = match planner::request_for(item, choice.chosen_alone) {
            Ok(req) => (Some(req), None),
            Err(why) => (None, Some(sentence(&why.to_string()))),
        };
        entries.push(Entry {
            item_id: choice.item_id.clone(),
            title: item.title.clone(),
            history_title: None,
            size_bytes: item.size_bytes,
            path: match f.mode {
                crate::Mode::Developer => display_path(&f.home, &item.path),
                crate::Mode::Simple => None,
            },
            device_backup: crate::backup::device_backup(f, item),
            restore: item.restore.clone(),
            request,
            wont_move,
        });
    }
    Ok(entries)
}

/// The entry for a ticked row of the Large files screen.
fn file_entry(f: &Finished, file: &crate::files::LooseFile, chosen_alone: bool) -> Entry {
    Entry {
        item_id: file.id.clone(),
        title: file.name.clone(),
        history_title: None,
        size_bytes: file.expected.size_bytes,
        path: match f.mode {
            crate::Mode::Developer => display_path(&f.home, &file.path),
            crate::Mode::Simple => None,
        },
        device_backup: None,
        restore: None,
        request: Some(crate::files::request_for(file, chosen_alone)),
        wont_move: None,
    }
}

/// Builds a plan of every entry still in the running. An entry the planner
/// refuses is marked and left out, and the plan is built again. Returns
/// `None` if no entry is left.
fn build_dropping_refused(
    home: &Path,
    dev: u64,
    entries: &mut [Entry],
    plan_id: u64,
    now: SystemTime,
) -> Result<Option<Plan>, CoreError> {
    // Each round either succeeds or takes one entry out, so this ends.
    for _ in 0..=entries.len() {
        let requests: Vec<ItemRequest> = entries.iter().filter_map(|e| e.request.clone()).collect();
        if requests.is_empty() {
            return Ok(None);
        }
        let (id, why) = match Plan::build(plan_id, home, dev, requests, now) {
            Ok(plan) => return Ok(Some(plan)),
            Err(PlanError::Refused {
                recommendation_id,
                refusal,
            }) => (recommendation_id, sentence(&refusal.to_string())),
            Err(PlanError::Overlap { inner, .. }) => (inner, INSIDE_ANOTHER.to_string()),
            Err(PlanError::KeepCopyMoves { recommendation_id }) => {
                (recommendation_id, KEEP_COPY_MOVES.to_string())
            }
            Err(err) => {
                tracing::warn!(%err, "cleanup plan could not be built");
                return Err(CoreError::CleanupUnavailable {
                    reason: PREVIEW_FAILED.to_string(),
                });
            }
        };
        let entry = entries
            .iter_mut()
            .find(|e| e.item_id == id && e.request.is_some())
            .ok_or_else(|| CoreError::CleanupUnavailable {
                reason: PREVIEW_FAILED.to_string(),
            })?;
        entry.request = None;
        entry.wont_move = Some(why);
    }
    Err(CoreError::CleanupUnavailable {
        reason: PREVIEW_FAILED.to_string(),
    })
}

/// The preview the app sees, and the plan to keep.
fn finish(entries: Vec<Entry>, plan: Option<Plan>) -> Prepared {
    let titles: HashMap<String, String> = entries
        .iter()
        .filter(|e| e.wont_move.is_none())
        .map(|e| {
            let title = e.history_title.clone().unwrap_or_else(|| e.title.clone());
            (e.item_id.clone(), title)
        })
        .collect();
    let items: Vec<CleanupPreviewItem> = entries
        .into_iter()
        .map(|e| CleanupPreviewItem {
            item_id: e.item_id,
            title: e.title,
            size_bytes: e.size_bytes,
            check: match e.wont_move {
                None => CleanupCheck::WillMove,
                Some(reason) => CleanupCheck::WontMove { reason },
            },
            path: e.path,
            device_backup: e.device_backup,
            restore: e.restore,
        })
        .collect();
    let bytes_to_move = items
        .iter()
        .filter(|i| i.check == CleanupCheck::WillMove)
        .fold(0u64, |sum, i| sum.saturating_add(i.size_bytes));
    Prepared {
        preview: CleanupPreview {
            plan: plan.as_ref().map(|p| PlanHandle { id: p.id() }),
            items,
            bytes_to_move,
            expires_in_seconds: planner::PLAN_LIFETIME.as_secs(),
        },
        plan: plan.map(|plan| Pending { plan, titles }),
    }
}

/// Plain words for an item the executor would refuse. Never holds a path.
fn item_refusal(refusal: &ItemRefusal) -> String {
    sentence(&refusal.to_string())
}

/// Plain words for an error that stops the whole preview. The error itself
/// goes to the log only.
fn execute_error(err: executor::ExecuteError) -> CoreError {
    tracing::warn!(%err, "cleanup preview refused");
    let reason = match err {
        executor::ExecuteError::TrashUnavailable => TRASH_UNAVAILABLE,
        executor::ExecuteError::TrashNotAllowed => TRASH_NOT_ALLOWED,
        executor::ExecuteError::HomeUnavailable => HOME_UNAVAILABLE,
        executor::ExecuteError::Plan(_) | executor::ExecuteError::DeleteNeedsRegenerable => {
            PREVIEW_FAILED
        }
    };
    CoreError::CleanupUnavailable {
        reason: reason.to_string(),
    }
}

/// `text` with a capital first letter and a full stop, ready to show.
pub(crate) fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => return String::new(),
    };
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_read_as_sentences() {
        assert_eq!(
            sentence("this changed since you reviewed it"),
            "This changed since you reviewed it."
        );
        assert_eq!(sentence("Done."), "Done.");
        assert_eq!(sentence(""), "");
    }

    #[test]
    fn item_refusals_never_carry_a_path() {
        let at = std::path::PathBuf::from("/Users/sam/secret");
        let refusals = [
            ItemRefusal::ChangedSinceReview,
            ItemRefusal::Unreadable { at: at.clone() },
            ItemRefusal::OtherVolumeInside { at: at.clone() },
            ItemRefusal::NeedsTrash { at: at.clone() },
            ItemRefusal::UnusualName { at: at.clone() },
            ItemRefusal::ContainsProtected {
                at: at.clone(),
                reason: safety::protected::BlockReason::GitRepository,
            },
            ItemRefusal::Safety(safety::Refusal::Symlink { at }),
        ];
        for r in &refusals {
            let text = item_refusal(r);
            assert!(!text.contains("secret"), "{text}");
        }
    }
}
