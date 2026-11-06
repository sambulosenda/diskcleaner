//! Finds installed apps and the files they leave in the home folder, for the
//! app uninstaller.
//!
//! Read only. Nothing here changes a file, starts a program or follows a
//! symlink: every folder entry is checked with `symlink_metadata`, and an
//! app's `Info.plist` is opened with `O_NOFOLLOW` after checking that its
//! `Contents` folder is a real folder.
//!
//! Leftovers are matched by the app's bundle id, in the folders where macOS
//! apps keep their data (`~/Library/Caches`, `~/Library/Containers` and so
//! on). Each match says how sure it is ([`Match`]): an exact bundle id match
//! is the app's own; a longer name that starts with the bundle id, or a
//! folder with the app's name, probably is, and the app must not tick those
//! for the user.
//!
//! Apple's own apps (bundle ids starting `com.apple.`) are never listed.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// The start of every Apple bundle id. Apple's apps are never offered.
pub const APPLE_PREFIX: &str = "com.apple.";

/// The longest app name the app is given, in characters.
pub const APP_NAME_MAX_CHARS: usize = 60;

/// The longest bundle id accepted. Real ones are far shorter.
pub const BUNDLE_ID_MAX_LEN: usize = 155;

/// An app found in an Applications folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The `.app` bundle folder, for example `/Applications/Slack.app`.
    pub bundle: PathBuf,
    /// `CFBundleIdentifier`, checked with [`valid_bundle_id`].
    pub bundle_id: String,
    /// The name to show: `CFBundleDisplayName`, `CFBundleName` or the
    /// bundle's file name, made safe to show.
    pub name: String,
    /// `CFBundleShortVersionString`, if there is one, made safe to show.
    pub version: Option<String>,
}

/// Where in `~/Library` a leftover was found. Says what kind of data it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeftoverKind {
    /// `~/Library/Application Support`: the app's own data.
    AppSupport,
    /// `~/Library/Caches`: copies the app can download or rebuild.
    Caches,
    /// `~/Library/Containers`: a sandboxed app's whole home.
    Container,
    /// `~/Library/Group Containers`: data the app shares with its helpers.
    GroupContainer,
    /// `~/Library/Preferences` (and `ByHost`): the app's settings.
    Preferences,
    /// `~/Library/Saved Application State`: which windows were open.
    SavedState,
    /// `~/Library/HTTPStorages`, `~/Library/WebKit` and `~/Library/Cookies`:
    /// web data such as cookies and sign-ins.
    WebData,
    /// `~/Library/Logs`: logs the app wrote.
    Logs,
    /// `~/Library/LaunchAgents`: something the app set to start at login.
    LaunchAgent,
    /// `~/Library/Application Scripts`: scripts for a sandboxed app.
    AppScripts,
}

/// How a leftover was matched to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Match {
    /// Named exactly by the bundle id (with the usual ending for its folder,
    /// such as `.plist`). This is the app's own.
    BundleId,
    /// The name starts with the bundle id and a dot, for example an updater's
    /// cache. Probably the app's.
    Related,
    /// A folder with the app's name. Probably the app's, but another app
    /// could use the same name.
    Name,
}

/// One file or folder an app left in the home folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    /// The full path, with the name as it is on disk.
    pub path: PathBuf,
    /// What kind of data it is.
    pub kind: LeftoverKind,
    /// How it was matched to the app.
    pub matched: Match,
    /// Whether it is a folder.
    pub is_dir: bool,
}

