//! Executor tests. Every test works in a fake home in a temp folder, never the
//! real home. Expected sizes come from the real scanner, so these tests also
//! prove the executor measures items the same way the scan does.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{symlink, MetadataExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use executor::{execute, ExecuteError, ItemRefusal, Mode, Outcome, Report, TRASH_DIR};
use planner::{Expected, ItemRequest, Plan, PlanError, PLAN_LIFETIME};
use safety::protected::{BlockReason, ReviewReason};
use safety::Refusal;
use sha2::{Digest, Sha256};

/// A fake home in a temp folder, with a Trash.
struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
    dev: u64,
}

impl Home {
    /// An almost empty home: `.Trash`, `Library/Caches`, `Documents`.
    fn plain() -> Home {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        for d in [TRASH_DIR, "Library/Caches", "Documents"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let dev = root.symlink_metadata().unwrap().dev();
        Home {
            _dir: dir,
            root,
            dev,
        }
    }

    /// A full fixture home (with its traps) plus a Trash, and its manifest.
    fn fixture() -> (Home, fixtures::Manifest) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let manifest = fixtures::build(&root, SystemTime::now()).unwrap();
        fs::create_dir(root.join(TRASH_DIR)).unwrap();
        let dev = root.symlink_metadata().unwrap().dev();
        (
            Home {
                _dir: dir,
                root,
                dev,
            },
            manifest,
        )
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Writes a file of `len` bytes, making its folders.
    fn file(&self, rel: &str, len: usize) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; len]).unwrap();
    }

    /// What the user reviewed: size from a real scan, mtime from lstat.
    fn expected(&self, rel: &str) -> Expected {
        let path = self.path(rel);
        let meta = path.symlink_metadata().unwrap();
        let size_bytes = if meta.is_dir() {
            let scan = scanner::scan(&self.root, scanner::ScanOptions::default(), |_| {}).unwrap();
            let id = scan.find(Path::new(rel)).unwrap();
            scan.dirs[id].total_bytes
        } else {
            meta.blocks() * 512
        };
        Expected {
            size_bytes,
            modified: meta.modified().unwrap(),
        }
    }

    fn request(&self, rel: &str) -> ItemRequest {
        ItemRequest {
            recommendation_id: rel.to_string(),
            path: self.path(rel),
            expected: self.expected(rel),
            regenerable: true,
            selected_individually: false,
        }
    }

    fn plan(&self, requests: Vec<ItemRequest>) -> Plan {
        Plan::build(1, &self.root, self.dev, requests, SystemTime::now()).unwrap()
    }

    fn plan_of(&self, rels: &[&str]) -> Plan {
        self.plan(rels.iter().map(|r| self.request(r)).collect())
    }

    /// Names directly in the Trash.
    fn trash_names(&self) -> BTreeSet<String> {
        fs::read_dir(self.path(TRASH_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect()
    }
}

fn run(plan: Plan, mode: Mode) -> Report {
    execute(plan, mode, SystemTime::now()).unwrap()
}

fn refusal(report: &Report, i: usize) -> &ItemRefusal {
    match &report.items[i].outcome {
        Outcome::Refused(r) => r,
        other => panic!("item {i} was not refused: {other:?}"),
    }
}

fn sha(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

// ---------------------------------------------------------------------------
// Moving to the Trash
// ---------------------------------------------------------------------------

#[test]
fn trash_moves_a_cache_folder_whole() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/a.bin", 10_000);
    home.file("Library/Caches/com.example/sub/b.bin", 3_000);
    let before = sha(&home.path("Library/Caches/com.example/sub/b.bin"));
    let expected = home.expected("Library/Caches/com.example").size_bytes;

    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Trash);

    let to = home.path(".Trash/com.example");
    assert_eq!(
        report.items[0].outcome,
        Outcome::MovedToTrash { to: to.clone() }
    );
    assert_eq!(report.total_bytes(), expected);
    assert!(!home.path("Library/Caches/com.example").exists());
    assert_eq!(sha(&to.join("sub/b.bin")), before);
}

#[test]
fn trash_moves_a_single_file() {
    let home = Home::plain();
    home.file("Library/Caches/big.log", 50_000);
    let report = run(home.plan_of(&["Library/Caches/big.log"]), Mode::Trash);
    assert_eq!(
        report.items[0].outcome,
        Outcome::MovedToTrash {
            to: home.path(".Trash/big.log")
        }
    );
}

