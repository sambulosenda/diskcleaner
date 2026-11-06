//! The T09 command contract against a fixture home (T04) and the built-in
//! rules (T07). See `docs/command-contract.md`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use roomy_ffi::{
    Category, CoreError, FullDiskAccess, ItemDetail, ItemSummary, Mode, PrivateArea, RoomyCore,
    ScanHandle, ScanPhase, Sort, Tier,
};

/// A fixture home and a core that scans it.
struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    core: Arc<RoomyCore>,
}

fn setup() -> Setup {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    let core = RoomyCore::with_home(root.clone()).unwrap();
    // Fixture homes are made just now, and the test machine runs its own
    // programs: switch off those two extra checks (tested in extra_checks.rs).
    core.set_recent_change_window_for_testing(std::time::Duration::ZERO);
    core.set_running_programs_for_testing(Some(Vec::new()));
    Setup {
        _dir: dir,
        root,
        core,
    }
}

/// Polls until the scan leaves `scanning` and `classifying`.
fn wait(core: &RoomyCore, handle: ScanHandle) -> ScanPhase {
    let start = Instant::now();
    loop {
        let phase = core.scan_progress(handle).unwrap().phase;
        if !matches!(phase, ScanPhase::Scanning | ScanPhase::Classifying) {
            return phase;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "scan never ended"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Starts a scan and waits for it to finish.
fn finished_scan(core: &RoomyCore, mode: Mode) -> ScanHandle {
    let handle = core.start_scan(mode);
    assert_eq!(wait(core, handle), ScanPhase::Finished);
    handle
}

fn all_items(core: &RoomyCore, handle: ScanHandle) -> Vec<ItemSummary> {
    core.list_items(handle, None, None, Sort::LargestFirst)
        .unwrap()
}

fn details(core: &RoomyCore, handle: ScanHandle) -> Vec<ItemDetail> {
    all_items(core, handle)
        .iter()
        .map(|i| core.item_detail(handle, i.id.clone()).unwrap())
        .collect()
}

#[test]
fn headline_is_the_exact_sum_of_likely_safe_items() {
    let s = setup();
    for mode in [Mode::Simple, Mode::Developer] {
        let h = finished_scan(&s.core, mode);
        let overview = s.core.overview(h).unwrap();
        let likely: u64 = s
            .core
            .list_items(h, None, Some(Tier::LikelySafe), Sort::LargestFirst)
            .unwrap()
            .iter()
            .map(|i| i.size_bytes)
            .sum();
        let by_hand: u64 = all_items(&s.core, h)
            .iter()
            .filter(|i| i.tier == Tier::LikelySafe)
            .map(|i| i.size_bytes)
            .sum();
        assert!(likely > 0, "the fixture has likely safe items in {mode:?}");
        assert_eq!(overview.likely_safe_bytes, likely);
        assert_eq!(overview.likely_safe_bytes, by_hand);
        let from_categories: u64 = overview
            .categories
            .iter()
            .map(|c| c.likely_safe_bytes)
            .sum();
        assert_eq!(overview.likely_safe_bytes, from_categories);
    }
}

#[test]
fn overview_totals_add_up() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let o = s.core.overview(h).unwrap();
    let items = all_items(&s.core, h);
    let item_bytes: u64 = items.iter().map(|i| i.size_bytes).sum();
    let category_bytes: u64 = o.categories.iter().map(|c| c.bytes).sum();
    let category_count: u64 = o.categories.iter().map(|c| c.item_count).sum();
    assert_eq!(category_bytes, item_bytes);
    assert_eq!(category_count, items.len() as u64);
    assert!(item_bytes <= o.scanned_bytes);
    assert!(o.disk_total > 0);
    assert!(o.disk_free <= o.disk_total);
    assert!(o.categories.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    assert!(o.categories.iter().all(|c| c.item_count > 0));
    let progress = s.core.scan_progress(h).unwrap();
    assert_eq!(progress.bytes_seen, o.scanned_bytes);
    assert!(progress.items_seen > 0);
}

#[test]
fn unreadable_areas_add_up_to_the_unreadable_count() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Simple);
    let o = s.core.overview(h).unwrap();
    let sum: u64 = o.unreadable_areas.iter().map(|a| a.folder_count).sum();
    assert_eq!(sum, o.unreadable_count);
    assert!(o
        .unreadable_areas
        .windows(2)
        .all(|w| w[0].folder_count >= w[1].folder_count));
    assert!(o.unreadable_areas.iter().all(|a| a.folder_count > 0));
}

#[test]
fn full_disk_access_is_unknown_without_any_private_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let core = RoomyCore::with_home(root.clone()).unwrap();
    assert_eq!(core.full_disk_access(), FullDiskAccess::Unknown);
    std::fs::create_dir_all(root.join(".Trash")).unwrap();
    assert_eq!(core.full_disk_access(), FullDiskAccess::Granted);
}

