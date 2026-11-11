//! The app uninstaller: lists installed apps, finds what each left in the
//! home folder, and turns what the user ticked into a cleanup plan.
//!
//! Nothing here changes a file. Apps and leftovers are found by
//! `analysers::apps` (read only) and measured with the read-only scanner.
//! The app sends app ids and part ids, never paths. The plan this builds is
//! checked by the executor's own preview, and carried out by the same
//! `run_cleanup` as any other cleanup, so the Trash, put back and History
//! work the same way.
//!
//! Safety, on top of everything every cleanup gets:
//! - Apple's apps, and Roomy itself, are never listed.
//! - An app in `/Applications` is outside the home folder, so the safety
//!   rules refuse it; only its leftovers are offered. An app in
//!   `~/Applications` is offered too.
//! - The uninstall is refused while the app is open, and each item is
//!   skipped by the executor if any program inside the app, or inside the
//!   item, is running when it acts.
//! - Leftovers matched less surely than by bundle id, and data the app
//!   keeps for itself (Application Support, containers), are never ticked
//!   for the user.
//! - If another copy of the app (same bundle id) is installed, its shared
//!   data is not offered at all.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use analysers::apps::{App, Leftover, LeftoverKind, Match};
use planner::{AppBundle, Expected, ItemRequest};

use crate::cleanup::{check_entries, Entry, Prepared};
use crate::types::{AppLocation, AppPart, AppReview, AppSummary, Mode};
use crate::views::display_path;
use crate::CoreError;

/// Roomy's own bundle id. Roomy never offers to remove itself.
pub(crate) const ROOMY_BUNDLE_ID: &str = "com.sambulosenda.roomy";

/// The part id of the app bundle itself.
const APP_PART_ID: &str = "app";

/// Shown above the list when another copy of the app is installed.
const OTHER_COPY: &str = "Another copy of this app is installed, and both use the same files, so Roomy leaves those alone.";
/// Shown when nothing was found to remove.
const NOTHING_FOUND: &str = "Roomy didn't find anything this app left in your home folder.";

/// What the core keeps between `list_apps`, `review_app` and
/// `preview_uninstall`.
#[derive(Debug, Default)]
pub(crate) struct AppsState {
    /// Bumped by every `list_apps`, so ids from an older list are refused.
    generation: u64,
    /// The apps from the latest `list_apps`, by index.
    apps: Vec<App>,
    /// The latest review, if any.
    review: Option<Review>,
}

/// The latest `review_app`: what the user saw, with what each part looked
/// like then, so the executor can refuse anything that changed since.
#[derive(Debug)]
struct Review {
    app_id: String,
    app: App,
    parts: Vec<Part>,
}

/// One part of a review.
#[derive(Debug, Clone)]
struct Part {
    item_id: String,
    title: String,
    path: PathBuf,
    expected: Expected,
    shown_path: Option<String>,
}

/// Where the core looks for apps.
pub(crate) fn app_folders(home: &Path) -> Vec<PathBuf> {
    vec![PathBuf::from("/Applications"), home.join("Applications")]
}

/// Lists the apps in `folders`, measures each, and remembers them in
/// `state` for `review_app`.
pub(crate) fn list(
    state: &mut AppsState,
    home: &Path,
    folders: &[PathBuf],
    running: &executor::Running,
) -> Vec<AppSummary> {
    state.generation = state.generation.wrapping_add(1);
    state.review = None;
    state.apps = analysers::apps::list_apps(folders)
        .into_iter()
        .filter(|a| !is_roomy(&a.bundle_id))
        .collect();
    let generation = state.generation;
    state
        .apps
        .iter()
        .enumerate()
        .map(|(i, app)| summary(app, &app_id(generation, i), home, running))
        .collect()
}

