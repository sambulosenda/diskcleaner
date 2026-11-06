//! The only crate allowed to modify the filesystem.
//!
//! [`execute`] takes a [`planner::Plan`] by value (so it can be used once),
//! and for each item, immediately before acting:
//!
//! 1. runs [`safety::validate`] again (validation steps 1 to 6);
//! 2. opens every folder from home down to the item's parent with
//!    `O_NOFOLLOW`, so no symlink can be followed and the parent cannot be
//!    swapped after it was checked;
//! 3. walks the item ([`survey`]): applies the protected-path rules to every
//!    path inside it, refuses anything on another volume, and measures it;
//! 4. checks the size (within 1%) and modification time still match the plan
//!    (validation step 7), and that the item is still the same file or folder;
//! 5. moves it into `~/.Trash` with a move that never replaces anything.
//!
//! Permanent delete is allowed only when every item in the plan is
//! regenerable (CLAUDE.md safety rule 5). It moves the item into a fresh
//! folder inside `~/.Trash` first, the same way, and only then removes it, so
//! the removal never runs on a path the user's files could be swapped into.
//! If the removal stops partway, what is left stays in the Trash.
//!
//! Every item gets its own outcome in the [`Report`]; a refused item never
//! stops the others. The report is what History will record.
//!
//! [`preview`] is the dry run: it runs steps 1 to 4 for every item, exactly as
//! [`execute`] does, and stops there. It only opens folders and reads
//! metadata, so it never changes anything.
//!
//! [`put_back`] is the undo for a move to the Trash: it moves what this
//! executor moved back to where it was, only if the Trash still holds the
//! same thing and nothing new is in its old place.
//!
//! Not here yet: the allow-listed tool commands (`docs/safety/allowed-commands.md`),
//! writing to History, and the UniFFI commands. Each is its own change.

mod put_back;
mod survey;
mod sys;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use planner::{Plan, PlanError, PlanItem};
use safety::protected::{BlockReason, ReviewReason};
use safety::Refusal;

pub use put_back::{put_back, PutBack, PutBackRefusal};
pub use survey::size_matches;

/// The Trash folder inside home.
pub const TRASH_DIR: &str = ".Trash";

/// How many different names to try in the Trash before giving up.
const MAX_NAME_TRIES: u32 = 1000;

/// What to do with the items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Move to the Trash. The default.
    Trash,
    /// Remove for good. Only when every item is regenerable.
    Delete,
}

