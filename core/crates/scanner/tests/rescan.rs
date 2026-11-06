//! Faster rescans (F12): with a complete list of changes, a rescan gives
//! exactly what a full scan gives; when it can't be sure, it refuses.

use std::fs;
use std::path::{Path, PathBuf};

use scanner::{rescan, scan, Changes, RescanError, ScanOptions, ScanResult};

struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fixtures::build(&root, std::time::SystemTime::now()).unwrap();
    Home { _dir: dir, root }
}

/// The options a rescan accepts: no largest-files list.
fn opts() -> ScanOptions {
    ScanOptions {
        largest_files: 0,
        ..ScanOptions::default()
    }
}

fn full(root: &Path) -> ScanResult {
    scan(root, opts(), |_| {}).unwrap()
}

fn again(previous: &ScanResult, changes: &Changes) -> Result<ScanResult, RescanError> {
    rescan(previous, changes, opts(), |_| {})
}

/// Everything a scan reports, compared field by field.
fn assert_same(got: &ScanResult, want: &ScanResult) {
    assert_eq!(got.root, want.root);
    assert_eq!(got.root_dev, want.root_dev);
    assert_eq!(got.dirs.len(), want.dirs.len(), "folder count");
    for (i, (g, w)) in got.dirs.iter().zip(&want.dirs).enumerate() {
        assert_eq!(g, w, "folder {i} ({:?})", got.path_of(i));
    }
    assert_eq!(got.unreadable, want.unreadable);
    assert_eq!(got.other_volumes, want.other_volumes);
    assert_eq!(got.symlinks, want.symlinks);
    assert_eq!(got.hard_links_skipped, want.hard_links_skipped);
    assert_eq!(got.total_bytes(), want.total_bytes());
    assert_eq!(got.total_files(), want.total_files());
}

fn folders(root: &Path, rels: &[&str]) -> Changes {
    Changes {
        folders: rels.iter().map(|r| root.join(r)).collect(),
        subtrees: Vec::new(),
    }
}

#[test]
fn nothing_changed_gives_the_same_result() {
    let home = home();
    let first = full(&home.root);
    let quick = again(&first, &Changes::default()).unwrap();
    assert_same(&quick, &full(&home.root));
}

#[test]
fn a_file_that_grew_or_appeared_is_counted() {
    let home = home();
    let first = full(&home.root);
    let caches = home.root.join("Library/Caches/Firefox");
    fs::write(caches.join("new-cache.bin"), vec![7u8; 300_000]).unwrap();
    fs::write(
        home.root.join("Downloads/Meeting notes.txt"),
        vec![1u8; 90_000],
    )
    .unwrap();
    let changes = folders(&home.root, &["Library/Caches/Firefox", "Downloads"]);
    let quick = again(&first, &changes).unwrap();
    assert_same(&quick, &full(&home.root));
    assert!(quick.total_bytes() > first.total_bytes());
}

#[test]
fn a_new_folder_is_read_in_full() {
    let home = home();
    let first = full(&home.root);
    let deep = home.root.join("Library/Caches/NewApp/a/b");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("blob"), vec![3u8; 50_000]).unwrap();
    // Only the folder that gained `NewApp` is named, as the event log would.
    let quick = again(&first, &folders(&home.root, &["Library/Caches"])).unwrap();
    assert_same(&quick, &full(&home.root));
    // A path the earlier scan never saw also works: its known parent is
    // listed again.
    let quick = again(&first, &folders(&home.root, &["Library/Caches/NewApp/a/b"])).unwrap();
    assert_same(&quick, &full(&home.root));
}

#[test]
fn a_folder_that_went_away_is_dropped() {
    let home = home();
    let parent = home.root.join("Library/Caches");
    let gone = tempfile::Builder::new()
        .prefix("Going")
        .tempdir_in(&parent)
        .unwrap();
    fs::create_dir(gone.path().join("inner")).unwrap();
    fs::write(gone.path().join("inner/file"), vec![9u8; 40_000]).unwrap();
    let first = full(&home.root);
    drop(gone);
    let quick = again(&first, &folders(&home.root, &["Library/Caches"])).unwrap();
    assert_same(&quick, &full(&home.root));
    assert!(quick.total_bytes() < first.total_bytes());
}

#[test]
fn a_whole_subtree_can_be_read_again() {
    let home = home();
    let first = full(&home.root);
    let deep = home.root.join("Library/Logs/Zoom");
    fs::write(deep.join("more.log"), vec![2u8; 20_000]).unwrap();
    let changes = Changes {
        folders: Vec::new(),
        subtrees: vec![home.root.join("Library/Logs")],
    };
    assert_same(&again(&first, &changes).unwrap(), &full(&home.root));
}