#[test]
fn trash_never_replaces_what_is_already_there() {
    let home = Home::plain();
    home.file(".Trash/notes.log", 10);
    home.file(".Trash/notes 2.log", 20);
    home.file("Library/Caches/notes.log", 30);
    let report = run(home.plan_of(&["Library/Caches/notes.log"]), Mode::Trash);
    assert_eq!(
        report.items[0].outcome,
        Outcome::MovedToTrash {
            to: home.path(".Trash/notes 3.log")
        }
    );
    assert_eq!(fs::read(home.path(".Trash/notes.log")).unwrap().len(), 10);
    assert_eq!(fs::read(home.path(".Trash/notes 2.log")).unwrap().len(), 20);
    assert_eq!(fs::read(home.path(".Trash/notes 3.log")).unwrap().len(), 30);
}

#[test]
fn a_symlink_inside_a_folder_is_moved_as_a_link_never_followed() {
    let home = Home::plain();
    home.file("Documents/thesis.txt", 4_000);
    home.file("Library/Caches/com.example/data.bin", 1_000);
    symlink(
        "../../../Documents",
        home.path("Library/Caches/com.example/docs"),
    )
    .unwrap();
    let thesis = sha(&home.path("Documents/thesis.txt"));

    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Trash);

    assert!(matches!(
        report.items[0].outcome,
        Outcome::MovedToTrash { .. }
    ));
    let moved_link = home.path(".Trash/com.example/docs");
    assert!(moved_link
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(sha(&home.path("Documents/thesis.txt")), thesis);
}

#[test]
fn hard_links_are_counted_once_like_the_scanner() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/a.bin", 40_000);
    fs::hard_link(
        home.path("Library/Caches/com.example/a.bin"),
        home.path("Library/Caches/com.example/b.bin"),
    )
    .unwrap();
    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Trash);
    assert!(matches!(
        report.items[0].outcome,
        Outcome::MovedToTrash { .. }
    ));
}

#[test]
fn one_refused_item_does_not_stop_the_others() {
    let home = Home::plain();
    home.file("Library/Caches/one/a", 1_000);
    home.file("Library/Caches/two/a", 1_000);
    let plan = home.plan_of(&["Library/Caches/one", "Library/Caches/two"]);
    home.file("Library/Caches/one/grew", 100_000);

    let report = run(plan, Mode::Trash);

    assert_eq!(refusal(&report, 0), &ItemRefusal::ChangedSinceReview);
    assert!(matches!(
        report.items[1].outcome,
        Outcome::MovedToTrash { .. }
    ));
    assert!(home.path("Library/Caches/one/a").exists());
}

// ---------------------------------------------------------------------------
// Checks immediately before acting
// ---------------------------------------------------------------------------

#[test]
fn a_parent_swapped_for_a_symlink_after_review_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 1_000);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    // Swap the Caches folder for a symlink to a look-alike in Documents.
    home.file("Documents/Caches/com.example/data", 1_000);
    fs::rename(home.path("Library/Caches"), home.path("Library/Caches-old")).unwrap();
    symlink(home.path("Documents/Caches"), home.path("Library/Caches")).unwrap();

    let report = run(plan, Mode::Trash);

    assert!(
        matches!(
            refusal(&report, 0),
            ItemRefusal::Safety(Refusal::Symlink { .. })
        ),
        "{report:?}"
    );
    assert!(home.path("Documents/Caches/com.example/data").exists());
}

#[test]
fn an_item_swapped_for_a_symlink_after_review_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 1_000);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    fs::rename(
        home.path("Library/Caches/com.example"),
        home.path("Library/Caches/elsewhere"),
    )
    .unwrap();
    symlink(
        home.path("Documents"),
        home.path("Library/Caches/com.example"),
    )
    .unwrap();

    let report = run(plan, Mode::Trash);

    assert!(matches!(
        refusal(&report, 0),
        ItemRefusal::Safety(Refusal::Symlink { .. })
    ));
    assert!(home.path("Documents").is_dir());
}

