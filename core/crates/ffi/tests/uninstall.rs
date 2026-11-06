//! The app uninstaller against a temporary home: listing apps, finding
//! their leftovers, the dry run, the move to the Trash, put back and
//! History. See the uninstaller section of `docs/command-contract.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use roomy_ffi::{
    AppLocation, AppReview, AppSummary, CleanupCheck, CoreError, Mode, PlanHandle, RoomyCore,
};

const SLACK_ID: &str = "com.tinyspeck.slackmacgap";

/// A temporary home with a Trash, a stand-in for `/Applications` outside
/// it, and a core looking at both.
struct Setup {
    _dir: tempfile::TempDir,
    home: PathBuf,
    /// Stands in for `/Applications`: outside the home folder.
    main_apps: PathBuf,
    core: Arc<RoomyCore>,
}

fn info_plist(id: &str, name: &str) -> Vec<u8> {
    let mut dict = plist::Dictionary::new();
    dict.insert("CFBundleIdentifier".into(), plist::Value::String(id.into()));
    dict.insert("CFBundleName".into(), plist::Value::String(name.into()));
    let mut out = Vec::new();
    plist::Value::Dictionary(dict)
        .to_writer_xml(&mut out)
        .unwrap();
    out
}

fn make_app(folder: &Path, name: &str, id: &str) -> PathBuf {
    let bundle = folder.join(format!("{name}.app"));
    fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    fs::write(bundle.join("Contents/Info.plist"), info_plist(id, name)).unwrap();
    fs::write(bundle.join("Contents/MacOS").join(name), vec![7u8; 20_000]).unwrap();
    bundle
}

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![1u8; bytes]).unwrap();
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    let home = base.join("home");
    let main_apps = base.join("Applications");
    fs::create_dir_all(home.join(".Trash")).unwrap();
    fs::create_dir_all(home.join("Applications")).unwrap();
    fs::create_dir_all(&main_apps).unwrap();
    make_app(&main_apps, "Slack", SLACK_ID);
    make_app(&main_apps, "Safari", "com.apple.Safari");
    make_app(&main_apps, "Roomy", "com.sambulosenda.roomy");
    make_app(&home.join("Applications"), "Tiny", "com.example.tiny");
    let lib = home.join("Library");
    write(
        &lib.join("Caches/com.tinyspeck.slackmacgap/Cache.db"),
        50_000,
    );
    write(
        &lib.join("Caches/com.tinyspeck.slackmacgap.ShipIt/update.zip"),
        9_000,
    );
    write(
        &lib.join("Application Support/Slack/IndexedDB/data"),
        80_000,
    );
    write(
        &lib.join("Application Support/com.tinyspeck.slackmacgap/x"),
        3_000,
    );
    write(
        &lib.join("Containers/com.tinyspeck.slackmacgap/Data/notes.txt"),
        2_000,
    );
    write(
        &lib.join("Preferences/com.tinyspeck.slackmacgap.plist"),
        500,
    );
    write(
        &lib.join("Saved Application State/com.tinyspeck.slackmacgap.savedState/w"),
        700,
    );
    write(&lib.join("Logs/Slack/main.log"), 4_000);
    write(&lib.join("Caches/com.other.app/keep"), 4_000);
    write(&lib.join("Preferences/com.other.app.plist"), 400);
    write(&lib.join("Caches/com.example.tiny/c"), 1_000);
    let core = RoomyCore::with_home(home.clone()).unwrap();
    core.set_app_folders_for_testing(vec![main_apps.clone(), home.join("Applications")]);
    // The files were made just now, and the test machine runs its own
    // programs: switch off those two extra checks unless a test needs them.
    core.set_recent_change_window_for_testing(std::time::Duration::ZERO);
    core.set_running_programs_for_testing(Some(Vec::new()));
    Setup {
        _dir: dir,
        home,
        main_apps,
        core,
    }
}

fn app<'a>(apps: &'a [AppSummary], name: &str) -> &'a AppSummary {
    apps.iter().find(|a| a.name == name).unwrap()
}

fn slack_review(s: &Setup, mode: Mode) -> AppReview {
    let apps = s.core.list_apps();
    s.core
        .review_app(app(&apps, "Slack").app_id.clone(), mode)
        .unwrap()
}

