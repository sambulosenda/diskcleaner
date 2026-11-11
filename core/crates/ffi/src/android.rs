//! The Android screen: the Android SDK parts, emulators and Gradle downloads
//! in the home folder, how big each is, and whether any project uses it; and
//! the dry run that turns the ticked ones into a cleanup plan.
//!
//! Nothing here changes a file. Parts and projects are found by
//! `analysers::android` (read only) and measured with the read-only scanner.
//! The app sends item ids, never paths. The plan this builds is checked by
//! the executor's own preview and carried out by the same `run_cleanup` as
//! any other cleanup, so it goes to the Trash only, and put back and History
//! work the same way.
//!
//! Safety, on top of everything every cleanup gets:
//! - Nothing is ticked for the user, even when no project uses it.
//! - Every item is chosen on its own, so emulators (review-only in the
//!   safety rules) can be offered at all.
//! - Each item is skipped by the executor while Android Studio, Gradle or
//!   the emulator (whichever uses it) is running.
//! - A part is called unused only when every project found names its
//!   versions plainly; see `analysers::android`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use analysers::android::{Part, PartKind, Project, Usage};
use planner::{Expected, ItemRequest};

use crate::cleanup::{check_entries, Entry, Prepared};
use crate::types::{AndroidKind, AndroidOverview, AndroidPart, AndroidProject, AndroidUsage, Mode};
use crate::uninstall::{home_dev, measure, shown};
use crate::CoreError;

/// Shown when there is no Android SDK, emulator or Gradle folder at all.
const NOTHING_FOUND: &str =
    "Roomy didn't find the Android SDK, any emulators or Gradle in your home folder.";
/// Shown when no Android project was found.
const NO_PROJECTS: &str = "Roomy didn't find any Android projects in your home folder, so it can't tell which of these you still use. If your projects are somewhere else, such as another disk, check before you remove anything.";

/// What the core keeps between `android_overview` and
/// `preview_android_cleanup`.
#[derive(Debug, Default)]
pub(crate) struct AndroidState {
    /// Bumped by every `android_overview`, so ids from an older one are refused.
    generation: u64,
    /// The items of the latest overview.
    items: Vec<Item>,
}

/// One item of the latest overview, with what it looked like then, so the
/// executor can refuse it if it changed since.
#[derive(Debug, Clone)]
struct Item {
    item_id: String,
    title: String,
    kind: PartKind,
    path: PathBuf,
    expected: Expected,
    shown_path: Option<String>,
}

/// Finds, measures and explains every Android part in `home`, and
/// remembers them for `prepare`. Replaces the earlier overview.
pub(crate) fn overview(state: &mut AndroidState, home: &Path, mode: Mode) -> AndroidOverview {
    state.generation = state.generation.wrapping_add(1);
    state.items.clear();
    let sdk_found = analysers::android::has_sdk(home);
    let found = analysers::android::find_parts(home);
    let projects = if found.is_empty() {
        Vec::new()
    } else {
        analysers::android::find_projects(home)
    };
    let Ok(dev) = home_dev(home) else {
        return empty(sdk_found, NOTHING_FOUND);
    };
    let mut parts = Vec::new();
    for (n, part) in found.iter().enumerate() {
        let Some(expected) = measure(&part.path, Some(dev)) else {
            continue;
        };
        // A path the safety rules would never let go is not shown at all.
        if safety::validate(home, dev, &part.path, true).is_err() {
            continue;
        }
        let item_id = format!("android-{}-{n}", state.generation);
        let usage = analysers::android::usage(part, &found, &projects);
        let (title, detail) = names(part);
        let shown_path = shown(home, &part.path, mode);
        state.items.push(Item {
            item_id: item_id.clone(),
            title: title.clone(),
            kind: part.kind,
            path: part.path.clone(),
            expected,
            shown_path: shown_path.clone(),
        });
        parts.push(AndroidPart {
            item_id,
            kind: kind(part.kind),
            title,
            detail,
            explain: explain(part.kind).to_string(),
            size_bytes: expected.size_bytes,
            usage_text: usage_text(part.kind, &usage),
            usage: usage_for_app(usage),
            last_used_unix_seconds: part.last_used.and_then(unix_seconds),
            path: shown_path,
        });
    }
    let total_bytes = parts
        .iter()
        .fold(0u64, |s, p| s.saturating_add(p.size_bytes));
    let unused_bytes = parts
        .iter()
        .filter(|p| p.usage == AndroidUsage::NotUsed)
        .fold(0u64, |s, p| s.saturating_add(p.size_bytes));
    let note = if parts.is_empty() {
        Some(NOTHING_FOUND.to_string())
    } else if projects.is_empty() {
        Some(NO_PROJECTS.to_string())
    } else {
        None
    };
    AndroidOverview {
        sdk_found,
        parts,
        projects: projects.iter().map(|p| project(home, p, mode)).collect(),
        total_bytes,
        unused_bytes,
        note,
    }
}

