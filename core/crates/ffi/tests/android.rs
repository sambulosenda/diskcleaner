//! The Android screen against a temporary home: finding SDK parts,
//! emulators and Gradle downloads, saying which ones projects use, the dry
//! run, the move to the Trash, put back and History. See the Android section
//! of `docs/command-contract.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use roomy_ffi::{
    AndroidKind, AndroidOverview, AndroidPart, AndroidUsage, CleanupCheck, CoreError, Mode,
    PlanHandle, RoomyCore,
};

struct Setup {
    _dir: tempfile::TempDir,
    home: PathBuf,
    core: Arc<RoomyCore>,
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn blob(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![3u8; bytes]).unwrap();
}

/// A home with an SDK, two emulators, Gradle, and one React Native project
/// that uses API 34, NDK 26 and Gradle 8.4.
fn setup() -> Setup {
    setup_with(true)
}

/// The same home, with or without the project.
fn setup_with(project: bool) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(dir.path()).unwrap().join("home");
    fs::create_dir_all(home.join(".Trash")).unwrap();
    // History lives in Application Support.
    fs::create_dir_all(home.join("Library/Application Support")).unwrap();
    let sdk = home.join("Library/Android/sdk");
    for (rel, bytes) in [
        (
            "system-images/android-34/google_apis_playstore/arm64-v8a",
            90_000,
        ),
        ("system-images/android-30/google_apis/x86_64", 70_000),
        ("platforms/android-34", 30_000),
        ("platforms/android-29", 20_000),
        ("build-tools/34.0.0", 10_000),
        ("build-tools/30.0.3", 9_000),
        ("ndk/26.1.10909125", 60_000),
        ("ndk/21.4.7075529", 50_000),
    ] {
        write(&sdk.join(rel).join("source.properties"), "Pkg.Revision=1\n");
        blob(&sdk.join(rel).join("data.bin"), bytes);
    }
    write(
        &home.join(".android/avd/Pixel_7.avd/config.ini"),
        "avd.ini.displayname=Pixel 7\nimage.sysdir.1=system-images/android-34/google_apis_playstore/arm64-v8a/\n",
    );
    blob(
        &home.join(".android/avd/Pixel_7.avd/userdata-qemu.img"),
        40_000,
    );
    write(&home.join(".android/avd/Pixel_7.ini"), "path=x\n");
    blob(
        &home.join(".gradle/wrapper/dists/gradle-8.4-bin/h/lib.jar"),
        8_000,
    );
    blob(
        &home.join(".gradle/wrapper/dists/gradle-7.2-all/h/lib.jar"),
        7_000,
    );
    blob(&home.join(".gradle/caches/modules-2/lib.jar"), 5_000);

    let android = home.join("Developer/Shop/android");
    if project {
        make_project(&android);
    }
    let core = RoomyCore::with_home(home.clone()).unwrap();
    core.set_recent_change_window_for_testing(std::time::Duration::ZERO);
    core.set_running_programs_for_testing(Some(Vec::new()));
    Setup {
        _dir: dir,
        home,
        core,
    }
}

/// A React Native app's `android` folder.
fn make_project(android: &Path) {
    write(&android.join("settings.gradle"), "include ':app'\n");
    write(
        &android.join("build.gradle"),
        "buildscript {\n ext {\n  compileSdkVersion = 34\n  ndkVersion = \"26.1.10909125\"\n }\n}\n",
    );
    write(
        &android.join("app/build.gradle"),
        "apply plugin: \"com.android.application\"\nandroid {\n compileSdk rootProject.ext.compileSdkVersion\n ndkVersion rootProject.ext.ndkVersion\n}\n",
    );
    write(
        &android.join("gradle/wrapper/gradle-wrapper.properties"),
        "distributionUrl=https\\://services.gradle.org/distributions/gradle-8.4-bin.zip\n",
    );
}

fn part<'a>(o: &'a AndroidOverview, title: &str) -> &'a AndroidPart {
    o.parts
        .iter()
        .find(|p| p.title == title)
        .unwrap_or_else(|| panic!("no {title} in {:?}", titles(o)))
}

fn titles(o: &AndroidOverview) -> Vec<String> {
    o.parts.iter().map(|p| p.title.clone()).collect()
}