#[test]
fn a_size_change_of_more_than_one_percent_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 100_000);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    home.file("Library/Caches/com.example/data", 300_000);
    let report = run(plan, Mode::Trash);
    assert_eq!(refusal(&report, 0), &ItemRefusal::ChangedSinceReview);
    assert!(home.path("Library/Caches/com.example/data").exists());
}

#[test]
fn a_modification_time_change_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 1_000);
    let mut req = home.request("Library/Caches/com.example");
    req.expected.modified -= Duration::from_secs(60);
    let report = run(home.plan(vec![req]), Mode::Trash);
    assert_eq!(refusal(&report, 0), &ItemRefusal::ChangedSinceReview);
}

#[test]
fn an_item_that_vanished_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 1_000);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    fs::rename(
        home.path("Library/Caches/com.example"),
        home.path("Library/Caches/gone"),
    )
    .unwrap();
    let report = run(plan, Mode::Trash);
    assert!(matches!(
        refusal(&report, 0),
        ItemRefusal::Safety(Refusal::Unreadable { .. })
    ));
}

#[test]
fn a_git_repository_inside_a_folder_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/clone/.git/HEAD", 10);
    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Trash);
    assert!(matches!(
        refusal(&report, 0),
        ItemRefusal::ContainsProtected {
            reason: BlockReason::GitRepository,
            ..
        }
    ));
    assert!(home
        .path("Library/Caches/com.example/clone/.git/HEAD")
        .exists());
}

#[test]
fn a_photos_library_inside_a_folder_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/Old.photoslibrary/db", 10);
    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Trash);
    assert!(matches!(
        refusal(&report, 0),
        ItemRefusal::ContainsProtected {
            reason: BlockReason::MediaLibrary,
            ..
        }
    ));
}

#[test]
fn a_signing_key_inside_a_folder_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/deep/er/release.keystore", 10);
    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Trash);
    assert!(matches!(
        refusal(&report, 0),
        ItemRefusal::ContainsProtected {
            reason: BlockReason::SigningMaterial,
            ..
        }
    ));
}

#[test]
fn a_review_only_file_inside_needs_the_item_chosen_on_its_own() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/.env", 10);

    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Trash);
    assert!(matches!(
        refusal(&report, 0),
        ItemRefusal::ContainsReviewOnly {
            reason: ReviewReason::EnvFile,
            ..
        }
    ));

    let mut req = home.request("Library/Caches/com.example");
    req.selected_individually = true;
    let report = run(home.plan(vec![req]), Mode::Trash);
    assert!(matches!(
        report.items[0].outcome,
        Outcome::MovedToTrash { .. }
    ));
}

// ---------------------------------------------------------------------------
// Whole-plan refusals: nothing is touched
// ---------------------------------------------------------------------------

#[test]
fn an_expired_plan_touches_nothing() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 1_000);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    let later = SystemTime::now() + PLAN_LIFETIME;
    let err = execute(plan, Mode::Trash, later).unwrap_err();
    assert!(matches!(err, ExecuteError::Plan(PlanError::Expired)));
    assert!(home.path("Library/Caches/com.example/data").exists());
}

#[test]
fn delete_is_refused_unless_every_item_is_regenerable() {
    let home = Home::plain();
    home.file("Library/Caches/one/a", 1_000);
    home.file("Library/Caches/two/a", 1_000);
    let mut two = home.request("Library/Caches/two");
    two.regenerable = false;
    let plan = home.plan(vec![home.request("Library/Caches/one"), two]);
    let err = execute(plan, Mode::Delete, SystemTime::now()).unwrap_err();
    assert!(matches!(err, ExecuteError::DeleteNeedsRegenerable));
    assert!(home.path("Library/Caches/one/a").exists());
    assert!(home.path("Library/Caches/two/a").exists());
}

#[test]
fn a_missing_trash_touches_nothing() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 1_000);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    fs::remove_dir(home.path(TRASH_DIR)).unwrap();
    let err = execute(plan, Mode::Trash, SystemTime::now()).unwrap_err();
    assert!(matches!(err, ExecuteError::TrashUnavailable));
    assert!(home.path("Library/Caches/com.example/data").exists());
}