/// Why the whole plan was refused before anything was touched.
#[derive(Debug, thiserror::Error)]
pub enum ExecuteError {
    /// The plan has expired or the clock went back.
    #[error(transparent)]
    Plan(#[from] PlanError),
    /// Permanent delete was asked for, but not every item is regenerable.
    #[error("only things your tools can rebuild can be removed for good, so use the Trash")]
    DeleteNeedsRegenerable,
    /// The home folder could not be opened, or is on another volume.
    #[error("your home folder could not be opened")]
    HomeUnavailable,
    /// `~/.Trash` is missing, a symlink, or on another volume.
    #[error("the Trash could not be found")]
    TrashUnavailable,
}

/// Why one item was not touched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ItemRefusal {
    /// The path failed a safety check just before acting.
    #[error(transparent)]
    Safety(#[from] Refusal),
    /// The item's size, modification time or identity changed since review.
    #[error("this changed since you reviewed it")]
    ChangedSinceReview,
    /// Something inside the item is hard-blocked.
    #[error("it holds {reason}, which the app never removes")]
    ContainsProtected {
        /// The protected path inside the item.
        at: PathBuf,
        /// Why it is protected.
        reason: BlockReason,
    },
    /// Something inside the item is review-only and the item was not
    /// selected on its own.
    #[error("it holds {reason}, so you need to choose it yourself")]
    ContainsReviewOnly {
        /// The review-only path inside the item.
        at: PathBuf,
        /// Why it is review-only.
        reason: ReviewReason,
    },
    /// Something inside the item may not be removed in v1 at all.
    #[error("it holds {reason}, which the app does not remove")]
    ContainsNotRemovable {
        /// The path inside the item.
        at: PathBuf,
        /// Why it may not be removed.
        reason: ReviewReason,
    },
    /// Something inside the item is on another disk.
    #[error("part of it is on a different disk")]
    OtherVolumeInside {
        /// The first path found on another disk.
        at: PathBuf,
    },
    /// Part of the item could not be read, so it could not be checked.
    #[error("part of it could not be checked")]
    Unreadable {
        /// The path that could not be read.
        at: PathBuf,
    },
    /// Permanent delete was asked for, but a folder in the item cannot be
    /// emptied by this user, so the delete would stop partway. A read-only
    /// folder inside the item can still go to the Trash with it. If the item's
    /// own folder is read-only, the move to the Trash fails too (Linux and
    /// macOS need write permission to move a folder), and then nothing moves.
    #[error("this can't be removed for good, but it can go to the Trash")]
    NeedsTrash {
        /// The first folder whose contents cannot be removed.
        at: PathBuf,
    },
    /// A name inside the item is not valid text, so it could not be checked.
    #[error("part of it has a name the app cannot check")]
    UnusualName {
        /// The path with the unusual name.
        at: PathBuf,
    },
}

/// What happened to one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Moved into the Trash at `to`.
    MovedToTrash {
        /// Where it is now.
        to: PathBuf,
    },
    /// Removed for good.
    Deleted,
    /// Not touched, for this reason.
    Refused(ItemRefusal),
    /// The move or removal failed.
    Failed {
        /// The system's description of the error.
        error: String,
        /// For a removal that stopped partway: where what is left now is,
        /// inside the Trash. `None` if the item was not moved at all.
        left_in: Option<PathBuf>,
    },
}

/// The result for one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReport {
    /// The item's recommendation.
    pub recommendation_id: String,
    /// The item's path when the plan was built.
    pub path: PathBuf,
    /// Allocated bytes moved or removed; 0 unless it succeeded.
    pub bytes: u64,
    /// What happened.
    pub outcome: Outcome,
    /// For an item moved to the Trash: the inode number of what was moved,
    /// so [`put_back`] can check the Trash still holds the same thing.
    pub moved_ino: Option<u64>,
}

/// The result of one plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The plan's id.
    pub plan_id: u64,
    /// Trash or delete.
    pub mode: Mode,
    /// One entry per item, in plan order.
    pub items: Vec<ItemReport>,
}

impl Report {
    /// Total bytes moved or removed.
    pub fn total_bytes(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }
}

/// What the dry run found for one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemCheck {
    /// The item's recommendation.
    pub recommendation_id: String,
    /// The item's path when the plan was built.
    pub path: PathBuf,
    /// The allocated bytes that would move, measured just now, or why the
    /// item would be refused.
    pub result: Result<u64, ItemRefusal>,
}

/// The dry run: runs every check [`execute`] runs before acting (steps 1 to
/// 4), for every item, and touches nothing. The plan is borrowed, not used
/// up, so the same plan can then be passed to [`execute`].
///
/// An item that passes here can still be refused by [`execute`] if it
/// changes in between; `execute` runs every check again.
pub fn preview(plan: &Plan, mode: Mode, now: SystemTime) -> Result<Vec<ItemCheck>, ExecuteError> {
    plan.check_fresh(now)?;
    if mode == Mode::Delete && !plan.all_regenerable() {
        return Err(ExecuteError::DeleteNeedsRegenerable);
    }
    let ctx = Context::open(plan.home(), plan.scan_root_dev())?;
    let checks = plan
        .items()
        .iter()
        .map(|item| ItemCheck {
            recommendation_id: item.recommendation_id().to_string(),
            path: item.path().to_path_buf(),
            result: ctx.prepare(item, mode).map(|ready| ready.survey.size_bytes),
        })
        .collect();
    Ok(checks)
}

