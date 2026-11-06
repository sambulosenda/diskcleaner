//! What changed since the last scan (F11) and quick rescans (F12), against
//! a fixture home.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use roomy_ffi::{Category, ChangeKind, ItemSummary, Mode, RoomyCore, ScanHandle, ScanPhase, Sort};

/// 30 MB: well over the 10 MB a change must reach to be shown.
const BIG: usize = 30_000_000;

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

fn wait(core: &RoomyCore, handle: ScanHandle) {
    let start = Instant::now();
    loop {
        match core.scan_progress(handle).unwrap().phase {
            ScanPhase::Scanning | ScanPhase::Classifying => {}
            ScanPhase::Finished => return,
            other => panic!("scan ended {other:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(120), "scan too slow");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn full(core: &RoomyCore, mode: Mode) -> ScanHandle {
    let h = core.start_scan(mode);
    wait(core, h);
    h
}

fn quick(core: &RoomyCore, mode: Mode) -> ScanHandle {
    let h = core.start_rescan(mode);
    wait(core, h);
    h
}

/// Items with the size of the leftover `Library` item set to 0: it holds
/// Roomy's own History database, which every scan adds a record to.
fn items(core: &RoomyCore, h: ScanHandle) -> Vec<ItemSummary> {
    let mut list = core.list_items(h, None, None, Sort::LargestFirst).unwrap();
    for item in &mut list {
        if item.title == "Other items in Library" {
            item.size_bytes = 0;
        }
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
    list
}

#[test]
fn the_first_scan_has_nothing_to_compare_with() {
    let s = setup();
    let h = full(&s.core, Mode::Simple);
    assert_eq!(s.core.scan_changes(h).unwrap(), None);
    // The other mode has its own records.
    let d = full(&s.core, Mode::Developer);
    assert_eq!(s.core.scan_changes(d).unwrap(), None);
    // Asking for an unknown scan is an error, as for every command.
    assert!(s.core.scan_changes(ScanHandle { id: 999 }).is_err());
}

#[test]
fn growth_new_and_gone_items_are_reported() {
    let s = setup();
    let installer = tempfile::Builder::new()
        .prefix("Old installer ")
        .suffix(".dmg")
        .tempfile_in(s.root.join("Downloads"))
        .unwrap();
    fs::write(installer.path(), vec![1u8; BIG]).unwrap();
    let first = full(&s.core, Mode::Simple);
    let before = s.core.overview(first).unwrap().scanned_bytes;

    fs::write(
        s.root.join("Library/Caches/Firefox/grown.bin"),
        vec![2u8; BIG],
    )
    .unwrap();
    drop(installer);
    let second = quick(&s.core, Mode::Simple);
    let changes = s.core.scan_changes(second).unwrap().expect("changes");

    assert_eq!(changes.scanned_bytes_before, before);
    assert_eq!(
        changes.scanned_bytes_now,
        s.core.overview(second).unwrap().scanned_bytes
    );
    let grew = changes
        .items
        .iter()
        .find(|c| c.kind == ChangeKind::Grew)
        .expect("an item grew");
    assert_eq!(grew.category, Category::Cache);
    assert!(grew.bytes_now - grew.bytes_before >= BIG as u64);
    let id = grew.item_id.clone().expect("a current item has an id");
    assert!(s.core.item_detail(second, id).is_ok());

    let gone = changes
        .items
        .iter()
        .find(|c| c.kind == ChangeKind::Gone)
        .expect("the installer is gone");
    assert_eq!(gone.item_id, None);
    assert_eq!(gone.bytes_now, 0);
    assert!(gone.bytes_before >= BIG as u64);
    assert!(changes
        .categories
        .iter()
        .any(|c| c.category == Category::Cache && c.bytes_now > c.bytes_before));
    // Biggest change first.
    let deltas: Vec<u64> = changes
        .items
        .iter()
        .map(|c| c.bytes_before.abs_diff(c.bytes_now))
        .collect();
    assert!(deltas.windows(2).all(|w| w[0] >= w[1]), "{deltas:?}");
}

#[test]
fn small_changes_are_not_listed() {
    let s = setup();
    full(&s.core, Mode::Simple);
    fs::write(
        s.root.join("Library/Caches/Firefox/small.bin"),
        vec![2u8; 4096],
    )
    .unwrap();
    let second = full(&s.core, Mode::Simple);
    let changes = s.core.scan_changes(second).unwrap().expect("changes");
    assert!(changes.items.is_empty(), "{:?}", changes.items);
    assert!(changes.categories.is_empty(), "{:?}", changes.categories);
}

#[test]
fn a_rescan_finds_the_same_items_as_a_full_scan() {
    let s = setup();
    full(&s.core, Mode::Developer);
    fs::write(
        s.root.join("Library/Caches/Firefox/grown.bin"),
        vec![2u8; BIG],
    )
    .unwrap();
    fs::create_dir_all(s.root.join("Library/Caches/NewApp/deep")).unwrap();
    fs::write(
        s.root.join("Library/Caches/NewApp/deep/blob"),
        vec![3u8; BIG],
    )
    .unwrap();
    let q = quick(&s.core, Mode::Developer);
    let quick_overview = s.core.overview(q).unwrap();
    // Only macOS keeps the file event log a quick rescan needs; elsewhere
    // a rescan is a full scan.
    assert_eq!(quick_overview.quick_rescan, cfg!(target_os = "macos"));
    let f = full(&s.core, Mode::Developer);
    assert!(!s.core.overview(f).unwrap().quick_rescan);
    assert_eq!(items(&s.core, q), items(&s.core, f));
}

#[test]
fn a_rescan_in_the_other_mode_is_a_full_scan() {
    let s = setup();
    full(&s.core, Mode::Developer);
    let h = quick(&s.core, Mode::Simple);
    assert!(!s.core.overview(h).unwrap().quick_rescan);
    assert_eq!(s.core.scan_changes(h).unwrap(), None);
}
