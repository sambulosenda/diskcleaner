//! The zero-data-loss run: the whole cleanup path, as the app drives it,
//! against full fixture homes, checked by a ledger that does not trust the
//! engine.
//!
//! Each run:
//!
//! 1. records a ledger of every path in the fixture home: kind, inode, link
//!    count, permission bits, size, SHA-256 of every file, symlink targets
//!    and file modification times;
//! 2. scans, ticks items the way the app does (every tier, so far more than a
//!    user would tick), previews, runs the cleanup, and records the ledger
//!    again;
//! 3. works out from the two ledgers alone what moved, and proves:
//!    - nothing outside the moved items changed, appeared or vanished;
//!    - every moved item is in the Trash, the same inode, with every file,
//!      folder and link inside it byte for byte as it was;
//!    - nothing already in the Trash was replaced or changed;
//!    - no moved path is protected (hard-blocked, inside a git repository,
//!      or review-only when it was not chosen on its own), checked path by
//!      path with the safety tables, and none is on a hand-written list of
//!      fixture paths that must never move whatever the rules say;
//!    - nothing the scan left unknown, or put in `keep`, moved;
//!    - what moved is exactly what the dry run said would move, with the
//!      same byte total;
//! 4. puts the cleanup back and proves the ledger is identical to step 1,
//!    inodes and modification times included, and the Trash holds only what
//!    it held before;
//! 5. scans again and repeats, to show a second cleanup moves the same set.
//!
//! Every run uses a fresh fixture home in a temp folder. Nothing here reads
//! or touches the real home folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::{symlink, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use roomy_ffi::{
    CleanupCheck, CleanupChoice, CleanupPreview, ItemSummary, Mode, RoomyCore, ScanHandle,
    ScanPhase, Sort, Tier,
};
use sha2::{Digest, Sha256};

const TRASH: &str = ".Trash";

// ---------------------------------------------------------------------------
// The ledger
// ---------------------------------------------------------------------------

/// What one path held, as far as anyone could tell it apart.
#[derive(Debug, Clone, PartialEq, Eq)]
enum What {
    /// A folder. Its modification time is left out on purpose: moving
    /// something out of a folder changes the folder's time.
    Dir,
    /// A regular file, with the SHA-256 of its bytes.
    File {
        size: u64,
        sha256: String,
        modified_ns: i128,
        links: u64,
    },
    /// A symlink, with its target exactly as stored. Never followed.
    Link { target: PathBuf, modified_ns: i128 },
}

/// One ledger line: what a path held plus the identity of its inode.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    what: What,
    ino: u64,
    dev: u64,
    mode: u32,
}

/// Every path under a home, relative to it, with `/` separators.
type Ledger = BTreeMap<String, Node>;

/// Reads the ledger of `root`. Never follows a symlink. Leaves out the
/// History folder, which the core creates on the first cleanup.
fn ledger(root: &Path) -> Ledger {
    let mut out = Ledger::new();
    let mut stack = vec![String::new()];
    while let Some(rel) = stack.pop() {
        let dir = if rel.is_empty() {
            root.to_path_buf()
        } else {
            root.join(&rel)
        };
        for entry in fs::read_dir(&dir).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            let child = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            if under(&child, history::FOLDER) {
                continue;
            }
            let path = root.join(&child);
            let meta = path.symlink_metadata().unwrap();
            let what = if meta.file_type().is_symlink() {
                What::Link {
                    target: fs::read_link(&path).unwrap(),
                    modified_ns: mtime_ns(&meta),
                }
            } else if meta.is_dir() {
                stack.push(child.clone());
                What::Dir
            } else {
                What::File {
                    size: meta.len(),
                    sha256: format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                    modified_ns: mtime_ns(&meta),
                    links: meta.nlink(),
                }
            };
            out.insert(
                child,
                Node {
                    what,
                    ino: meta.ino(),
                    dev: meta.dev(),
                    mode: meta.mode(),
                },
            );
        }
    }
    out
}

fn mtime_ns(meta: &fs::Metadata) -> i128 {
    i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec())
}

