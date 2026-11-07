//! The Overview's rows and totals: items that share a title get the name
//! that tells them apart (Developer mode only, never a path), and the
//! Review first total is the exact sum of those items.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use roomy_ffi::{Mode, RoomyCore, ScanHandle, ScanPhase, Sort, Tier};

struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    core: Arc<RoomyCore>,
}

fn setup() -> Setup {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
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
        let phase = core.scan_progress(handle).unwrap().phase;
        if !matches!(phase, ScanPhase::Scanning | ScanPhase::Classifying) {
            assert_eq!(phase, ScanPhase::Finished);
            return handle;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "scan never ended"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn node_modules_rows_are_named_by_their_project_in_developer_mode() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let items = s
        .core
        .list_items(h, None, None, Sort::LargestFirst)
        .unwrap();
    let node_modules: Vec<_> = items
        .iter()
        .filter(|i| i.title == "Project packages (node_modules)")
        .collect();
    assert!(node_modules.len() >= 2, "fixture needs several projects");
    for item in &node_modules {
        let place = item.place.as_deref().expect("every project row has a name");
        assert!(!place.is_empty());
        assert!(
            !place.contains('/'),
            "a place is a name, never a path: {place}"
        );
        assert!(!place.contains(s.root.to_string_lossy().as_ref()));
        assert_ne!(place, "node_modules");
    }
}

#[test]
fn top_items_carry_the_same_names_as_the_lists() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let overview = s.core.overview(h).unwrap();
    let items = s
        .core
        .list_items(h, None, None, Sort::LargestFirst)
        .unwrap();
    for top in &overview.top_items {
        let listed = items.iter().find(|i| i.id == top.id).expect("listed");
        assert_eq!(top.place, listed.place);
    }
}

#[test]
fn simple_mode_rows_have_no_place() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Simple);
    let items = s
        .core
        .list_items(h, None, None, Sort::LargestFirst)
        .unwrap();
    assert!(items.iter().all(|i| i.place.is_none()));
    let overview = s.core.overview(h).unwrap();
    assert!(overview.top_items.iter().all(|i| i.place.is_none()));
}

#[test]
fn review_first_total_is_the_exact_sum_of_review_first_items() {
    let s = setup();
    for mode in [Mode::Simple, Mode::Developer] {
        let h = finished_scan(&s.core, mode);
        let overview = s.core.overview(h).unwrap();
        let review = s
            .core
            .list_items(h, None, Some(Tier::ReviewFirst), Sort::LargestFirst)
            .unwrap();
        let sum = review.iter().map(|i| i.size_bytes).sum::<u64>();
        assert_eq!(overview.review_first_bytes, sum);
    }
}
