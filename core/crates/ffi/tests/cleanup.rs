//! The cleanup preview (dry run) against a fixture home. See the cleanup
//! section of `docs/command-contract.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use roomy_ffi::{
    CleanupCheck, CleanupChoice, CoreError, ItemSummary, Mode, PlanHandle, RoomyCore, ScanHandle,
    ScanPhase, Sort, Tier,
};

/// A fixture home with a Trash, and a core that scans it.
struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    core: Arc<RoomyCore>,
}

fn setup(with_trash: bool) -> Setup {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    if with_trash {
        fs::create_dir(root.join(".Trash")).unwrap();
    }
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

fn finished_scan(core: &RoomyCore, mode: Mode) -> ScanHandle {
    let handle = core.start_scan(mode);
    let start = Instant::now();
    loop {
        match core.scan_progress(handle).unwrap().phase {
            ScanPhase::Scanning | ScanPhase::Classifying => {}
            ScanPhase::Finished => return handle,
            other => panic!("scan ended {other:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn items(core: &RoomyCore, h: ScanHandle, tier: Tier) -> Vec<ItemSummary> {
    core.list_items(h, None, Some(tier), Sort::LargestFirst)
        .unwrap()
}

fn chosen(items: &[ItemSummary]) -> Vec<CleanupChoice> {
    items
        .iter()
        .map(|i| CleanupChoice {
            item_id: i.id.clone(),
            chosen_alone: true,
        })
        .collect()
}

/// Every path under `root` with its size and modification time, so a test
/// can show nothing changed. Never follows a symlink.
fn snapshot(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            out.push(format!(
                "{} {} {:?}",
                entry.path().display(),
                meta.len(),
                meta.modified().unwrap()
            ));
            if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn preview_says_what_would_move_and_moves_nothing() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let likely = items(&s.core, h, Tier::LikelySafe);
    assert!(!likely.is_empty());
    let before = snapshot(&s.root);

    let preview = s.core.preview_cleanup(h, chosen(&likely)).unwrap();

    assert_eq!(snapshot(&s.root), before, "the preview changed something");
    assert_eq!(fs::read_dir(s.root.join(".Trash")).unwrap().count(), 0);
    assert_eq!(preview.items.len(), likely.len());
    assert!(preview.plan.is_some());
    assert_eq!(preview.expires_in_seconds, 600);
    let will_move: Vec<_> = preview
        .items
        .iter()
        .filter(|i| i.check == CleanupCheck::WillMove)
        .collect();
    assert!(!will_move.is_empty(), "{preview:?}");
    let sum: u64 = will_move.iter().map(|i| i.size_bytes).sum();
    assert_eq!(preview.bytes_to_move, sum);
    // Developer mode shows where each item is, as a display path.
    assert!(preview
        .items
        .iter()
        .all(|i| i.path.as_deref().is_some_and(|p| p.starts_with("~/"))));
}

#[test]
fn keep_items_are_never_offered() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let keep = items(&s.core, h, Tier::Keep);
    assert!(!keep.is_empty());
    let preview = s.core.preview_cleanup(h, chosen(&keep)).unwrap();
    assert!(preview.plan.is_none());
    assert_eq!(preview.bytes_to_move, 0);
    for item in &preview.items {
        match &item.check {
            CleanupCheck::WontMove { reason } => assert!(!reason.is_empty()),
            CleanupCheck::WillMove => panic!("{item:?} would move"),
        }
    }
}

#[test]
fn simple_mode_previews_carry_no_path() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Simple);
    let mut all = items(&s.core, h, Tier::LikelySafe);
    all.extend(items(&s.core, h, Tier::Keep));
    let preview = s.core.preview_cleanup(h, chosen(&all)).unwrap();
    let root = s.root.to_string_lossy().into_owned();
    for item in &preview.items {
        assert_eq!(item.path, None);
        if let CleanupCheck::WontMove { reason } = &item.check {
            assert!(!reason.contains(&root), "{reason}");
            assert!(!reason.contains('/'), "{reason}");
        }
    }
}

#[test]
fn an_item_changed_since_the_scan_wont_move() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let likely = items(&s.core, h, Tier::LikelySafe);
    let first = s
        .core
        .item_detail(h, likely[0].id.clone())
        .unwrap()
        .path
        .unwrap();
    let place = s.root.join(first.trim_start_matches("~/"));
    if place.is_dir() {
        fs::write(place.join("new since the scan.bin"), vec![1u8; 4 << 20]).unwrap();
    } else {
        fs::write(&place, vec![1u8; 4 << 20]).unwrap();
    }
    let preview = s.core.preview_cleanup(h, chosen(&likely[..1])).unwrap();
    assert_eq!(
        preview.items[0].check,
        CleanupCheck::WontMove {
            reason: "This changed since you reviewed it.".to_string()
        }
    );
    assert!(preview.plan.is_none());
}

