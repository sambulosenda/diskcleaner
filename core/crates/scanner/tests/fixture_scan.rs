//! The scanner against a fixture home (T04): totals, hard links, symlinks,
//! unreadable folders, cancellation and the largest-files list.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use fixtures::{paths, EntryKind, Manifest, MANIFEST_FILE};
use scanner::{scan, ScanError, ScanOptions, ScanResult, UnreadableReason};

/// A fixture home, its canonical root and its manifest.
struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
    manifest: Manifest,
}

fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let manifest = fixtures::build(&root, std::time::SystemTime::now()).unwrap();
    Home {
        _dir: dir,
        root,
        manifest,
    }
}

fn scan_home(root: &Path) -> ScanResult {
    scan(root, ScanOptions::default(), |_| {}).unwrap()
}

fn allocated(path: &Path) -> u64 {
    std::fs::symlink_metadata(path).unwrap().blocks() * 512
}

/// What `du` would say: the allocated size of the root, the manifest file and
/// every manifest entry (folders, files, symlinks themselves), counting the
/// second name of a hard link pair zero times. Entries under `skip` (an
/// unreadable folder) are left out, but `skip` itself is counted.
fn expected_allocated(home: &Home, skip: Option<&str>) -> u64 {
    let inside_skip = |p: &str| skip.is_some_and(|s| p.starts_with(&format!("{s}/")));
    allocated(&home.root)
        + allocated(&home.root.join(MANIFEST_FILE))
        + home
            .manifest
            .entries
            .iter()
            .filter(|e| e.hard_link_of.is_none() && !inside_skip(&e.path))
            .map(|e| allocated(&home.root.join(&e.path)))
            .sum::<u64>()
}

fn manifest_file_len(home: &Home) -> u64 {
    std::fs::symlink_metadata(home.root.join(MANIFEST_FILE))
        .unwrap()
        .len()
}

fn count(manifest: &Manifest, kind: EntryKind) -> u64 {
    manifest.entries.iter().filter(|e| e.kind == kind).count() as u64
}

#[test]
fn total_equals_the_manifest_allocated_total() {
    let home = home();
    let result = scan_home(&home.root);

    assert_eq!(result.total_bytes(), expected_allocated(&home, None));
    assert!(result.total_bytes() > 0);
    // File lengths match the manifest exactly (plus the manifest file itself).
    assert_eq!(
        result.root_dir().total_logical_bytes,
        home.manifest.total_file_bytes() + manifest_file_len(&home)
    );
    // One record per folder: every manifest folder plus the root.
    assert_eq!(
        result.dirs.len() as u64,
        count(&home.manifest, EntryKind::Dir) + 1
    );
    assert!(result.unreadable.is_empty());
    assert!(result.other_volumes.is_empty());
    assert_eq!(result.entries_skipped, 0);
}

#[test]
fn folder_totals_add_up() {
    let home = home();
    let result = scan_home(&home.root);
    for (id, dir) in result.dirs.iter().enumerate() {
        let children: u64 = dir
            .children
            .iter()
            .map(|&c| result.dirs[c].total_bytes)
            .sum();
        assert_eq!(dir.total_bytes, dir.own_bytes + children, "folder {id}");
        for &c in &dir.children {
            assert_eq!(result.dirs[c].parent, Some(id));
        }
    }
    // A known folder is found by path and its path round-trips.
    let id = result.find(Path::new(paths::DERIVED_DATA)).unwrap();
    assert_eq!(
        result.path_of(id).unwrap(),
        home.root.join(paths::DERIVED_DATA)
    );
    let expected: u64 = home
        .manifest
        .entries
        .iter()
        .filter(|e| e.path.starts_with(&format!("{}/", paths::DERIVED_DATA)))
        .map(|e| e.size)
        .sum();
    assert_eq!(result.dirs[id].total_logical_bytes, expected);
}