/// One way a name in a folder can match the app.
#[derive(Debug, Clone, Copy)]
enum Pattern {
    /// The bundle id itself.
    Exact,
    /// The bundle id followed by this ending, for example `.plist`.
    Ending(&'static str),
    /// `<bundle id>.<anything>.plist`, as in `Preferences/ByHost`.
    ByHost,
    /// `group.<bundle id>` or `<team id>.<bundle id>`.
    Group,
    /// `<bundle id>.<more>`, matched as [`Match::Related`].
    Related,
    /// The app's name, matched as [`Match::Name`].
    Name,
}

/// A folder under the home folder where apps keep data, and what to look for.
struct Place {
    rel: &'static str,
    kind: LeftoverKind,
    patterns: &'static [Pattern],
}

/// Every place leftovers are looked for, relative to home.
const PLACES: &[Place] = &[
    Place {
        rel: "Library/Application Support",
        kind: LeftoverKind::AppSupport,
        patterns: &[Pattern::Exact, Pattern::Related, Pattern::Name],
    },
    Place {
        rel: "Library/Caches",
        kind: LeftoverKind::Caches,
        patterns: &[Pattern::Exact, Pattern::Related, Pattern::Name],
    },
    Place {
        rel: "Library/Containers",
        kind: LeftoverKind::Container,
        patterns: &[Pattern::Exact],
    },
    Place {
        rel: "Library/Group Containers",
        kind: LeftoverKind::GroupContainer,
        patterns: &[Pattern::Group],
    },
    Place {
        rel: "Library/Preferences",
        kind: LeftoverKind::Preferences,
        patterns: &[Pattern::Ending(".plist")],
    },
    Place {
        rel: "Library/Preferences/ByHost",
        kind: LeftoverKind::Preferences,
        patterns: &[Pattern::ByHost],
    },
    Place {
        rel: "Library/Saved Application State",
        kind: LeftoverKind::SavedState,
        patterns: &[Pattern::Ending(".savedState")],
    },
    Place {
        rel: "Library/HTTPStorages",
        kind: LeftoverKind::WebData,
        patterns: &[Pattern::Exact, Pattern::Ending(".binarycookies")],
    },
    Place {
        rel: "Library/WebKit",
        kind: LeftoverKind::WebData,
        patterns: &[Pattern::Exact],
    },
    Place {
        rel: "Library/Cookies",
        kind: LeftoverKind::WebData,
        patterns: &[Pattern::Ending(".binarycookies")],
    },
    Place {
        rel: "Library/Logs",
        kind: LeftoverKind::Logs,
        patterns: &[Pattern::Exact, Pattern::Name],
    },
    Place {
        rel: "Library/LaunchAgents",
        kind: LeftoverKind::LaunchAgent,
        patterns: &[Pattern::Ending(".plist"), Pattern::Related],
    },
    Place {
        rel: "Library/Application Scripts",
        kind: LeftoverKind::AppScripts,
        patterns: &[Pattern::Exact],
    },
];

/// Why an app bundle could not be read.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The bundle or its `Contents` folder is missing, a symlink or not a folder.
    #[error("the app is not a real app bundle")]
    NotABundle,
    /// `Info.plist` could not be read.
    #[error("the app's Info.plist could not be read")]
    InfoUnreadable,
    /// `Info.plist` has no bundle id, or one that is not safe to use.
    #[error("the app has no usable bundle id")]
    NoBundleId,
    /// The app is one of Apple's.
    #[error("the app is one of Apple's")]
    Apple,
}

/// Lists the apps directly inside each of `folders` (for example
/// `/Applications` and `~/Applications`), sorted by name. Apple's apps,
/// symlinks and bundles that can't be read are left out. A folder that
/// can't be listed adds nothing.
pub fn list_apps(folders: &[PathBuf]) -> Vec<App> {
    let mut apps = Vec::new();
    for folder in folders {
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !has_app_extension(&path) {
                continue;
            }
            match read_app(&path) {
                Ok(app) => apps.push(app),
                Err(err) => tracing::debug!(%err, path = %path.display(), "app skipped"),
            }
        }
    }
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.bundle.cmp(&b.bundle))
    });
    apps
}

/// Reads one `.app` bundle. Refuses symlinks, bundles with no usable bundle
/// id, and Apple's apps.
pub fn read_app(bundle: &Path) -> Result<App, AppError> {
    let meta = fs::symlink_metadata(bundle).map_err(|_| AppError::NotABundle)?;
    if !meta.is_dir() {
        return Err(AppError::NotABundle);
    }
    let contents = bundle.join("Contents");
    let meta = fs::symlink_metadata(&contents).map_err(|_| AppError::NotABundle)?;
    if !meta.is_dir() {
        return Err(AppError::NotABundle);
    }
    let bytes = crate::ios_backup::read_capped(&contents.join("Info.plist"))
        .map_err(|_| AppError::InfoUnreadable)?;
    let fallback = bundle
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    parse_info(&bytes, bundle, &fallback)
}