/// Finds and measures what the app `app_id` left in `home`, and remembers
/// it for `preview_uninstall`.
pub(crate) fn review(
    state: &mut AppsState,
    home: &Path,
    app_id: &str,
    mode: Mode,
    running: &executor::Running,
) -> Result<AppReview, CoreError> {
    state.review = None;
    let index = index_of(state, app_id)?;
    let app = state.apps[index].clone();
    let home_dev = home_dev(home)?;
    let others: Vec<App> = state
        .apps
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, a)| a.clone())
        .collect();
    let shared = others
        .iter()
        .any(|o| o.bundle_id.eq_ignore_ascii_case(&app.bundle_id));
    let mut found: Vec<(Part, AppPart)> = Vec::new();
    if location(&app, home) == AppLocation::HomeApplications {
        if let Some(part) = app_part(&app, home, home_dev, mode) {
            found.push(part);
        }
    }
    if !shared {
        let leftovers = analysers::apps::find_leftovers(home, &app, &others);
        let mut parts: Vec<(Part, AppPart)> = leftovers
            .iter()
            .enumerate()
            .filter_map(|(n, l)| leftover_part(l, n, home, home_dev, mode))
            .collect();
        parts.sort_by_key(|p| std::cmp::Reverse(p.1.size_bytes));
        found.extend(parts);
    }
    let note = if shared {
        Some(OTHER_COPY.to_string())
    } else if found.is_empty() {
        Some(NOTHING_FOUND.to_string())
    } else {
        None
    };
    let (parts, shown): (Vec<Part>, Vec<AppPart>) = found.into_iter().unzip();
    let review = AppReview {
        app: summary(&app, app_id, home, running),
        parts: shown,
        note,
    };
    state.review = Some(Review {
        app_id: app_id.to_string(),
        app,
        parts,
    });
    Ok(review)
}

/// Builds the dry run for the ticked parts of the latest review, exactly as
/// a cleanup would check them, and keeps the plan of what would move.
pub(crate) fn prepare(
    state: &AppsState,
    home: &Path,
    app_id: &str,
    part_ids: &[String],
    plan_id: u64,
    guards: &executor::Guards,
    now: SystemTime,
) -> Result<Prepared, CoreError> {
    let review = state
        .review
        .as_ref()
        .filter(|r| r.app_id == app_id)
        .ok_or(CoreError::UnknownApp)?;
    // Checked again here, so a bug elsewhere can't offer one of these.
    if analysers::apps::is_apple(&review.app.bundle_id) || is_roomy(&review.app.bundle_id) {
        return Err(CoreError::UnknownApp);
    }
    if is_open(&review.app, &guards.running) {
        return Err(CoreError::CleanupUnavailable {
            reason: format!(
                "{name} is open. Quit {name}, then try again.",
                name = review.app.name
            ),
        });
    }
    let belongs_to = AppBundle {
        name: review.app.name.clone(),
        bundle: review.app.bundle.clone(),
    };
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for id in part_ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let part = review
            .parts
            .iter()
            .find(|p| &p.item_id == id)
            .ok_or(CoreError::UnknownItem)?;
        entries.push(Entry {
            item_id: part.item_id.clone(),
            title: part.title.clone(),
            history_title: Some(format!("{}: {}", review.app.name, part.title)),
            size_bytes: part.expected.size_bytes,
            path: part.shown_path.clone(),
            device_backup: None,
            restore: None,
            request: Some(ItemRequest {
                recommendation_id: part.item_id.clone(),
                path: part.path.clone(),
                expected: part.expected,
                // Trash only: an app's data is not something a tool rebuilds.
                regenerable: false,
                // Every part is its own row that the user ticks; the ones
                // that need choosing on their own are never ticked for them.
                selected_individually: true,
                owners: &[],
                belongs_to: Some(belongs_to.clone()),
                keep_copy: None,
            }),
            wont_move: None,
        });
    }
    check_entries(home, home_dev(home)?, entries, plan_id, guards, now)
}

/// The id for app number `index` of list `generation`.
fn app_id(generation: u64, index: usize) -> String {
    format!("app-{generation}-{index}")
}

/// The index of `app_id` in the current list, or `UnknownApp`.
fn index_of(state: &AppsState, app_id: &str) -> Result<usize, CoreError> {
    (0..state.apps.len())
        .find(|i| app_id == self::app_id(state.generation, *i))
        .ok_or(CoreError::UnknownApp)
}