#[test]
fn repeated_ids_count_once() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let likely = items(&s.core, h, Tier::LikelySafe);
    let mut choices = chosen(&likely[..1]);
    choices.extend(chosen(&likely[..1]));
    let preview = s.core.preview_cleanup(h, choices).unwrap();
    assert_eq!(preview.items.len(), 1);
}

#[test]
fn nothing_chosen_and_unknown_ids_are_errors() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    assert!(matches!(
        s.core.preview_cleanup(h, vec![]),
        Err(CoreError::NothingSelected)
    ));
    let bogus = vec![CleanupChoice {
        item_id: "0".repeat(32),
        chosen_alone: true,
    }];
    assert!(matches!(
        s.core.preview_cleanup(h, bogus),
        Err(CoreError::UnknownItem)
    ));
    assert!(matches!(
        s.core.preview_cleanup(ScanHandle { id: 999 }, vec![]),
        Err(CoreError::UnknownScan)
    ));
}

#[test]
fn without_a_trash_nothing_can_move() {
    let s = setup(false);
    let h = finished_scan(&s.core, Mode::Developer);
    let likely = items(&s.core, h, Tier::LikelySafe);
    match s.core.preview_cleanup(h, chosen(&likely)) {
        Err(CoreError::CleanupUnavailable { reason }) => {
            assert_eq!(
                reason,
                "Roomy couldn't open your Trash, so nothing can be moved."
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn discard_is_safe_to_repeat() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let likely = items(&s.core, h, Tier::LikelySafe);
    let plan = s.core.preview_cleanup(h, chosen(&likely)).unwrap().plan;
    let plan = plan.unwrap();
    s.core.discard_cleanup(plan);
    s.core.discard_cleanup(plan);
    s.core.discard_cleanup(PlanHandle { id: 12345 });
    // A new preview makes a new plan with a new id.
    let again = s.core.preview_cleanup(h, chosen(&likely)).unwrap().plan;
    assert!(again.is_some_and(|p| p.id > plan.id));
}

// ---------------------------------------------------------------------------
// Running a cleanup, putting it back, History
// ---------------------------------------------------------------------------

/// Every path under `root`, with the size of each file and the target of
/// each symlink, leaving out anything under the `skip` folders. Folder times
/// are left out on purpose: moving an item changes its parent folder's.
fn contents(root: &Path, skip: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if skip.iter().any(|s| path.starts_with(s)) {
                continue;
            }
            let meta = path.symlink_metadata().unwrap();
            let what = if meta.is_dir() {
                stack.push(path.clone());
                "folder".to_string()
            } else if meta.file_type().is_symlink() {
                format!("link to {}", fs::read_link(&path).unwrap().display())
            } else {
                format!("{} bytes", meta.len())
            };
            out.push(format!("{} {what}", path.display()));
        }
    }
    out.sort();
    out
}

/// A preview of every likely safe item, and its plan.
fn likely_plan(s: &Setup, h: ScanHandle) -> (roomy_ffi::CleanupPreview, PlanHandle) {
    let likely = items(&s.core, h, Tier::LikelySafe);
    let preview = s.core.preview_cleanup(h, chosen(&likely)).unwrap();
    let plan = preview.plan.unwrap();
    (preview, plan)
}

/// The `~/` display path of every item that would move, as real paths.
fn moving_paths(s: &Setup, preview: &roomy_ffi::CleanupPreview) -> Vec<PathBuf> {
    preview
        .items
        .iter()
        .filter(|i| i.check == CleanupCheck::WillMove)
        .map(|i| {
            s.root
                .join(i.path.as_deref().unwrap().trim_start_matches("~/"))
        })
        .collect()
}

#[test]
fn a_cleanup_moves_what_the_preview_said_and_nothing_else() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let (preview, plan) = likely_plan(&s, h);
    let moving = moving_paths(&s, &preview);
    let mut skip = moving.clone();
    skip.push(s.root.join(".Trash"));
    skip.push(s.root.join(history::FOLDER));
    let before = contents(&s.root, &skip);

    let result = s.core.run_cleanup(plan).unwrap();

    assert!(result.items.iter().all(|i| i.done), "{result:?}");
    assert_eq!(result.items.len(), moving.len());
    assert_eq!(result.bytes_moved, preview.bytes_to_move);
    assert!(result.cleanup.is_some());
    assert!(result.saved_to_history);
    for path in &moving {
        assert!(
            path.symlink_metadata().is_err(),
            "{} is still there",
            path.display()
        );
    }
    // Everything else is exactly as it was.
    assert_eq!(contents(&s.root, &skip), before);
    // The plan is used up.
    assert!(matches!(
        s.core.run_cleanup(plan),
        Err(CoreError::CleanupUnavailable { .. })
    ));
}

