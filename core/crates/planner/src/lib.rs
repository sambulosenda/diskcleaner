//! Turn selected recommendation IDs into a single-use plan. Never touches the
//! filesystem itself: plan-time path checks are done by the `safety` crate,
//! which only reads metadata.
//!
//! This crate holds the [`Plan`] type the executor acts on. Mapping
//! recommendation IDs from the UI to [`ItemRequest`]s (finding each item's
//! path, size and modification time in the scan) is a later task.
//!
//! A plan:
//! - holds only paths that passed [`safety::validate`] when it was built;
//! - has no two items where one is the same as, or inside, another;
//! - expires [`PLAN_LIFETIME`] after it was built;
//! - is single use: it cannot be cloned, and the executor takes it by value.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use safety::{Refusal, ValidatedPath};

/// How long a plan stays usable after it was built (CLAUDE.md safety rule 4).
pub const PLAN_LIFETIME: Duration = Duration::from_secs(10 * 60);

/// What the item looked like when the user reviewed it. The executor refuses
/// the item if it no longer matches (validation step 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expected {
    /// Allocated bytes (`st_blocks * 512`) of the item and everything inside
    /// it, symlinks counted as themselves, each hard-linked file counted once.
    /// This is how the scanner measures a folder's `total_bytes`.
    pub size_bytes: u64,
    /// The item's own modification time (`lstat`, never following a symlink).
    pub modified: SystemTime,
}

/// One item the user selected, before plan-time validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRequest {
    /// The recommendation this item came from. Recorded in History.
    pub recommendation_id: String,
    /// Absolute path, from the engine's own scan. Never from the UI.
    pub path: PathBuf,
    /// What the item looked like when the user reviewed it.
    pub expected: Expected,
    /// Whether every byte of the item can be rebuilt by its tool (from the
    /// matching rule's `regenerable`). Permanent delete needs this on every item.
    pub regenerable: bool,
    /// Whether the user selected this item on its own (not as part of a group).
    pub selected_individually: bool,
}

/// One item of a built plan. Only [`Plan::build`] makes these.
#[derive(Debug, PartialEq, Eq)]
pub struct PlanItem {
    recommendation_id: String,
    path: ValidatedPath,
    expected: Expected,
    regenerable: bool,
    selected_individually: bool,
}

impl PlanItem {
    /// The recommendation this item came from.
    pub fn recommendation_id(&self) -> &str {
        &self.recommendation_id
    }

    /// The path, as validated when the plan was built.
    pub fn path(&self) -> &Path {
        self.path.path()
    }

    /// What the item looked like when the user reviewed it.
    pub fn expected(&self) -> Expected {
        self.expected
    }

    /// Whether the item can be rebuilt by its tool.
    pub fn regenerable(&self) -> bool {
        self.regenerable
    }

    /// Whether the user selected this item on its own.
    pub fn selected_individually(&self) -> bool {
        self.selected_individually
    }
}

/// Why a plan could not be built, or can no longer be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// Nothing was selected.
    #[error("nothing was selected")]
    Empty,
    /// One item failed a plan-time safety check. The whole plan is refused.
    #[error("one of the items cannot be removed: {refusal}")]
    Refused {
        /// The item's recommendation.
        recommendation_id: String,
        /// Why it was refused.
        refusal: Refusal,
    },
    /// Two items are the same place, or one is inside the other.
    #[error("two of the items overlap")]
    Overlap {
        /// The outer (or first) item's recommendation.
        outer: String,
        /// The inner (or repeated) item's recommendation.
        inner: String,
    },
    /// The plan is older than [`PLAN_LIFETIME`].
    #[error("this review has expired, so please review again")]
    Expired,
    /// The clock is earlier than when the plan was built.
    #[error("the clock changed since the review, so please review again")]
    ClockWentBack,
}

/// A single-use, validated list of items to clean. Deliberately not `Clone`.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    id: u64,
    created_at: SystemTime,
    home: PathBuf,
    scan_root_dev: u64,
    items: Vec<PlanItem>,
}

impl Plan {
    /// Builds a plan, validating every path with [`safety::validate`].
    ///
    /// - `id`: a number the caller chooses, unique while the app runs.
    /// - `home`: the home path passed to the core.
    /// - `scan_root_dev`: the device id of the scan root.
    /// - `now`: the current time; the plan expires [`PLAN_LIFETIME`] later.
    ///
    /// Refuses the whole plan if any item fails, so the user never gets a
    /// cleanup that is quietly smaller than what they reviewed.
    pub fn build(
        id: u64,
        home: &Path,
        scan_root_dev: u64,
        requests: Vec<ItemRequest>,
        now: SystemTime,
    ) -> Result<Plan, PlanError> {
        if requests.is_empty() {
            return Err(PlanError::Empty);
        }
        check_no_overlap(&requests)?;
        let mut items = Vec::with_capacity(requests.len());
        for req in requests {
            let path = safety::validate(home, scan_root_dev, &req.path, req.selected_individually)
                .map_err(|refusal| PlanError::Refused {
                    recommendation_id: req.recommendation_id.clone(),
                    refusal,
                })?;
            items.push(PlanItem {
                recommendation_id: req.recommendation_id,
                path,
                expected: req.expected,
                regenerable: req.regenerable,
                selected_individually: req.selected_individually,
            });
        }
        Ok(Plan {
            id,
            created_at: now,
            home: home.to_path_buf(),
            scan_root_dev,
            items,
        })
    }