#[test]
fn a_private_folder_that_cannot_be_read_is_named_and_means_no_access() {
    use std::os::unix::fs::PermissionsExt;
    let s = setup();
    let mail = s.root.join("Library/Mail");
    std::fs::create_dir_all(mail.join("V10")).unwrap();
    std::fs::set_permissions(&mail, std::fs::Permissions::from_mode(0o000)).unwrap();
    let still_readable = std::fs::read_dir(&mail).is_ok();
    let access = s.core.full_disk_access();
    let h = finished_scan(&s.core, Mode::Simple);
    let o = s.core.overview(h).unwrap();
    std::fs::set_permissions(&mail, std::fs::Permissions::from_mode(0o755)).unwrap();
    if still_readable {
        eprintln!("skipped: running as root, so the folder is still readable");
        return;
    }
    assert_eq!(access, FullDiskAccess::NotGranted);
    let mail_area = o
        .unreadable_areas
        .iter()
        .find(|a| a.area == PrivateArea::Mail);
    assert_eq!(mail_area.map(|a| a.folder_count), Some(1));
}

#[test]
fn top_items_are_the_largest_reviewable_items() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let top = s.core.overview(h).unwrap().top_items;
    assert!(!top.is_empty());
    assert!(top.len() <= 8);
    assert!(top.iter().all(|i| i.tier != Tier::Keep));
    let mut expected: Vec<ItemSummary> = all_items(&s.core, h)
        .into_iter()
        .filter(|i| i.tier != Tier::Keep)
        .collect();
    expected.truncate(8);
    assert_eq!(top, expected);
}

#[test]
fn list_items_filters_and_sorts() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let all = all_items(&s.core, h);
    assert!(all.windows(2).all(|w| {
        w[0].size_bytes > w[1].size_bytes
            || (w[0].size_bytes == w[1].size_bytes && w[0].id < w[1].id)
    }));

    let smallest = s
        .core
        .list_items(h, None, None, Sort::SmallestFirst)
        .unwrap();
    assert_eq!(smallest.len(), all.len());
    assert!(smallest.windows(2).all(|w| {
        w[0].size_bytes < w[1].size_bytes
            || (w[0].size_bytes == w[1].size_bytes && w[0].id < w[1].id)
    }));

    let by_title = s.core.list_items(h, None, None, Sort::Title).unwrap();
    assert!(by_title
        .windows(2)
        .all(|w| w[0].title.to_lowercase() <= w[1].title.to_lowercase()));

    for tier in [Tier::LikelySafe, Tier::ReviewFirst, Tier::Keep] {
        let got = s
            .core
            .list_items(h, None, Some(tier), Sort::LargestFirst)
            .unwrap();
        let want: Vec<_> = all.iter().filter(|i| i.tier == tier).cloned().collect();
        assert_eq!(got, want);
    }
    let build = s
        .core
        .list_items(
            h,
            Some(Category::Build),
            Some(Tier::LikelySafe),
            Sort::LargestFirst,
        )
        .unwrap();
    assert!(!build.is_empty(), "the fixture has likely safe build data");
    assert!(build
        .iter()
        .all(|i| i.category == Category::Build && i.tier == Tier::LikelySafe));
}

#[test]
fn simple_mode_sends_no_path() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Simple);
    let root = s.root.to_string_lossy().into_owned();
    for d in details(&s.core, h) {
        assert_eq!(d.path, None, "{} has a path in simple mode", d.id);
        let mut text = vec![&d.id, &d.title, &d.explain, &d.risk, &d.rule_id];
        text.extend(d.restore.as_ref());
        text.extend(d.command.as_ref());
        for t in text {
            assert!(!t.contains(&root), "{t:?} holds the home path");
            assert!(!t.contains("~/"), "{t:?} looks like a path");
        }
    }
}

#[test]
fn developer_mode_paths_are_display_strings_inside_home() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let root = s.root.to_string_lossy().into_owned();
    let all = details(&s.core, h);
    assert!(!all.is_empty());
    for d in &all {
        let path = d.path.as_deref().unwrap();
        assert!(path.starts_with("~/"), "{path}");
        assert!(!path.contains(&root), "{path}");
    }
    assert!(all.iter().any(|d| d.rule_id == "xcode.derived_data"));
}