#[test]
fn hard_link_pair_is_counted_once() {
    let home = home();
    let result = scan_home(&home.root);
    let link_len = home.manifest.get(paths::TRAP_HARD_LINK_A).unwrap().size;
    assert!(link_len > 0);

    assert_eq!(result.hard_links_skipped, 1);
    let files_once = count(&home.manifest, EntryKind::File) - 1;
    assert_eq!(result.total_files(), files_once + 1); // + the manifest file

    // Exactly one of the two folders holds the file's bytes.
    let logical = |rel: &str| {
        let id = result.find(Path::new(rel)).unwrap();
        result.dirs[id].total_logical_bytes
    };
    let without_link = |rel: &str| -> u64 {
        home.manifest
            .entries
            .iter()
            .filter(|e| e.path.starts_with(&format!("{rel}/")) && e.hard_link_of.is_none())
            .filter(|e| e.path != paths::TRAP_HARD_LINK_A)
            .map(|e| e.size)
            .sum()
    };
    let extra = (logical("Documents") - without_link("Documents"))
        + (logical("Movies") - without_link("Movies"));
    assert_eq!(extra, link_len);
    // Always in the folder of the name that sorts first, whichever name a
    // thread reached first: Documents before Movies.
    assert_eq!(logical("Documents") - without_link("Documents"), link_len);
    assert_eq!(logical("Movies"), without_link("Movies"));

    // And it appears once in the largest-files list.
    let names = result
        .largest_files
        .iter()
        .filter(|f| f.path.ends_with("Family video.mov"))
        .count();
    assert_eq!(names, 1);
}

#[test]
fn symlinks_are_counted_but_never_traversed() {
    let home = home();
    let result = scan_home(&home.root);

    assert_eq!(result.symlinks, count(&home.manifest, EntryKind::Symlink));
    assert_eq!(result.symlinks, 2);
    for rel in [paths::TRAP_SYMLINK_ETC, paths::TRAP_SYMLINK_DOCUMENTS] {
        assert!(result.find(Path::new(rel)).is_none(), "{rel} was entered");
        let link = home.root.join(rel);
        assert!(result
            .largest_files
            .iter()
            .all(|f| !f.path.starts_with(&link)));
    }
    // Nothing outside the root was reached through the /etc link.
    assert!(result
        .largest_files
        .iter()
        .all(|f| f.path.starts_with(&home.root)));
    assert!(result.other_volumes.is_empty());
}

/// Restores a folder's permissions when dropped, so the temp folder can be
/// cleaned up even if an assertion fails.
struct Restore(PathBuf, u32);

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(self.1));
    }
}

#[test]
fn unreadable_folder_is_reported_not_an_error() {
    let home = home();
    let rel = paths::MESSAGES_ATTACHMENTS;
    let folder = home.root.join(rel);
    let mode = std::fs::symlink_metadata(&folder).unwrap().mode();
    // Test setup only: lock a fixture folder inside the temp home.
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o000)).unwrap();
    let _restore = Restore(folder.clone(), mode);
    if std::fs::read_dir(&folder).is_ok() {
        // Running as root: permissions do not stop root reading.
        eprintln!("skipped: running as root, so the folder is still readable");
        return;
    }

    let result = scan_home(&home.root);

    assert_eq!(result.unreadable.len(), 1);
    assert_eq!(result.unreadable[0].path, folder);
    assert_eq!(
        result.unreadable[0].reason,
        UnreadableReason::PermissionDenied
    );
    let id = result.find(Path::new(rel)).unwrap();
    assert!(result.dirs[id].unreadable);
    assert!(result.dirs[id].children.is_empty());
    assert_eq!(result.total_bytes(), expected_allocated(&home, Some(rel)));
}

#[test]
fn cancel_flag_stops_the_scan() {
    let home = home();
    let opts = ScanOptions::default();
    opts.cancel.store(true, Ordering::Relaxed);
    let err = scan(&home.root, opts, |_| {}).unwrap_err();
    assert!(matches!(err, ScanError::Cancelled));
}

