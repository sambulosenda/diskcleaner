//! The library sizes screen (PRD E6, E7, C1): how much Photos, Music, TV,
//! Mail, Messages attachments and Final Cut Pro take, from the scan. Shown
//! only: nothing inside a library ever becomes a cleanup item.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fixtures::paths::{MESSAGES_ATTACHMENTS, PHOTOS_LIBRARY};
use roomy_ffi::{CoreError, LibraryKind, Mode, RoomyCore, ScanHandle, ScanPhase, Sort};

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

fn setup() -> (tempfile::TempDir, PathBuf, Arc<RoomyCore>) {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    let core = RoomyCore::with_home(root.clone()).unwrap();
    (dir, root, core)
}

/// Allocated bytes of everything under `path`, the way the scanner counts.
fn du(path: &Path) -> u64 {
    let scan = scanner::scan(path, scanner::ScanOptions::default(), |_| {}).unwrap();
    scan.total_bytes()
}

#[test]
fn the_fixture_photos_library_and_attachments_are_measured() {
    let (_dir, root, core) = setup();
    for mode in [Mode::Simple, Mode::Developer] {
        let h = finished_scan(&core, mode);
        let rows = core.libraries(h).unwrap();
        let kinds: Vec<LibraryKind> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![
                LibraryKind::Photos,
                LibraryKind::Music,
                LibraryKind::Tv,
                LibraryKind::Mail,
                LibraryKind::MessagesAttachments,
                LibraryKind::FinalCut,
            ]
        );
        let photos = &rows[0];
        assert!(photos.found);
        assert_eq!(photos.library_count, 1);
        assert_eq!(photos.bytes, du(&root.join(PHOTOS_LIBRARY)));
        let attachments = &rows[4];
        assert!(attachments.found);
        assert_eq!(attachments.bytes, du(&root.join(MESSAGES_ATTACHMENTS)));
    }
}

#[test]
fn nothing_inside_a_photos_library_or_attachments_is_offered_for_cleanup() {
    let (_dir, _root, core) = setup();
    let h = finished_scan(&core, Mode::Developer);
    for item in core.list_items(h, None, None, Sort::LargestFirst).unwrap() {
        let detail = core.item_detail(h, item.id.clone()).unwrap();
        let path = detail.path.unwrap_or_default();
        if path.contains(".photoslibrary") || path.contains("Messages/Attachments") {
            assert_eq!(
                detail.tier,
                roomy_ffi::Tier::Keep,
                "{path} must be shown only"
            );
        }
    }
}

#[test]
fn libraries_need_a_finished_scan() {
    let (_dir, _root, core) = setup();
    assert!(matches!(
        core.libraries(ScanHandle { id: 999 }),
        Err(CoreError::UnknownScan)
    ));
}