#[test]
fn a_trash_that_is_a_symlink_touches_nothing() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 1_000);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    fs::remove_dir(home.path(TRASH_DIR)).unwrap();
    symlink(home.path("Documents"), home.path(TRASH_DIR)).unwrap();
    let err = execute(plan, Mode::Trash, SystemTime::now()).unwrap_err();
    assert!(matches!(err, ExecuteError::TrashUnavailable));
    assert!(home.path("Library/Caches/com.example/data").exists());
    assert_eq!(fs::read_dir(home.path("Documents")).unwrap().count(), 0);
}

// ---------------------------------------------------------------------------
// Permanent delete
// ---------------------------------------------------------------------------

#[test]
fn delete_removes_the_item_and_leaves_nothing_behind() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/a.bin", 10_000);
    home.file("Library/Caches/com.example/sub/b.bin", 3_000);
    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Delete);
    assert_eq!(report.items[0].outcome, Outcome::Deleted);
    assert!(report.total_bytes() > 0);
    assert!(!home.path("Library/Caches/com.example").exists());
    assert!(home.trash_names().is_empty(), "{:?}", home.trash_names());
}

#[test]
fn delete_never_follows_a_symlink_inside_the_item() {
    let home = Home::plain();
    home.file("Documents/thesis.txt", 4_000);
    home.file("Library/Caches/com.example/data.bin", 1_000);
    symlink(
        home.path("Documents"),
        home.path("Library/Caches/com.example/docs"),
    )
    .unwrap();
    symlink(
        home.path("Documents/thesis.txt"),
        home.path("Library/Caches/com.example/thesis-link"),
    )
    .unwrap();
    let thesis = sha(&home.path("Documents/thesis.txt"));

    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Delete);

    assert_eq!(report.items[0].outcome, Outcome::Deleted);
    assert_eq!(sha(&home.path("Documents/thesis.txt")), thesis);
}

#[test]
fn delete_of_a_hard_link_keeps_the_other_name() {
    let home = Home::plain();
    home.file("Documents/keep.bin", 8_000);
    fs::create_dir_all(home.path("Library/Caches/com.example")).unwrap();
    fs::hard_link(
        home.path("Documents/keep.bin"),
        home.path("Library/Caches/com.example/keep.bin"),
    )
    .unwrap();
    let keep = sha(&home.path("Documents/keep.bin"));

    // The scanner counts a hard-linked file under the name that sorts first
    // (here the one in Documents), while the executor counts it inside the
    // item. The sizes then differ, and the executor refuses: the safe side.
    // See the PR notes (pnpm stores hard links into node_modules).
    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Delete);
    assert_eq!(refusal(&report, 0), &ItemRefusal::ChangedSinceReview);

    let mut req = home.request("Library/Caches/com.example");
    let file = home.path("Documents/keep.bin").symlink_metadata().unwrap();
    req.expected.size_bytes += file.blocks() * 512;
    let report = run(home.plan(vec![req]), Mode::Delete);
    assert_eq!(report.items[0].outcome, Outcome::Deleted);
    assert_eq!(sha(&home.path("Documents/keep.bin")), keep);
}

// ---------------------------------------------------------------------------
// Zero data loss on the full fixture home
// ---------------------------------------------------------------------------

/// Cleans several items of the fixture home (including the cache folders
/// that hold the symlink traps, and `node_modules` beside a `.git`), then
/// checks that every other path the fixture made is still there, unchanged.
#[test]
fn cleaning_the_fixture_home_changes_nothing_else() {
    for mode in [Mode::Trash, Mode::Delete] {
        let (home, manifest) = Home::fixture();
        let targets = [
            fixtures::paths::TRAP_SYMLINK_ETC
                .rsplit_once('/')
                .unwrap()
                .0,
            fixtures::paths::TRAP_SYMLINK_DOCUMENTS
                .rsplit_once('/')
                .unwrap()
                .0,
            fixtures::paths::DERIVED_DATA_ORPHAN,
            fixtures::paths::TRAP_GIT_NODE_MODULES,
            fixtures::paths::NPM_CACHE,
        ];
        let report = run(home.plan_of(&targets), mode);
        for item in &report.items {
            assert!(
                matches!(
                    item.outcome,
                    Outcome::MovedToTrash { .. } | Outcome::Deleted
                ),
                "{mode:?}: {item:?}"
            );
        }

        let targeted = |p: &str| {
            targets
                .iter()
                .any(|t| p == *t || p.starts_with(&format!("{t}/")))
        };
        let mut checked = 0;
        for entry in manifest.entries.iter().filter(|e| !targeted(&e.path)) {
            let path = home.path(&entry.path);
            let meta = path
                .symlink_metadata()
                .unwrap_or_else(|_| panic!("{mode:?}: {} is gone", entry.path));
            match entry.kind {
                fixtures::EntryKind::Dir => assert!(meta.is_dir(), "{}", entry.path),
                fixtures::EntryKind::Symlink => assert_eq!(
                    fs::read_link(&path).unwrap().to_str(),
                    entry.target.as_deref(),
                    "{}",
                    entry.path
                ),
                fixtures::EntryKind::File => assert_eq!(
                    Some(sha(&path)),
                    entry.sha256,
                    "{mode:?}: {} changed",
                    entry.path
                ),
            }
            checked += 1;
        }
        assert!(checked > 100, "only {checked} paths checked");
        // /etc, the target of one trap, is untouched too.
        assert!(Path::new("/etc").is_dir());
    }
}