#[test]
fn item_detail_matches_its_summary() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    for (summary, detail) in all_items(&s.core, h).iter().zip(details(&s.core, h)) {
        assert_eq!(summary.id, detail.id);
        assert_eq!(summary.title, detail.title);
        assert_eq!(summary.size_bytes, detail.size_bytes);
        assert_eq!(summary.tier, detail.tier);
        assert_eq!(summary.category, detail.category);
        if detail.tier == Tier::Keep {
            assert!(!detail.regenerable);
            assert_eq!(detail.action, roomy_ffi::Action::ShowOnly);
        }
        if detail.rule_id == "unknown" {
            assert_eq!(detail.rule_version, 0);
            assert_eq!(detail.tier, Tier::Keep);
        }
    }
}

#[test]
fn item_ids_are_stable_across_scans() {
    let s = setup();
    let a = finished_scan(&s.core, Mode::Developer);
    let b = finished_scan(&s.core, Mode::Developer);
    assert_ne!(a, b);
    assert_eq!(all_items(&s.core, a), all_items(&s.core, b));
}

#[test]
fn handles_start_at_one_and_go_up() {
    let s = setup();
    let a = s.core.start_scan(Mode::Simple);
    let b = s.core.start_scan(Mode::Simple);
    assert_eq!(a.id, 1);
    assert_eq!(b.id, 2);
    s.core.close_scan(a);
    s.core.close_scan(b);
}

#[test]
fn unknown_handles_and_items_are_errors() {
    let s = setup();
    let nope = ScanHandle { id: 999 };
    assert!(matches!(
        s.core.scan_progress(nope),
        Err(CoreError::UnknownScan)
    ));
    assert!(matches!(s.core.overview(nope), Err(CoreError::UnknownScan)));
    assert!(matches!(
        s.core.list_items(nope, None, None, Sort::Title),
        Err(CoreError::UnknownScan)
    ));
    assert!(matches!(
        s.core.item_detail(nope, "0".repeat(32)),
        Err(CoreError::UnknownScan)
    ));
    s.core.cancel_scan(nope);
    s.core.close_scan(nope);

    let h = finished_scan(&s.core, Mode::Simple);
    assert!(matches!(
        s.core.item_detail(h, "0".repeat(32)),
        Err(CoreError::UnknownItem)
    ));
    assert!(matches!(
        s.core.item_detail(h, "../../etc/passwd".to_string()),
        Err(CoreError::UnknownItem)
    ));
}

#[test]
fn closing_a_scan_forgets_it() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Simple);
    s.core.close_scan(h);
    s.core.close_scan(h);
    assert!(matches!(s.core.overview(h), Err(CoreError::UnknownScan)));
    assert!(matches!(
        s.core.scan_progress(h),
        Err(CoreError::UnknownScan)
    ));
}

#[test]
fn cancelling_ends_the_scan() {
    let s = setup();
    let h = s.core.start_scan(Mode::Developer);
    s.core.cancel_scan(h);
    s.core.cancel_scan(h);
    match wait(&s.core, h) {
        // The fixture is small, so the scan may win the race.
        ScanPhase::Finished => assert!(s.core.overview(h).is_ok()),
        ScanPhase::Cancelled => {
            assert!(matches!(s.core.overview(h), Err(CoreError::ScanCancelled)))
        }
        other => panic!("unexpected phase {other:?}"),
    }
}

#[test]
fn cancelling_a_finished_scan_keeps_its_result() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Simple);
    s.core.cancel_scan(h);
    assert_eq!(s.core.scan_progress(h).unwrap().phase, ScanPhase::Finished);
    assert!(s.core.overview(h).is_ok());
}

#[test]
fn a_missing_home_fails_in_plain_words() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-home");
    let core = RoomyCore::with_home(missing.clone()).unwrap();
    let h = core.start_scan(Mode::Simple);
    let ScanPhase::Failed { reason } = wait(&core, h) else {
        panic!("expected the scan to fail");
    };
    assert!(!reason.contains("no-such-home"), "{reason}");
    match core.overview(h) {
        Err(CoreError::ScanFailed { reason: r }) => assert_eq!(r, reason),
        other => panic!("expected ScanFailed, got {other:?}"),
    }
}

#[test]
fn the_real_home_constructor_works() {
    // Only builds the core; never scans the real home folder.
    assert!(RoomyCore::new().is_ok());
}

#[test]
fn scans_can_run_at_the_same_time() {
    let s = setup();
    let handles: Vec<ScanHandle> = (0..4)
        .map(|i| {
            s.core.start_scan(if i % 2 == 0 {
                Mode::Simple
            } else {
                Mode::Developer
            })
        })
        .collect();
    for h in &handles {
        assert_eq!(wait(&s.core, *h), ScanPhase::Finished);
    }
    assert_eq!(
        all_items(&s.core, handles[0]),
        all_items(&s.core, handles[2])
    );
}
