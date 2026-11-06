//! The duplicate finder against a temporary home: the search, the dry run,
//! the move to the Trash, put back and History. See the duplicate finder
//! section of `docs/command-contract.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use roomy_ffi::{
    CleanupCheck, CoreError, DuplicateGroup, DuplicateReport, DuplicateSearch, Mode, RoomyCore,
};

const MIB: usize = 1024 * 1024;

/// A temporary home with a Trash and a core looking at it.
struct Setup {
    _dir: tempfile::TempDir,
    home: PathBuf,
    core: Arc<RoomyCore>,
}

/// `len` bytes that depend on `seed`.
fn bytes(seed: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31) ^ seed)
        .collect()
}

fn write(home: &Path, rel: &str, data: &[u8]) {
    let path = home.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    let home = base.join("home");
    fs::create_dir_all(home.join(".Trash")).unwrap();
    fs::create_dir_all(home.join("Library/Application Support")).unwrap();
    let movie = bytes(1, 3 * MIB);
    write(&home, "Movies/holiday.mov", &movie);
    write(&home, "Downloads/holiday.mov", &movie);
    write(&home, "Desktop/holiday copy.mov", &movie);
    let photo = bytes(2, MIB + 5);
    write(&home, "Pictures/cat.jpg", &photo);
    write(&home, "Documents/Old/cat.jpg", &photo);
    // Same size as the photo, different content: not a copy.
    write(&home, "Documents/dog.jpg", &bytes(3, MIB + 5));
    // Copies in Library are never looked at.
    write(&home, "Library/Caches/a.bin", &movie);
    // A copy in a code project is never offered.
    write(&home, "Documents/app/.git/HEAD", b"ref");
    write(&home, "Documents/app/holiday.mov", &movie);
    let core = RoomyCore::with_home(home.clone()).unwrap();
    core.set_recent_change_window_for_testing(Duration::ZERO);
    core.set_running_programs_for_testing(Some(Vec::new()));
    Setup {
        _dir: dir,
        home,
        core,
    }
}

