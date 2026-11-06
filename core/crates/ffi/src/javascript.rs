//! The JavaScript screen: each project's `node_modules` with when the
//! project was last worked on, and the package managers' download caches.
//!
//! Read only. The rows come from the finished scan (the `js.*` rules in
//! `rules/js.toml`); for each `node_modules` the core then reads the
//! modified times of the project's `package.json`, lock files and git
//! files, after `safety::validate` has checked the `node_modules` folder
//! (no symlink anywhere in it or its parents, same volume, not blocked).
//!
//! Cleaning works exactly like every other list: the app ticks rows by item
//! id and sends them to `preview_cleanup`, so the same plan, extra checks,
//! Trash move, put back and History apply.

use std::path::Path;

use analysers::js::{self as analysis, ActivitySource as RawSource, PackageManager as RawManager};
use classifier::ClassifiedItem;

use crate::run::unix_seconds;
use crate::types::{ItemSummary, Mode};
use crate::views::{display_path, summary, Finished};

/// Rules whose items are one project's `node_modules`.
const NODE_MODULES_RULE_PREFIX: &str = "js.node_modules";
/// Every JavaScript rule starts with this.
const JS_RULE_PREFIX: &str = "js.";

/// The package manager a project's lock file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum PackageManager {
    /// npm (`package-lock.json`, `npm-shrinkwrap.json`).
    Npm,
    /// pnpm (`pnpm-lock.yaml`).
    Pnpm,
    /// Yarn (`yarn.lock`).
    Yarn,
    /// Bun (`bun.lock`, `bun.lockb`).
    Bun,
}

impl From<RawManager> for PackageManager {
    fn from(m: RawManager) -> Self {
        match m {
            RawManager::Npm => Self::Npm,
            RawManager::Pnpm => Self::Pnpm,
            RawManager::Yarn => Self::Yarn,
            RawManager::Bun => Self::Bun,
        }
    }
}

/// Which file gave a project's last-activity date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ActivitySource {
    /// The project's git repository: a commit, a staged change or a branch
    /// switch.
    Git,
    /// A lock file: packages added, removed or updated.
    LockFile,
    /// `package.json`.
    Manifest,
}

impl From<RawSource> for ActivitySource {
    fn from(s: RawSource) -> Self {
        match s {
            RawSource::Git => Self::Git,
            RawSource::LockFile => Self::LockFile,
            RawSource::Manifest => Self::Manifest,
        }
    }
}

/// One JavaScript project's `node_modules`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JsProject {
    /// The `node_modules` item, as every list shows it. Its `id` is what
    /// the app ticks and sends to `preview_cleanup`.
    pub item: ItemSummary,
    /// The project folder's name in Developer mode; the item's title in
    /// simple mode, where no part of a path goes to the app.
    pub name: String,
    /// Developer mode only: the project folder as a display string
    /// starting `~`.
    pub folder: Option<String>,
    /// When the project was last worked on, in seconds since 1970: the
    /// newest of `package.json`, the lock files and the git repository's
    /// `index` and `logs/HEAD`. `None` if none of them could be read.
    pub last_activity_unix_seconds: Option<i64>,
    /// Which file gave `last_activity_unix_seconds`.
    pub activity_source: Option<ActivitySource>,
    /// From the newest lock file, if there is one.
    pub package_manager: Option<PackageManager>,
    /// The command that fetches the packages again, for example
    /// `pnpm install`. `None` when there is no lock file.
    pub install_command: Option<String>,
    /// Whether the project folder has a `package.json`. Without one, the
    /// packages may not come back with an install command.
    pub has_manifest: bool,
}

/// What the JavaScript screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct JavaScriptOverview {
    /// Each project's `node_modules`, largest first, then by id.
    pub projects: Vec<JsProject>,
    /// The package managers' download caches and stores, largest first,
    /// then by id.
    pub caches: Vec<ItemSummary>,
    /// Every row's size added up.
    pub total_bytes: u64,
}

/// Builds the JavaScript screen for a finished scan. Reads metadata only.
pub(crate) fn overview(f: &Finished) -> JavaScriptOverview {
    let mut projects = Vec::new();
    let mut caches = Vec::new();
    let mut total_bytes = 0u64;
    // `f.items` is already largest first, then by id.
    for item in &f.items {
        if !item.rule_id.starts_with(JS_RULE_PREFIX) {
            continue;
        }
        total_bytes = total_bytes.saturating_add(item.size_bytes);
        if is_node_modules(item) {
            projects.push(project(f, item));
        } else {
            caches.push(summary(item));
        }
    }
    JavaScriptOverview {
        projects,
        caches,
        total_bytes,
    }
}

/// Whether the item is one project's `node_modules`.
fn is_node_modules(item: &ClassifiedItem) -> bool {
    item.rule_id.starts_with(NODE_MODULES_RULE_PREFIX)
        && item.path.file_name().is_some_and(|n| n == "node_modules")
}

/// One project row. The project folder is read only if the `node_modules`
/// folder passes `safety::validate`; otherwise the row has no date.
fn project(f: &Finished, item: &ClassifiedItem) -> JsProject {
    let project_dir = item.path.parent();
    let (name, folder) = match (f.mode, project_dir) {
        (Mode::Developer, Some(dir)) => (folder_name(dir, &item.title), display_path(&f.home, dir)),
        _ => (item.title.clone(), None),
    };
    let activity = match project_dir {
        // Chosen alone: this only decides whether the folder is safe to
        // read, not whether it may move.
        Some(dir) => match safety::validate(&f.home, f.root_dev, &item.path, true) {
            Ok(_) => analysis::project_activity(&f.home, dir),
            Err(refusal) => {
                tracing::warn!(%refusal, "project folder not read");
                analysis::ProjectActivity::default()
            }
        },
        None => analysis::ProjectActivity::default(),
    };
    JsProject {
        item: summary(item),
        name,
        folder,
        last_activity_unix_seconds: activity.last_activity.map(unix_seconds),
        activity_source: activity.source.map(ActivitySource::from),
        package_manager: activity.package_manager.map(PackageManager::from),
        install_command: activity
            .package_manager
            .map(|m| m.install_command().to_string()),
        has_manifest: activity.has_manifest,
    }
}

/// The last part of `dir`, or `fallback` if it has none.
fn folder_name(dir: &Path, fallback: &str) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_string())
}