/// Carries out `plan`. The plan is used up whatever happens.
///
/// `now` is the current time, used to check the plan has not expired.
pub fn execute(plan: Plan, mode: Mode, now: SystemTime) -> Result<Report, ExecuteError> {
    plan.check_fresh(now)?;
    if mode == Mode::Delete && !plan.all_regenerable() {
        return Err(ExecuteError::DeleteNeedsRegenerable);
    }
    let home = plan.home().to_path_buf();
    let dev = plan.scan_root_dev();
    let plan_id = plan.id();
    let ctx = Context::open(&home, dev)?;
    tracing::info!(plan_id, ?mode, items = plan.items().len(), "executing plan");
    let items = plan
        .into_items()
        .into_iter()
        .enumerate()
        .map(|(n, item)| {
            let report = ctx.act(item, mode, plan_id, n);
            tracing::info!(
                plan_id,
                recommendation_id = %report.recommendation_id,
                bytes = report.bytes,
                outcome = ?report.outcome,
                "item done"
            );
            report
        })
        .collect();
    Ok(Report {
        plan_id,
        mode,
        items,
    })
}

/// Open handles shared by every item of one plan.
struct Context {
    home: PathBuf,
    dev: u64,
    home_fd: OwnedFd,
    trash_fd: OwnedFd,
    trash_path: PathBuf,
}

/// An item checked and ready to move: its parent folder is open, and it is
/// still the file or folder that was measured.
struct Ready {
    parent_fd: OwnedFd,
    name: OsString,
    survey: survey::Survey,
}

impl Context {
    /// Opens home and `~/.Trash`, refusing symlinks and other volumes.
    fn open(home: &Path, dev: u64) -> Result<Context, ExecuteError> {
        let home_fd = sys::open_dir(home).map_err(|_| ExecuteError::HomeUnavailable)?;
        match sys::fstat(&home_fd) {
            Ok(st) if st.dev == dev => {}
            _ => return Err(ExecuteError::HomeUnavailable),
        }
        let trash_fd = sys::open_dir_at(&home_fd, OsStr::new(TRASH_DIR))
            .map_err(|_| ExecuteError::TrashUnavailable)?;
        match sys::fstat(&trash_fd) {
            Ok(st) if st.dev == dev => {}
            _ => return Err(ExecuteError::TrashUnavailable),
        }
        Ok(Context {
            home: home.to_path_buf(),
            dev,
            home_fd,
            trash_fd,
            trash_path: home.join(TRASH_DIR),
        })
    }

    /// Checks and acts on one item. Never panics; every failure is an outcome.
    fn act(&self, item: PlanItem, mode: Mode, plan_id: u64, n: usize) -> ItemReport {
        let path = item.path().to_path_buf();
        let mut moved_ino = None;
        let (bytes, outcome) = match self.prepare(&item, mode) {
            Err(refusal) => (0, Outcome::Refused(refusal)),
            Ok(ready) => {
                let size = ready.survey.size_bytes;
                let outcome = match mode {
                    Mode::Trash => self.move_to_trash(&ready),
                    Mode::Delete => self.delete(&ready, plan_id, n),
                };
                if matches!(outcome, Outcome::MovedToTrash { .. }) {
                    moved_ino = Some(ready.survey.ino);
                }
                let bytes = match outcome {
                    Outcome::MovedToTrash { .. } | Outcome::Deleted => size,
                    _ => 0,
                };
                (bytes, outcome)
            }
        };
        ItemReport {
            recommendation_id: item.recommendation_id().to_string(),
            path,
            bytes,
            outcome,
            moved_ino,
        }
    }

