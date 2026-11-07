//! The duplicate finder's check in the executor: a copy moves to the Trash
//! only while the copy the user keeps is still there, unchanged, and the
//! same byte for byte. Every test works in a fake home in a temp folder.

use std::fs;
use std::os::unix::fs::{symlink, MetadataExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use executor::{execute, preview, Guards, ItemRefusal, Mode, Outcome, Running, TRASH_DIR};
use planner::{Expected, ItemRequest, KeepCopy, Plan, PlanError};

/// The extra checks with the recent-change window off, as in `execute.rs`.
fn quiet() -> Guards {
    Guards {
        recent_window: Duration::ZERO,
        protected_folders: Vec::new(),
        running: Running::Known(Vec::new()),
    }
}

/// A fake home with a Trash and two identical files in Documents.
struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
    dev: u64,
}

const DATA_LEN: usize = 3 * 1024 * 1024 + 17;

fn data() -> Vec<u8> {
    (0..DATA_LEN).map(|i| (i % 251) as u8).collect()
}

impl Home {
    fn new() -> Home {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(root.join(TRASH_DIR)).unwrap();
        fs::create_dir_all(root.join("Documents/Trips")).unwrap();
        fs::create_dir_all(root.join("Downloads")).unwrap();
        fs::write(root.join("Documents/Trips/beach.mov"), data()).unwrap();
        fs::write(root.join("Downloads/beach.mov"), data()).unwrap();
        let dev = root.symlink_metadata().unwrap().dev();
        Home {
            _dir: dir,
            root,
            dev,
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// The kept copy, as the duplicate finder saw it.
    fn keep(&self, rel: &str) -> KeepCopy {
        let meta = self.path(rel).symlink_metadata().unwrap();
        KeepCopy {
            path: self.path(rel),
            dev: meta.dev(),
            ino: meta.ino(),
            len: meta.len(),
            modified: meta.modified().unwrap(),
        }
    }

    /// A request to move `rel`, keeping `keep`.
    fn request(&self, rel: &str, keep: KeepCopy) -> ItemRequest {
        let meta = self.path(rel).symlink_metadata().unwrap();
        ItemRequest {
            recommendation_id: rel.to_string(),
            path: self.path(rel),
            expected: Expected {
                size_bytes: meta.blocks() * 512,
                modified: meta.modified().unwrap(),
            },
            regenerable: false,
            selected_individually: true,
            owners: &[],
            belongs_to: None,
            keep_copy: Some(keep),
        }
    }

    /// Moves the Downloads copy, keeping the Documents one, and returns
    /// what happened to it.
    fn run(&self, request: ItemRequest) -> Outcome {
        let plan = Plan::build(1, &self.root, self.dev, vec![request], SystemTime::now()).unwrap();
        let report = execute(plan, Mode::Trash, &quiet(), SystemTime::now()).unwrap();
        report.items[0].outcome.clone()
    }

    fn standard_request(&self) -> ItemRequest {
        self.request(
            "Downloads/beach.mov",
            self.keep("Documents/Trips/beach.mov"),
        )
    }
}

/// Writes `bytes` over `path` and puts its modification time back, so only
/// the content check can tell.
fn tamper(path: &Path, bytes: &[u8]) {
    let modified = path.symlink_metadata().unwrap().modified().unwrap();
    fs::write(path, bytes).unwrap();
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

#[test]
fn an_identical_copy_moves_and_the_kept_copy_stays() {
    let home = Home::new();
    let outcome = home.run(home.standard_request());
    assert!(
        matches!(outcome, Outcome::MovedToTrash { .. }),
        "{outcome:?}"
    );
    assert!(!home.path("Downloads/beach.mov").exists());
    assert_eq!(
        fs::read(home.path("Documents/Trips/beach.mov")).unwrap(),
        data()
    );
    assert_eq!(fs::read(home.path(".Trash/beach.mov")).unwrap(), data());
}

#[test]
fn the_dry_run_runs_the_same_check() {
    let home = Home::new();
    let request = home.standard_request();
    let mut bytes = data();
    bytes[DATA_LEN / 2] ^= 1;
    tamper(&home.path("Downloads/beach.mov"), &bytes);
    let plan = Plan::build(1, &home.root, home.dev, vec![request], SystemTime::now()).unwrap();
    let checks = preview(&plan, Mode::Trash, &quiet(), SystemTime::now()).unwrap();
    assert_eq!(checks[0].result, Err(ItemRefusal::NotSameAsKeptCopy));
    assert!(home.path("Downloads/beach.mov").exists());
}

#[test]
fn a_copy_that_now_differs_stays_even_with_the_same_size_and_date() {
    let home = Home::new();
    let request = home.standard_request();
    let mut bytes = data();
    *bytes.last_mut().unwrap() ^= 1;
    tamper(&home.path("Downloads/beach.mov"), &bytes);
    let outcome = home.run(request);
    assert_eq!(outcome, Outcome::Refused(ItemRefusal::NotSameAsKeptCopy));
    assert_eq!(fs::read(home.path("Downloads/beach.mov")).unwrap(), bytes);
}

#[test]
fn nothing_moves_if_the_kept_copy_changed() {
    let home = Home::new();
    let request = home.standard_request();
    // Edited (new date): refused.
    fs::write(home.path("Documents/Trips/beach.mov"), b"edited").unwrap();
    let outcome = home.run(request);
    assert_eq!(outcome, Outcome::Refused(ItemRefusal::KeptCopyChanged));
    assert!(home.path("Downloads/beach.mov").exists());
}

#[test]
fn nothing_moves_if_the_kept_copy_was_edited_back_to_its_old_date() {
    let home = Home::new();
    let request = home.standard_request();
    let mut bytes = data();
    bytes[0] ^= 1;
    tamper(&home.path("Documents/Trips/beach.mov"), &bytes);
    let outcome = home.run(request);
    assert_eq!(outcome, Outcome::Refused(ItemRefusal::NotSameAsKeptCopy));
    assert!(home.path("Downloads/beach.mov").exists());
}

#[test]
fn nothing_moves_if_the_kept_copy_is_gone_or_replaced() {
    // Gone.
    let home = Home::new();
    let request = home.standard_request();
    fs::remove_file(home.path("Documents/Trips/beach.mov")).unwrap();
    assert_eq!(
        home.run(request),
        Outcome::Refused(ItemRefusal::KeptCopyChanged)
    );
    assert!(home.path("Downloads/beach.mov").exists());

    // Replaced by a new file with the same content and date.
    let home = Home::new();
    let request = home.standard_request();
    let kept = home.path("Documents/Trips/beach.mov");
    let modified = kept.symlink_metadata().unwrap().modified().unwrap();
    // The new file is written before the old one goes, so the file system
    // cannot hand it the old inode number.
    fs::write(home.path("Documents/other.mov"), data()).unwrap();
    fs::remove_file(&kept).unwrap();
    fs::rename(home.path("Documents/other.mov"), &kept).unwrap();
    fs::File::options()
        .write(true)
        .open(&kept)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(
        home.run(request),
        Outcome::Refused(ItemRefusal::KeptCopyChanged)
    );
    assert!(home.path("Downloads/beach.mov").exists());
}

#[test]
fn nothing_moves_if_the_kept_copy_is_now_a_symlink() {
    let home = Home::new();
    let request = home.standard_request();
    let kept = home.path("Documents/Trips/beach.mov");
    fs::rename(&kept, home.path("Documents/moved.mov")).unwrap();
    symlink(home.path("Documents/moved.mov"), &kept).unwrap();
    assert_eq!(
        home.run(request),
        Outcome::Refused(ItemRefusal::KeptCopyChanged)
    );
    assert!(home.path("Downloads/beach.mov").exists());
}

#[test]
fn nothing_moves_if_a_folder_above_the_kept_copy_is_now_a_symlink() {
    let home = Home::new();
    let request = home.standard_request();
    fs::rename(
        home.path("Documents/Trips"),
        home.path("Documents/Elsewhere"),
    )
    .unwrap();
    symlink(
        home.path("Documents/Elsewhere"),
        home.path("Documents/Trips"),
    )
    .unwrap();
    assert_eq!(
        home.run(request),
        Outcome::Refused(ItemRefusal::KeptCopyChanged)
    );
    assert!(home.path("Downloads/beach.mov").exists());
}

#[test]
fn a_hard_link_is_never_kept_as_a_copy_of_itself() {
    let home = Home::new();
    fs::hard_link(
        home.path("Downloads/beach.mov"),
        home.path("Documents/same file.mov"),
    )
    .unwrap();
    let request = home.request("Downloads/beach.mov", home.keep("Documents/same file.mov"));
    assert_eq!(
        home.run(request),
        Outcome::Refused(ItemRefusal::NotSameAsKeptCopy)
    );
    assert!(home.path("Downloads/beach.mov").exists());
}

#[test]
fn a_kept_copy_outside_home_is_refused() {
    let home = Home::new();
    let outside = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(outside.path()).unwrap().join("beach.mov");
    fs::write(&path, data()).unwrap();
    let meta = path.symlink_metadata().unwrap();
    let keep = KeepCopy {
        path,
        dev: meta.dev(),
        ino: meta.ino(),
        len: meta.len(),
        modified: meta.modified().unwrap(),
    };
    let request = home.request("Downloads/beach.mov", keep);
    assert_eq!(
        home.run(request),
        Outcome::Refused(ItemRefusal::KeptCopyChanged)
    );
    assert!(home.path("Downloads/beach.mov").exists());
}

#[test]
fn the_kept_copy_can_never_be_in_the_same_plan() {
    let home = Home::new();
    let a = home.request(
        "Downloads/beach.mov",
        home.keep("Documents/Trips/beach.mov"),
    );
    let b = home.request(
        "Documents/Trips/beach.mov",
        home.keep("Downloads/beach.mov"),
    );
    let err = Plan::build(1, &home.root, home.dev, vec![a, b], SystemTime::now()).unwrap_err();
    assert!(matches!(err, PlanError::KeepCopyMoves { .. }), "{err:?}");
}

#[test]
fn moving_two_copies_that_keep_the_same_third_works() {
    let home = Home::new();
    fs::write(home.path("Documents/third.mov"), data()).unwrap();
    let keep = home.keep("Documents/Trips/beach.mov");
    let a = home.request("Downloads/beach.mov", keep.clone());
    let b = home.request("Documents/third.mov", keep);
    let plan = Plan::build(1, &home.root, home.dev, vec![a, b], SystemTime::now()).unwrap();
    let report = execute(plan, Mode::Trash, &quiet(), SystemTime::now()).unwrap();
    assert!(report
        .items
        .iter()
        .all(|i| matches!(i.outcome, Outcome::MovedToTrash { .. })));
    assert_eq!(
        fs::read(home.path("Documents/Trips/beach.mov")).unwrap(),
        data()
    );
}