#[test]
fn only_the_waiting_plan_can_run() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let (_, old) = likely_plan(&s, h);
    let (_, new) = likely_plan(&s, h);
    assert!(matches!(
        s.core.run_cleanup(old),
        Err(CoreError::CleanupUnavailable { .. })
    ));
    s.core.discard_cleanup(new);
    assert!(matches!(
        s.core.run_cleanup(new),
        Err(CoreError::CleanupUnavailable { .. })
    ));
    assert_eq!(fs::read_dir(s.root.join(".Trash")).unwrap().count(), 0);
}

#[test]
fn put_back_restores_the_last_cleanup_once() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let (preview, plan) = likely_plan(&s, h);
    let skip = [s.root.join(history::FOLDER)];
    let before = contents(&s.root, &skip);
    let result = s.core.run_cleanup(plan).unwrap();
    let cleanup = result.cleanup.unwrap();
    assert!(!moving_paths(&s, &preview).is_empty());

    let back = s.core.put_back_cleanup(cleanup).unwrap();

    assert!(back.items.iter().all(|i| i.done), "{back:?}");
    assert_eq!(back.bytes_put_back, result.bytes_moved);
    // Every file, folder and link is back where it was, the Trash is empty.
    assert_eq!(contents(&s.root, &skip), before);
    // Only once.
    assert!(matches!(
        s.core.put_back_cleanup(cleanup),
        Err(CoreError::NothingToPutBack)
    ));
    // History shows it.
    let history = s.core.cleanup_history(10);
    assert_eq!(history.len(), 1);
    assert!(history[0].put_back_at.is_some());
    assert_eq!(history[0].bytes_in_trash, 0);
    assert!(history[0]
        .items
        .iter()
        .all(|i| i.state == roomy_ffi::HistoryItemState::PutBack));
}

#[test]
fn history_is_empty_before_any_cleanup_and_makes_no_file() {
    let s = setup(true);
    assert!(s.core.cleanup_history(10).is_empty());
    assert!(!s.root.join(history::FOLDER).exists());
}

#[test]
fn history_holds_no_paths() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let (_, plan) = likely_plan(&s, h);
    s.core.run_cleanup(plan).unwrap();
    let text = format!("{:?}", s.core.cleanup_history(10));
    assert!(!text.contains(&*s.root.to_string_lossy()), "{text}");
    assert!(!text.contains("~/"), "{text}");
    let db = fs::read(history::path_for(&s.root)).unwrap();
    let root = s.root.to_string_lossy();
    assert!(
        !db.windows(root.len()).any(|w| w == root.as_bytes()),
        "the History file holds a path"
    );
}

// ---------------------------------------------------------------------------
// Checking items for the menu bar panel
// ---------------------------------------------------------------------------

#[test]
fn check_items_says_what_the_preview_says_and_moves_nothing() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let likely = items(&s.core, h, Tier::LikelySafe);
    let ids: Vec<String> = likely.iter().map(|i| i.id.clone()).collect();
    let before = snapshot(&s.root);

    let checked = s.core.check_items(h, ids).unwrap();

    assert_eq!(snapshot(&s.root), before, "checking changed something");
    let preview = s.core.preview_cleanup(h, chosen(&likely)).unwrap();
    assert_eq!(checked, preview.items);
}

#[test]
fn check_items_leaves_the_waiting_plan_alone() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    let likely = items(&s.core, h, Tier::LikelySafe);
    let plan = s
        .core
        .preview_cleanup(h, chosen(&likely))
        .unwrap()
        .plan
        .unwrap();

    let ids: Vec<String> = likely.iter().map(|i| i.id.clone()).collect();
    s.core.check_items(h, ids).unwrap();

    // The review the main window holds can still be carried out.
    assert!(s.core.run_cleanup(plan).is_ok());
}

#[test]
fn check_items_locks_an_item_whose_app_is_open() {
    let s = setup(true);
    s.core
        .set_running_programs_for_testing(Some(vec![PathBuf::from(
            "/Applications/Android Studio.app/Contents/MacOS/studio",
        )]));
    let h = finished_scan(&s.core, Mode::Developer);
    let mut all = items(&s.core, h, Tier::LikelySafe);
    all.extend(items(&s.core, h, Tier::ReviewFirst));
    let ids: Vec<String> = all.iter().map(|i| i.id.clone()).collect();

    let checked = s.core.check_items(h, ids).unwrap();

    assert!(
        checked.iter().any(|i| matches!(
            &i.check,
            CleanupCheck::WontMove { reason } if reason.contains("Android Studio is open")
        )),
        "{checked:?}"
    );
    // Others still pass.
    assert!(checked.iter().any(|i| i.check == CleanupCheck::WillMove));
}

#[test]
fn check_items_with_nothing_or_an_unknown_id_is_an_error() {
    let s = setup(true);
    let h = finished_scan(&s.core, Mode::Developer);
    assert!(matches!(
        s.core.check_items(h, vec![]),
        Err(CoreError::NothingSelected)
    ));
    assert!(matches!(
        s.core.check_items(h, vec!["0".repeat(32)]),
        Err(CoreError::UnknownItem)
    ));
}
