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
