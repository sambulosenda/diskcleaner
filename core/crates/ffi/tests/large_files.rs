//! The Large files screen (E2) against a fixture home: what is listed, what
//! is left out, and cleaning a listed file through the usual Trash, put
//! back and History.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use roomy_ffi::{
    CleanupCheck, CleanupChoice, CoreError, FileRow, HistoryItemState, LargeFiles, Mode, RoomyCore,
    ScanHandle, ScanPhase,
};

const MB: usize = 1 << 20;
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    core: Arc<RoomyCore>,
}

fn write(root: &Path, rel: &str, bytes: usize) -> PathBuf {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Not all zeros, so no file system stores it sparsely.
    fs::write(&path, vec![7u8; bytes]).unwrap();
    path
}

/// Sets `path`'s modification time to `days` ago. Works for folders too.
fn age(path: &Path, days: u32) {
    let when = SystemTime::now() - DAY * days;
    fs::File::open(path).unwrap().set_modified(when).unwrap();
}

/// A fixture home plus large files in and out of the places that are
/// listed, and a core that scans it.
fn setup() -> Setup {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    fs::create_dir(root.join(".Trash")).unwrap();
    write(&root, "Movies/Exports/Holiday.mov", 6 * MB);
    write(&root, "Documents/Taxes 2019.pdf", 3 * MB);
    // Places whose files are never listed on their own.
    write(&root, "Library/Application Support/Thing/data.db", 9 * MB);
    write(&root, ".ollama/models/blobs/sha256-1", 9 * MB);
    write(
        &root,
        "Pictures/Photos Library.photoslibrary/originals/A/1.heic",
        9 * MB,
    );
    write(&root, "Movies/Trip.fcpbundle/Render Files/r.mov", 9 * MB);
    write(&root, "Music/Music/Media.localized/Music/song.m4a", 9 * MB);
    let linked = write(&root, "Documents/linked-a.bin", 8 * MB);
    fs::hard_link(&linked, root.join("Documents/linked-b.bin")).unwrap();
    // Downloads: old and new, a file and a folder.
    let old_file = write(&root, "Downloads/old-video.mp4", 5 * MB);
    age(&old_file, 200);
    let old_folder_file = write(&root, "Downloads/Old photos/1.jpg", 2 * MB);
    age(&old_folder_file, 200);
    age(&root.join("Downloads/Old photos"), 200);
    write(&root, "Downloads/new.pdf", 4 * MB);
    let core = RoomyCore::with_home(root.clone()).unwrap();
    core.set_recent_change_window_for_testing(Duration::ZERO);
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

fn names(rows: &[FileRow]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

fn row<'a>(rows: &'a [FileRow], name: &str) -> &'a FileRow {
    rows.iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("{name} not in {:?}", names(rows)))
}

fn view(s: &Setup, mode: Mode) -> (ScanHandle, LargeFiles) {
    let h = finished_scan(&s.core, mode);
    (h, s.core.large_files(h).unwrap())
}

#[test]
fn lists_large_files_largest_first_with_place_and_dates() {
    let s = setup();
    let (_, v) = view(&s, Mode::Simple);
    let large = &v.large_files;
    assert!(large.len() <= 500);
    let sizes: Vec<u64> = large.iter().map(|r| r.size_bytes).collect();
    let mut sorted = sizes.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(sizes, sorted);
    let movie = row(large, "Holiday.mov");
    assert_eq!(movie.place, "Movies");
    assert!(!movie.is_folder);
    assert!(movie.size_bytes >= 6 * MB as u64);
    // Nothing opened these files from Finder or an app, so there is no
    // last-opened record.
    assert_eq!(movie.last_opened_unix_seconds, None);
    assert!(movie.modified_unix_seconds > 0);
    assert_eq!(row(large, "old-video.mp4").place, "Downloads");
    assert_eq!(v.old_after_days, 90);
}

#[test]
fn files_that_are_part_of_something_else_are_never_listed() {
    let s = setup();
    let (_, v) = view(&s, Mode::Developer);
    let listed = names(&v.large_files);
    for name in [
        "data.db",
        "sha256-1",
        "1.heic",
        "r.mov",
        "song.m4a",
        // Removing one name of a hard-linked file frees nothing.
        "linked-a.bin",
        "linked-b.bin",
    ] {
        assert!(!listed.contains(&name), "{name} was listed: {listed:?}");
    }
    // An installer a rule explains is on the Downloads screen, not here.
    let dmg = Path::new(fixtures::paths::DOWNLOAD_DMG)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    assert!(!listed.contains(&dmg));
    assert!(!names(&v.old_downloads).contains(&dmg));
    for r in v.large_files.iter().chain(&v.old_downloads) {
        let path = r.path.as_deref().unwrap();
        assert!(path.starts_with("~/"), "{path}");
        assert!(!path.starts_with("~/Library/"), "{path}");
        assert!(!path.contains("/."), "{path}");
    }
}