#[test]
fn a_missing_folder_nobody_mentioned_means_a_full_scan() {
    let home = home();
    let parent = home.root.join("Library/Caches");
    let gone = tempfile::Builder::new()
        .prefix("Going")
        .tempdir_in(&parent)
        .unwrap();
    let name = gone.path().file_name().unwrap().to_owned();
    let first = full(&home.root);
    drop(gone);
    // The change list names the folder that went away but not its parent:
    // the rescan can't trust it.
    let changes = Changes {
        folders: vec![parent.join(name)],
        subtrees: Vec::new(),
    };
    assert!(matches!(
        again(&first, &changes),
        Err(RescanError::NeedsFullScan(_))
    ));
}

#[test]
fn folders_with_hard_links_are_never_read_again() {
    let home = home();
    let first = full(&home.root);
    for rel in ["Documents", "Movies"] {
        assert!(
            matches!(
                again(&first, &folders(&home.root, &[rel])),
                Err(RescanError::NeedsFullScan(_))
            ),
            "{rel}"
        );
    }
    let changes = Changes {
        folders: Vec::new(),
        subtrees: vec![home.root.clone()],
    };
    assert!(matches!(
        again(&first, &changes),
        Err(RescanError::NeedsFullScan(_))
    ));
}

#[test]
fn a_new_hard_link_in_a_changed_folder_means_a_full_scan() {
    let home = home();
    let first = full(&home.root);
    let caches = home.root.join("Library/Caches/Firefox");
    fs::write(caches.join("a"), b"hello").unwrap();
    fs::hard_link(caches.join("a"), caches.join("b")).unwrap();
    assert!(matches!(
        again(&first, &folders(&home.root, &["Library/Caches/Firefox"])),
        Err(RescanError::NeedsFullScan(_))
    ));
}

#[test]
fn changes_outside_the_folder_mean_a_full_scan() {
    let home = home();
    let first = full(&home.root);
    let changes = Changes {
        folders: vec![PathBuf::from("/somewhere/else")],
        subtrees: Vec::new(),
    };
    assert!(matches!(
        again(&first, &changes),
        Err(RescanError::NeedsFullScan(_))
    ));
    let changes = Changes {
        folders: vec![home.root.join("Library/../Documents")],
        subtrees: Vec::new(),
    };
    assert!(matches!(
        again(&first, &changes),
        Err(RescanError::NeedsFullScan(_))
    ));
}

#[test]
fn the_largest_files_list_needs_a_full_scan() {
    let home = home();
    let first = full(&home.root);
    let result = rescan(&first, &Changes::default(), ScanOptions::default(), |_| {});
    assert!(matches!(result, Err(RescanError::NeedsFullScan(_))));
}

#[test]
fn a_rescan_can_be_cancelled() {
    let home = home();
    let first = full(&home.root);
    let o = opts();
    o.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let result = rescan(&first, &folders(&home.root, &["Library"]), o, |_| {});
    assert!(matches!(
        result,
        Err(RescanError::Scan(scanner::ScanError::Cancelled))
    ));
}

/// The real file event log, end to end. macOS only.
#[cfg(target_os = "macos")]
#[test]
fn the_file_event_log_finds_what_changed() {
    let home = home();
    let since = scanner::fsevents::current_event_id().unwrap();
    let first = full(&home.root);
    let caches = home.root.join("Library/Caches/Firefox");
    fs::write(caches.join("new-cache.bin"), vec![7u8; 300_000]).unwrap();
    let new_dir = home.root.join("Library/Caches/NewApp");
    fs::create_dir(&new_dir).unwrap();
    fs::write(new_dir.join("blob"), vec![3u8; 50_000]).unwrap();
    let changes =
        scanner::fsevents::changes_since(&home.root, since, scanner::fsevents::REPLAY_TIMEOUT)
            .unwrap();
    assert!(
        changes
            .folders
            .iter()
            .any(|f| f.ends_with("Library/Caches/Firefox")),
        "{changes:?}"
    );
    assert_same(&again(&first, &changes).unwrap(), &full(&home.root));
}

#[test]
fn a_different_folder_under_the_same_name_is_read_in_full() {
    let home = home();
    let parent = home.root.join("Library/Caches");
    let old = tempfile::Builder::new()
        .prefix("Swapped")
        .tempdir_in(&parent)
        .unwrap();
    let name = old.path().file_name().unwrap().to_owned();
    fs::create_dir(old.path().join("inner")).unwrap();
    fs::write(old.path().join("inner/file"), vec![9u8; 40_000]).unwrap();
    let first = full(&home.root);
    drop(old);
    // A new, empty folder takes the old one's name. The event log names
    // only the parent, whose entries changed.
    fs::create_dir(parent.join(&name)).unwrap();
    let quick = again(&first, &folders(&home.root, &["Library/Caches"])).unwrap();
    assert_same(&quick, &full(&home.root));
}