    /// Every check that runs immediately before acting.
    fn prepare(&self, item: &PlanItem, mode: Mode) -> Result<Ready, ItemRefusal> {
        let path = item.path();
        safety::validate(&self.home, self.dev, path, item.selected_individually())?;
        let (parent_fd, name) = self.open_parent(path)?;
        let before =
            sys::stat_at(&parent_fd, &name).map_err(|_| ItemRefusal::ChangedSinceReview)?;
        if before.kind == sys::Kind::Symlink || before.dev != self.dev {
            return Err(ItemRefusal::ChangedSinceReview);
        }
        let survey = survey::survey(&self.home, path, self.dev, item.selected_individually())?;
        if mode == Mode::Delete {
            if let Some(at) = survey.locked_dir.clone() {
                return Err(ItemRefusal::NeedsTrash { at });
            }
        }
        let expected = item.expected();
        if !size_matches(expected.size_bytes, survey.size_bytes)
            || survey.modified != expected.modified
        {
            return Err(ItemRefusal::ChangedSinceReview);
        }
        // The walk read the item by its path. Make sure that path still names
        // the same file or folder our open parent holds.
        let after = sys::stat_at(&parent_fd, &name).map_err(|_| ItemRefusal::ChangedSinceReview)?;
        if after != before || (survey.dev, survey.ino) != (before.dev, before.ino) {
            return Err(ItemRefusal::ChangedSinceReview);
        }
        Ok(Ready {
            parent_fd,
            name,
            survey,
        })
    }

    /// Opens each folder from home down to `path`'s parent without following
    /// symlinks, checking each is on the scan's volume. Returns the open
    /// parent and the item's own name.
    fn open_parent(&self, path: &Path) -> Result<(OwnedFd, OsString), ItemRefusal> {
        let rest = path
            .strip_prefix(&self.home)
            .map_err(|_| ItemRefusal::Safety(Refusal::OutsideHome))?;
        let parts: Vec<&OsStr> = rest.iter().collect();
        let Some((name, parents)) = parts.split_last() else {
            return Err(ItemRefusal::Safety(Refusal::NotCanonical));
        };
        let mut fd: Option<OwnedFd> = None;
        let mut at = self.home.clone();
        for part in parents {
            at.push(part);
            let parent = fd.as_ref().unwrap_or(&self.home_fd);
            let next = sys::open_dir_at(parent, part)
                .map_err(|_| ItemRefusal::Unreadable { at: at.clone() })?;
            match sys::fstat(&next) {
                Ok(st) if st.dev == self.dev => {}
                _ => return Err(ItemRefusal::OtherVolumeInside { at }),
            }
            fd = Some(next);
        }
        let parent_fd = match fd {
            Some(fd) => fd,
            None => self
                .home_fd
                .try_clone()
                .map_err(|_| ItemRefusal::Unreadable {
                    at: self.home.clone(),
                })?,
        };
        Ok((parent_fd, name.to_os_string()))
    }

    /// Moves the item into `~/.Trash`, under a free name.
    fn move_to_trash(&self, ready: &Ready) -> Outcome {
        match move_under_free_name(
            &ready.parent_fd,
            &ready.name,
            &self.trash_fd,
            ready.survey.is_dir,
        ) {
            Ok(new_name) => Outcome::MovedToTrash {
                to: self.trash_path.join(new_name),
            },
            Err(err) => Outcome::Failed {
                error: err.to_string(),
                left_in: None,
            },
        }
    }

    /// Moves the item into a fresh folder in the Trash, then removes it.
    fn delete(&self, ready: &Ready, plan_id: u64, n: usize) -> Outcome {
        let (staging_name, staging_fd) = match self.make_staging(plan_id, n) {
            Ok(made) => made,
            Err(err) => {
                return Outcome::Failed {
                    error: err.to_string(),
                    left_in: None,
                }
            }
        };
        if let Err(err) =
            sys::rename_no_replace(&ready.parent_fd, &ready.name, &staging_fd, &ready.name)
        {
            // Nothing moved; tidy the empty folder we made. Best effort.
            let _ = fs::remove_dir(self.trash_path.join(&staging_name));
            return Outcome::Failed {
                error: err.to_string(),
                left_in: None,
            };
        }
        let staging_path = self.trash_path.join(&staging_name);
        match remove_staged(&staging_fd, &staging_path, &ready.name, &ready.survey) {
            Ok(()) => Outcome::Deleted,
            Err(err) => Outcome::Failed {
                error: err.to_string(),
                left_in: Some(staging_path),
            },
        }
    }