/// Reads the bundle id, name and version from `Info.plist` bytes.
fn parse_info(bytes: &[u8], bundle: &Path, fallback_name: &str) -> Result<App, AppError> {
    let value =
        plist::Value::from_reader(Cursor::new(bytes)).map_err(|_| AppError::InfoUnreadable)?;
    let dict = value.as_dictionary().ok_or(AppError::InfoUnreadable)?;
    let text = |key: &str| dict.get(key).and_then(plist::Value::as_string);
    let bundle_id = text("CFBundleIdentifier")
        .filter(|id| valid_bundle_id(id))
        .ok_or(AppError::NoBundleId)?
        .to_string();
    if is_apple(&bundle_id) {
        return Err(AppError::Apple);
    }
    let name = text("CFBundleDisplayName")
        .and_then(clean_text)
        .or_else(|| text("CFBundleName").and_then(clean_text))
        .or_else(|| clean_text(fallback_name))
        .ok_or(AppError::NoBundleId)?;
    let version = text("CFBundleShortVersionString").and_then(clean_text);
    Ok(App {
        bundle: bundle.to_path_buf(),
        bundle_id,
        name,
        version,
    })
}

/// Whether `id` is safe to build file names from: letters, digits, `.`,
/// `-` and `_` only, at least two parts, no empty part, and not too long.
/// So it can never hold `/`, `..` or a hidden name.
pub fn valid_bundle_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= BUNDLE_ID_MAX_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && id.split('.').count() >= 2
        && id.split('.').all(|part| !part.is_empty())
}

/// Whether `id` is one of Apple's bundle ids.
pub fn is_apple(id: &str) -> bool {
    id.to_ascii_lowercase().starts_with(APPLE_PREFIX)
}