    /// The plan's id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// When the plan was built.
    pub fn created_at(&self) -> SystemTime {
        self.created_at
    }

    /// The home path the plan was built for.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The device id of the scan root.
    pub fn scan_root_dev(&self) -> u64 {
        self.scan_root_dev
    }

    /// The items, in the order they were requested.
    pub fn items(&self) -> &[PlanItem] {
        &self.items
    }

    /// Whether every item can be rebuilt by its tool.
    pub fn all_regenerable(&self) -> bool {
        self.items.iter().all(PlanItem::regenerable)
    }

    /// Fails if the plan has expired at `now`, or `now` is before it was built.
    pub fn check_fresh(&self, now: SystemTime) -> Result<(), PlanError> {
        let age = now
            .duration_since(self.created_at)
            .map_err(|_| PlanError::ClockWentBack)?;
        if age >= PLAN_LIFETIME {
            return Err(PlanError::Expired);
        }
        Ok(())
    }

    /// Takes the items out, using up the plan.
    pub fn into_items(self) -> Vec<PlanItem> {
        self.items
    }
}

/// Refuses two requests for the same path, or one inside another. Compares
/// path components, so `a/b` is not mistaken for being inside `a/bc`.
///
/// Names are compared exactly. Two spellings of one folder on a
/// case-insensitive disk are caught by the executor instead: after the first
/// is moved, the second no longer exists and is refused.
fn check_no_overlap(requests: &[ItemRequest]) -> Result<(), PlanError> {
    for (i, a) in requests.iter().enumerate() {
        for b in &requests[i + 1..] {
            let (outer, inner) = if b.path.starts_with(&a.path) {
                (a, b)
            } else if a.path.starts_with(&b.path) {
                (b, a)
            } else {
                continue;
            };
            return Err(PlanError::Overlap {
                outer: outer.recommendation_id.clone(),
                inner: inner.recommendation_id.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    fn request(id: &str, path: PathBuf) -> ItemRequest {
        ItemRequest {
            recommendation_id: id.to_string(),
            path,
            expected: Expected {
                size_bytes: 0,
                modified: SystemTime::UNIX_EPOCH,
            },
            regenerable: true,
            selected_individually: false,
        }
    }

    /// A temp home with `Library/Caches/a` and `Library/Caches/ab` in it.
    fn home() -> (tempfile::TempDir, PathBuf, u64) {
        let dir = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(home.join("Library/Caches/a/inner")).unwrap();
        std::fs::create_dir_all(home.join("Library/Caches/ab")).unwrap();
        let dev = home.symlink_metadata().unwrap().dev();
        (dir, home, dev)
    }

    #[test]
    fn empty_plan_is_refused() {
        let (_d, home, dev) = home();
        let err = Plan::build(1, &home, dev, vec![], SystemTime::now()).unwrap_err();
        assert_eq!(err, PlanError::Empty);
    }

    #[test]
    fn builds_a_plan_of_valid_items() {
        let (_d, home, dev) = home();
        let reqs = vec![
            request("a", home.join("Library/Caches/a")),
            request("ab", home.join("Library/Caches/ab")),
        ];
        let plan = Plan::build(7, &home, dev, reqs, SystemTime::now()).unwrap();
        assert_eq!(plan.id(), 7);
        assert_eq!(plan.items().len(), 2);
        assert!(plan.all_regenerable());
    }

    #[test]
    fn one_refused_item_refuses_the_whole_plan() {
        let (_d, home, dev) = home();
        std::fs::create_dir(home.join(".ssh")).unwrap();
        let reqs = vec![
            request("ok", home.join("Library/Caches/a")),
            request("ssh", home.join(".ssh")),
        ];
        let err = Plan::build(1, &home, dev, reqs, SystemTime::now()).unwrap_err();
        assert!(
            matches!(err, PlanError::Refused { ref recommendation_id, .. } if recommendation_id == "ssh"),
            "{err:?}"
        );
    }

    #[test]
    fn nested_items_are_refused() {
        let (_d, home, dev) = home();
        let reqs = vec![
            request("inner", home.join("Library/Caches/a/inner")),
            request("outer", home.join("Library/Caches/a")),
        ];
        let err = Plan::build(1, &home, dev, reqs, SystemTime::now()).unwrap_err();
        assert_eq!(
            err,
            PlanError::Overlap {
                outer: "outer".into(),
                inner: "inner".into()
            }
        );
    }

    #[test]
    fn repeated_items_are_refused() {
        let (_d, home, dev) = home();
        let path = home.join("Library/Caches/a");
        let reqs = vec![request("one", path.clone()), request("two", path)];
        let err = Plan::build(1, &home, dev, reqs, SystemTime::now()).unwrap_err();
        assert!(matches!(err, PlanError::Overlap { .. }), "{err:?}");
    }

    #[test]
    fn plan_expires_after_ten_minutes() {
        let (_d, home, dev) = home();
        let now = SystemTime::now();
        let reqs = vec![request("a", home.join("Library/Caches/a"))];
        let plan = Plan::build(1, &home, dev, reqs, now).unwrap();
        assert_eq!(plan.check_fresh(now), Ok(()));
        let almost = now + PLAN_LIFETIME - Duration::from_secs(1);
        assert_eq!(plan.check_fresh(almost), Ok(()));
        assert_eq!(
            plan.check_fresh(now + PLAN_LIFETIME),
            Err(PlanError::Expired)
        );
        assert_eq!(
            plan.check_fresh(now - Duration::from_secs(1)),
            Err(PlanError::ClockWentBack)
        );
    }
}