/// Whether `id` is Roomy's own bundle id (or one of its helpers').
fn is_roomy(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    id == ROOMY_BUNDLE_ID || id.starts_with(&format!("{ROOMY_BUNDLE_ID}."))
}

/// The device id of the home folder. Leftovers on another volume are left out.
pub(crate) fn home_dev(home: &Path) -> Result<u64, CoreError> {
    std::fs::symlink_metadata(home)
        .map(|m| m.dev())
        .map_err(|_| CoreError::HomeNotFound)
}

/// Whether the app is in `home`'s Applications folder.
fn location(app: &App, home: &Path) -> AppLocation {
    if app.bundle.parent() == Some(home.join("Applications").as_path()) {
        AppLocation::HomeApplications
    } else {
        AppLocation::Applications
    }
}

/// Whether any program inside the app's bundle is running. If the running
/// programs can't be listed, the app counts as open.
fn is_open(app: &App, running: &executor::Running) -> bool {
    match running {
        executor::Running::Known(programs) => programs.iter().any(|p| inside(p, &app.bundle)),
        executor::Running::Unknown => true,
    }
}

/// Whether `path` is `folder` or inside it, ignoring ASCII case.
fn inside(path: &Path, folder: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let mut parts = path.iter();
    folder.iter().all(|f| {
        parts
            .next()
            .is_some_and(|p| p.as_bytes().eq_ignore_ascii_case(f.as_bytes()))
    })
}

/// What the list shows for one app.
fn summary(app: &App, app_id: &str, home: &Path, running: &executor::Running) -> AppSummary {
    AppSummary {
        app_id: app_id.to_string(),
        name: app.name.clone(),
        version: app.version.clone(),
        bundle_id: app.bundle_id.clone(),
        size_bytes: measure(&app.bundle, None).map_or(0, |m| m.size_bytes),
        location: location(app, home),
        bundle_path: app.bundle.to_string_lossy().into_owned(),
        is_open: is_open(app, running),
    }
}

/// The part for the app bundle itself, when it is inside home.
fn app_part(app: &App, home: &Path, dev: u64, mode: Mode) -> Option<(Part, AppPart)> {
    let expected = measure(&app.bundle, Some(dev))?;
    safety::validate(home, dev, &app.bundle, true).ok()?;
    let title = "The app itself".to_string();
    let shown_path = shown(home, &app.bundle, mode);
    Some((
        Part {
            item_id: APP_PART_ID.to_string(),
            title: title.clone(),
            path: app.bundle.clone(),
            expected,
            shown_path: shown_path.clone(),
        },
        AppPart {
            item_id: APP_PART_ID.to_string(),
            title,
            explain: format!("{} itself. You can download it again later.", app.name),
            size_bytes: expected.size_bytes,
            preselected: true,
            sure: true,
            path: shown_path,
        },
    ))
}

/// The part for one leftover, if it can be measured and could be removed.
fn leftover_part(
    leftover: &Leftover,
    n: usize,
    home: &Path,
    dev: u64,
    mode: Mode,
) -> Option<(Part, AppPart)> {
    let expected = measure(&leftover.path, Some(dev))?;
    // A path the safety rules would never let go is not shown at all.
    let validated = safety::validate(home, dev, &leftover.path, true).ok()?;
    let sure = leftover.matched == Match::BundleId;
    let preselected = sure && validated.review_only().is_none() && ticks_itself(leftover.kind);
    let (title, explain) = words(leftover.kind);
    let explain = if sure {
        explain.to_string()
    } else {
        format!("{explain} Roomy matched it by a similar name, so check it belongs to this app.")
    };
    let item_id = format!("part-{n}");
    let shown_path = shown(home, &leftover.path, mode);
    Some((
        Part {
            item_id: item_id.clone(),
            title: title.to_string(),
            path: leftover.path.clone(),
            expected,
            shown_path: shown_path.clone(),
        },
        AppPart {
            item_id,
            title: title.to_string(),
            explain,
            size_bytes: expected.size_bytes,
            preselected,
            sure,
            path: shown_path,
        },
    ))
}