#[test]
fn the_fixture_traps_are_refused() {
    let (home, _) = Home::fixture();
    let mut env = home.request("projects/a");
    env.selected_individually = true;
    // A whole repository is refused when the plan is built.
    assert!(Plan::build(1, &home.root, home.dev, vec![env], SystemTime::now()).is_err());
    let report = run(
        home.plan_of(&["AndroidStudioProjects/Shopper/app"]),
        Mode::Trash,
    );
    assert!(matches!(
        refusal(&report, 0),
        ItemRefusal::ContainsProtected {
            reason: BlockReason::SigningMaterial,
            ..
        }
    ));
    assert!(home.path(fixtures::paths::TRAP_KEYSTORE).exists());
}

// ---------------------------------------------------------------------------
// Read-only folders (for example Go's module cache)
// ---------------------------------------------------------------------------

/// Sets a folder's permission bits.
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn delete_refuses_an_item_with_a_read_only_folder_before_moving_it() {
    for locked in ["Library/Caches/gomod/pkg@v1", "Library/Caches/gomod"] {
        let home = Home::plain();
        home.file("Library/Caches/gomod/pkg@v1/go.mod", 1_000);
        chmod(&home.path(locked), 0o555);

        let report = run(home.plan_of(&["Library/Caches/gomod"]), Mode::Delete);
        assert_eq!(
            refusal(&report, 0),
            &ItemRefusal::NeedsTrash {
                at: home.path(locked)
            },
            "{locked}"
        );
        assert!(home.path("Library/Caches/gomod/pkg@v1/go.mod").exists());
        assert!(home.trash_names().is_empty());

        let report = run(home.plan_of(&["Library/Caches/gomod"]), Mode::Trash);
        match &report.items[0].outcome {
            Outcome::MovedToTrash { to } => {
                let moved = to.join(
                    locked
                        .trim_start_matches("Library/Caches/gomod")
                        .trim_start_matches('/'),
                );
                // Let the temp folder be cleaned up.
                chmod(&moved, 0o755);
            }
            // Moving a read-only folder to another parent needs write
            // permission on the folder itself on Linux and macOS (to update its `..`),
            // so for a locked item folder the move fails. Nothing has moved.
            Outcome::Failed { left_in: None, .. } if locked == "Library/Caches/gomod" => {
                assert!(home.path("Library/Caches/gomod/pkg@v1/go.mod").exists());
                chmod(&home.path(locked), 0o755);
            }
            other => panic!("{locked}: {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// The dry run
// ---------------------------------------------------------------------------

/// Every path in the fixture manifest, with what it held.
fn snapshot(home: &Home, manifest: &fixtures::Manifest) -> Vec<(String, Option<String>)> {
    manifest
        .entries
        .iter()
        .map(|e| {
            let path = home.path(&e.path);
            let meta = path
                .symlink_metadata()
                .unwrap_or_else(|_| panic!("{} is gone", e.path));
            let held = match e.kind {
                fixtures::EntryKind::File => Some(sha(&path)),
                fixtures::EntryKind::Symlink => {
                    Some(fs::read_link(&path).unwrap().display().to_string())
                }
                fixtures::EntryKind::Dir => None,
            };
            (format!("{} {:?}", e.path, meta.modified().unwrap()), held)
        })
        .collect()
}

#[test]
fn preview_reports_what_would_move_and_touches_nothing() {
    let (home, manifest) = Home::fixture();
    let before = snapshot(&home, &manifest);
    let plan = home.plan_of(&[
        fixtures::paths::DERIVED_DATA_ORPHAN,
        fixtures::paths::NPM_CACHE,
        "AndroidStudioProjects/Shopper/app",
    ]);
    let checks = executor::preview(&plan, Mode::Trash, SystemTime::now()).unwrap();
    assert_eq!(checks.len(), 3);
    for check in &checks[..2] {
        let bytes = check.result.as_ref().unwrap();
        assert!(*bytes > 0, "{check:?}");
    }
    assert!(matches!(
        checks[2].result,
        Err(ItemRefusal::ContainsProtected {
            reason: BlockReason::SigningMaterial,
            ..
        })
    ));
    // Nothing moved, nothing changed, the Trash is still empty.
    assert_eq!(snapshot(&home, &manifest), before);
    assert!(home.trash_names().is_empty());

    // The same plan can still be carried out afterwards.
    let report = run(plan, Mode::Trash);
    assert!(matches!(
        report.items[0].outcome,
        Outcome::MovedToTrash { .. }
    ));
    assert!(matches!(
        report.items[1].outcome,
        Outcome::MovedToTrash { .. }
    ));
    assert!(matches!(report.items[2].outcome, Outcome::Refused(_)));
}

#[test]
fn preview_sees_a_change_since_review() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/a.bin", 4096);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    home.file("Library/Caches/com.example/b.bin", 1 << 20);
    let checks = executor::preview(&plan, Mode::Trash, SystemTime::now()).unwrap();
    assert_eq!(checks[0].result, Err(ItemRefusal::ChangedSinceReview));
    assert!(home.path("Library/Caches/com.example/b.bin").exists());
}

#[test]
fn preview_of_an_expired_plan_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/a.bin", 4096);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    let later = SystemTime::now() + PLAN_LIFETIME;
    assert!(matches!(
        executor::preview(&plan, Mode::Trash, later),
        Err(ExecuteError::Plan(PlanError::Expired))
    ));
}

#[test]
fn preview_without_a_trash_is_refused() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/a.bin", 4096);
    let plan = home.plan_of(&["Library/Caches/com.example"]);
    fs::remove_dir(home.path(TRASH_DIR)).unwrap();
    assert!(matches!(
        executor::preview(&plan, Mode::Trash, SystemTime::now()),
        Err(ExecuteError::TrashUnavailable)
    ));
}

// ---------------------------------------------------------------------------
// Putting back (undo)
// ---------------------------------------------------------------------------

/// The put-back entries for every item of `report` that moved to the Trash.
fn put_backs(report: &Report) -> Vec<executor::PutBack> {
    report
        .items
        .iter()
        .filter_map(|i| match &i.outcome {
            Outcome::MovedToTrash { to } => Some(executor::PutBack {
                recommendation_id: i.recommendation_id.clone(),
                original: i.path.clone(),
                in_trash: to.clone(),
                identity: i.moved.unwrap(),
            }),
            _ => None,
        })
        .collect()
}

#[test]
fn put_back_restores_what_moved() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/sub/b.bin", 3_000);
    home.file("Library/Caches/big.log", 5_000);
    let before = sha(&home.path("Library/Caches/com.example/sub/b.bin"));
    let report = run(
        home.plan_of(&["Library/Caches/com.example", "Library/Caches/big.log"]),
        Mode::Trash,
    );
    let entries = put_backs(&report);
    assert_eq!(entries.len(), 2);

    let results = executor::put_back(&home.root, home.dev, &entries).unwrap();

    assert_eq!(results, vec![Ok(()), Ok(())]);
    assert_eq!(
        sha(&home.path("Library/Caches/com.example/sub/b.bin")),
        before
    );
    assert!(home.path("Library/Caches/big.log").is_file());
    assert!(home.trash_names().is_empty());
}