/// Runs a search and waits for it to finish.
fn search(core: &RoomyCore, mode: Mode) -> DuplicateReport {
    core.start_duplicate_search(mode);
    let start = Instant::now();
    loop {
        match core.duplicate_search_progress() {
            DuplicateSearch::Finished { report } => return report,
            DuplicateSearch::Looking { .. } | DuplicateSearch::Comparing { .. } => {}
            other => panic!("search ended as {other:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(30), "search hung");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The group holding a copy called `name`. A group is named after its
/// oldest copy, and the test files may all have the same date.
fn group<'a>(report: &'a DuplicateReport, name: &str) -> &'a DuplicateGroup {
    report
        .groups
        .iter()
        .find(|g| g.copies.iter().any(|c| c.name == name))
        .unwrap()
}

fn locations(g: &DuplicateGroup) -> Vec<String> {
    let mut l: Vec<String> = g.copies.iter().map(|c| c.location.clone()).collect();
    l.sort();
    l
}

/// Every file under `root` (Trash and History left out), with its content.
fn contents(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            if rel.starts_with(".Trash") || rel.starts_with("Library/Application Support") {
                continue;
            }
            if p.symlink_metadata().unwrap().is_dir() {
                stack.push(p);
            } else {
                out.push((rel, fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn finds_copies_by_content_in_the_users_own_folders_only() {
    let s = setup();
    let report = search(&s.core, Mode::Simple);
    assert_eq!(report.groups.len(), 2, "{report:?}");
    assert_eq!(
        locations(group(&report, "holiday.mov")),
        ["Desktop", "Downloads", "Movies"]
    );
    assert_eq!(
        locations(group(&report, "cat.jpg")),
        ["Documents \u{203a} Old", "Pictures"]
    );
    // Simple mode shows no paths; every copy can be chosen.
    for g in &report.groups {
        assert!(g.copies.iter().all(|c| c.path.is_none() && c.can_move));
    }
    // Keeping one copy of each frees the rest.
    let movie = group(&report, "holiday.mov");
    assert_eq!(movie.could_free_bytes, 2 * movie.size_bytes);
    assert_eq!(
        report.could_free_bytes,
        report
            .groups
            .iter()
            .map(|g| g.could_free_bytes)
            .sum::<u64>()
    );
    assert!(report.folders.contains(&"Documents".to_string()));
}

#[test]
fn developer_mode_shows_paths() {
    let s = setup();
    let report = search(&s.core, Mode::Developer);
    let paths: Vec<String> = group(&report, "cat.jpg")
        .copies
        .iter()
        .map(|c| c.path.clone().unwrap())
        .collect();
    assert!(
        paths.contains(&"~/Pictures/cat.jpg".to_string()),
        "{paths:?}"
    );
}

#[test]
fn moves_only_the_ticked_copies_keeps_one_and_can_put_back() {
    let s = setup();
    let report = search(&s.core, Mode::Simple);
    let before = contents(&s.home);
    let movie = group(&report, "holiday.mov");
    let ids: Vec<String> = movie.copies[1..]
        .iter()
        .map(|c| c.copy_id.clone())
        .collect();
    let kept_location = movie.copies[0].location.clone();
    let preview = s.core.preview_duplicate_cleanup(ids.clone()).unwrap();
    assert_eq!(contents(&s.home), before, "the dry run moved something");
    assert!(
        preview
            .items
            .iter()
            .all(|i| i.check == CleanupCheck::WillMove),
        "{:?}",
        preview.items
    );
    assert_eq!(preview.bytes_to_move, movie.could_free_bytes);
    let result = s.core.run_cleanup(preview.plan.unwrap()).unwrap();
    assert!(result.items.iter().all(|i| i.done), "{:?}", result.items);
    // Exactly one copy is left, and it is the one not ticked.
    let left: Vec<&str> = ["Movies", "Downloads", "Desktop"]
        .into_iter()
        .filter(|f| {
            fs::read_dir(s.home.join(f))
                .unwrap()
                .any(|e| e.unwrap().file_name().to_string_lossy().contains("holiday"))
        })
        .collect();
    assert_eq!(left, [kept_location.as_str()]);
    // The photo, the dog and the project copy are untouched.
    assert!(s.home.join("Pictures/cat.jpg").exists());
    assert!(s.home.join("Documents/Old/cat.jpg").exists());
    assert!(s.home.join("Documents/app/holiday.mov").exists());
    // History names the file.
    assert!(result.saved_to_history);
    let history = s.core.cleanup_history(10);
    assert!(history[0]
        .items
        .iter()
        .all(|i| i.title == format!("Copy of {}", movie.name)));
    // Put back restores every file exactly.
    let back = s.core.put_back_cleanup(result.cleanup.unwrap()).unwrap();
    assert!(back.items.iter().all(|i| i.done), "{:?}", back.items);
    assert_eq!(contents(&s.home), before);
}

#[test]
fn ticking_every_copy_is_refused_and_nothing_moves() {
    let s = setup();
    let report = search(&s.core, Mode::Simple);
    let before = contents(&s.home);
    let cat = group(&report, "cat.jpg");
    let all: Vec<String> = cat.copies.iter().map(|c| c.copy_id.clone()).collect();
    let err = s.core.preview_duplicate_cleanup(all).unwrap_err();
    match err {
        CoreError::CleanupUnavailable { reason } => {
            assert!(reason.contains("Keep at least one copy"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(contents(&s.home), before);
}

#[test]
fn a_kept_copy_changed_after_the_review_keeps_both() {
    let s = setup();
    let report = search(&s.core, Mode::Simple);
    let cat = group(&report, "cat.jpg");
    // Copies are oldest first, so the first one is kept.
    let kept = if cat.copies[0].location == "Pictures" {
        s.home.join("Pictures/cat.jpg")
    } else {
        s.home.join("Documents/Old/cat.jpg")
    };
    let preview = s
        .core
        .preview_duplicate_cleanup(vec![cat.copies[1].copy_id.clone()])
        .unwrap();
    fs::write(&kept, b"edited after the review").unwrap();
    let result = s.core.run_cleanup(preview.plan.unwrap()).unwrap();
    assert!(!result.items[0].done);
    assert!(s.home.join("Pictures/cat.jpg").exists());
    assert!(s.home.join("Documents/Old/cat.jpg").exists());
}

#[test]
fn a_copy_that_differs_by_review_time_is_shown_as_staying() {
    let s = setup();
    let report = search(&s.core, Mode::Simple);
    let cat = group(&report, "cat.jpg");
    let moving = if cat.copies[1].location == "Pictures" {
        s.home.join("Pictures/cat.jpg")
    } else {
        s.home.join("Documents/Old/cat.jpg")
    };
    fs::write(&moving, bytes(9, MIB + 5)).unwrap();
    let preview = s
        .core
        .preview_duplicate_cleanup(vec![cat.copies[1].copy_id.clone()])
        .unwrap();
    assert!(preview.plan.is_none());
    assert!(matches!(
        preview.items[0].check,
        CleanupCheck::WontMove { .. }
    ));
}

#[test]
fn ids_from_an_older_search_are_refused() {
    let s = setup();
    let old = search(&s.core, Mode::Simple);
    let id = old.groups[0].copies[1].copy_id.clone();
    search(&s.core, Mode::Simple);
    assert!(matches!(
        s.core.preview_duplicate_cleanup(vec![id]),
        Err(CoreError::UnknownItem)
    ));
}

#[test]
fn nothing_can_be_previewed_before_a_search() {
    let s = setup();
    assert_eq!(
        s.core.duplicate_search_progress(),
        DuplicateSearch::NotStarted
    );
    assert!(matches!(
        s.core.preview_duplicate_cleanup(vec!["dup-1-0-0".into()]),
        Err(CoreError::CleanupUnavailable { .. })
    ));
}

#[test]
fn a_cancelled_search_stays_cancelled() {
    let s = setup();
    s.core.start_duplicate_search(Mode::Simple);
    s.core.cancel_duplicate_search();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        s.core.duplicate_search_progress(),
        DuplicateSearch::Cancelled
    );
}
