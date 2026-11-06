//! The classifier against a fixture home (T04) and the repository rules
//! (T07): every item lands in the expected rule, traps are never
//! `likely_safe`, ids are stable and the order is fixed.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use classifier::{check_ties, classify, record_files, ClassifiedItem, ItemKind, UNKNOWN_RULE_ID};
use fixtures::paths;
use rules::{Action, Mode, Rule, Tier};
use scanner::{scan, ScanOptions, ScanResult};

/// The repository's `rules/` folder.
fn repo_rules() -> Vec<Rule> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rules");
    let (rules, errors) = rules::load_dir(&dir);
    assert!(
        errors.is_empty(),
        "repository rules have errors: {errors:?}"
    );
    rules
}

/// A fixture home in a fresh temp folder, with its canonical root.
fn home() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fixtures::build(&root, std::time::SystemTime::now()).unwrap();
    (dir, root)
}

fn scan_for(root: &Path, rules: &[Rule], mode: Mode) -> ScanResult {
    let opts = ScanOptions {
        record_files: Some(record_files(rules, mode)),
        ..ScanOptions::default()
    };
    scan(root, opts, |_| {}).unwrap()
}

fn run(root: &Path, rules: &[Rule], mode: Mode) -> Vec<ClassifiedItem> {
    classify(&scan_for(root, rules, mode), rules, mode)
}

/// Path of `item` relative to `root`, as text.
fn rel(root: &Path, item: &ClassifiedItem) -> String {
    item.path
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn join(base: &str, name: &str) -> String {
    format!("{base}/{name}")
}

/// Every rule item the fixture should give in developer mode: relative
/// path to rule id.
fn expected_developer() -> BTreeMap<String, &'static str> {
    let mut want = expected_simple();
    let derived = [
        paths::DERIVED_DATA_LIVE_A,
        paths::DERIVED_DATA_LIVE_B,
        paths::DERIVED_DATA_ORPHAN,
    ];
    for p in derived {
        want.insert(p.to_string(), "xcode.derived_data");
    }
    want.insert(paths::XCODE_ARCHIVE.to_string(), "xcode.archives");
    for v in paths::IOS_DEVICE_SUPPORT_VERSIONS {
        want.insert(
            join(paths::IOS_DEVICE_SUPPORT, v),
            "xcode.ios_device_support",
        );
    }
    for v in paths::WATCHOS_DEVICE_SUPPORT_VERSIONS {
        want.insert(
            join(paths::WATCHOS_DEVICE_SUPPORT, v),
            "xcode.watchos_device_support",
        );
    }
    want.insert(paths::COCOAPODS_CACHE.to_string(), "cocoapods.cache");
    for p in paths::JS_PROJECTS {
        want.insert(join(p, "node_modules"), "js.node_modules");
    }
    want.insert(join(paths::NPM_CACHE, "_cacache"), "js.npm_cache");
    want.insert(join(paths::PNPM_STORE, "store"), "js.pnpm_store");
    want.insert(paths::YARN_CACHE.to_string(), "js.yarn_cache");
    want.insert(paths::BUN_CACHE.to_string(), "js.bun_cache");
    for c in paths::EXPO_CACHES {
        want.insert(join(paths::EXPO_HOME, c), "js.expo_cache");
    }
    // The two simulator rules tie (same match.path), so neither is used and
    // the simulator devices stay unknown.
    want
}

/// Every rule item the fixture should give in simple mode.
fn expected_simple() -> BTreeMap<String, &'static str> {
    let mut want = BTreeMap::new();
    for id in paths::BACKUP_DEVICE_IDS {
        want.insert(join(paths::MOBILESYNC_BACKUP, id), "ios.device_backup");
    }
    want.insert(paths::DOWNLOAD_DMG.to_string(), "downloads.disk_image");
    want.insert(paths::SAFARI_CACHE.to_string(), "browser.safari_cache");
    want.insert(paths::CHROME_CACHE.to_string(), "browser.chrome_cache");
    want.insert(paths::FIREFOX_CACHE.to_string(), "browser.firefox_cache");
    want.insert(join(paths::CACHES, "com.spotify.client"), "spotify.cache");
    want.insert(paths::SLACK_CACHE.to_string(), "slack.cache");
    for f in paths::USER_LOG_FOLDERS {
        want.insert(join(paths::USER_LOGS, f), "logs.user");
    }
    want.insert(
        paths::IPHONE_SOFTWARE_UPDATE.to_string(),
        "ios.software_update",
    );
    want
}

