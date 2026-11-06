//! The Tool clean-ups screen's commands: `brew cleanup`, `xcrun simctl
//! delete unavailable` and `sdkmanager --uninstall <package>`, from
//! `docs/safety/allowed-commands.md`.
//!
//! The app sends a [`ToolCleanupKind`], never a command or a path. The core
//! runs the read-only preview, keeps it as a single-use plan, and runs the
//! command only when the app hands that plan back. For the Android SDK the
//! app also sends the id of one package from that preview; the executor
//! refuses any id the preview did not list (see `executor::tools`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::SystemTime;

use executor::tools::{self, RealToolRunner, ToolCleanup, ToolLocations, ToolPlan};

use crate::{CoreError, RoomyCore};

/// One of the tool clean-ups Roomy can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ToolCleanupKind {
    /// Old versions and downloads Homebrew no longer needs (`brew cleanup`).
    Homebrew,
    /// Simulators for iOS versions no longer installed
    /// (`xcrun simctl delete unavailable`).
    UnavailableSimulators,
    /// One installed Android SDK package (`sdkmanager --uninstall`).
    AndroidSdkPackage,
}

impl From<ToolCleanupKind> for ToolCleanup {
    fn from(k: ToolCleanupKind) -> Self {
        match k {
            ToolCleanupKind::Homebrew => Self::Homebrew,
            ToolCleanupKind::UnavailableSimulators => Self::UnavailableSimulators,
            ToolCleanupKind::AndroidSdkPackage => Self::AndroidSdkPackage,
        }
    }
}

impl From<ToolCleanup> for ToolCleanupKind {
    fn from(c: ToolCleanup) -> Self {
        match c {
            ToolCleanup::Homebrew => Self::Homebrew,
            ToolCleanup::UnavailableSimulators => Self::UnavailableSimulators,
            ToolCleanup::AndroidSdkPackage => Self::AndroidSdkPackage,
        }
    }
}

/// Opaque id for one tool clean-up plan. Single use; expires ten minutes
/// after the preview that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ToolPlanHandle {
    /// The plan's number.
    pub id: u64,
}

/// One thing a clean-up would remove.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToolPreviewItem {
    /// The tool's own name for it: a path for Homebrew, a device id for a
    /// simulator, a package id for the Android SDK. For the Android SDK this
    /// is what the app sends back to choose the package.
    pub id: String,
    /// A short name for people.
    pub title: String,
    /// One more line: a folder, an iOS version, or a package version.
    pub detail: String,
    /// Bytes on disk, an estimate.
    pub size_bytes: u64,
}

/// The dry run of a tool clean-up. Nothing has been removed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToolCleanupPreview {
    /// The plan to hand to `run_tool_cleanup`, or `None` if there is
    /// nothing to remove.
    pub plan: Option<ToolPlanHandle>,
    /// Which clean-up this is.
    pub kind: ToolCleanupKind,
    /// The command Roomy would run, such as `brew cleanup`. For the Android
    /// SDK it ends in `<package>`.
    pub command_line: String,
    /// What would be removed. For the Android SDK, the packages to choose
    /// one from.
    pub items: Vec<ToolPreviewItem>,
    /// Bytes all the items take.
    pub total_bytes: u64,
    /// How long the plan stays usable, in seconds.
    pub expires_in_seconds: u64,
}

/// What a tool clean-up did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToolCleanupResult {
    /// Which clean-up ran.
    pub kind: ToolCleanupKind,
    /// The command that ran, as typed.
    pub command_line: String,
    /// Whether the tool said it finished without a problem.
    pub success: bool,
    /// Bytes freed, if known.
    pub reclaimed_bytes: Option<u64>,
    /// Whether the run, with the tool's full output, was saved to History.
    pub saved_to_history: bool,
}

/// The tool clean-up state the core keeps.
pub(crate) struct ToolSlot {
    /// Test-only: where the programs are, instead of looking for them.
    locations: Mutex<Option<ToolLocations>>,
    /// The next plan number. Starts at 1.
    next_id: AtomicU64,
    /// The one plan waiting for the user, with its number.
    pending: Mutex<Option<(u64, ToolPlan)>>,
}

impl ToolSlot {
    pub(crate) fn new() -> Self {
        Self {
            locations: Mutex::new(None),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(None),
        }
    }
}