/// Builds the dry run for the ticked items of the latest overview, exactly
/// as a cleanup would check them, and keeps the plan of what would move.
pub(crate) fn prepare(
    state: &AndroidState,
    home: &Path,
    item_ids: &[String],
    plan_id: u64,
    guards: &executor::Guards,
    now: SystemTime,
) -> Result<Prepared, CoreError> {
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for id in item_ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let item = state
            .items
            .iter()
            .find(|i| &i.item_id == id)
            .ok_or(CoreError::UnknownItem)?;
        entries.push(Entry {
            item_id: item.item_id.clone(),
            title: item.title.clone(),
            history_title: Some(format!("Android: {}", item.title)),
            size_bytes: item.expected.size_bytes,
            path: item.shown_path.clone(),
            device_backup: None,
            restore: None,
            request: Some(ItemRequest {
                recommendation_id: item.item_id.clone(),
                path: item.path.clone(),
                expected: item.expected,
                regenerable: regenerable(item.kind),
                // Every item is its own row that the user ticks.
                selected_individually: true,
                owners: planner::owner_apps(rule_for(item.kind)),
                belongs_to: None,
                keep_copy: None,
            }),
            wont_move: None,
        });
    }
    check_entries(home, home_dev(home)?, entries, plan_id, guards, now)
}

/// The overview when nothing could be looked at.
fn empty(sdk_found: bool, note: &str) -> AndroidOverview {
    AndroidOverview {
        sdk_found,
        parts: Vec::new(),
        projects: Vec::new(),
        total_bytes: 0,
        unused_bytes: 0,
        note: Some(note.to_string()),
    }
}

/// Whether a part's tool can download or make it again. An emulator holds
/// the apps and data put in it, so it can't. (Cleanups are Trash only on
/// this screen anyway.)
fn regenerable(kind: PartKind) -> bool {
    kind != PartKind::Emulator
}

/// The rule in `rules/android.toml` that covers this kind of part, for the
/// apps that must be closed before it moves. Platforms and build tools have
/// no rule of their own; they belong to Android Studio, like the NDK.
fn rule_for(kind: PartKind) -> &'static str {
    match kind {
        PartKind::SystemImage => "android.system_image",
        PartKind::Emulator => "android.emulator",
        PartKind::Platform | PartKind::BuildTools | PartKind::Ndk => "android.ndk",
        PartKind::GradleVersion => "android.gradle_wrapper",
        PartKind::GradleCache => "android.gradle_caches",
    }
}

/// The app's kind for an analyser kind.
fn kind(kind: PartKind) -> AndroidKind {
    match kind {
        PartKind::SystemImage => AndroidKind::SystemImage,
        PartKind::Emulator => AndroidKind::Emulator,
        PartKind::Platform => AndroidKind::Platform,
        PartKind::BuildTools => AndroidKind::BuildTools,
        PartKind::Ndk => AndroidKind::Ndk,
        PartKind::GradleVersion => AndroidKind::GradleVersion,
        PartKind::GradleCache => AndroidKind::GradleCache,
    }
}

