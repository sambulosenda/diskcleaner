//! The Docker screen's commands: what Docker uses, and the two allow-listed
//! clean-ups (`docker image prune`, `docker builder prune`).
//!
//! The app sends a [`DockerCleanupKind`], never a command or an id of a
//! Docker object. The core runs the read-only preview, keeps it as a
//! single-use plan, and runs the command only when the app hands that plan
//! back. The executor checks the plan again first (see
//! `executor::commands`). Volumes are shown, never offered for removal.

use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::SystemTime;

use analysers::docker::{self as analysis, DockerState, RealDocker};
use executor::commands::{self, DockerCleanup, DockerPlan, RealDockerRunner};

use crate::{CoreError, RoomyCore};

/// One of the Docker clean-ups Roomy can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DockerCleanupKind {
    /// Images that no tag and no container uses (`docker image prune`).
    UnusedImages,
    /// Build cache Docker marks as safe to free (`docker builder prune`).
    BuildCache,
}

impl From<DockerCleanupKind> for DockerCleanup {
    fn from(k: DockerCleanupKind) -> Self {
        match k {
            DockerCleanupKind::UnusedImages => Self::UnusedImages,
            DockerCleanupKind::BuildCache => Self::BuildCache,
        }
    }
}

impl From<DockerCleanup> for DockerCleanupKind {
    fn from(c: DockerCleanup) -> Self {
        match c {
            DockerCleanup::UnusedImages => Self::UnusedImages,
            DockerCleanup::BuildCache => Self::BuildCache,
        }
    }
}

/// One row of `docker system df`. Sizes are Docker's own estimates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct DockerUsageRow {
    /// How many there are.
    pub count: u64,
    /// How many are in use.
    pub active: u64,
    /// Bytes they use.
    pub size_bytes: u64,
    /// Bytes Docker says could be freed.
    pub reclaimable_bytes: u64,
}

impl From<analysis::Usage> for DockerUsageRow {
    fn from(u: analysis::Usage) -> Self {
        Self {
            count: u.count,
            active: u.active,
            size_bytes: u.size_bytes,
            reclaimable_bytes: u.reclaimable_bytes,
        }
    }
}

/// Whether Roomy could ask Docker.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum DockerStatus {
    /// Docker's command-line tool is not installed.
    NotInstalled,
    /// Docker is installed but not running.
    NotRunning,
    /// Docker could not be asked. `reason` is plain words with no path.
    Unavailable {
        /// Why.
        reason: String,
    },
    /// The rows hold Docker's answer.
    Ready,
}

/// What the Docker screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DockerOverview {
    /// Whether the rows below hold real numbers. All zero unless `Ready`.
    pub status: DockerStatus,
    /// Images.
    pub images: DockerUsageRow,
    /// Containers, running or stopped.
    pub containers: DockerUsageRow,
    /// Volumes. Shown only: Roomy never removes them.
    pub volumes: DockerUsageRow,
    /// Build cache.
    pub build_cache: DockerUsageRow,
    /// Space Docker Desktop's disk image takes on this Mac, if it has one.
    /// Everything above lives inside it.
    pub disk_image_bytes: Option<u64>,
}

/// Opaque id for one Docker clean-up plan. Single use; expires ten minutes
/// after the preview that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DockerPlanHandle {
    /// The plan's number.
    pub id: u64,
}

/// One thing a clean-up would remove.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DockerPreviewItem {
    /// Docker's short id for it.
    pub id: String,
    /// Bytes, as Docker estimates it.
    pub size_bytes: u64,
    /// When it was made or last used, in Docker's words ("3 weeks ago").
    pub age: String,
}

/// The dry run of a Docker clean-up. Nothing has been removed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DockerCleanupPreview {
    /// The plan to hand to `run_docker_cleanup`, or `None` if there is
    /// nothing to remove.
    pub plan: Option<DockerPlanHandle>,
    /// Which clean-up this is.
    pub kind: DockerCleanupKind,
    /// The command Roomy would run, such as `docker image prune`.
    pub command_line: String,
    /// What would be removed, as Docker listed it.
    pub items: Vec<DockerPreviewItem>,
    /// Bytes Docker says would be freed.
    pub total_bytes: u64,
    /// How long the plan stays usable, in seconds.
    pub expires_in_seconds: u64,
}

/// What a Docker clean-up did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DockerCleanupResult {
    /// Which clean-up ran.
    pub kind: DockerCleanupKind,
    /// Whether Docker said it finished without a problem.
    pub success: bool,
    /// Bytes Docker says it freed, if it said.
    pub reclaimed_bytes: Option<u64>,
    /// Whether the run, with Docker's full output, was saved to History.
    pub saved_to_history: bool,
}

/// The Docker state the core keeps.
pub(crate) struct DockerSlot {
    /// The folders searched for the `docker` program.
    search_path: Mutex<OsString>,
    /// The next plan number. Starts at 1.
    next_id: AtomicU64,
    /// The one plan waiting for the user, with its number.
    pending: Mutex<Option<(u64, DockerPlan)>>,
}

impl DockerSlot {
    /// The Docker state for a core that works on `home`.
    pub(crate) fn new(home: &Path) -> Self {
        Self {
            search_path: Mutex::new(analysis::search_path(home)),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(None),
        }
    }