/// The rule items found: relative path to rule id.
fn found(root: &Path, items: &[ClassifiedItem]) -> BTreeMap<String, String> {
    items
        .iter()
        .filter(|i| i.rule_id != UNKNOWN_RULE_ID)
        .map(|i| (rel(root, i), i.rule_id.clone()))
        .collect()
}

fn owned(map: BTreeMap<String, &'static str>) -> BTreeMap<String, String> {
    map.into_iter().map(|(k, v)| (k, v.to_string())).collect()
}

#[test]
fn every_fixture_item_lands_in_its_rule_in_developer_mode() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let items = run(&root, &rules, Mode::Developer);
    assert_eq!(found(&root, &items), owned(expected_developer()));
}

#[test]
fn simple_mode_leaves_out_developer_rules() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let items = run(&root, &rules, Mode::Simple);
    assert_eq!(found(&root, &items), owned(expected_simple()));
    for item in &items {
        if let Some(rule) = rules.iter().find(|r| r.id == item.rule_id) {
            assert_eq!(rule.mode, Mode::Simple, "{}", item.rule_id);
        }
    }
}

#[test]
fn file_rules_match_files_and_folder_rules_match_folders() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let items = run(&root, &rules, Mode::Developer);
    for item in &items {
        let meta = std::fs::symlink_metadata(&item.path).unwrap();
        assert!(!meta.file_type().is_symlink(), "{}", item.path.display());
        match item.kind {
            ItemKind::File => assert!(meta.is_file(), "{}", item.path.display()),
            ItemKind::Folder => assert!(meta.is_dir(), "{}", item.path.display()),
        }
    }
    let dmg = items
        .iter()
        .find(|i| i.rule_id == "downloads.disk_image")
        .unwrap();
    assert_eq!(dmg.kind, ItemKind::File);
    assert_eq!(
        dmg.size_bytes,
        std::fs::symlink_metadata(root.join(paths::DOWNLOAD_DMG))
            .unwrap()
            .blocks()
            * 512
    );
    // Only .dmg: the .pkg, .zip and the document stay unknown.
    for p in [
        paths::DOWNLOAD_PKG,
        paths::DOWNLOAD_ZIP,
        paths::DOWNLOAD_NORMAL,
    ] {
        assert!(!items.iter().any(|i| i.path == root.join(p)), "{p}");
    }
}

/// Paths that must never be, or sit inside, a `likely_safe` item.
const TRAPS: &[&str] = &[
    paths::TRAP_HARD_LINK_A,
    paths::TRAP_HARD_LINK_B,
    paths::TRAP_GIT_DIR,
    paths::TRAP_ENV_FILE,
    paths::TRAP_KEYSTORE,
    paths::PHOTOS_LIBRARY,
    paths::MESSAGES_ATTACHMENTS,
    paths::SLACK_LOCAL_STORAGE,
    paths::EXPO_STATE,
    paths::DOCUMENTS,
    paths::DOWNLOADS,
    paths::DOWNLOAD_DMG,
    paths::DOWNLOAD_NORMAL,
    paths::MOBILESYNC_BACKUP,
    paths::XCODE_ARCHIVE,
    paths::ANDROID_AVD,
];

/// Symlink traps: nothing may be classified at or below them, because
/// the scan never follows them.
const SYMLINK_TRAPS: &[&str] = &[paths::TRAP_SYMLINK_ETC, paths::TRAP_SYMLINK_DOCUMENTS];

#[test]
fn trap_paths_never_classify_as_likely_safe() {
    let rules = repo_rules();
    let (_dir, root) = home();
    for mode in [Mode::Simple, Mode::Developer] {
        let items = run(&root, &rules, mode);
        for trap in TRAPS {
            let trap_path = root.join(trap);
            for item in items.iter().filter(|i| trap_path.starts_with(&i.path)) {
                assert_ne!(
                    item.tier,
                    Tier::LikelySafe,
                    "{trap} sits in likely_safe item {}",
                    item.path.display()
                );
            }
        }
        for trap in SYMLINK_TRAPS {
            let trap_path = root.join(trap);
            assert!(
                !items.iter().any(|i| i.path.starts_with(&trap_path)),
                "an item was found through symlink {trap}"
            );
        }
    }
}