#[test]
fn root_must_be_an_absolute_real_folder() {
    let home = home();
    let opts = ScanOptions::default;
    assert!(matches!(
        scan(Path::new("relative/home"), opts(), |_| {}),
        Err(ScanError::RootNotAbsolute(_))
    ));
    let link = home.root.join(paths::TRAP_SYMLINK_DOCUMENTS);
    assert!(matches!(
        scan(&link, opts(), |_| {}),
        Err(ScanError::RootIsSymlink(_))
    ));
    let file = home.root.join(MANIFEST_FILE);
    assert!(matches!(
        scan(&file, opts(), |_| {}),
        Err(ScanError::RootNotAFolder(_))
    ));
}

#[test]
fn largest_files_are_sorted_and_limited() {
    let home = home();
    let opts = ScanOptions {
        largest_files: 5,
        ..ScanOptions::default()
    };
    let result = scan(&home.root, opts, |_| {}).unwrap();
    assert_eq!(result.largest_files.len(), 5);
    assert!(result
        .largest_files
        .windows(2)
        .all(|w| w[0].allocated_bytes >= w[1].allocated_bytes));
    // The first is the largest allocated file in the tree. The manifest
    // file itself is in the tree too, and as the fixture grows it can be
    // the largest file.
    let biggest = home
        .manifest
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::File)
        .map(|e| allocated(&home.root.join(&e.path)))
        .chain([allocated(&home.root.join(MANIFEST_FILE))])
        .max()
        .unwrap();
    assert_eq!(result.largest_files[0].allocated_bytes, biggest);
    for f in &result.largest_files {
        assert_eq!(f.allocated_bytes, allocated(&f.path));
        assert_eq!(
            f.logical_bytes,
            std::fs::symlink_metadata(&f.path).unwrap().len()
        );
    }
}

#[test]
fn progress_ends_with_a_finished_call_matching_the_result() {
    let home = home();
    let calls = Mutex::new(Vec::new());
    let result = scan(&home.root, ScanOptions::default(), |p| {
        calls.lock().unwrap().push(p)
    })
    .unwrap();
    let calls = calls.into_inner().unwrap();
    let last = calls.last().unwrap();
    assert!(last.finished);
    assert_eq!(calls.iter().filter(|p| p.finished).count(), 1);
    assert_eq!(last.bytes, result.total_bytes());
    assert_eq!(last.files, result.total_files());
    assert_eq!(last.dirs, result.dirs.len() as u64);
}

#[test]
fn two_scans_give_the_same_tree() {
    let home = home();
    let a = scan_home(&home.root);
    let b = scan(
        &home.root,
        ScanOptions {
            threads: Some(1),
            ..ScanOptions::default()
        },
        |_| {},
    )
    .unwrap();
    // Folder records match exactly, hard link included, whatever the
    // number of threads.
    assert_eq!(a.dirs.len(), b.dirs.len());
    assert_eq!(a.total_bytes(), b.total_bytes());
    for (x, y) in a.dirs.iter().zip(&b.dirs) {
        assert_eq!(x.name, y.name);
        assert_eq!(x.parent, y.parent);
        assert_eq!(x.children, y.children);
        assert_eq!(x.total_bytes, y.total_bytes, "{:?}", x.name);
        assert_eq!(x.total_logical_bytes, y.total_logical_bytes, "{:?}", x.name);
        assert_eq!(x.total_files, y.total_files, "{:?}", x.name);
    }
}