    /// Makes an empty, uniquely named folder in the Trash for one removal.
    fn make_staging(&self, plan_id: u64, n: usize) -> std::io::Result<(OsString, OwnedFd)> {
        let mut last_err = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
        for attempt in 0..MAX_NAME_TRIES {
            let name = OsString::from(format!("Roomy removing {plan_id}-{n}-{attempt}"));
            match sys::mkdir_at(&self.trash_fd, &name) {
                Ok(()) => {
                    let fd = sys::open_dir_at(&self.trash_fd, &name)?;
                    return Ok((name, fd));
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => last_err = err,
                Err(err) => return Err(err),
            }
        }
        Err(last_err)
    }
}

/// Moves `name` from `from_dir` into `to_dir`, keeping its name if free, or
/// adding " 2", " 3" and so on (before a file's extension) like Finder does.
/// Never replaces anything. Returns the name used.
fn move_under_free_name(
    from_dir: &OwnedFd,
    name: &OsStr,
    to_dir: &OwnedFd,
    is_dir: bool,
) -> std::io::Result<OsString> {
    let mut last_err = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    for attempt in 1..=MAX_NAME_TRIES {
        let candidate = numbered_name(name, attempt, is_dir);
        match sys::rename_no_replace(from_dir, name, to_dir, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => last_err = err,
            Err(err) => return Err(err),
        }
    }
    Err(last_err)
}

/// `name` for attempt 1; then `name 2`, `name 3`... For a file with an
/// extension, the number goes before it (`notes 2.txt`).
fn numbered_name(name: &OsStr, attempt: u32, is_dir: bool) -> OsString {
    if attempt <= 1 {
        return name.to_os_string();
    }
    let bytes = name.as_bytes();
    let dot = if is_dir {
        None
    } else {
        bytes.iter().rposition(|b| *b == b'.').filter(|i| *i > 0)
    };
    let (stem, ext) = match dot {
        Some(i) => bytes.split_at(i),
        None => (bytes, &b""[..]),
    };
    let mut out = stem.to_vec();
    out.extend_from_slice(format!(" {attempt}").as_bytes());
    out.extend_from_slice(ext);
    OsString::from_vec(out)
}

/// Removes an item already moved into the staging folder, then the folder.
///
/// Checks first that the staging folder and the item are still the ones we
/// made and moved. `remove_dir_all` does not follow symlinks inside the tree.
fn remove_staged(
    staging_fd: &OwnedFd,
    staging_path: &Path,
    name: &OsStr,
    survey: &survey::Survey,
) -> std::io::Result<()> {
    let changed = || std::io::Error::other("the item changed while it was being removed");
    let staging = sys::fstat(staging_fd)?;
    let on_disk = staging_path.symlink_metadata()?;
    if std::os::unix::fs::MetadataExt::ino(&on_disk) != staging.ino
        || on_disk.file_type().is_symlink()
    {
        return Err(changed());
    }
    let item = sys::stat_at(staging_fd, name)?;
    if (item.dev, item.ino) != (survey.dev, survey.ino) {
        return Err(changed());
    }
    let item_path = staging_path.join(name);
    if item.kind == sys::Kind::Dir {
        fs::remove_dir_all(&item_path)?;
    } else {
        fs::remove_file(&item_path)?;
    }
    fs::remove_dir(staging_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_names_keep_the_extension_of_files() {
        let n = |s: &str, a, d| numbered_name(OsStr::new(s), a, d);
        assert_eq!(n("notes.txt", 1, false), "notes.txt");
        assert_eq!(n("notes.txt", 2, false), "notes 2.txt");
        assert_eq!(n("archive.tar.gz", 3, false), "archive.tar 3.gz");
        assert_eq!(n(".hidden", 2, false), ".hidden 2");
        assert_eq!(n("com.google.Chrome", 2, true), "com.google.Chrome 2");
        assert_eq!(n("README", 2, false), "README 2");
    }
}