/// Every path under `root` (Trash and History left out), with file sizes.
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
            if meta.is_dir() {
                out.push(format!("{}/", path.display()));
                stack.push(path);
            } else {
                out.push(format!("{} {}", path.display(), meta.len()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn lists_every_part_with_what_uses_it() {
    let s = setup();
    let o = s.core.android_overview(Mode::Developer);
    assert!(o.sdk_found);
    assert_eq!(o.note, None);
    assert_eq!(o.projects.len(), 1);
    assert_eq!(o.projects[0].name, "Shop");
    assert_eq!(
        o.projects[0].path.as_deref(),
        Some("~/Developer/Shop/android")
    );
    assert!(o.projects[0].last_activity_unix_seconds.is_some());
    let shop = || vec!["Shop".to_string()];

    let image = part(&o, "Android 14 (API 34) emulator image");
    assert_eq!(image.kind, AndroidKind::SystemImage);
    assert_eq!(
        image.detail.as_deref(),
        Some("Google Play, for Apple silicon Macs")
    );
    assert_eq!(
        image.usage,
        AndroidUsage::Used {
            by: vec!["Pixel 7".into()]
        }
    );
    assert_eq!(image.usage_text, "Used by Pixel 7");
    assert!(image.size_bytes >= 90_000);
    assert_eq!(
        image.path.as_deref(),
        Some("~/Library/Android/sdk/system-images/android-34/google_apis_playstore/arm64-v8a")
    );
    assert_eq!(
        part(&o, "Android 11 (API 30) emulator image").usage,
        AndroidUsage::NotUsed
    );

    assert_eq!(
        part(&o, "Android 14 (API 34) SDK platform").usage,
        AndroidUsage::Used { by: shop() }
    );
    assert_eq!(
        part(&o, "Android 10 (API 29) SDK platform").usage,
        AndroidUsage::NotUsed
    );
    // The project names no build tools, so the newest is probably its.
    assert_eq!(
        part(&o, "Build tools 34.0.0").usage,
        AndroidUsage::ProbablyUsed { by: shop() }
    );
    assert_eq!(part(&o, "Build tools 30.0.3").usage, AndroidUsage::NotUsed);
    assert_eq!(
        part(&o, "NDK 26.1.10909125").usage,
        AndroidUsage::Used { by: shop() }
    );
    assert_eq!(part(&o, "NDK 21.4.7075529").usage, AndroidUsage::NotUsed);
    assert_eq!(
        part(&o, "Gradle 8.4").usage,
        AndroidUsage::Used { by: shop() }
    );
    let old_gradle = part(&o, "Gradle 7.2");
    assert_eq!(old_gradle.usage, AndroidUsage::NotUsed);
    assert_eq!(old_gradle.usage_text, "No project Diskwell found uses this");
    assert_eq!(part(&o, "Gradle cache").usage, AndroidUsage::NotApplicable);

    let emulator = part(&o, "Pixel 7");
    assert_eq!(emulator.kind, AndroidKind::Emulator);
    assert_eq!(emulator.detail.as_deref(), Some("Runs Android 14 (API 34)"));
    assert!(emulator.last_used_unix_seconds.is_some());

    let unused: u64 = o
        .parts
        .iter()
        .filter(|p| p.usage == AndroidUsage::NotUsed)
        .map(|p| p.size_bytes)
        .sum();
    assert_eq!(o.unused_bytes, unused);
    assert!(o.total_bytes > o.unused_bytes);
}

#[test]
fn simple_mode_carries_no_path() {
    let s = setup();
    let o = s.core.android_overview(Mode::Simple);
    assert!(o.parts.iter().all(|p| p.path.is_none()));
    assert!(o.projects.iter().all(|p| p.path.is_none()));
}

#[test]
fn a_home_without_android_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(dir.path()).unwrap();
    let core = RoomyCore::with_home(home).unwrap();
    let o = core.android_overview(Mode::Developer);
    assert!(!o.sdk_found);
    assert!(o.parts.is_empty());
    assert!(o.note.unwrap().contains("didn't find the Android SDK"));
}

#[test]
fn no_projects_found_is_said_plainly() {
    let s = setup_with(false);
    let o = s.core.android_overview(Mode::Developer);
    assert!(o.projects.is_empty());
    assert!(o.note.unwrap().contains("didn't find any Android projects"));
}

#[test]
fn cleanup_moves_only_what_was_ticked_and_can_be_put_back() {
    let s = setup();
    let o = s.core.android_overview(Mode::Developer);
    let before = contents(&s.home);
    let ids: Vec<String> = [
        "Android 11 (API 30) emulator image",
        "NDK 21.4.7075529",
        "Pixel 7",
    ]
    .iter()
    .map(|t| part(&o, t).item_id.clone())
    .collect();
    let preview = s.core.preview_android_cleanup(ids).unwrap();
    assert_eq!(contents(&s.home), before, "the dry run moved something");
    assert!(
        preview
            .items
            .iter()
            .all(|i| i.check == CleanupCheck::WillMove),
        "{:?}",
        preview.items
    );
    let result = s.core.run_cleanup(preview.plan.unwrap()).unwrap();
    assert!(result.items.iter().all(|i| i.done), "{:?}", result.items);
    assert!(result.saved_to_history);
    let sdk = s.home.join("Library/Android/sdk");
    assert!(!sdk
        .join("system-images/android-30/google_apis/x86_64")
        .exists());
    assert!(!sdk.join("ndk/21.4.7075529").exists());
    assert!(!s.home.join(".android/avd/Pixel_7.avd").exists());
    // Everything else stays, including the emulator's small .ini file.
    assert!(sdk
        .join("system-images/android-34/google_apis_playstore/arm64-v8a")
        .exists());
    assert!(sdk.join("ndk/26.1.10909125").exists());
    assert!(s.home.join(".android/avd/Pixel_7.ini").exists());
    assert!(s
        .home
        .join("Developer/Shop/android/app/build.gradle")
        .exists());
    let history = s.core.cleanup_history(10);
    assert!(
        history[0]
            .items
            .iter()
            .any(|i| i.title == "Android: NDK 21.4.7075529"),
        "{:?}",
        history[0].items
    );
    let back = s.core.put_back_cleanup(result.cleanup.unwrap()).unwrap();
    assert!(back.items.iter().all(|i| i.done), "{:?}", back.items);
    assert_eq!(contents(&s.home), before);
}

#[test]
fn nothing_moves_while_the_emulator_or_android_studio_runs() {
    let s = setup();
    let o = s.core.android_overview(Mode::Developer);
    let ids = vec![
        part(&o, "Pixel 7").item_id.clone(),
        part(&o, "Build tools 30.0.3").item_id.clone(),
    ];
    let before = contents(&s.home);
    s.core.set_running_programs_for_testing(Some(vec![s
        .home
        .join("Library/Android/sdk/emulator/qemu/darwin-aarch64/qemu-system-aarch64")]));
    let preview = s.core.preview_android_cleanup(ids.clone()).unwrap();
    let emulator = preview.items.iter().find(|i| i.title == "Pixel 7").unwrap();
    assert!(
        matches!(&emulator.check, CleanupCheck::WontMove { reason } if reason.contains("emulator")),
        "{emulator:?}"
    );
    s.core
        .set_running_programs_for_testing(Some(vec![PathBuf::from(
            "/Applications/Android Studio.app/Contents/MacOS/studio",
        )]));
    let preview = s.core.preview_android_cleanup(ids.clone()).unwrap();
    assert!(preview.plan.is_none(), "{:?}", preview.items);
    // If the running programs can't be listed, nothing moves either.
    s.core.set_running_programs_for_testing(None);
    let preview = s.core.preview_android_cleanup(ids).unwrap();
    assert!(preview.plan.is_none(), "{:?}", preview.items);
    assert_eq!(contents(&s.home), before);
}

#[test]
fn a_part_changed_since_the_overview_stays() {
    let s = setup();
    let o = s.core.android_overview(Mode::Developer);
    let id = part(&o, "NDK 21.4.7075529").item_id.clone();
    blob(
        &s.home.join("Library/Android/sdk/ndk/21.4.7075529/more.bin"),
        200_000,
    );
    let preview = s.core.preview_android_cleanup(vec![id]).unwrap();
    assert!(preview.plan.is_none(), "{:?}", preview.items);
    assert!(s.home.join("Library/Android/sdk/ndk/21.4.7075529").exists());
}

#[test]
fn symlinked_parts_are_never_offered() {
    let s = setup();
    let outside = s.home.parent().unwrap().join("outside");
    write(&outside.join("source.properties"), "Pkg.Revision=1\n");
    std::os::unix::fs::symlink(&outside, s.home.join("Library/Android/sdk/ndk/99.0.0")).unwrap();
    let o = s.core.android_overview(Mode::Developer);
    assert!(
        !titles(&o).iter().any(|t| t.contains("99.0.0")),
        "{:?}",
        titles(&o)
    );
}

#[test]
fn stale_or_unknown_ids_are_refused() {
    let s = setup();
    let old = s.core.android_overview(Mode::Developer);
    let old_id = part(&old, "NDK 21.4.7075529").item_id.clone();
    s.core.android_overview(Mode::Developer);
    assert!(matches!(
        s.core.preview_android_cleanup(vec![old_id]),
        Err(CoreError::UnknownItem)
    ));
    assert!(matches!(
        s.core.preview_android_cleanup(vec![]),
        Err(CoreError::NothingSelected)
    ));
    assert!(s.core.run_cleanup(PlanHandle { id: 1 }).is_err());
}

#[test]
fn recent_changes_still_hold_items_back() {
    let s = setup();
    s.core
        .set_recent_change_window_for_testing(std::time::Duration::from_secs(600));
    let o = s.core.android_overview(Mode::Developer);
    let ids: Vec<String> = o.parts.iter().map(|p| p.item_id.clone()).collect();
    let preview = s.core.preview_android_cleanup(ids).unwrap();
    assert!(preview.plan.is_none());
}