/// Whether a leftover of this kind may be ticked for the user. Data the app
/// keeps for itself can hold things the user made, so the user ticks it.
fn ticks_itself(kind: LeftoverKind) -> bool {
    !matches!(
        kind,
        LeftoverKind::AppSupport | LeftoverKind::Container | LeftoverKind::GroupContainer
    )
}

/// The title and plain-words explanation for a kind of leftover.
fn words(kind: LeftoverKind) -> (&'static str, &'static str) {
    match kind {
        LeftoverKind::AppSupport => (
            "App data",
            "Files the app keeps for itself, such as downloads, settings and sign-ins. It can hold things you made in the app.",
        ),
        LeftoverKind::Caches => (
            "Caches",
            "Copies the app keeps to work faster. It can download them again.",
        ),
        LeftoverKind::Container => (
            "App container",
            "Everything this app saved for itself, which can include documents you made in it. Check before you remove it.",
        ),
        LeftoverKind::GroupContainer => (
            "Shared app data",
            "Data the app shares with its helpers or its other apps. Check before you remove it.",
        ),
        LeftoverKind::Preferences => ("Settings", "The app's settings and preferences."),
        LeftoverKind::SavedState => (
            "Saved windows",
            "Which windows the app had open, so it could reopen them.",
        ),
        LeftoverKind::WebData => (
            "Web data",
            "Cookies and website data the app kept, such as sign-ins.",
        ),
        LeftoverKind::Logs => ("Logs", "Records the app kept of what it did."),
        LeftoverKind::LaunchAgent => (
            "Starts at login",
            "Tells your Mac to start part of the app when you log in. It stops after you log out.",
        ),
        LeftoverKind::AppScripts => ("App scripts", "Scripts the app was allowed to run."),
    }
}

/// The path to show, in Developer mode only.
pub(crate) fn shown(home: &Path, path: &Path, mode: Mode) -> Option<String> {
    match mode {
        Mode::Developer => display_path(home, path),
        Mode::Simple => None,
    }
}

/// Measures `path` the way the scanner (and the executor) do: allocated
/// bytes of it and everything inside, each hard-linked file once, and its
/// own modification time. `None` for a symlink, something unreadable, or
/// (with `dev`) anything on another volume.
pub(crate) fn measure(path: &Path, dev: Option<u64>) -> Option<Expected> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || dev.is_some_and(|d| d != meta.dev()) {
        return None;
    }
    let modified = meta.modified().ok()?;
    if meta.is_file() {
        return Some(Expected {
            size_bytes: meta.blocks().saturating_mul(512),
            modified,
        });
    }
    if !meta.is_dir() {
        return None;
    }
    let opts = scanner::ScanOptions {
        cancel: Arc::new(AtomicBool::new(false)),
        largest_files: 0,
        ..scanner::ScanOptions::default()
    };
    let scan = scanner::scan(path, opts, |_| {}).ok()?;
    Some(Expected {
        size_bytes: scan.total_bytes(),
        modified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roomy_never_offers_itself() {
        assert!(is_roomy("com.sambulosenda.roomy"));
        assert!(is_roomy("COM.sambulosenda.Roomy.helper"));
        assert!(!is_roomy("com.sambulosenda.roomyx"));
    }

    #[test]
    fn running_inside_the_bundle_counts_as_open() {
        let app = App {
            bundle: PathBuf::from("/Applications/Slack.app"),
            bundle_id: "com.tinyspeck.slackmacgap".into(),
            name: "Slack".into(),
            version: None,
        };
        let running = |p: &str| executor::Running::Known(vec![PathBuf::from(p)]);
        assert!(is_open(
            &app,
            &running("/Applications/Slack.app/Contents/MacOS/Slack")
        ));
        assert!(!is_open(
            &app,
            &running("/Applications/Slack 2.app/Contents/MacOS/Slack")
        ));
        assert!(is_open(&app, &executor::Running::Unknown));
    }

    #[test]
    fn data_the_app_keeps_for_itself_is_never_ticked_for_the_user() {
        assert!(!ticks_itself(LeftoverKind::AppSupport));
        assert!(!ticks_itself(LeftoverKind::Container));
        assert!(!ticks_itself(LeftoverKind::GroupContainer));
        assert!(ticks_itself(LeftoverKind::Caches));
    }
}
