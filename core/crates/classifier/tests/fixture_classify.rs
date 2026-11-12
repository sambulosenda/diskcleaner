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
    want.insert(paths::REACT_NATIVE_CACHE.to_string(), "react_native.cache");
    want.insert(paths::EAS_CLI_CACHE.to_string(), "eas_cli.cache");
    for p in paths::JS_PROJECTS {
        want.insert(join(p, "node_modules"), "js.node_modules");
    }
    let elsewhere = [
        "js.node_modules_developer",
        "js.node_modules_code",
        "js.node_modules_dev",
        "js.node_modules_src",
        "js.node_modules_repos",
        "js.node_modules_github",
    ];
    for (p, id) in paths::JS_PROJECTS_ELSEWHERE.iter().zip(elsewhere) {
        want.insert(join(p, "node_modules"), id);
    }
    let docs_and_home = ["js.node_modules_documents", "js.node_modules_home"];
    for (p, id) in paths::JS_PROJECTS_DOCS_AND_HOME.iter().zip(docs_and_home) {
        want.insert(join(p, "node_modules"), id);
    }
    want.insert(join(paths::NPM_CACHE, "_cacache"), "js.npm_cache");
    want.insert(paths::NPX_CACHE.to_string(), "js.npx_cache");
    want.insert(join(paths::PNPM_STORE, "store"), "js.pnpm_store");
    want.insert(paths::YARN_CACHE.to_string(), "js.yarn_cache");
    want.insert(paths::BUN_CACHE.to_string(), "js.bun_cache");
    for c in paths::EXPO_CACHES {
        want.insert(join(paths::EXPO_HOME, c), "js.expo_cache");
    }
    for id in paths::SIM_DEVICE_IDS {
        want.insert(join(paths::SIM_DEVICES, id), "xcode.simulator_device");
    }
    expected_android_rust_ai_and_editors(&mut want);
    want
}