/// The app's usage for the analyser's.
fn usage_for_app(usage: Usage) -> AndroidUsage {
    match usage {
        Usage::Used { by } => AndroidUsage::Used { by },
        Usage::ProbablyUsed { by } => AndroidUsage::ProbablyUsed { by },
        Usage::NotSure { by } => AndroidUsage::NotSure { by },
        Usage::NotUsed => AndroidUsage::NotUsed,
        Usage::NotApplicable => AndroidUsage::NotApplicable,
    }
}

/// The Android version name for an API level, as people know it.
fn android_version(api: u32) -> Option<&'static str> {
    Some(match api {
        21 => "5",
        22 => "5.1",
        23 => "6",
        24 => "7",
        25 => "7.1",
        26 => "8",
        27 => "8.1",
        28 => "9",
        29 => "10",
        30 => "11",
        31 => "12",
        32 => "12L",
        33 => "13",
        34 => "14",
        35 => "15",
        36 => "16",
        _ => return None,
    })
}

/// "Android 14 (API 34)", or "API 19" for a level without a known name.
fn android_name(api: Option<u32>) -> Option<String> {
    let api = api?;
    Some(match android_version(api) {
        Some(v) => format!("Android {v} (API {api})"),
        None => format!("API {api}"),
    })
}

/// Plain words for a system image's kind.
fn image_tag(tag: &str) -> String {
    match tag {
        "google_apis" => "Google APIs".to_string(),
        "google_apis_playstore" => "Google Play".to_string(),
        "default" => "Basic".to_string(),
        "google_atd" | "aosp_atd" => "For automated tests".to_string(),
        "android-wear" | "wear" => "Wear OS".to_string(),
        "android-tv" | "google-tv" => "TV".to_string(),
        "android-automotive" | "android-automotive-playstore" => "Car".to_string(),
        other => other.replace(['_', '-'], " "),
    }
}

/// Plain words for the processor a system image is for.
fn abi(abi: &str) -> String {
    match abi {
        "arm64-v8a" => "for Apple silicon Macs".to_string(),
        "x86_64" | "x86" => "for Intel Macs".to_string(),
        other => format!("for {other}"),
    }
}

/// The title and the line under it for one part.
fn names(part: &Part) -> (String, Option<String>) {
    let version = android_name(part.api_level);
    match part.kind {
        PartKind::SystemImage => {
            let title = match &version {
                Some(v) => format!("{v} emulator image"),
                None => format!("Emulator image {}", part.name),
            };
            let detail = match (&part.image_tag, &part.abi) {
                (Some(t), Some(a)) => Some(format!("{}, {}", image_tag(t), abi(a))),
                _ => None,
            };
            (title, detail)
        }
        PartKind::Emulator => {
            let title = part
                .display_name
                .clone()
                .unwrap_or_else(|| part.name.replace('_', " "));
            (title, version.map(|v| format!("Runs {v}")))
        }
        PartKind::Platform => match version {
            Some(v) => (format!("{v} SDK platform"), None),
            None => (format!("SDK platform {}", part.name), None),
        },
        PartKind::BuildTools => (format!("Build tools {}", part.name), None),
        PartKind::Ndk => (format!("NDK {}", part.name), None),
        PartKind::GradleVersion => {
            let rest = part.name.strip_prefix("gradle-").unwrap_or(&part.name);
            let (version, all) = match rest.rsplit_once('-') {
                Some((v, "all")) => (v, true),
                Some((v, _)) => (v, false),
                None => (rest, false),
            };
            let detail = all.then(|| "With its documentation and source".to_string());
            (format!("Gradle {version}"), detail)
        }
        PartKind::GradleCache => ("Gradle cache".to_string(), None),
    }
}