#[test]
fn every_offered_item_passes_the_safety_crate() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let dev = std::fs::symlink_metadata(&root).unwrap().dev();
    for item in run(&root, &rules, Mode::Developer) {
        if item.tier == Tier::Keep {
            assert_eq!(item.action, Action::ShowOnly);
            assert!(!item.regenerable);
            continue;
        }
        let checked = safety::validate(&root, dev, &item.path, true);
        let checked = checked.unwrap_or_else(|r| panic!("{}: {r:?}", item.path.display()));
        if item.tier == Tier::LikelySafe {
            assert_eq!(
                checked.review_only(),
                None,
                "likely_safe item is review-only: {}",
                item.path.display()
            );
        }
    }
}

#[test]
fn unknown_space_is_grouped_by_top_level_folder_as_keep() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let result = scan_for(&root, &rules, Mode::Developer);
    let items = classify(&result, &rules, Mode::Developer);
    let unknown: Vec<&ClassifiedItem> = items
        .iter()
        .filter(|i| i.rule_id == UNKNOWN_RULE_ID)
        .collect();
    assert!(!unknown.is_empty());
    for item in &unknown {
        assert_eq!(item.tier, Tier::Keep);
        assert_eq!(item.action, Action::ShowOnly);
        assert_eq!(item.path.parent(), Some(root.as_path()));
    }
    for name in ["Documents", "Pictures", "Library"] {
        assert!(
            unknown.iter().any(|i| i.path == root.join(name)),
            "no unknown item for {name}"
        );
    }
    // The simulator devices (tied rules) are part of Library's unknown space.
    for id in paths::SIM_DEVICE_IDS {
        let p = root.join(paths::SIM_DEVICES).join(id);
        assert!(!items.iter().any(|i| i.path == p));
    }
    // Every byte below home is in exactly one item, except files directly in home.
    let sum: u64 = items.iter().map(|i| i.size_bytes).sum();
    assert_eq!(sum, result.total_bytes() - result.root_dir().own_bytes);
}

#[test]
fn ids_are_stable_across_scans_and_homes() {
    let rules = repo_rules();
    let (_a, root_a) = home();
    let first = run(&root_a, &rules, Mode::Developer);
    let second = run(&root_a, &rules, Mode::Developer);
    assert_eq!(first, second);
    let ids = |items: &[ClassifiedItem], root: &Path| -> Vec<(String, String)> {
        let mut v: Vec<_> = items
            .iter()
            .map(|i| (rel(root, i), i.id.to_string()))
            .collect();
        v.sort();
        v
    };
    let (_b, root_b) = home();
    let other = run(&root_b, &rules, Mode::Developer);
    assert_eq!(ids(&first, &root_a), ids(&other, &root_b));
    let mut unique: Vec<_> = first.iter().map(|i| &i.id).collect();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), first.len(), "ids are not unique");
}

#[test]
fn output_is_sorted_by_size_then_id() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let items = run(&root, &rules, Mode::Developer);
    for pair in items.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        assert!(
            a.size_bytes > b.size_bytes || (a.size_bytes == b.size_bytes && a.id < b.id),
            "{} before {}",
            a.path.display(),
            b.path.display()
        );
    }
}

#[test]
fn the_simulator_rules_tie() {
    let ties = check_ties(&repo_rules(), Mode::Developer);
    assert_eq!(ties.len(), 1);
    let pair = [ties[0].first.as_str(), ties[0].second.as_str()];
    assert!(pair.contains(&"xcode.simulator_device"));
    assert!(pair.contains(&"xcode.simulators_unavailable"));
    assert!(check_ties(&repo_rules(), Mode::Simple).is_empty());
}

#[test]
fn without_recorded_files_file_and_requires_rules_match_nothing() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let result = scan(&root, ScanOptions::default(), |_| {}).unwrap();
    let items = classify(&result, &rules, Mode::Developer);
    for id in [
        "downloads.disk_image",
        "ios.software_update",
        "ios.device_backup",
        "xcode.archives",
    ] {
        assert!(!items.iter().any(|i| i.rule_id == id), "{id}");
    }
}