#[test]
fn old_downloads_are_the_ones_not_opened_or_changed_for_ninety_days() {
    let s = setup();
    let (_, v) = view(&s, Mode::Simple);
    let old = &v.old_downloads;
    let file = row(old, "old-video.mp4");
    let folder = row(old, "Old photos");
    assert!(folder.is_folder);
    assert!(folder.size_bytes >= 2 * MB as u64);
    assert!(!names(old).contains(&"new.pdf"));
    // The same file has the same id in both lists, so ticking it in one
    // ticks it in the other.
    assert_eq!(file.item_id, row(&v.large_files, "old-video.mp4").item_id);
    let sizes: Vec<u64> = old.iter().map(|r| r.size_bytes).collect();
    let mut sorted = sizes.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(sizes, sorted);
}

#[test]
fn simple_mode_rows_carry_no_path() {
    let s = setup();
    let (_, v) = view(&s, Mode::Simple);
    let root = s.root.to_string_lossy().into_owned();
    for r in v.large_files.iter().chain(&v.old_downloads) {
        assert_eq!(r.path, None);
        assert!(!r.name.contains('/'), "{r:?}");
        assert!(!r.place.contains(&root), "{r:?}");
    }
}

#[test]
fn a_ticked_file_moves_to_the_trash_and_can_be_put_back() {
    let s = setup();
    let (h, v) = view(&s, Mode::Simple);
    let movie = row(&v.large_files, "Holiday.mov").clone();
    let folder = row(&v.old_downloads, "Old photos").clone();
    let choices = [&movie, &folder]
        .iter()
        .map(|r| CleanupChoice {
            item_id: r.item_id.clone(),
            chosen_alone: true,
        })
        .collect();
    let preview = s.core.preview_cleanup(h, choices).unwrap();
    assert!(preview
        .items
        .iter()
        .all(|i| i.check == CleanupCheck::WillMove && i.path.is_none()));
    assert_eq!(preview.items[0].title, "Holiday.mov");
    let movie_path = s.root.join("Movies/Exports/Holiday.mov");
    let folder_path = s.root.join("Downloads/Old photos");
    assert!(movie_path.is_file(), "the preview moved something");

    let result = s.core.run_cleanup(preview.plan.unwrap()).unwrap();
    assert!(result.items.iter().all(|i| i.done), "{result:?}");
    assert!(!movie_path.exists());
    assert!(!folder_path.exists());
    assert_eq!(fs::read_dir(s.root.join(".Trash")).unwrap().count(), 2);
    let history = s.core.cleanup_history(10);
    assert_eq!(history.len(), 1);
    assert!(history[0]
        .items
        .iter()
        .any(|i| i.title == "Holiday.mov" && i.state == HistoryItemState::Moved));

    let back = s.core.put_back_cleanup(result.cleanup.unwrap()).unwrap();
    assert!(back.items.iter().all(|i| i.done), "{back:?}");
    assert!(movie_path.is_file());
    assert!(folder_path.join("1.jpg").is_file());
}

#[test]
fn a_file_grouped_with_others_wont_move() {
    // Files in Downloads, Desktop and Documents are review only: they move
    // only when the user ticked each one on its own.
    let s = setup();
    let (h, v) = view(&s, Mode::Simple);
    let doc = row(&v.large_files, "Taxes 2019.pdf");
    let preview = s
        .core
        .preview_cleanup(
            h,
            vec![CleanupChoice {
                item_id: doc.item_id.clone(),
                chosen_alone: false,
            }],
        )
        .unwrap();
    assert!(preview.plan.is_none());
    assert!(matches!(
        preview.items[0].check,
        CleanupCheck::WontMove { .. }
    ));
    assert!(s.root.join("Documents/Taxes 2019.pdf").is_file());
}

#[test]
fn a_file_changed_since_the_scan_wont_move() {
    let s = setup();
    let (h, v) = view(&s, Mode::Simple);
    let movie = row(&v.large_files, "Holiday.mov");
    write(&s.root, "Movies/Exports/Holiday.mov", 7 * MB);
    let preview = s
        .core
        .preview_cleanup(
            h,
            vec![CleanupChoice {
                item_id: movie.item_id.clone(),
                chosen_alone: true,
            }],
        )
        .unwrap();
    assert!(preview.plan.is_none());
    assert_eq!(
        preview.items[0].check,
        CleanupCheck::WontMove {
            reason: "This changed since you reviewed it.".to_string()
        }
    );
}

#[test]
fn a_file_in_a_kept_folder_wont_move() {
    let s = setup();
    let (h, v) = view(&s, Mode::Simple);
    let movie = row(&v.large_files, "Holiday.mov");
    let kept = s.root.join("Movies").to_string_lossy().into_owned();
    s.core.set_protected_folders(vec![kept]).unwrap();
    let preview = s
        .core
        .preview_cleanup(
            h,
            vec![CleanupChoice {
                item_id: movie.item_id.clone(),
                chosen_alone: true,
            }],
        )
        .unwrap();
    assert!(preview.plan.is_none());
}

#[test]
fn ids_from_another_scan_are_unknown() {
    let s = setup();
    let (_, v) = view(&s, Mode::Simple);
    let h2 = finished_scan(&s.core, Mode::Simple);
    let unknown = s.core.preview_cleanup(
        h2,
        vec![CleanupChoice {
            item_id: "file-99999".to_string(),
            chosen_alone: true,
        }],
    );
    assert!(matches!(unknown, Err(CoreError::UnknownItem)));
    assert!(!v.large_files.is_empty());
    assert!(matches!(
        s.core.large_files(ScanHandle { id: 9999 }),
        Err(CoreError::UnknownScan)
    ));
}