/// What a kind of part is, and what happens if it goes, in plain words.
fn explain(kind: PartKind) -> &'static str {
    match kind {
        PartKind::SystemImage => "The Android software an emulator runs, for one Android version. Emulators that use it won't start until you download it again from the SDK Manager in Android Studio.",
        PartKind::Emulator => "One Android emulator, with the apps, data and saved states inside it. These are lost if you remove it; a new emulator starts empty.",
        PartKind::Platform => "What projects build against for one Android version. A project that needs it won't build until you download it again from the SDK Manager.",
        PartKind::BuildTools => "Tools that package Android apps, for one version. If a project needs it, Android Studio downloads it again when it next builds.",
        PartKind::Ndk => "Tools for building the parts of Android apps written in C or C++, for one version. A project that needs it won't build until you download it again.",
        PartKind::GradleVersion => "A copy of one Gradle version that a project's Gradle wrapper downloaded. Projects that use it download it again on their next build.",
        PartKind::GradleCache => "Libraries and build results Gradle has downloaded or made, kept so builds are faster. The next build downloads and makes them again, which takes longer.",
    }
}

/// The usage in plain words.
fn usage_text(kind: PartKind, usage: &Usage) -> String {
    let what = match kind {
        PartKind::SystemImage => "emulator",
        _ => "project",
    };
    match usage {
        Usage::Used { by } => match kind {
            PartKind::SystemImage => format!("Used by {}", list(by, "emulator")),
            _ => format!("Used by {}", list(by, "project")),
        },
        Usage::ProbablyUsed { by } => format!(
            "Probably used by {}, which leaves the version to Android Studio",
            list(by, "project")
        ),
        Usage::NotSure { by } => match kind {
            PartKind::SystemImage => format!(
                "Roomy couldn't tell which image {} uses",
                list(by, "emulator")
            ),
            _ => format!(
                "Roomy couldn't read the settings of {}, so it can't tell",
                list(by, "project")
            ),
        },
        Usage::NotUsed => format!("No {what} Roomy found uses this"),
        Usage::NotApplicable => match kind {
            PartKind::GradleCache => "Shared by every Gradle project".to_string(),
            _ => String::new(),
        },
    }
}

/// "Shop", "Shop and Notes", or "Shop and 3 other projects".
fn list(names: &[String], noun: &str) -> String {
    match names {
        [] => format!("a {noun}"),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [first, rest @ ..] => format!("{first} and {} other {noun}s", rest.len()),
    }
}

/// What the app shows for one project.
fn project(home: &Path, p: &Project, mode: Mode) -> AndroidProject {
    AndroidProject {
        name: p.name.clone(),
        path: shown(home, &p.root, mode),
        last_activity_unix_seconds: p.last_activity.and_then(unix_seconds),
    }
}

/// Seconds since 1970, or `None` for a time before then.
fn unix_seconds(t: SystemTime) -> Option<i64> {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_plainly() {
        assert_eq!(
            android_name(Some(34)).as_deref(),
            Some("Android 14 (API 34)")
        );
        assert_eq!(android_name(Some(19)).as_deref(), Some("API 19"));
        assert_eq!(
            list(&["A".into(), "B".into(), "C".into()], "project"),
            "A and 2 other projects"
        );
        assert_eq!(image_tag("google_apis_playstore"), "Google Play");
    }

    #[test]
    fn only_emulators_are_not_regenerable() {
        assert!(!regenerable(PartKind::Emulator));
        assert!(regenerable(PartKind::SystemImage));
        assert!(regenerable(PartKind::GradleCache));
    }

    #[test]
    fn every_kind_has_an_owner_app() {
        for kind in [
            PartKind::SystemImage,
            PartKind::Emulator,
            PartKind::Platform,
            PartKind::BuildTools,
            PartKind::Ndk,
            PartKind::GradleVersion,
            PartKind::GradleCache,
        ] {
            assert!(!planner::owner_apps(rule_for(kind)).is_empty(), "{kind:?}");
        }
    }
}