/// Whether `path` is `folder` or inside it.
fn under(path: &str, folder: &str) -> bool {
    path == folder
        || path
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The entries of `ledger` at or under `root`, re-keyed relative to `root`
/// (`""` is `root` itself).
fn subtree(ledger: &Ledger, root: &str) -> BTreeMap<String, Node> {
    ledger
        .iter()
        .filter(|(p, _)| under(p, root))
        .map(|(p, n)| (p[root.len()..].to_string(), n.clone()))
        .collect()
}

/// The top-level paths that vanished: vanished paths whose parent did not.
fn vanished_roots(before: &Ledger, after: &Ledger) -> Vec<String> {
    let gone: BTreeSet<&String> = before.keys().filter(|p| !after.contains_key(*p)).collect();
    gone.iter()
        .filter(|p| match p.rsplit_once('/') {
            Some((parent, _)) => !gone.contains(&parent.to_string()),
            None => true,
        })
        .map(|p| p.to_string())
        .collect()
}

/// Names directly in the Trash.
fn trash_names(ledger: &Ledger) -> BTreeSet<String> {
    ledger
        .keys()
        .filter_map(|p| p.strip_prefix(&format!("{TRASH}/")))
        .filter(|rest| !rest.contains('/'))
        .map(str::to_string)
        .collect()
}

/// Proves that going from `before` to `after` only moved whole items into
/// the Trash, each intact. Returns the moved items' paths.
///
/// This uses nothing but the two ledgers, so it holds whatever the engine
/// says it did.
fn prove_only_moves_into_trash(before: &Ledger, after: &Ledger) -> Vec<String> {
    // 1. Nothing that is still there changed in any way.
    for (path, was) in before {
        if let Some(now) = after.get(path) {
            assert_eq!(now, was, "{path} changed in place");
        }
    }
    // 2. Every vanished item is one new entry in the Trash: same inode and
    //    the same tree, byte for byte.
    let roots = vanished_roots(before, after);
    let new_in_trash: BTreeSet<String> = trash_names(after)
        .difference(&trash_names(before))
        .cloned()
        .collect();
    let mut matched = BTreeSet::new();
    for root in &roots {
        assert!(
            !under(root, TRASH),
            "{root}, already in the Trash, was moved or removed"
        );
        let node = &before[root];
        let landed: Vec<&String> = new_in_trash
            .iter()
            .filter(|name| {
                let n = &after[&format!("{TRASH}/{name}")];
                n.ino == node.ino && n.dev == node.dev
            })
            .collect();
        assert_eq!(landed.len(), 1, "{root} is not in the Trash ({landed:?})");
        let in_trash = format!("{TRASH}/{}", landed[0]);
        assert_eq!(
            subtree(after, &in_trash),
            subtree(before, root),
            "{root} is not the same in the Trash"
        );
        assert!(matched.insert(landed[0].clone()));
    }
    // 3. Nothing else is new: no new name in the Trash without an item, and
    //    nothing new anywhere outside the moved items.
    assert_eq!(matched, new_in_trash, "the Trash holds something extra");
    for path in after.keys().filter(|p| !before.contains_key(*p)) {
        assert!(
            new_in_trash
                .iter()
                .any(|name| under(path, &format!("{TRASH}/{name}"))),
            "{path} appeared"
        );
    }
    roots
}

// ---------------------------------------------------------------------------
// The independent safety check of what moved
// ---------------------------------------------------------------------------

/// Fixture paths where nothing may move, whatever the rules say: not the
/// path, nothing inside it, and nothing that holds it. Written by hand from
/// `fixtures::paths`, not from the safety tables, so a mistake in those
/// tables cannot hide here.
fn never_touch() -> Vec<String> {
    use fixtures::paths as p;
    let mut list: Vec<String> = [
        p::PHOTOS_LIBRARY,
        p::MESSAGES_ATTACHMENTS,
        p::SLACK_LOCAL_STORAGE,
        p::EXPO_STATE,
        p::JS_EDITOR_EXTENSION,
        p::JS_PROJECT_ON_DESKTOP,
        p::TRAP_HARD_LINK_A,
        p::TRAP_HARD_LINK_B,
        p::TRAP_GIT_DIR,
        p::TRAP_ENV_FILE,
        p::TRAP_KEYSTORE,
        // No rule claims an ordinary document, so it must stay put.
        p::DOWNLOAD_NORMAL,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    list.extend(p::XCODE_PROJECTS.iter().map(|s| s.to_string()));
    for project in p::JS_PROJECTS
        .iter()
        .chain(p::JS_PROJECTS_ELSEWHERE.iter())
        .chain(p::JS_PROJECTS_DOCS_AND_HOME.iter())
    {
        list.push(format!("{project}/package.json"));
    }
    for project in p::ANDROID_PROJECTS {
        list.push(format!("{project}/build.gradle"));
    }
    list
}

/// Folders that may hold items that move (such as a project's
/// `node_modules` in Documents, or one iPhone backup the user chose on its
/// own) but must never move as a whole, nor inside anything that moves.
fn never_move_whole() -> Vec<String> {
    use fixtures::paths as p;
    let mut list: Vec<String> = [
        p::DOCUMENTS,
        p::DOWNLOADS,
        "Desktop",
        "Pictures",
        "Movies",
        "Music",
        "Applications",
        "Public",
        "Library",
        p::MOBILESYNC_BACKUP,
        p::CACHES,
        p::ANDROID_SDK,
        p::DERIVED_DATA,
        p::SIM_DEVICES,
        p::NPM_CACHE,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    list.extend(p::JS_PROJECTS.iter().map(|s| s.to_string()));
    list.extend(p::JS_PROJECTS_ELSEWHERE.iter().map(|s| s.to_string()));
    list.extend(p::JS_PROJECTS_DOCS_AND_HOME.iter().map(|s| s.to_string()));
    list.extend(p::ANDROID_PROJECTS.iter().map(|s| s.to_string()));
    list
}

/// Checks every path that moved (not just the item roots) against the
/// protected-path rules, the way `docs/safety/protected-paths.md` states
/// them, using only the `before` ledger to see the disk.
fn prove_nothing_protected_moved(before: &Ledger, moved: &[String], chosen_alone: bool) {
    let never = never_touch();
    let whole = never_move_whole();
    for root in moved {
        for blocked in never.iter().filter(|b| before.contains_key(*b)) {
            assert!(
                !under(root, blocked) && !under(blocked, root),
                "{root} moved, and it is, holds or is inside {blocked}, which must never move"
            );
        }
        for folder in whole.iter().filter(|f| before.contains_key(*f)) {
            assert!(
                !under(folder, root),
                "{root} moved, and it is or holds {folder}, which must never move as a whole"
            );
        }
        for path in before.keys().filter(|p| under(p, root)) {
            let rel: Vec<&str> = path.split('/').collect();
            assert_eq!(
                safety::protected::hard_block(&rel),
                None,
                "{path} is hard-blocked but moved"
            );
            let git = safety::protected::git_block(&rel, |i| {
                let folder = rel[..i].join("/");
                let dot_git = if folder.is_empty() {
                    ".git".to_string()
                } else {
                    format!("{folder}/.git")
                };
                Ok::<bool, ()>(before.contains_key(&dot_git))
            });
            assert_eq!(git, Ok(None), "{path} is inside a git repository but moved");
            match safety::protected::review_only(&rel) {
                None => {}
                Some(reason) if chosen_alone && reason.deletable_in_v1() => {}
                Some(reason) => panic!(
                    "{path} is review-only ({reason:?}) but moved (chosen alone: {chosen_alone})"
                ),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Driving the core the way the app does
// ---------------------------------------------------------------------------

struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
    core: Arc<RoomyCore>,
}

/// A fresh fixture home with an empty Trash, and a core for it.
fn fixture_home() -> Home {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    fs::create_dir(root.join(TRASH)).unwrap();
    let core = RoomyCore::with_home(root.clone()).unwrap();
    // Fixture homes are made just now, and the test machine runs its own
    // programs: switch off those two extra checks (tested in extra_checks.rs).
    core.set_recent_change_window_for_testing(std::time::Duration::ZERO);
    core.set_running_programs_for_testing(Some(Vec::new()));
    Home {
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
        assert!(start.elapsed() < Duration::from_secs(120), "scan too slow");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Which items a run ticks.
#[derive(Debug, Clone, Copy)]
enum Pick {
    /// What the app preselects: `likely_safe` only.
    LikelySafe,
    /// Every item in every tier, far more than a user would tick.
    Everything,
}

fn listed(core: &RoomyCore, h: ScanHandle, pick: Pick) -> Vec<ItemSummary> {
    let tiers: &[Tier] = match pick {
        Pick::LikelySafe => &[Tier::LikelySafe],
        Pick::Everything => &[Tier::LikelySafe, Tier::ReviewFirst, Tier::Keep],
    };
    tiers
        .iter()
        .flat_map(|t| {
            core.list_items(h, None, Some(*t), Sort::LargestFirst)
                .unwrap()
        })
        .collect()
}

fn choices(items: &[ItemSummary], chosen_alone: bool) -> Vec<CleanupChoice> {
    items
        .iter()
        .map(|i| CleanupChoice {
            item_id: i.id.clone(),
            chosen_alone,
        })
        .collect()
}

/// The ids the dry run said would move.
fn will_move(preview: &CleanupPreview) -> BTreeSet<String> {
    preview
        .items
        .iter()
        .filter(|i| i.check == CleanupCheck::WillMove)
        .map(|i| i.item_id.clone())
        .collect()
}

/// What one cleanup cycle found, to compare runs.
#[derive(Debug, PartialEq, Eq)]
struct Cycle {
    moved: Vec<String>,
    bytes: u64,
}

/// One full cycle on `home`: scan, preview, prove the preview touched
/// nothing, run, prove the ledger, put back, prove the ledger again.
fn cycle(home: &Home, mode: Mode, pick: Pick, chosen_alone: bool) -> Cycle {
    let before = ledger(&home.root);
    let h = finished_scan(&home.core, mode);
    let items = listed(&home.core, h, pick);
    assert!(!items.is_empty());

    // What the scan put in `keep` or left unknown, by path, for the check
    // after the run. Asked in developer mode so the paths are known.
    let (keep, unknown) = not_offered(&home.core);

    let preview = home
        .core
        .preview_cleanup(h, choices(&items, chosen_alone))
        .unwrap();
    assert_eq!(ledger(&home.root), before, "the dry run changed something");
    let promised = will_move(&preview);
    let Some(plan) = preview.plan else {
        assert!(promised.is_empty());
        return Cycle {
            moved: vec![],
            bytes: 0,
        };
    };

    let result = home.core.run_cleanup(plan).unwrap();
    let after = ledger(&home.root);
    let moved = prove_only_moves_into_trash(&before, &after);
    prove_nothing_protected_moved(&before, &moved, chosen_alone);
    for m in &moved {
        // Nothing in `keep` moved, nor anything inside or holding it.
        for k in &keep {
            assert!(!under(m, k) && !under(k, m), "{m} moved, but {k} is keep");
        }
        // An unknown item is what is left of a folder once the matched
        // items inside it are counted (for example `Library`). It must not
        // be inside anything that moved; the ledger already shows that its
        // own files outside the moved items are untouched.
        for u in &unknown {
            assert!(
                !under(u, m),
                "{m} moved, and it holds {u}, which is unknown"
            );
        }
    }

    // What moved is what the dry run promised, item for item and byte for
    // byte, and every promised item moved.
    let done: BTreeSet<String> = result
        .items
        .iter()
        .filter(|i| i.done)
        .map(|i| i.item_id.clone())
        .collect();
    assert_eq!(
        done, promised,
        "the cleanup did not do what the dry run said"
    );
    assert_eq!(result.items.len(), promised.len());
    assert_eq!(moved.len(), promised.len(), "moved {moved:?}");
    assert_eq!(result.bytes_moved, preview.bytes_to_move);
    if mode == Mode::Developer {
        let shown: BTreeSet<String> = preview
            .items
            .iter()
            .filter(|i| i.check == CleanupCheck::WillMove)
            .map(|i| {
                i.path
                    .as_deref()
                    .unwrap()
                    .trim_start_matches("~/")
                    .to_string()
            })
            .collect();
        assert_eq!(
            moved.iter().cloned().collect::<BTreeSet<_>>(),
            shown,
            "the paths that moved are not the ones the dry run showed"
        );
    }

    // Put back: the home is exactly as it was.
    let cleanup = result.cleanup.unwrap();
    let back = home.core.put_back_cleanup(cleanup).unwrap();
    assert!(back.items.iter().all(|i| i.done), "{back:?}");
    assert_eq!(back.bytes_put_back, result.bytes_moved);
    let restored = ledger(&home.root);
    if restored != before {
        let diff: Vec<_> = before
            .keys()
            .chain(restored.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|p| before.get(*p) != restored.get(*p))
            .take(20)
            .collect();
        panic!("putting back did not restore the home: {diff:?}");
    }
    home.core.close_scan(h);
    Cycle {
        moved,
        bytes: result.bytes_moved,
    }
}

/// The paths of the `keep` items and of the unknown items, from a
/// developer-mode scan.
fn not_offered(core: &RoomyCore) -> (Vec<String>, Vec<String>) {
    let h = finished_scan(core, Mode::Developer);
    let mut keep = Vec::new();
    let mut unknown = Vec::new();
    for item in core.list_items(h, None, None, Sort::LargestFirst).unwrap() {
        let detail = core.item_detail(h, item.id).unwrap();
        let path = detail.path.unwrap().trim_start_matches("~/").to_string();
        if detail.rule_id == "unknown" {
            unknown.push(path);
        } else if detail.tier == Tier::Keep {
            keep.push(path);
        }
    }
    core.close_scan(h);
    // The fixture has unknown leftovers (such as `Library`) in every run.
    assert!(!unknown.is_empty());
    (keep, unknown)
}

/// Two full cycles on one fixture home, and a summary line for the log.
fn run(mode: Mode, pick: Pick, chosen_alone: bool) -> Cycle {
    let home = fixture_home();
    let first = cycle(&home, mode, pick, chosen_alone);
    let second = cycle(&home, mode, pick, chosen_alone);
    assert_eq!(first, second, "a second cleanup moved something different");
    eprintln!(
        "zero data loss: {mode:?}, {pick:?}, chosen alone {chosen_alone}: \
         {} items, {} bytes moved, checked and put back, twice",
        first.moved.len(),
        first.bytes
    );
    first
}

// ---------------------------------------------------------------------------
// The runs
// ---------------------------------------------------------------------------

#[test]
fn developer_mode_likely_safe() {
    let c = run(Mode::Developer, Pick::LikelySafe, true);
    assert!(c.moved.len() >= 10, "only {} items moved", c.moved.len());
}

#[test]
fn developer_mode_everything_chosen_alone() {
    let likely = run(Mode::Developer, Pick::LikelySafe, true);
    let all = run(Mode::Developer, Pick::Everything, true);
    assert!(all.moved.len() >= likely.moved.len());
}

#[test]
fn developer_mode_everything_chosen_as_a_group() {
    let c = run(Mode::Developer, Pick::Everything, false);
    assert!(!c.moved.is_empty());
}

#[test]
fn simple_mode_likely_safe() {
    let c = run(Mode::Simple, Pick::LikelySafe, true);
    assert!(!c.moved.is_empty());
}

#[test]
fn simple_mode_everything_chosen_alone() {
    let c = run(Mode::Simple, Pick::Everything, true);
    assert!(!c.moved.is_empty());
}

/// Things already in the Trash, with the same names as the items that will
/// move, are never replaced or changed; the ledger includes them.
#[test]
fn what_is_already_in_the_trash_is_never_replaced() {
    let home = fixture_home();
    let h = finished_scan(&home.core, Mode::Developer);
    for item in listed(&home.core, h, Pick::Everything) {
        let Some(path) = home.core.item_detail(h, item.id).unwrap().path else {
            continue;
        };
        let name = path.rsplit('/').next().unwrap().to_string();
        let place = home.root.join(TRASH).join(&name);
        if place.symlink_metadata().is_err() {
            fs::write(&place, format!("the user's own {name}")).unwrap();
        }
    }
    let held = trash_names(&ledger(&home.root)).len();
    assert!(held >= 10, "only {held} names in the Trash");
    let c = cycle(&home, Mode::Developer, Pick::Everything, true);
    assert!(!c.moved.is_empty());
}

/// Changes made between the dry run and the cleanup: the changed items are
/// refused, everything else still moves, and the ledger still holds.
#[test]
fn changes_after_the_dry_run_are_refused_and_lose_nothing() {
    let home = fixture_home();
    let h = finished_scan(&home.core, Mode::Developer);
    let items = listed(&home.core, h, Pick::LikelySafe);
    let preview = home.core.preview_cleanup(h, choices(&items, true)).unwrap();
    let plan = preview.plan.unwrap();
    let folders: Vec<(String, String)> = preview
        .items
        .iter()
        .filter(|i| i.check == CleanupCheck::WillMove)
        .map(|i| {
            let rel = i
                .path
                .as_deref()
                .unwrap()
                .trim_start_matches("~/")
                .to_string();
            (i.item_id.clone(), rel)
        })
        .filter(|(_, rel)| home.root.join(rel).is_dir())
        .take(4)
        .collect();
    assert_eq!(folders.len(), 4, "need four folder items");

    // A repository appears in one, a signing key in another, a third grows
    // by far more than 1%, and a link to Documents appears in the fourth.
    // The `.git` and the key are empty files, so they take no space, and each
    // folder gets its old modification time back: only the check that looks
    // inside the item can catch those two.
    let plant = |rel: &str, make: &dyn Fn(&Path)| {
        let folder = home.root.join(rel);
        let was = folder.symlink_metadata().unwrap().modified().unwrap();
        make(&folder);
        fs::File::open(&folder).unwrap().set_modified(was).unwrap();
        assert_eq!(folder.symlink_metadata().unwrap().modified().unwrap(), was);
    };
    plant(&folders[0].1, &|f| fs::write(f.join(".git"), b"").unwrap());
    plant(&folders[1].1, &|f| {
        fs::write(f.join("upload.p12"), b"").unwrap()
    });
    plant(&folders[2].1, &|f| {
        fs::write(f.join("grew.bin"), vec![9u8; 64 << 20]).unwrap()
    });
    plant(&folders[3].1, &|f| {
        symlink(
            home.root.join(fixtures::paths::DOCUMENTS),
            f.join("docs-link"),
        )
        .unwrap()
    });

    let before = ledger(&home.root);
    let result = home.core.run_cleanup(plan).unwrap();
    let after = ledger(&home.root);
    let moved = prove_only_moves_into_trash(&before, &after);
    prove_nothing_protected_moved(&before, &moved, true);

    // Each refused for its own reason, not just because something changed.
    let reasons = [
        "version control",
        "signing keys",
        "changed since you reviewed it",
    ];
    for ((id, rel), want) in folders[..3].iter().zip(reasons) {
        let row = result.items.iter().find(|i| &i.item_id == id).unwrap();
        assert!(!row.done, "{rel} changed after the dry run but moved");
        let reason = row.reason.as_deref().unwrap_or_default();
        assert!(reason.contains(want), "{rel}: {reason}");
        assert!(!moved.contains(rel));
    }
    // The fourth moved, with the link inside it as a link.
    assert!(moved.contains(&folders[3].1));
    let done = result.items.iter().filter(|i| i.done).count();
    assert_eq!(done, moved.len());
    assert!(done > 0, "the unchanged items should still move");
    // The link moved as a link: Documents itself is untouched (checked by the
    // ledger above), and the link inside the Trash still says the same thing.

    let back = home.core.put_back_cleanup(result.cleanup.unwrap()).unwrap();
    assert!(back.items.iter().all(|i| i.done), "{back:?}");
    assert_eq!(ledger(&home.root), before);
}

/// The fixture traps that point outside home are still intact afterwards.
#[test]
fn links_out_of_home_are_never_followed() {
    let etc_before = ledger_shallow(Path::new("/etc"));
    run(Mode::Developer, Pick::Everything, true);
    assert_eq!(ledger_shallow(Path::new("/etc")), etc_before);
}

/// The names and inodes directly inside `dir`, without reading any file.
fn ledger_shallow(dir: &Path) -> BTreeSet<(String, u64)> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            let ino = e.path().symlink_metadata().ok()?.ino();
            Some((e.file_name().to_string_lossy().into_owned(), ino))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The extra checks, switched on
// ---------------------------------------------------------------------------

/// Sets every file and folder in `path` older than two hours, if it is newer.
/// Symlinks are left alone.
fn age_to_two_hours(path: &Path) {
    let when = std::time::SystemTime::now() - Duration::from_secs(2 * 60 * 60);
    let meta = path.symlink_metadata().unwrap();
    if meta.file_type().is_symlink() {
        return;
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            age_to_two_hours(&entry.unwrap().path());
        }
    }
    if meta.modified().unwrap() > when {
        fs::File::open(path).unwrap().set_modified(when).unwrap();
    }
}

/// Every item in every tier is ticked, with all four extra checks holding
/// something back: a folder the user keeps (DerivedData), an open app
/// (Android Studio), a git project that no longer ignores `node_modules`,
/// and a cache that changed a moment ago. The ledger proves none of those
/// moved, the reasons say why, everything else still moved, and putting
/// back restores the home exactly.
#[test]
fn the_extra_checks_hold_things_back_and_lose_nothing() {
    let home = fixture_home();
    age_to_two_hours(&home.root);
    // The real ten-minute window, and Android Studio open.
    home.core
        .set_recent_change_window_for_testing(Duration::from_secs(10 * 60));
    home.core
        .set_running_programs_for_testing(Some(vec![PathBuf::from(
            "/Applications/Android Studio.app/Contents/MacOS/studio",
        )]));
    let kept = home.root.join(fixtures::paths::DERIVED_DATA);
    let set = home
        .core
        .set_protected_folders(vec![kept.to_str().unwrap().to_string()])
        .unwrap();
    assert_eq!(set, vec![kept.to_str().unwrap().to_string()]);
    // The project stops ignoring node_modules (same size, so only the git
    // check can tell).
    fs::write(
        home.root.join(fixtures::paths::TRAP_GIT_IGNORE),
        b"# Dependencies\nnode_mod/\n.DS_Store\n",
    )
    .unwrap();
    // Firefox writes to its cache just before the scan.
    fs::write(
        home.root.join(fixtures::paths::FIREFOX_CACHE).join("fresh"),
        b"just now",
    )
    .unwrap();

    let before = ledger(&home.root);
    let h = finished_scan(&home.core, Mode::Developer);
    let items = listed(&home.core, h, Pick::Everything);
    let preview = home.core.preview_cleanup(h, choices(&items, true)).unwrap();
    assert_eq!(ledger(&home.root), before, "the dry run changed something");

    let held: Vec<(&str, &str)> = vec![
        (
            fixtures::paths::DERIVED_DATA,
            "in a folder you asked Roomy to keep",
        ),
        (fixtures::paths::GRADLE_CACHES, "Android Studio is open"),
        (
            fixtures::paths::ANDROID_STUDIO_CACHE,
            "Android Studio is open",
        ),
        (fixtures::paths::TRAP_GIT_NODE_MODULES, "doesn't ignore it"),
        (
            fixtures::paths::FIREFOX_CACHE,
            "changed in the last few minutes",
        ),
    ];
    for (place, why) in &held {
        let rows: Vec<_> = preview
            .items
            .iter()
            .filter(|i| {
                let rel = i.path.as_deref().unwrap().trim_start_matches("~/");
                under(rel, place)
            })
            .collect();
        assert!(!rows.is_empty(), "no ticked item under {place}");
        for row in rows {
            match &row.check {
                CleanupCheck::WontMove { reason } => {
                    assert!(reason.contains(why), "{place}: {reason}")
                }
                other => panic!("{place} would move: {other:?}"),
            }
        }
    }

    let plan = preview.plan.unwrap();
    let result = home.core.run_cleanup(plan).unwrap();
    let after = ledger(&home.root);
    let moved = prove_only_moves_into_trash(&before, &after);
    prove_nothing_protected_moved(&before, &moved, true);
    assert!(!moved.is_empty(), "everything else should still move");
    for m in &moved {
        for (place, _) in &held {
            assert!(!under(m, place) && !under(place, m), "{m} moved");
        }
    }
    assert_eq!(will_move(&preview).len(), moved.len());

    let back = home.core.put_back_cleanup(result.cleanup.unwrap()).unwrap();
    assert!(back.items.iter().all(|i| i.done), "{back:?}");
    assert_eq!(ledger(&home.root), before);
}

/// The folders the user keeps must be inside home, and a bad list leaves the
/// earlier one in place.
#[test]
fn kept_folders_must_be_inside_home() {
    let home = fixture_home();
    let good = home.root.join("Developer").to_str().unwrap().to_string();
    assert_eq!(
        home.core
            .set_protected_folders(vec![good.clone(), good.clone()])
            .unwrap(),
        vec![good.clone()]
    );
    assert!(home
        .core
        .set_protected_folders(vec!["/etc".to_string()])
        .is_err());
    assert!(home
        .core
        .set_protected_folders(vec![home.root.to_str().unwrap().to_string()])
        .is_err());
    assert_eq!(home.core.protected_folders(), vec![good]);
    assert!(home.core.set_protected_folders(vec![]).unwrap().is_empty());
}