/// On Linux, `/dev` holds other mounts (`/dev/pts`, `/dev/shm`). The scanner
/// must list them as other volumes and not enter them.
#[cfg(target_os = "linux")]
#[test]
fn other_volumes_are_not_entered() {
    let dev = Path::new("/dev");
    let Ok(dev_meta) = std::fs::symlink_metadata(dev) else {
        return;
    };
    let mounts: Vec<PathBuf> = ["pts", "shm", "mqueue"]
        .iter()
        .map(|n| dev.join(n))
        .filter(|p| {
            std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && m.dev() != dev_meta.dev())
        })
        .collect();
    if mounts.is_empty() {
        eprintln!("skipped: no mounts under /dev here");
        return;
    }
    let result = scan(dev, ScanOptions::default(), |_| {}).unwrap();
    for m in &mounts {
        assert!(
            result.other_volumes.contains(m),
            "{} not listed",
            m.display()
        );
        let rel = m.strip_prefix(dev).unwrap();
        assert!(result.find(rel).is_none(), "{} was entered", m.display());
    }
}

#[test]
fn records_files_only_in_folders_asked_for() {
    let home = home();
    let want = scanner::RecordFiles(std::sync::Arc::new(|rel: &Path| {
        rel == Path::new(paths::DOWNLOADS)
    }));
    let opts = ScanOptions {
        record_files: Some(want),
        ..ScanOptions::default()
    };
    let result = scan(&home.root, opts, |_| {}).unwrap();
    let downloads = result.find(Path::new(paths::DOWNLOADS)).unwrap();
    let names: Vec<String> = result.dirs[downloads]
        .files
        .iter()
        .map(|f| f.name.to_string_lossy().into_owned())
        .collect();
    let mut expected: Vec<String> = [
        paths::DOWNLOAD_DMG,
        paths::DOWNLOAD_NORMAL,
        paths::DOWNLOAD_ZIP,
        paths::DOWNLOAD_PKG,
    ]
    .iter()
    .map(|p| p.rsplit('/').next().unwrap().to_string())
    .collect();
    expected.sort();
    assert_eq!(names, expected);
    let dmg = &result.dirs[downloads].files[0];
    assert_eq!(
        dmg.allocated_bytes,
        allocated(&home.root.join(paths::DOWNLOAD_DMG))
    );
    // No other folder lists its files.
    let others = result
        .dirs
        .iter()
        .enumerate()
        .filter(|(i, d)| *i != downloads && !d.files.is_empty())
        .count();
    assert_eq!(others, 0);
    // Without the option nothing is listed.
    assert!(scan_home(&home.root)
        .dirs
        .iter()
        .all(|d| d.files.is_empty()));
}

#[test]
fn marks_folders_that_hold_git() {
    let home = home();
    let result = scan_home(&home.root);
    let repo = Path::new(paths::TRAP_GIT_DIR).parent().unwrap();
    let mut want = vec![
        home.root.join(repo),
        home.root.join(paths::RUST_PROJECTS[1]),
        home.root.join(paths::CARGO_GIT_CHECKOUT),
    ];
    want.sort();
    let mut with_git: Vec<PathBuf> = result
        .dirs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.has_git)
        .map(|(i, _)| result.path_of(i).unwrap())
        .collect();
    with_git.sort();
    assert_eq!(with_git, want);
}

#[test]
fn keeps_largest_files_only_from_folders_allowed() {
    let home = home();
    // Only files directly in Downloads, or below it, may be kept by path.
    let allow = scanner::RecordFiles(std::sync::Arc::new(|rel: &Path| {
        rel.starts_with(paths::DOWNLOADS)
    }));
    let opts = ScanOptions {
        largest_in: Some(allow),
        ..ScanOptions::default()
    };
    let limited = scan(&home.root, opts, |_| {}).unwrap();
    let everything = scan_home(&home.root);
    assert!(!limited.largest_files.is_empty());
    let downloads = home.root.join(paths::DOWNLOADS);
    assert!(limited
        .largest_files
        .iter()
        .all(|f| f.path.starts_with(&downloads)));
    // Files elsewhere were kept without the option.
    assert!(everything
        .largest_files
        .iter()
        .any(|f| !f.path.starts_with(&downloads)));
    // Sizes are the same either way: the option only changes the list.
    assert_eq!(limited.total_bytes(), everything.total_bytes());
}