/// Locks a mutex, carrying on if a thread panicked while holding it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl RoomyCore {
    /// Rust-only, for tests: uses exactly these programs, so a test can put
    /// stand-in `brew`, `xcrun` and `sdkmanager` programs in a temporary
    /// folder.
    pub fn set_tool_programs_for_testing(
        &self,
        brew: Option<PathBuf>,
        xcrun: Option<PathBuf>,
        sdkmanager: Option<PathBuf>,
    ) {
        *lock(&self.tools.locations) = Some(ToolLocations {
            brew,
            xcrun,
            sdkmanager,
            ..Default::default()
        });
    }

    /// The runner, looking for the programs afresh each time so a tool
    /// installed while Roomy is open is found.
    fn tool_runner(&self) -> RealToolRunner {
        let locations = lock(&self.tools.locations)
            .clone()
            .unwrap_or_else(|| ToolLocations::find(self.home()));
        RealToolRunner::new(locations)
    }
}

#[uniffi::export]
impl RoomyCore {
    /// The dry run of one tool clean-up: runs its read-only preview from
    /// the allow-list and says what it would remove. Removes nothing. Can
    /// take a minute (`sdkmanager` is slow), so the app calls it off the
    /// main thread.
    ///
    /// Keeps a single-use plan, replacing any earlier tool plan. Throws
    /// `tool_unavailable` if the tool is missing or its answer could not be
    /// read.
    pub fn preview_tool_cleanup(
        &self,
        kind: ToolCleanupKind,
    ) -> Result<ToolCleanupPreview, CoreError> {
        *lock(&self.tools.pending) = None;
        let cleanup = ToolCleanup::from(kind);
        let plan =
            tools::preview_tool(&self.tool_runner(), cleanup, self.home(), SystemTime::now())
                .map_err(unavailable)?;
        let items: Vec<ToolPreviewItem> = plan
            .items()
            .iter()
            .map(|i| ToolPreviewItem {
                id: i.id.clone(),
                title: i.title.clone(),
                detail: i.detail.clone(),
                size_bytes: i.size_bytes,
            })
            .collect();
        let total_bytes = plan.total_bytes();
        let handle = if items.is_empty() {
            None
        } else {
            let id = self.tools.next_id.fetch_add(1, Ordering::Relaxed);
            *lock(&self.tools.pending) = Some((id, plan));
            Some(ToolPlanHandle { id })
        };
        Ok(ToolCleanupPreview {
            plan: handle,
            kind,
            command_line: cleanup.command_line(None),
            items,
            total_bytes,
            expires_in_seconds: planner::PLAN_LIFETIME.as_secs(),
        })
    }

    /// Drops the waiting tool plan, if it is `plan`. Safe to call more than
    /// once or with an unknown plan.
    pub fn discard_tool_cleanup(&self, plan: ToolPlanHandle) {
        let mut pending = lock(&self.tools.pending);
        if pending.as_ref().is_some_and(|(id, _)| *id == plan.id) {
            *pending = None;
        }
    }

    /// Runs the waiting tool plan's command. The plan is used up.
    ///
    /// `package` is the id of one package from the preview for the Android
    /// SDK, and must be `None` for the others. Throws `cleanup_unavailable`
    /// if `plan` is not the waiting plan, and `tool_unavailable` if the
    /// executor refused it (expired, a package not in the review, or
    /// something new appeared since). Nothing is removed then. A run that
    /// started is saved to History with the tool's full output.
    pub fn run_tool_cleanup(
        &self,
        plan: ToolPlanHandle,
        package: Option<String>,
    ) -> Result<ToolCleanupResult, CoreError> {
        let taken = {
            let mut slot = lock(&self.tools.pending);
            match slot.take() {
                Some((id, p)) if id == plan.id => p,
                other => {
                    *slot = other;
                    return Err(crate::run::plan_gone());
                }
            }
        };
        let ran = tools::run_tool(
            &self.tool_runner(),
            taken,
            package.as_deref(),
            self.home(),
            SystemTime::now(),
        )
        .map_err(unavailable)?;
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
        Ok(ToolCleanupResult {
            kind: ran.cleanup.into(),
            command_line: ran.command_line,
            success: ran.success,
            reclaimed_bytes: ran.reclaimed_bytes,
            saved_to_history: saved,
        })
    }
}

/// An executor refusal as the error the app shows.
fn unavailable(e: tools::ToolError) -> CoreError {
    CoreError::ToolUnavailable {
        reason: e.to_string(),
    }
}