#[test]
fn put_back_never_replaces_something_new() {
    let home = Home::plain();
    home.file("Library/Caches/big.log", 5_000);
    let report = run(home.plan_of(&["Library/Caches/big.log"]), Mode::Trash);
    home.file("Library/Caches/big.log", 10);
    let results = executor::put_back(&home.root, home.dev, &put_backs(&report)).unwrap();
    assert_eq!(results, vec![Err(executor::PutBackRefusal::PlaceTaken)]);
    assert_eq!(
        fs::read(home.path("Library/Caches/big.log")).unwrap().len(),
        10
    );
    assert_eq!(home.trash_names(), BTreeSet::from(["big.log".to_string()]));
}

#[test]
fn put_back_refuses_something_else_in_the_trash() {
    let home = Home::plain();
    home.file("Library/Caches/big.log", 5_000);
    let report = run(home.plan_of(&["Library/Caches/big.log"]), Mode::Trash);
    // The user emptied the Trash and something else now has the same name.
    fs::remove_file(home.path(".Trash/big.log")).unwrap();
    home.file(".Trash/big.log", 7);
    let results = executor::put_back(&home.root, home.dev, &put_backs(&report)).unwrap();
    assert_eq!(results, vec![Err(executor::PutBackRefusal::NotInTrash)]);
    assert!(!home.path("Library/Caches/big.log").exists());
}