    fn path(&self) -> OsString {
        lock(&self.search_path).clone()
    }
}

/// Locks a mutex, carrying on if a thread panicked while holding it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl RoomyCore {
    /// Rust-only, for tests: looks for `docker` in exactly these folders,
    /// so a test can put a stand-in `docker` program in a temporary folder.
    pub fn set_docker_search_path_for_testing(&self, path: OsString) {
        *lock(&self.docker.search_path) = path;
    }
}

#[uniffi::export]
impl RoomyCore {
    /// What Docker uses, from `docker system df`, and the size of Docker
    /// Desktop's disk image. Read only. Takes up to 20 seconds if Docker is
    /// slow to answer, so the app calls it off the main thread.
    pub fn docker_overview(&self) -> DockerOverview {
        let state = analysis::usage(&RealDocker::with_search_path(self.docker.path()));
        let disk_image_bytes = analysis::disk_image_bytes(self.home());
        let (status, usage) = match state {
            DockerState::NotInstalled => (DockerStatus::NotInstalled, Default::default()),
            DockerState::NotRunning => (DockerStatus::NotRunning, Default::default()),
            DockerState::Unavailable { reason } => {
                (DockerStatus::Unavailable { reason }, Default::default())
            }
            DockerState::Ready(usage) => (DockerStatus::Ready, usage),
        };
        DockerOverview {
            status,
            images: usage.images.into(),
            containers: usage.containers.into(),
            volumes: usage.volumes.into(),
            build_cache: usage.build_cache.into(),
            disk_image_bytes,
        }
    }

    /// The dry run of one Docker clean-up: runs its read-only preview from
    /// the allow-list and says what it would remove. Removes nothing.
    ///
    /// Keeps a single-use plan, replacing any earlier Docker plan. Throws
    /// `docker_unavailable` if Docker is missing, stopped or its answer
    /// could not be read.
    pub fn preview_docker_cleanup(
        &self,
        kind: DockerCleanupKind,
    ) -> Result<DockerCleanupPreview, CoreError> {
        *lock(&self.docker.pending) = None;
        let runner = RealDockerRunner::with_search_path(self.docker.path());
        let plan = commands::preview_docker(&runner, kind.into(), SystemTime::now())
            .map_err(unavailable)?;
        let items: Vec<DockerPreviewItem> = plan
            .items()
            .iter()
            .map(|i| DockerPreviewItem {
                id: i.id.clone(),
                size_bytes: i.size_bytes,
                age: i.age.clone(),
            })
            .collect();
        let total_bytes = plan.total_bytes();
        let handle = if items.is_empty() {
            None
        } else {
            let id = self.docker.next_id.fetch_add(1, Ordering::Relaxed);
            *lock(&self.docker.pending) = Some((id, plan));
            Some(DockerPlanHandle { id })
        };
        Ok(DockerCleanupPreview {
            plan: handle,
            kind,
            command_line: DockerCleanup::from(kind).command_line(),
            items,
            total_bytes,
            expires_in_seconds: planner::PLAN_LIFETIME.as_secs(),
        })
    }

    /// Drops the waiting Docker plan, if it is `plan`. Safe to call more
    /// than once or with an unknown plan.
    pub fn discard_docker_cleanup(&self, plan: DockerPlanHandle) {
        let mut pending = lock(&self.docker.pending);
        if pending.as_ref().is_some_and(|(id, _)| *id == plan.id) {
            *pending = None;
        }
    }

    /// Runs the waiting Docker plan's command. The plan is used up.
    ///
    /// Throws `cleanup_unavailable` if `plan` is not the waiting plan, and
    /// `docker_unavailable` if the executor refused it (expired, Docker
    /// stopped, or something new appeared since the review). Nothing is
    /// removed then. A run that started is saved to History with Docker's
    /// full output, whether or not Docker reported a problem.
    pub fn run_docker_cleanup(
        &self,
        plan: DockerPlanHandle,
    ) -> Result<DockerCleanupResult, CoreError> {
        let taken = {
            let mut slot = lock(&self.docker.pending);
            match slot.take() {
                Some((id, p)) if id == plan.id => p,
                other => {
                    *slot = other;
                    return Err(crate::run::plan_gone());
                }
            }
        };
        let runner = RealDockerRunner::with_search_path(self.docker.path());
        let ran = commands::run_docker(&runner, taken, SystemTime::now()).map_err(unavailable)?;
        let record = history::CommandRun {
            id: 0,
            finished_at: crate::run::unix_seconds(SystemTime::now()),
            command_id: ran.cleanup.id().to_string(),
            command_line: ran.command_line.clone(),
            success: ran.success,
            reclaimed_bytes: ran.reclaimed_bytes,
            stdout: ran.stdout.clone(),
            stderr: ran.stderr.clone(),
        };
        let saved = self
            .with_history(true, |h| h.record_command_run(&record))
            .is_some();
        Ok(DockerCleanupResult {
            kind: ran.cleanup.into(),
            success: ran.success,
            reclaimed_bytes: ran.reclaimed_bytes,
            saved_to_history: saved,
        })
    }
}

/// An executor refusal as the error the app shows.
fn unavailable(e: commands::DockerError) -> CoreError {
    CoreError::DockerUnavailable {
        reason: e.to_string(),
    }
}