/// Finds what `app` left in `home`, by bundle id and name.
///
/// `others` is every other installed app. A [`Match::Related`] name that
/// also starts with another app's longer bundle id (for example
/// `com.google.Chrome.canary` when removing `com.google.Chrome`) is that
/// app's, and a [`Match::Name`] folder that another app is also called is
/// left out. Symlinks are never returned. Each path is returned once, with
/// its surest match.
pub fn find_leftovers(home: &Path, app: &App, others: &[App]) -> Vec<Leftover> {
    let mut found: Vec<Leftover> = Vec::new();
    for place in PLACES {
        let folder = home.join(place.rel);
        if !is_real_dir(&folder) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(matched) = match_name(name, place.patterns, app, others) else {
                continue;
            };
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !(meta.is_dir() || meta.is_file()) {
                continue;
            }
            if let Some(existing) = found.iter_mut().find(|l| l.path == path) {
                existing.matched = existing.matched.min(matched);
                continue;
            }
            found.push(Leftover {
                path,
                kind: place.kind,
                matched,
                is_dir: meta.is_dir(),
            });
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// How `name` (one entry of a [`Place`]) matches `app`, if it does.
/// Compares without regard to ASCII case, as Mac disks usually do.
fn match_name(name: &str, patterns: &[Pattern], app: &App, others: &[App]) -> Option<Match> {
    let folded = name.to_ascii_lowercase();
    let id = app.bundle_id.to_ascii_lowercase();
    let mut best: Option<Match> = None;
    for pattern in patterns {
        let hit = match pattern {
            Pattern::Exact => (folded == id).then_some(Match::BundleId),
            Pattern::Ending(ending) => (folded == format!("{id}{}", ending.to_ascii_lowercase()))
                .then_some(Match::BundleId),
            Pattern::ByHost => {
                let rest = folded.strip_prefix(&format!("{id}."));
                rest.and_then(|r| r.strip_suffix(".plist"))
                    .filter(|host| !host.is_empty() && !host.contains('.'))
                    .map(|_| Match::BundleId)
            }
            Pattern::Group => is_group_name(&folded, &id).then_some(Match::BundleId),
            Pattern::Related => folded
                .strip_prefix(&format!("{id}."))
                .filter(|rest| !rest.is_empty())
                .filter(|_| !belongs_to_longer_id(&folded, &id, others))
                .map(|_| Match::Related),
            Pattern::Name => (name_matches(&folded, app)
                && !others
                    .iter()
                    .any(|o| o.bundle_id != app.bundle_id && name_matches(&folded, o)))
            .then_some(Match::Name),
        };
        if let Some(hit) = hit {
            best = Some(best.map_or(hit, |b| b.min(hit)));
        }
    }
    best
}

/// Whether `folded` is `group.<id>` or `<team id>.<id>`, where a team id is
/// ten capital letters or digits (compared folded).
fn is_group_name(folded: &str, id: &str) -> bool {
    if folded == format!("group.{id}") {
        return true;
    }
    match folded.split_once('.') {
        Some((team, rest)) => {
            team.len() == 10 && team.bytes().all(|b| b.is_ascii_alphanumeric()) && rest == id
        }
        None => false,
    }
}

/// Whether `folded` also starts with another installed app's bundle id that
/// is longer than `id`, so it is more likely that app's.
fn belongs_to_longer_id(folded: &str, id: &str, others: &[App]) -> bool {
    others.iter().any(|o| {
        let other = o.bundle_id.to_ascii_lowercase();
        other.len() > id.len()
            && other.starts_with(id)
            && (folded == other || folded.starts_with(&format!("{other}.")))
    })
}

/// Whether the folded entry name is the app's name. Names shorter than
/// three characters are never matched, since they are too likely to clash.
fn name_matches(folded: &str, app: &App) -> bool {
    let name = app.name.to_ascii_lowercase();
    name.chars().count() >= 3 && folded == name
}

/// Whether `path` is a real folder (not a symlink).
fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// Whether the path's name ends in `.app`, ignoring case.
fn has_app_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

/// Text from an app's `Info.plist` made safe to show, the same way as a
/// device name: no control or text-direction characters, single spaces,
/// at most [`APP_NAME_MAX_CHARS`] characters.
fn clean_text(raw: &str) -> Option<String> {
    let cleaned = crate::ios_backup::clean_device_name(raw)?;
    if cleaned.chars().count() <= APP_NAME_MAX_CHARS {
        return Some(cleaned);
    }
    let cut: String = cleaned.chars().take(APP_NAME_MAX_CHARS - 1).collect();
    Some(format!("{}\u{2026}", cut.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, name: &str) -> Vec<u8> {
        let mut dict = plist::Dictionary::new();
        dict.insert("CFBundleIdentifier".into(), plist::Value::String(id.into()));
        dict.insert("CFBundleName".into(), plist::Value::String(name.into()));
        dict.insert(
            "CFBundleShortVersionString".into(),
            plist::Value::String("4.1".into()),
        );
        let mut out = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_xml(&mut out)
            .unwrap();
        out
    }

    fn make_app(dir: &Path, file: &str, id: &str, name: &str) -> PathBuf {
        let bundle = dir.join(file);
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("Contents/Info.plist"), info(id, name)).unwrap();
        bundle
    }

    fn app(id: &str, name: &str) -> App {
        App {
            bundle: PathBuf::from(format!("/Applications/{name}.app")),
            bundle_id: id.into(),
            name: name.into(),
            version: None,
        }
    }

    #[test]
    fn bundle_ids_must_be_safe_to_build_names_from() {
        assert!(valid_bundle_id("com.tinyspeck.slackmacgap"));
        assert!(valid_bundle_id("org.mozilla-firefox_x.app"));
        for bad in [
            "",
            "slack",
            "com..slack",
            ".com.slack",
            "com.slack.",
            "com/slack",
            "../..",
            "com.slack app",
            "com.slack\0",
            "com.sl\u{e4}ck",
        ] {
            assert!(!valid_bundle_id(bad), "{bad:?}");
        }
        assert!(!valid_bundle_id(&format!("com.{}", "a".repeat(200))));
    }

    #[test]
    fn apple_apps_are_never_listed() {
        let dir = tempfile::tempdir().unwrap();
        make_app(dir.path(), "Safari.app", "com.apple.Safari", "Safari");
        make_app(dir.path(), "Xcode.app", "COM.APPLE.dt.Xcode", "Xcode");
        make_app(
            dir.path(),
            "Slack.app",
            "com.tinyspeck.slackmacgap",
            "Slack",
        );
        let apps = list_apps(&[dir.path().to_path_buf()]);
        let ids: Vec<&str> = apps.iter().map(|a| a.bundle_id.as_str()).collect();
        assert_eq!(ids, ["com.tinyspeck.slackmacgap"]);
        assert_eq!(apps[0].name, "Slack");
        assert_eq!(apps[0].version.as_deref(), Some("4.1"));
    }

    #[test]
    fn symlinked_and_broken_bundles_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let real = make_app(dir.path(), "Real.app", "com.example.real", "Real");
        std::os::unix::fs::symlink(&real, dir.path().join("Link.app")).unwrap();
        fs::create_dir_all(dir.path().join("Empty.app")).unwrap();
        let odd = dir.path().join("Odd.app");
        fs::create_dir_all(&odd).unwrap();
        std::os::unix::fs::symlink(real.join("Contents"), odd.join("Contents")).unwrap();
        make_app(dir.path(), "NoId.app", "not-an-id", "NoId");
        fs::write(dir.path().join("File.app"), b"x").unwrap();
        let apps = list_apps(&[dir.path().to_path_buf(), dir.path().join("missing")]);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].bundle, real);
    }

    #[test]
    fn names_are_made_safe_and_fall_back_to_the_file_name() {
        let mut dict = plist::Dictionary::new();
        dict.insert(
            "CFBundleIdentifier".into(),
            plist::Value::String("com.example.x".into()),
        );
        dict.insert(
            "CFBundleDisplayName".into(),
            plist::Value::String("\u{202E}Evil\u{0007}  Name".into()),
        );
        let mut bytes = Vec::new();
        plist::Value::Dictionary(dict.clone())
            .to_writer_binary(&mut bytes)
            .unwrap();
        let app = parse_info(&bytes, Path::new("/Applications/X.app"), "X").unwrap();
        assert_eq!(app.name, "Evil Name");
        dict.remove("CFBundleDisplayName");
        let mut bytes = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_xml(&mut bytes)
            .unwrap();
        let app = parse_info(&bytes, Path::new("/Applications/X.app"), "Fallback").unwrap();
        assert_eq!(app.name, "Fallback");
    }

    /// A home with Slack's leftovers, some look-alikes, and a symlink.
    fn slack_home() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let lib = home.join("Library");
        for d in [
            "Application Support/Slack",
            "Application Support/com.tinyspeck.slackmacgap",
            "Caches/com.tinyspeck.slackmacgap",
            "Caches/com.tinyspeck.slackmacgap.ShipIt",
            "Caches/com.tinyspeck.slackmacgap.beta",
            "Caches/com.tinyspeck.slackmacgapx",
            "Containers/com.tinyspeck.slackmacgap",
            "Group Containers/group.com.tinyspeck.slackmacgap",
            "Group Containers/BQR82RBBHL.com.tinyspeck.slackmacgap",
            "Group Containers/BQR82RBBHL.com.other",
            "Saved Application State/com.tinyspeck.slackmacgap.savedState",
            "HTTPStorages/com.tinyspeck.slackmacgap",
            "WebKit/com.tinyspeck.slackmacgap",
            "Logs/Slack",
            "Application Scripts/com.tinyspeck.slackmacgap",
            "Preferences/ByHost",
            "LaunchAgents",
            "Cookies",
        ] {
            fs::create_dir_all(lib.join(d)).unwrap();
        }
        for f in [
            "Preferences/com.tinyspeck.slackmacgap.plist",
            "Preferences/com.tinyspeck.slackmacgap.plist.lockfile",
            "Preferences/ByHost/com.tinyspeck.slackmacgap.0A1B2C3D.plist",
            "HTTPStorages/com.tinyspeck.slackmacgap.binarycookies",
            "Cookies/com.tinyspeck.slackmacgap.binarycookies",
            "LaunchAgents/com.tinyspeck.slackmacgap.plist",
            "LaunchAgents/com.tinyspeck.slackmacgap.helper.plist",
        ] {
            fs::write(lib.join(f), b"x").unwrap();
        }
        std::os::unix::fs::symlink(
            home.join("Documents"),
            lib.join("Caches/com.tinyspeck.slackmacgap.link"),
        )
        .unwrap();
        (dir, home)
    }

    #[test]
    fn finds_leftovers_by_bundle_id_and_name() {
        let (_d, home) = slack_home();
        let slack = app("com.tinyspeck.slackmacgap", "Slack");
        let beta = app("com.tinyspeck.slackmacgap.beta", "Slack Beta");
        let found = find_leftovers(&home, &slack, &[beta]);
        let got: Vec<(String, Match)> = found
            .iter()
            .map(|l| {
                let rel = l.path.strip_prefix(home.join("Library")).unwrap();
                (rel.to_string_lossy().into_owned(), l.matched)
            })
            .collect();
        let want: Vec<(String, Match)> = [
            (
                "Application Scripts/com.tinyspeck.slackmacgap",
                Match::BundleId,
            ),
            ("Application Support/Slack", Match::Name),
            (
                "Application Support/com.tinyspeck.slackmacgap",
                Match::BundleId,
            ),
            ("Caches/com.tinyspeck.slackmacgap", Match::BundleId),
            ("Caches/com.tinyspeck.slackmacgap.ShipIt", Match::Related),
            ("Containers/com.tinyspeck.slackmacgap", Match::BundleId),
            (
                "Cookies/com.tinyspeck.slackmacgap.binarycookies",
                Match::BundleId,
            ),
            (
                "Group Containers/BQR82RBBHL.com.tinyspeck.slackmacgap",
                Match::BundleId,
            ),
            (
                "Group Containers/group.com.tinyspeck.slackmacgap",
                Match::BundleId,
            ),
            ("HTTPStorages/com.tinyspeck.slackmacgap", Match::BundleId),
            (
                "HTTPStorages/com.tinyspeck.slackmacgap.binarycookies",
                Match::BundleId,
            ),
            (
                "LaunchAgents/com.tinyspeck.slackmacgap.helper.plist",
                Match::Related,
            ),
            (
                "LaunchAgents/com.tinyspeck.slackmacgap.plist",
                Match::BundleId,
            ),
            ("Logs/Slack", Match::Name),
            (
                "Preferences/ByHost/com.tinyspeck.slackmacgap.0A1B2C3D.plist",
                Match::BundleId,
            ),
            (
                "Preferences/com.tinyspeck.slackmacgap.plist",
                Match::BundleId,
            ),
            (
                "Saved Application State/com.tinyspeck.slackmacgap.savedState",
                Match::BundleId,
            ),
            ("WebKit/com.tinyspeck.slackmacgap", Match::BundleId),
        ]
        .iter()
        .map(|(p, m)| (p.to_string(), *m))
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn matching_ignores_case() {
        let (_d, home) = slack_home();
        let found = find_leftovers(&home, &app("COM.TinySpeck.SlackMacGap", "slack"), &[]);
        assert!(found
            .iter()
            .any(|l| l.path.ends_with("Caches/com.tinyspeck.slackmacgap")));
    }

    #[test]
    fn a_name_another_app_shares_is_left_out() {
        let (_d, home) = slack_home();
        let slack = app("com.tinyspeck.slackmacgap", "Slack");
        let twin = app("com.example.slack", "Slack");
        let found = find_leftovers(&home, &slack, &[twin]);
        assert!(found.iter().all(|l| l.matched != Match::Name));
    }

    #[test]
    fn short_names_are_never_matched() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Library/Application Support/Go")).unwrap();
        let found = find_leftovers(dir.path(), &app("com.example.go", "Go"), &[]);
        assert!(found.is_empty());
    }

    #[test]
    fn a_symlinked_library_folder_is_not_searched() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(elsewhere.join("com.example.x")).unwrap();
        fs::create_dir_all(home.join("Library")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, home.join("Library/Caches")).unwrap();
        assert!(find_leftovers(&home, &app("com.example.x", "Example"), &[]).is_empty());
    }
}