#[test]
fn put_back_checks_more_than_the_inode_number() {
    // Some file systems reuse a freed inode number at once, so a new entry
    // in the Trash can carry the old number. Only a full match counts.
    let home = Home::plain();
    home.file("Library/Caches/big.log", 5_000);
    let report = run(home.plan_of(&["Library/Caches/big.log"]), Mode::Trash);
    let real = put_backs(&report);
    let changes: [fn(&mut executor::TrashIdentity); 3] = [
        |i| i.ctime.1 = (i.ctime.1 + 1) % 1_000_000_000,
        |i| i.size += 1,
        |i| i.is_dir = !i.is_dir,
    ];
    for change in changes {
        let mut entries = real.clone();
        change(&mut entries[0].identity);
        let results = executor::put_back(&home.root, home.dev, &entries).unwrap();
        assert_eq!(results, vec![Err(executor::PutBackRefusal::NotInTrash)]);
        assert!(!home.path("Library/Caches/big.log").exists());
    }
    // The real identity still works.
    let results = executor::put_back(&home.root, home.dev, &real).unwrap();
    assert_eq!(results, vec![Ok(())]);
}

#[test]
fn put_back_never_follows_a_swapped_folder() {
    let home = Home::plain();
    home.file("Library/Caches/com.example/data", 1_000);
    let report = run(home.plan_of(&["Library/Caches/com.example"]), Mode::Trash);
    // The folder it came from is swapped for a link to Documents.
    fs::remove_dir(home.path("Library/Caches")).unwrap();
    symlink(home.path("Documents"), home.path("Library/Caches")).unwrap();
    let results = executor::put_back(&home.root, home.dev, &put_backs(&report)).unwrap();
    assert_eq!(results, vec![Err(executor::PutBackRefusal::FolderChanged)]);
    assert!(!home.path("Documents/com.example").exists());
    assert!(home.trash_names().contains("com.example"));
}

#[test]
fn put_back_refuses_entries_it_did_not_make() {
    let home = Home::plain();
    home.file(".Trash/x", 10);
    let meta = home.path(".Trash/x").symlink_metadata().unwrap();
    let identity = executor::TrashIdentity {
        ino: meta.ino(),
        is_dir: meta.is_dir(),
        size: meta.size() as i64,
        ctime: (meta.ctime(), meta.ctime_nsec()),
    };
    let bad = [
        ("/etc/x", ".Trash/x"),
        ("Library/../../x", ".Trash/x"),
        ("Library/Caches/x", "Library/x"),
        ("Library/Caches/x", ".Trash/sub/x"),
    ];
    for (original, in_trash) in bad {
        let entry = executor::PutBack {
            recommendation_id: "r".into(),
            original: home.root.join(original),
            in_trash: home.root.join(in_trash),
            identity,
        };
        let results = executor::put_back(&home.root, home.dev, &[entry]).unwrap();
        assert_eq!(
            results,
            vec![Err(executor::PutBackRefusal::NotOurs)],
            "{original}"
        );
    }
    assert!(home.path(".Trash/x").exists());
}