/// The developer-mode items from the Android, Rust, AI tool and editor rules.
fn expected_android_rust_ai_and_editors(want: &mut BTreeMap<String, &'static str>) {
    want.insert(paths::GRADLE_CACHES.to_string(), "android.gradle_caches");
    for v in paths::GRADLE_VERSIONS {
        want.insert(join(paths::GRADLE_DISTS, v), "android.gradle_wrapper");
    }
    for api in paths::ANDROID_SYSTEM_IMAGE_APIS {
        want.insert(
            format!(
                "{}/system-images/android-{api}/google_apis/arm64-v8a",
                paths::ANDROID_SDK
            ),
            "android.system_image",
        );
    }
    want.insert(
        format!("{}/ndk/{}", paths::ANDROID_SDK, paths::ANDROID_NDK),
        "android.ndk",
    );
    for n in paths::ANDROID_AVD_NAMES {
        want.insert(
            format!("{}/{n}.avd", paths::ANDROID_AVD),
            "android.emulator",
        );
    }
    want.insert(
        paths::ANDROID_STUDIO_CACHE.to_string(),
        "android.studio_cache",
    );
    want.insert(
        paths::CARGO_REGISTRY_CACHE.to_string(),
        "rust.cargo_registry_cache",
    );
    want.insert(
        paths::CARGO_REGISTRY_SRC.to_string(),
        "rust.cargo_registry_src",
    );
    for n in paths::RUSTUP_TOOLCHAIN_NAMES {
        want.insert(join(paths::RUSTUP_TOOLCHAINS, n), "rust.toolchain");
    }
    let targets = [
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
    for (p, id) in paths::RUST_PROJECTS.iter().zip(targets) {
        want.insert(join(p, "target"), id);
    }
    want.insert(
        paths::CLAUDE_CONVERSATIONS.to_string(),
        "claude_code.conversations",
    );
    want.insert(paths::CLAUDE_LOGS.to_string(), "claude_code.logs");
    let editor_ids = [
        (
            "cursor.cache",
            "cursor.cached_data",
            "cursor.extension_downloads",
        ),
        (
            "vscode.cache",
            "vscode.cached_data",
            "vscode.extension_downloads",
        ),
    ];
    for (app, ids) in [paths::CURSOR_DATA, paths::VSCODE_DATA]
        .into_iter()
        .zip(editor_ids)
    {
        let [a, b, c] = paths::EDITOR_CACHE_FOLDERS;
        want.insert(join(app, a), ids.0);
        want.insert(join(app, b), ids.1);
        want.insert(join(app, c), ids.2);
    }
    want.insert(paths::JETBRAINS_CACHE.to_string(), "jetbrains.cache");
    more_developer_items(want);
}

/// Items for the toolchain rules: Python, Go, Homebrew, web tools, Maven,
/// Dart, Xcode, SwiftPM and Carthage.
fn more_developer_items(want: &mut BTreeMap<String, &'static str>) {
    let one = [
        (paths::PIP_CACHE, "python.pip_cache"),
        (paths::UV_CACHE, "python.uv_cache"),
        (paths::GO_BUILD_CACHE, "go.build_cache"),
        (paths::GO_MODULE_CACHE, "go.module_cache"),
        (paths::HOMEBREW_CACHE, "homebrew.cache"),
        (paths::MAVEN_REPOSITORY, "maven.repository"),
        (paths::PUB_HOSTED, "dart.pub_cache"),
        (paths::XCODE_CACHE, "xcode.cache"),
        (paths::XCODE_PREVIEWS, "xcode.previews"),
        (paths::SIMULATOR_CACHES, "xcode.simulator_caches"),
        (paths::SWIFTPM_CACHE, "swiftpm.cache"),
        (paths::CARTHAGE_CACHE, "carthage.cache"),
    ];
    for (p, id) in one {
        want.insert(p.to_string(), id);
    }
    want.insert(join(paths::POETRY_HOME, "cache"), "python.poetry_cache");
    want.insert(
        join(paths::POETRY_HOME, "artifacts"),
        "python.poetry_artifacts",
    );
    let venvs = join(paths::POETRY_HOME, "virtualenvs");
    for v in paths::POETRY_VIRTUALENVS {
        want.insert(join(&venvs, v), "python.poetry_virtualenv");
    }
    let web_ids = [
        "deno.cache",
        "pnpm.metadata_cache",
        "node_gyp.headers",
        "typescript.types_cache",
        "electron.cache",
        "electron_builder.cache",
    ];
    for (p, id) in paths::WEB_TOOL_CACHES.iter().zip(web_ids) {
        want.insert(p.to_string(), id);
    }
    for b in paths::PLAYWRIGHT_BROWSERS {
        want.insert(join(paths::PLAYWRIGHT_HOME, b), "playwright.browser");
    }
    for b in paths::PUPPETEER_BROWSERS {
        want.insert(join(paths::PUPPETEER_HOME, b), "puppeteer.browser");
    }
    ai_model_items(want);
}

/// Items for the local AI model rules: Ollama, LM Studio and Hugging Face.
fn ai_model_items(want: &mut BTreeMap<String, &'static str>) {
    want.insert(paths::OLLAMA_MODELS.to_string(), "ollama.models");
    for m in paths::LM_STUDIO_MODEL_NAMES {
        want.insert(join(paths::LM_STUDIO_MODELS, m), "lm_studio.model");
    }
    want.insert(
        join(
            paths::LM_STUDIO_LEGACY_MODELS,
            paths::LM_STUDIO_LEGACY_MODEL,
        ),
        "lm_studio.model_legacy",
    );
    for m in paths::HUGGINGFACE_MODELS {
        want.insert(join(paths::HUGGINGFACE_HUB, m), "huggingface.model");
    }
    want.insert(
        join(paths::HUGGINGFACE_HUB, paths::HUGGINGFACE_DATASET),
        "huggingface.dataset",
    );
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
    want.insert(
        paths::DOWNLOAD_PKG.to_string(),
        "downloads.installer_package",
    );
    want.insert(
        paths::DOWNLOAD_ZIP_INSTALLER.to_string(),
        "downloads.zip_installer",
    );
    for (cache, id) in [
        (paths::EDGE_CACHE, "browser.edge_cache"),
        (paths::BRAVE_CACHE, "browser.brave_cache"),
        (paths::ARC_CACHE, "browser.arc_cache"),
        (paths::ZOOM_CACHE, "zoom.cache"),
        (paths::DISCORD_CACHE, "discord.cache"),
        (paths::TEAMS_CLASSIC_CACHE, "teams.classic_cache"),
        (paths::TEAMS_CACHE, "teams.cache"),
        (paths::WHATSAPP_CACHE, "whatsapp.cache"),
    ] {
        want.insert(cache.to_string(), id);
    }
    for f in paths::USER_LOG_FOLDERS {
        want.insert(join(paths::USER_LOGS, f), "logs.user");
    }
    want.insert(
        paths::IPHONE_SOFTWARE_UPDATE.to_string(),
        "ios.software_update",
    );
    for event in paths::FCP_EVENTS {
        let e = join(paths::FCP_LIBRARY, event);
        want.insert(join(&e, "Render Files"), "final_cut.render_files");
        want.insert(
            join(&e, "Transcoded Media/Proxy Media"),
            "final_cut.proxy_media",
        );
    }
    want.insert(
        paths::ADOBE_MEDIA_CACHE_FILES.to_string(),
        "adobe.media_cache_files",
    );
    want.insert(
        paths::ADOBE_MEDIA_CACHE_DB.to_string(),
        "adobe.media_cache_database",
    );
    for v in paths::AFTER_EFFECTS_VERSIONS {
        want.insert(
            join(paths::AFTER_EFFECTS_CACHE, v),
            "adobe.after_effects_disk_cache",
        );
    }
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
    // The photos zip and the document stay unknown.
    for p in [paths::DOWNLOAD_ZIP, paths::DOWNLOAD_NORMAL] {
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
    paths::ZOOM_DATA,
    paths::DISCORD_LOCAL_STORAGE,
    paths::TEAMS_APP_DATA,
    paths::WHATSAPP_MESSAGES,
    paths::DOWNLOAD_ZIP,
    paths::DOWNLOAD_PKG,
    paths::DOWNLOAD_ZIP_INSTALLER,
    paths::EXPO_STATE,
    paths::DOCUMENTS,
    paths::DOWNLOADS,
    paths::DOWNLOAD_DMG,
    paths::DOWNLOAD_NORMAL,
    paths::MOBILESYNC_BACKUP,
    paths::XCODE_ARCHIVE,
    paths::ANDROID_AVD,
    paths::TRAP_OLLAMA_KEY,
    paths::TRAP_LM_STUDIO_CONVERSATIONS,
    paths::TRAP_HUGGINGFACE_TOKEN,
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
    // The project on the Desktop is not matched: its node_modules is part of

    let on_desktop = root.join(paths::JS_PROJECT_ON_DESKTOP).join("node_modules");
    assert!(!items.iter().any(|i| i.path == on_desktop));
    // Nor is an editor extension's node_modules in a hidden folder.
    let extension = root.join(paths::JS_EDITOR_EXTENSION).join("node_modules");
    assert!(!items.iter().any(|i| i.path == extension));
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
fn no_rules_tie() {
    assert!(check_ties(&repo_rules(), Mode::Developer).is_empty());
    assert!(check_ties(&repo_rules(), Mode::Simple).is_empty());
}

#[test]
fn simulator_devices_are_one_review_first_item_each() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let items = run(&root, &rules, Mode::Developer);
    for id in paths::SIM_DEVICE_IDS {
        let p = root.join(paths::SIM_DEVICES).join(id);
        let item = items.iter().find(|i| i.path == p).unwrap();
        assert_eq!(item.rule_id, "xcode.simulator_device");
        assert_eq!(item.tier, Tier::ReviewFirst);
        assert!(!item.regenerable);
    }
    // The device set file next to the devices is not a device.
    let set = root.join(paths::SIM_DEVICES).join("device_set.plist");
    assert!(!items.iter().any(|i| i.path == set));
}

#[test]
fn node_modules_are_found_in_code_folders_at_any_depth() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let items = run(&root, &rules, Mode::Developer);
    for project in paths::JS_PROJECTS
        .iter()
        .chain(paths::JS_PROJECTS_ELSEWHERE.iter())
        .chain(paths::JS_PROJECTS_DOCS_AND_HOME.iter())
    {
        let p = root.join(project).join("node_modules");
        let item = items
            .iter()
            .find(|i| i.path == p)
            .unwrap_or_else(|| panic!("no item for {}", p.display()));
        assert!(item.rule_id.starts_with("js.node_modules"));
        // Never likely_safe: the user picks each project.
        assert_ne!(item.tier, Tier::LikelySafe);
    }
    // A node_modules nested inside another is part of the outer item, not
    // an item of its own.
    let nested = root.join(paths::JS_NESTED_MODULES);
    assert!(!items.iter().any(|i| i.path == nested));
    let outer = root
        .join(paths::JS_PROJECTS_ELSEWHERE[0])
        .join("node_modules");
    let outer = items.iter().find(|i| i.path == outer).unwrap();
    assert_eq!(outer.tier, Tier::ReviewFirst);
    assert!(outer.lowered.is_empty(), "{:?}", outer.lowered);
}

#[test]
fn npx_tools_are_one_cache_item_beside_the_npm_cache() {
    let rules = repo_rules();
    let (_dir, root) = home();
    let items = run(&root, &rules, Mode::Developer);
    let npx = items
        .iter()
        .find(|i| i.path == root.join(paths::NPX_CACHE))
        .unwrap();
    assert_eq!(npx.rule_id, "js.npx_cache");
    assert_eq!(npx.tier, Tier::LikelySafe);
    // The node_modules inside an npx tool is part of the npx item.
    let inside = root.join(paths::NPX_TOOL).join("node_modules");
    assert!(!items.iter().any(|i| i.path == inside));
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
