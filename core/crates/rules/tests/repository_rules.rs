//! T07: the rule files in `rules/` load, each one finds what it is meant to
//! find in a fixture home, and none finds what it must not.

mod common;

use std::collections::BTreeSet;

use common::repo_root;
use fixtures::{paths, Manifest};
use rules::{load_dir, Glob, Rule, Tier};

/// Every rule in `rules/`, failing the test if any has a problem.
fn repo_rules() -> Vec<Rule> {
    let (rules, errors) = load_dir(&repo_root().join("rules"));
    assert!(errors.is_empty(), "{errors:#?}");
    rules
}

/// The manifest of a fresh fixture home. The home itself is removed when
/// the returned folder is dropped.
fn fixture_manifest() -> (tempfile::TempDir, Manifest) {
    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = fixtures::build(dir.path(), std::time::SystemTime::now()).expect("fixture");
    (dir, manifest)
}

/// The fixture paths `rule` matches, including its `match.requires` check.
fn matches_in(rule: &Rule, manifest: &Manifest) -> Vec<String> {
    let glob = Glob::parse(&rule.matcher.path).expect("valid glob");
    let requires: Vec<Glob> = rule
        .matcher
        .requires
        .iter()
        .map(|r| Glob::parse(r).expect("valid glob"))
        .collect();
    manifest
        .entries
        .iter()
        .map(|e| e.path.as_str())
        .filter(|p| glob.matches(p))
        .filter(|p| {
            requires.iter().all(|req| {
                let prefix = format!("{p}/");
                manifest
                    .entries
                    .iter()
                    .filter_map(|e| e.path.strip_prefix(&prefix))
                    .any(|inner| req.matches(inner))
            })
        })
        .map(str::to_string)
        .collect()
}

fn rule<'a>(rules: &'a [Rule], id: &str) -> &'a Rule {
    rules
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no rule {id}"))
}

#[test]
fn the_t07_rules_are_all_there() {
    let ids: BTreeSet<String> = repo_rules().into_iter().map(|r| r.id).collect();
    let expected: BTreeSet<String> = [
        // Xcode
        "xcode.derived_data",
        "xcode.archives",
        "xcode.ios_device_support",
        "xcode.watchos_device_support",
        "xcode.simulator_device",
        "xcode.simulators_unavailable",
        "cocoapods.cache",
        // JavaScript
        "js.node_modules",
        "js.npm_cache",
        "js.pnpm_store",
        "js.yarn_cache",
        "js.bun_cache",
        "js.expo_cache",
        // Everyday
        "ios.device_backup",
        "downloads.disk_image",
        "browser.safari_cache",
        "browser.chrome_cache",
        "browser.firefox_cache",
        "spotify.cache",
        "slack.cache",
        "logs.user",
        "ios.software_update",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(ids, expected);
}

#[test]
fn every_rule_matches_something_in_the_fixture_home() {
    let (_dir, manifest) = fixture_manifest();
    let missing: Vec<String> = repo_rules()
        .into_iter()
        .filter(|r| matches_in(r, &manifest).is_empty())
        .map(|r| r.id)
        .collect();
    assert!(missing.is_empty(), "no fixture match for: {missing:?}");
}

#[test]
fn rules_match_exactly_the_expected_fixture_items() {
    let (_dir, manifest) = fixture_manifest();
    let rules = repo_rules();
    let found = |id: &str| -> BTreeSet<String> {
        matches_in(rule(&rules, id), &manifest)
            .into_iter()
            .collect()
    };
    let set = |items: Vec<String>| -> BTreeSet<String> { items.into_iter().collect() };

    // One item per project or version, never the folder that holds them.
    assert_eq!(
        found("js.node_modules"),
        set(paths::JS_PROJECTS
            .iter()
            .map(|p| format!("{p}/node_modules"))
            .collect())
    );
    assert_eq!(
        found("xcode.ios_device_support"),
        set(paths::IOS_DEVICE_SUPPORT_VERSIONS
            .iter()
            .map(|v| format!("{}/{v}", paths::IOS_DEVICE_SUPPORT))
            .collect())
    );
    assert_eq!(
        found("ios.device_backup"),
        set(paths::BACKUP_DEVICE_IDS
            .iter()
            .map(|d| format!("{}/{d}", paths::MOBILESYNC_BACKUP))
            .collect())
    );
    assert_eq!(
        found("xcode.archives"),
        set(vec![paths::XCODE_ARCHIVE.to_string()])
    );

    // Simulator rules need a device.plist, so the device set file is left alone.
    let devices = set(paths::SIM_DEVICE_IDS
        .iter()
        .map(|d| format!("{}/{d}", paths::SIM_DEVICES))
        .collect());
    assert_eq!(found("xcode.simulator_device"), devices);
    assert_eq!(found("xcode.simulators_unavailable"), devices);

    // Expo's sign-in state is not a cache.
    assert_eq!(
        found("js.expo_cache"),
        set(paths::EXPO_CACHES
            .iter()
            .map(|c| format!("{}/{c}", paths::EXPO_HOME))
            .collect())
    );

    // Only disk images in Downloads, not the other downloads.
    assert_eq!(
        found("downloads.disk_image"),
        set(vec![paths::DOWNLOAD_DMG.to_string()])
    );
    assert_eq!(
        found("slack.cache"),
        set(vec![paths::SLACK_CACHE.to_string()])
    );
}

#[test]
fn no_rule_matches_personal_or_sign_in_data() {
    let rules = repo_rules();
    let must_not_match = [
        paths::EXPO_STATE,
        paths::SLACK_LOCAL_STORAGE,
        paths::DOWNLOAD_NORMAL,
        paths::DOWNLOAD_ZIP,
        paths::TRAP_GIT_DIR,
        paths::TRAP_ENV_FILE,
        paths::TRAP_KEYSTORE,
        paths::TRAP_HARD_LINK_A,
        paths::PHOTOS_LIBRARY,
        paths::MESSAGES_ATTACHMENTS,
        "Library/Messages/chat.db",
        "Documents/Taxes/2025 return.pdf",
        "projects/a/package.json",
        "Developer/Weather/Weather/App.swift",
    ];
    for r in &rules {
        let glob = Glob::parse(&r.matcher.path).expect("valid glob");
        for p in must_not_match {
            assert!(!glob.matches(p), "{} matches {p}", r.id);
        }
    }
}

#[test]
fn wording_follows_the_t07_writing_rules() {
    for r in repo_rules() {
        let texts = [
            Some(r.title.as_str()),
            Some(r.explain.as_str()),
            Some(r.risk.as_str()),
            r.restore.as_deref(),
        ];
        for text in texts.into_iter().flatten() {
            let lower = text.to_lowercase();
            assert!(!lower.contains("junk"), "{}: {text}", r.id);
            assert!(!lower.contains("rm -"), "{}: {text}", r.id);
        }
        // The tier says how safe it is; the explanation never promises it.
        assert!(
            !r.explain.to_lowercase().contains("safe"),
            "{}: {}",
            r.id,
            r.explain
        );
    }
}

#[test]
fn items_that_cannot_come_back_are_never_preselected_without_review() {
    for r in repo_rules() {
        if !r.regenerable && r.tier == Tier::LikelySafe {
            // Only logs are allowed this: they are not needed, but do not come back.
            assert_eq!(
                r.id, "logs.user",
                "{} is likely_safe but not regenerable",
                r.id
            );
        }
    }
}