/// The display path of every part.
fn paths(review: &AppReview) -> Vec<String> {
    review
        .parts
        .iter()
        .map(|p| p.path.clone().unwrap())
        .collect()
}

fn preselected(review: &AppReview) -> Vec<String> {
    review
        .parts
        .iter()
        .filter(|p| p.preselected)
        .map(|p| p.item_id.clone())
        .collect()
}

fn all_ids(review: &AppReview) -> Vec<String> {
    review.parts.iter().map(|p| p.item_id.clone()).collect()
}

/// Every path under `root` (Trash and History left out), with sizes.
fn contents(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.starts_with(root.join(".Trash"))
                || path.starts_with(root.join("Library/Application Support/Roomy"))
            {
                continue;
            }
            let meta = entry.metadata().unwrap();
            out.push(format!("{} {}", path.display(), meta.len()));
            if meta.is_dir() {
                stack.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn lists_apps_but_never_apple_apps_or_roomy() {
    let s = setup();
    let apps = s.core.list_apps();
    let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Slack", "Tiny"]);
    let slack = app(&apps, "Slack");
    assert_eq!(slack.location, AppLocation::Applications);
    assert_eq!(slack.bundle_id, SLACK_ID);
    assert!(slack.size_bytes >= 20_000);
    assert!(!slack.is_open);
    assert_eq!(
        slack.bundle_path,
        s.main_apps.join("Slack.app").to_string_lossy()
    );
    assert_eq!(app(&apps, "Tiny").location, AppLocation::HomeApplications);
}

#[test]
fn review_finds_leftovers_and_ticks_only_the_safe_sure_ones() {
    let s = setup();
    let review = slack_review(&s, Mode::Developer);
    let mut got = paths(&review);
    got.sort();
    assert_eq!(
        got,
        [
            "~/Library/Application Support/Slack",
            "~/Library/Application Support/com.tinyspeck.slackmacgap",
            "~/Library/Caches/com.tinyspeck.slackmacgap",
            "~/Library/Caches/com.tinyspeck.slackmacgap.ShipIt",
            "~/Library/Containers/com.tinyspeck.slackmacgap",
            "~/Library/Logs/Slack",
            "~/Library/Preferences/com.tinyspeck.slackmacgap.plist",
            "~/Library/Saved Application State/com.tinyspeck.slackmacgap.savedState",
        ]
    );
    // The app is in /Applications, which is outside home: not offered.
    assert!(review.parts.iter().all(|p| p.item_id != "app"));
    // Largest first.
    let sizes: Vec<u64> = review.parts.iter().map(|p| p.size_bytes).collect();
    assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
    for part in &review.parts {
        let path = part.path.as_deref().unwrap();
        let review_only = path.contains("Application Support") || path.contains("Containers");
        if review_only || !part.sure {
            assert!(!part.preselected, "{path} must not be ticked for the user");
        }
        if path.ends_with("Application Support/Slack") || path.ends_with("Logs/Slack") {
            assert!(!part.sure, "{path} was matched by name");
            assert!(part.explain.contains("similar name"));
        }
    }
    let ticked: Vec<&str> = review
        .parts
        .iter()
        .filter(|p| p.preselected)
        .map(|p| p.path.as_deref().unwrap())
        .collect();
    let mut ticked = ticked;
    ticked.sort();
    assert_eq!(
        ticked,
        [
            "~/Library/Caches/com.tinyspeck.slackmacgap",
            "~/Library/Preferences/com.tinyspeck.slackmacgap.plist",
            "~/Library/Saved Application State/com.tinyspeck.slackmacgap.savedState",
        ]
    );
    assert_eq!(review.note, None);
}

#[test]
fn simple_mode_reviews_carry_no_path() {
    let s = setup();
    let review = slack_review(&s, Mode::Simple);
    assert!(!review.parts.is_empty());
    assert!(review.parts.iter().all(|p| p.path.is_none()));
}

#[test]
fn uninstall_moves_only_what_was_ticked_and_can_be_put_back() {
    let s = setup();
    let review = slack_review(&s, Mode::Developer);
    let before = contents(&s.home);
    let ids = all_ids(&review);
    let preview = s
        .core
        .preview_uninstall(review.app.app_id.clone(), ids.clone())
        .unwrap();
    assert_eq!(contents(&s.home), before, "the dry run moved something");
    assert!(
        preview
            .items
            .iter()
            .all(|i| i.check == CleanupCheck::WillMove),
        "{:?}",
        preview.items
    );
    let plan = preview.plan.unwrap();
    let result = s.core.run_cleanup(plan).unwrap();
    assert!(result.items.iter().all(|i| i.done), "{:?}", result.items);
    assert!(result.saved_to_history);
    let lib = s.home.join("Library");
    for gone in [
        "Caches/com.tinyspeck.slackmacgap",
        "Application Support/Slack",
        "Containers/com.tinyspeck.slackmacgap",
        "Preferences/com.tinyspeck.slackmacgap.plist",
    ] {
        assert!(!lib.join(gone).exists(), "{gone} still there");
    }
    // Other apps' files and the app itself are untouched.
    assert!(lib.join("Caches/com.other.app/keep").exists());
    assert!(lib.join("Preferences/com.other.app.plist").exists());
    assert!(lib.join("Caches/com.example.tiny/c").exists());
    assert!(s.main_apps.join("Slack.app/Contents/Info.plist").exists());
    // History names the app.
    let history = s.core.cleanup_history(10);
    assert!(history[0].items.iter().any(|i| i.title == "Slack: Caches"));
    // Put back restores every file exactly.
    let back = s.core.put_back_cleanup(result.cleanup.unwrap()).unwrap();
    assert!(back.items.iter().all(|i| i.done), "{:?}", back.items);
    assert_eq!(contents(&s.home), before);
}

#[test]
fn an_app_in_home_applications_can_go_too() {
    let s = setup();
    let apps = s.core.list_apps();
    let tiny = app(&apps, "Tiny");
    let review = s
        .core
        .review_app(tiny.app_id.clone(), Mode::Developer)
        .unwrap();
    assert_eq!(review.parts[0].item_id, "app");
    assert!(review.parts[0].preselected);
    let preview = s
        .core
        .preview_uninstall(tiny.app_id.clone(), preselected(&review))
        .unwrap();
    let result = s.core.run_cleanup(preview.plan.unwrap()).unwrap();
    assert!(result.items.iter().all(|i| i.done), "{:?}", result.items);
    assert!(!s.home.join("Applications/Tiny.app").exists());
    assert!(!s.home.join("Library/Caches/com.example.tiny").exists());
}

#[test]
fn nothing_moves_while_the_app_is_open() {
    let s = setup();
    let review = slack_review(&s, Mode::Developer);
    let before = contents(&s.home);
    s.core.set_running_programs_for_testing(Some(vec![s
        .main_apps
        .join("Slack.app/Contents/MacOS/Slack")]));
    let err = s
        .core
        .preview_uninstall(review.app.app_id.clone(), all_ids(&review))
        .unwrap_err();
    match err {
        CoreError::CleanupUnavailable { reason } => {
            assert_eq!(reason, "Slack is open. Quit Slack, then try again.")
        }
        other => panic!("expected cleanup_unavailable, got {other:?}"),
    }
    assert_eq!(contents(&s.home), before);
    // If the running programs can't be listed, the app counts as open.
    s.core.set_running_programs_for_testing(None);
    assert!(matches!(
        s.core
            .preview_uninstall(review.app.app_id.clone(), all_ids(&review)),
        Err(CoreError::CleanupUnavailable { .. })
    ));
}

#[test]
fn an_item_with_a_running_helper_inside_stays() {
    let s = setup();
    let review = slack_review(&s, Mode::Developer);
    let helper = s
        .home
        .join("Library/Application Support/Slack/IndexedDB/data");
    s.core.set_running_programs_for_testing(Some(vec![helper]));
    let preview = s
        .core
        .preview_uninstall(review.app.app_id.clone(), all_ids(&review))
        .unwrap();
    for item in &preview.items {
        let path = item.path.as_deref().unwrap();
        if path == "~/Library/Application Support/Slack" {
            assert!(
                matches!(&item.check, CleanupCheck::WontMove { reason } if reason.contains("Slack is open")),
                "{item:?}"
            );
        } else {
            assert_eq!(item.check, CleanupCheck::WillMove, "{item:?}");
        }
    }
}

#[test]
fn a_part_changed_since_the_review_stays() {
    let s = setup();
    let review = slack_review(&s, Mode::Developer);
    write(
        &s.home
            .join("Library/Caches/com.tinyspeck.slackmacgap/new.db"),
        500_000,
    );
    let preview = s
        .core
        .preview_uninstall(review.app.app_id.clone(), all_ids(&review))
        .unwrap();
    let cache = preview
        .items
        .iter()
        .find(|i| i.path.as_deref() == Some("~/Library/Caches/com.tinyspeck.slackmacgap"))
        .unwrap();
    assert!(
        matches!(&cache.check, CleanupCheck::WontMove { reason } if reason.contains("changed since")),
        "{cache:?}"
    );
}

#[test]
fn a_leftover_holding_a_git_project_is_refused() {
    let s = setup();
    write(
        &s.home
            .join("Library/Caches/com.tinyspeck.slackmacgap/repo/.git/HEAD"),
        30,
    );
    let review = slack_review(&s, Mode::Developer);
    let preview = s
        .core
        .preview_uninstall(review.app.app_id.clone(), all_ids(&review))
        .unwrap();
    let cache = preview
        .items
        .iter()
        .find(|i| i.path.as_deref() == Some("~/Library/Caches/com.tinyspeck.slackmacgap"))
        .unwrap();
    assert!(
        matches!(cache.check, CleanupCheck::WontMove { .. }),
        "{cache:?}"
    );
    let result = s.core.run_cleanup(preview.plan.unwrap()).unwrap();
    assert!(result.items.iter().all(|i| i.item_id != cache.item_id));
    assert!(s
        .home
        .join("Library/Caches/com.tinyspeck.slackmacgap/repo/.git/HEAD")
        .exists());
}

#[test]
fn symlinks_are_never_offered() {
    let s = setup();
    fs::create_dir_all(s.home.join("Documents/precious")).unwrap();
    std::os::unix::fs::symlink(
        s.home.join("Documents/precious"),
        s.home.join("Library/Caches/com.tinyspeck.slackmacgap.link"),
    )
    .unwrap();
    let review = slack_review(&s, Mode::Developer);
    assert!(paths(&review).iter().all(|p| !p.ends_with(".link")));
}

#[test]
fn another_copy_of_the_app_keeps_its_shared_data() {
    let s = setup();
    make_app(&s.home.join("Applications"), "Slack Copy", SLACK_ID);
    let review = slack_review(&s, Mode::Developer);
    assert!(review.parts.is_empty());
    assert!(review.note.unwrap().contains("Another copy"));
}

#[test]
fn stale_or_unknown_ids_are_refused() {
    let s = setup();
    let first = s.core.list_apps();
    let old_id = app(&first, "Slack").app_id.clone();
    let apps = s.core.list_apps();
    assert!(matches!(
        s.core.review_app(old_id.clone(), Mode::Simple),
        Err(CoreError::UnknownApp)
    ));
    assert!(matches!(
        s.core.review_app("app-x".into(), Mode::Simple),
        Err(CoreError::UnknownApp)
    ));
    let slack = app(&apps, "Slack").app_id.clone();
    let tiny = app(&apps, "Tiny").app_id.clone();
    s.core.review_app(slack.clone(), Mode::Simple).unwrap();
    // Only the app last reviewed can be previewed.
    assert!(matches!(
        s.core.preview_uninstall(tiny, vec!["part-0".into()]),
        Err(CoreError::UnknownApp)
    ));
    assert!(matches!(
        s.core
            .preview_uninstall(slack.clone(), vec!["part-999".into()]),
        Err(CoreError::UnknownItem)
    ));
    assert!(matches!(
        s.core.preview_uninstall(slack, vec![]),
        Err(CoreError::NothingSelected)
    ));
    // A failed preview leaves no plan behind.
    assert!(s.core.run_cleanup(PlanHandle { id: 1 }).is_err());
}

#[test]
fn recent_changes_still_hold_items_back() {
    let s = setup();
    s.core
        .set_recent_change_window_for_testing(roomy_ffi_window());
    let review = slack_review(&s, Mode::Developer);
    let preview = s
        .core
        .preview_uninstall(review.app.app_id.clone(), all_ids(&review))
        .unwrap();
    assert!(preview.plan.is_none());
    assert!(preview
        .items
        .iter()
        .all(|i| matches!(&i.check, CleanupCheck::WontMove { reason } if reason.contains("last few minutes"))));
}

/// The standard recent-change window.
fn roomy_ffi_window() -> std::time::Duration {
    std::time::Duration::from_secs(10 * 60)
}
