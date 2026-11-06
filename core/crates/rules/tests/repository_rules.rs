//! T07: the rule files in `rules/` load, each one finds what it is meant to
//! find in a fixture home, and none finds what it must not.

mod common;

use std::collections::BTreeSet;

use common::repo_root;
use fixtures::{paths, Manifest};
use rules::{load_dir, Category, Glob, Rule, Tier};

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
        "cocoapods.cache",
        // JavaScript
        "js.node_modules",
        "js.node_modules_developer",
        "js.node_modules_code",
        "js.node_modules_dev",
        "js.node_modules_src",
        "js.node_modules_repos",
        "js.node_modules_github",
        "js.node_modules_documents",
        "js.node_modules_home",
        "js.npm_cache",
        "js.npx_cache",
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
        // Android
        "android.gradle_caches",
        "android.gradle_wrapper",
        "android.system_image",
        "android.ndk",
        "android.emulator",
        "android.studio_cache",
        // Rust
        "rust.cargo_registry_cache",
        "rust.cargo_registry_src",
        "rust.toolchain",
        "rust.target",
        "rust.target_developer",
        "rust.target_code",
        "rust.target_dev",
        "rust.target_src",
        "rust.target_repos",
        "rust.target_github",
        "rust.target_documents",
        "rust.target_home",
        // AI tools
        "claude_code.conversations",
        "claude_code.logs",
        // Editors
        "cursor.cache",
        "cursor.cached_data",
        "cursor.extension_downloads",
        "vscode.cache",
        "vscode.cached_data",
        "vscode.extension_downloads",
        "jetbrains.cache",
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
    // Each code folder's rule finds its project's node_modules, and, since
    // the manifest lists every path, the package nested inside it too. The
    // classifier makes the nested one part of the outer item.
    assert_eq!(
        found("js.node_modules_developer"),
        set(vec![
            format!("{}/node_modules", paths::JS_PROJECTS_ELSEWHERE[0]),
            paths::JS_NESTED_MODULES.to_string(),
        ])
    );
    assert_eq!(
        found("js.node_modules_code"),
        set(vec![format!(
            "{}/node_modules",
            paths::JS_PROJECTS_ELSEWHERE[1]
        )])
    );
    assert_eq!(
        found("js.node_modules_documents"),
        set(vec![format!(
            "{}/node_modules",
            paths::JS_PROJECTS_DOCS_AND_HOME[0]
        )])
    );
    assert_eq!(
        found("js.node_modules_home"),
        set(vec![format!(
            "{}/node_modules",
            paths::JS_PROJECTS_DOCS_AND_HOME[1]
        )])
    );
    // No rule reaches a project on the Desktop, or an editor extension in a
    // hidden folder.
    for target in [paths::JS_PROJECT_ON_DESKTOP, paths::JS_EDITOR_EXTENSION] {
        let modules = format!("{target}/node_modules");
        for r in &rules {
            assert!(
                !matches_in(r, &manifest).contains(&modules),
                "{} matches {modules}",
                r.id
            );
        }
    }
    assert_eq!(
        found("js.npx_cache"),
        set(vec![paths::NPX_CACHE.to_string()])
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
fn android_rust_ai_and_editor_rules_match_exactly_the_expected_items() {
    let (_dir, manifest) = fixture_manifest();
    let rules = repo_rules();
    let found = |id: &str| -> BTreeSet<String> {
        matches_in(rule(&rules, id), &manifest)
            .into_iter()
            .collect()
    };
    let one = |p: &str| -> BTreeSet<String> { [p.to_string()].into_iter().collect() };

    assert_eq!(found("android.gradle_caches"), one(paths::GRADLE_CACHES));
    assert_eq!(
        found("android.gradle_wrapper"),
        paths::GRADLE_VERSIONS
            .iter()
            .map(|v| format!("{}/{v}", paths::GRADLE_DISTS))
            .collect()
    );
    assert_eq!(
        found("android.system_image"),
        paths::ANDROID_SYSTEM_IMAGE_APIS
            .iter()
            .map(|api| format!(
                "{}/system-images/android-{api}/google_apis/arm64-v8a",
                paths::ANDROID_SDK
            ))
            .collect()
    );
    assert_eq!(
        found("android.ndk"),
        one(&format!(
            "{}/ndk/{}",
            paths::ANDROID_SDK,
            paths::ANDROID_NDK
        ))
    );
    // The `.avd` folders only, never the `.ini` files beside them.
    assert_eq!(
        found("android.emulator"),
        paths::ANDROID_AVD_NAMES
            .iter()
            .map(|n| format!("{}/{n}.avd", paths::ANDROID_AVD))
            .collect()
    );
    assert_eq!(
        found("android.studio_cache"),
        one(paths::ANDROID_STUDIO_CACHE)
    );

    assert_eq!(
        found("rust.cargo_registry_cache"),
        one(paths::CARGO_REGISTRY_CACHE)
    );
    assert_eq!(
        found("rust.cargo_registry_src"),
        one(paths::CARGO_REGISTRY_SRC)
    );
    assert_eq!(
        found("rust.toolchain"),
        paths::RUSTUP_TOOLCHAIN_NAMES
            .iter()
            .map(|n| format!("{}/{n}", paths::RUSTUP_TOOLCHAINS))
            .collect()
    );
    let target_rules = [
        "rust.target",
        "rust.target_developer",
        "rust.target_code",
        "rust.target_dev",
        "rust.target_src",
        "rust.target_repos",
        "rust.target_github",
        "rust.target_documents",
        "rust.target_home",
    ];
    for (id, project) in target_rules.iter().zip(paths::RUST_PROJECTS) {
        assert_eq!(found(id), one(&format!("{project}/target")), "{id}");
    }

    assert_eq!(
        found("claude_code.conversations"),
        one(paths::CLAUDE_CONVERSATIONS)
    );
    assert_eq!(found("claude_code.logs"), one(paths::CLAUDE_LOGS));

    for (prefix, app) in [
        ("cursor", paths::CURSOR_DATA),
        ("vscode", paths::VSCODE_DATA),
    ] {
        let ids = ["cache", "cached_data", "extension_downloads"];
        for (id, folder) in ids.iter().zip(paths::EDITOR_CACHE_FOLDERS) {
            let id = format!("{prefix}.{id}");
            assert_eq!(found(&id), one(&format!("{app}/{folder}")), "{id}");
        }
    }
    assert_eq!(found("jetbrains.cache"), one(paths::JETBRAINS_CACHE));
}

#[test]
fn no_rule_matches_a_folder_called_target_that_is_not_cargos() {
    let (_dir, manifest) = fixture_manifest();
    for r in repo_rules() {
        let hits = matches_in(&r, &manifest);
        for trap in [paths::TRAP_PLAIN_TARGET, paths::TRAP_HALF_TARGET] {
            assert!(!hits.iter().any(|h| h == trap), "{} matches {trap}", r.id);
        }
    }
}

#[test]
fn no_rule_matches_tool_settings_installs_or_git_checkouts() {
    let (_dir, manifest) = fixture_manifest();
    let never = [
        paths::CARGO_BIN,
        paths::CARGO_CREDENTIALS,
        paths::CARGO_GIT_CHECKOUT,
        paths::RUSTUP_SETTINGS,
        paths::CLAUDE_SETTINGS,
        paths::CURSOR_CHAT_DB,
        ".cargo",
        ".cargo/bin",
        ".cargo/git",
        ".cargo/registry",
        ".rustup",
        ".claude",
        ".claude/projects",
        ".claude/CLAUDE.md",
        ".gradle",
        ".android",
        ".android/avd",
        "Library/Android/sdk",
        "Library/Application Support/Cursor",
        "Library/Application Support/Cursor/User",
        "Library/Application Support/Code/User/settings.json",
    ];
    for r in repo_rules() {
        let hits = matches_in(&r, &manifest);
        for p in never {
            assert!(
                !hits
                    .iter()
                    .any(|h| h == p || p.starts_with(&format!("{h}/"))),
                "{} matches {p} or a folder holding it",
                r.id
            );
        }
    }
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
            assert!(
                ["logs.user", "claude_code.logs"].contains(&r.id.as_str()),
                "{} is likely_safe but not regenerable",
                r.id
            );
            assert_eq!(r.category, Category::Logs, "{}", r.id);
        }
    }
}
