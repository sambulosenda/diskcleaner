//! The extra checks ([`executor::Guards`] and the git check): each one skips
//! the item in the dry run and again in the cleanup itself, and a skipped
//! item is never touched. Every test works in a fake home in a temp folder.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use executor::{
    execute, preview, GitConcern, Guards, ItemRefusal, Mode, Outcome, Running,
    RECENT_CHANGE_WINDOW, TRASH_DIR,
};
use planner::{Expected, ItemRequest, OwnerApp, Plan};

const XCODE: OwnerApp = OwnerApp {
    name: "Xcode",
    programs: &["Xcode"],
    bundles: &["Xcode.app"],
    prefixes: &[],
};

/// Two hours ago: old enough for the recent-change check.
fn long_ago() -> SystemTime {
    SystemTime::now() - Duration::from_secs(2 * 60 * 60)
}

/// The real checks, with nothing kept and nothing running.
fn standard() -> Guards {
    Guards {
        recent_window: RECENT_CHANGE_WINDOW,
        protected_folders: Vec::new(),
        running: Running::Known(Vec::new()),
    }
}

struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
    dev: u64,
}

impl Home {
    fn new() -> Home {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(root.join(TRASH_DIR)).unwrap();
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

    fn file(&self, rel: &str, data: &[u8]) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }

    /// Sets every file and folder in the home back to [`long_ago`].
    fn age_everything(&self) {
        age(&self.root, long_ago());
    }

    fn request(&self, rel: &str, owners: &'static [OwnerApp]) -> ItemRequest {
        let scan = scanner::scan(&self.root, scanner::ScanOptions::default(), |_| {}).unwrap();
        let id = scan.find(Path::new(rel)).unwrap();
        let meta = self.path(rel).symlink_metadata().unwrap();
        ItemRequest {
            recommendation_id: rel.to_string(),
            path: self.path(rel),
            expected: Expected {
                size_bytes: scan.dirs[id].total_bytes,
                modified: meta.modified().unwrap(),
            },
            regenerable: true,
            selected_individually: true,
            owners,
            belongs_to: None,
            keep_copy: None,
        }
    }

    fn plan(&self, rel: &str, owners: &'static [OwnerApp]) -> Plan {
        let req = self.request(rel, owners);
        Plan::build(1, &self.root, self.dev, vec![req], SystemTime::now()).unwrap()
    }

    /// The dry run's answer for a one-item plan of `rel`.
    fn check(
        &self,
        rel: &str,
        owners: &'static [OwnerApp],
        guards: &Guards,
    ) -> Result<u64, ItemRefusal> {
        let plan = self.plan(rel, owners);
        let mut checks = preview(&plan, Mode::Trash, guards, SystemTime::now()).unwrap();
        checks.remove(0).result
    }

    /// Runs a one-item plan of `rel` and returns the refusal, checking that
    /// nothing was moved.
    fn refused_by_cleanup(
        &self,
        rel: &str,
        owners: &'static [OwnerApp],
        guards: &Guards,
    ) -> ItemRefusal {
        let plan = self.plan(rel, owners);
        let report = execute(plan, Mode::Trash, guards, SystemTime::now()).unwrap();
        assert!(self.path(rel).exists(), "{rel} moved");
        assert_eq!(fs::read_dir(self.path(TRASH_DIR)).unwrap().count(), 0);
        match &report.items[0].outcome {
            Outcome::Refused(r) => r.clone(),
            other => panic!("{rel} was not refused: {other:?}"),
        }
    }

    /// A git project at `rel` with a `.git` folder.
    fn repo(&self, rel: &str) {
        self.file(&format!("{rel}/.git/HEAD"), b"ref: refs/heads/main\n");
        self.file(&format!("{rel}/src/main.rs"), b"fn main() {}\n");
        self.file(&format!("{rel}/target/debug/app"), &[1u8; 8192]);
    }
}

/// Sets `path` and everything inside it to `when`. Symlinks are left alone.
fn age(path: &Path, when: SystemTime) {
    let meta = path.symlink_metadata().unwrap();
    if meta.file_type().is_symlink() {
        return;
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            age(&entry.unwrap().path(), when);
        }
    }
    fs::File::open(path).unwrap().set_modified(when).unwrap();
}

/// A version 2 git index holding `paths` at merge stage `stage`.
fn index(paths: &[&str], stage: u16) -> Vec<u8> {
    let mut out = b"DIRC".to_vec();
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&(paths.len() as u32).to_be_bytes());
    for path in paths {
        let start = out.len();
        out.extend_from_slice(&[0u8; 40]);
        out.extend_from_slice(&[0xcd; 20]);
        out.extend_from_slice(&((stage << 12) | path.len() as u16).to_be_bytes());
        out.extend_from_slice(path.as_bytes());
        let used = out.len() - start;
        out.resize(start + ((used + 8) & !7), 0);
    }
    out.extend_from_slice(&[0u8; 20]);
    out
}

// ---------------------------------------------------------------------------
// Changed recently
// ---------------------------------------------------------------------------

#[test]
fn an_item_changed_just_now_is_skipped_and_an_old_one_is_not() {
    let home = Home::new();
    home.file("Library/Caches/app/blob", &[3u8; 4096]);
    assert_eq!(
        home.check("Library/Caches/app", &[], &standard()),
        Err(ItemRefusal::ChangedRecently)
    );
    home.age_everything();
    assert!(home.check("Library/Caches/app", &[], &standard()).is_ok());
}

#[test]
fn a_new_file_deep_inside_an_old_folder_is_noticed() {
    let home = Home::new();
    home.file("Library/Caches/app/a/b/old", &[3u8; 4096]);
    home.age_everything();
    home.file("Library/Caches/app/a/b/new", &[4u8; 10]);
    // Every folder gets its old time back; only the new file is recent.
    for dir in [
        "Library/Caches/app/a/b",
        "Library/Caches/app/a",
        "Library/Caches/app",
    ] {
        fs::File::open(home.path(dir))
            .unwrap()
            .set_modified(long_ago())
            .unwrap();
    }
    assert_eq!(
        home.refused_by_cleanup("Library/Caches/app", &[], &standard()),
        ItemRefusal::ChangedRecently
    );
}

#[test]
fn a_time_in_the_future_counts_as_recent() {
    let home = Home::new();
    home.file("Library/Caches/app/blob", &[3u8; 4096]);
    home.age_everything();
    let soon = SystemTime::now() + Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(home.path("Library/Caches/app/blob"))
        .unwrap()
        .set_modified(soon)
        .unwrap();
    assert_eq!(
        home.check("Library/Caches/app", &[], &standard()),
        Err(ItemRefusal::ChangedRecently)
    );
}

// ---------------------------------------------------------------------------
// The owner app is open
// ---------------------------------------------------------------------------

#[test]
fn an_item_is_skipped_while_its_app_is_open() {
    let home = Home::new();
    home.file(
        "Library/Developer/Xcode/DerivedData/App-abc/Build/x",
        &[1u8; 4096],
    );
    home.age_everything();
    let rel = "Library/Developer/Xcode/DerivedData/App-abc";
    let open = Guards {
        running: Running::Known(vec![PathBuf::from(
            "/Applications/Xcode.app/Contents/MacOS/Xcode",
        )]),
        ..standard()
    };
    let refusal = ItemRefusal::AppOpen {
        app: "Xcode".to_string(),
    };
    assert_eq!(home.check(rel, &[XCODE], &open), Err(refusal.clone()));
    assert_eq!(home.refused_by_cleanup(rel, &[XCODE], &open), refusal);
    assert_eq!(
        refusal.to_string(),
        "Xcode is open and may be using it, so quit Xcode and try again"
    );
    // Closed: it would move. With no owner, the app being open doesn't matter.
    assert!(home.check(rel, &[XCODE], &standard()).is_ok());
    assert!(home.check(rel, &[], &open).is_ok());
}

#[test]
fn if_running_apps_cannot_be_listed_items_with_an_owner_are_skipped() {
    let home = Home::new();
    home.file("Library/Caches/app/blob", &[1u8; 4096]);
    home.age_everything();
    let unknown = Guards {
        running: Running::Unknown,
        ..standard()
    };
    assert_eq!(
        home.check("Library/Caches/app", &[XCODE], &unknown),
        Err(ItemRefusal::AppNotChecked {
            app: "Xcode".to_string()
        })
    );
    assert!(home.check("Library/Caches/app", &[], &unknown).is_ok());
}

#[test]
fn opening_the_app_after_the_dry_run_still_stops_the_cleanup() {
    let home = Home::new();
    home.file("Library/Caches/app/blob", &[1u8; 4096]);
    home.age_everything();
    let plan = home.plan("Library/Caches/app", &[XCODE]);
    let checks = preview(&plan, Mode::Trash, &standard(), SystemTime::now()).unwrap();
    assert!(checks[0].result.is_ok());
    let open = Guards {
        running: Running::Known(vec![PathBuf::from(
            "/Applications/Xcode.app/Contents/MacOS/Xcode",
        )]),
        ..standard()
    };
    let report = execute(plan, Mode::Trash, &open, SystemTime::now()).unwrap();
    assert!(matches!(
        report.items[0].outcome,
        Outcome::Refused(ItemRefusal::AppOpen { .. })
    ));
    assert!(home.path("Library/Caches/app/blob").exists());
}

// ---------------------------------------------------------------------------
// Folders the user keeps
// ---------------------------------------------------------------------------

#[test]
fn items_in_or_holding_a_kept_folder_are_skipped() {
    let home = Home::new();
    home.file("Developer/keep/target/debug/app", &[1u8; 4096]);
    home.file("Developer/other/target/debug/app", &[1u8; 4096]);
    home.age_everything();
    let keep = Guards {
        protected_folders: vec![home.path("Developer/keep")],
        ..standard()
    };
    assert_eq!(
        home.refused_by_cleanup("Developer/keep/target", &[], &keep),
        ItemRefusal::InFolderYouKeep
    );
    assert_eq!(
        home.check("Developer", &[], &keep),
        Err(ItemRefusal::HoldsFolderYouKeep)
    );
    assert!(home.check("Developer/other/target", &[], &keep).is_ok());
}

// ---------------------------------------------------------------------------
// Git projects
// ---------------------------------------------------------------------------

#[test]
fn an_ignored_build_folder_in_a_git_project_can_go() {
    let home = Home::new();
    home.repo("Developer/app");
    home.file("Developer/app/.gitignore", b"/target\n");
    home.file(
        "Developer/app/.git/index",
        &index(&["src/main.rs", ".gitignore"], 0),
    );
    home.age_everything();
    assert!(home.check("Developer/app/target", &[], &standard()).is_ok());
}

#[test]
fn a_build_folder_the_project_does_not_ignore_is_skipped() {
    let home = Home::new();
    home.repo("Developer/app");
    home.file("Developer/app/.gitignore", b"*.log\n");
    home.age_everything();
    assert_eq!(
        home.refused_by_cleanup("Developer/app/target", &[], &standard()),
        ItemRefusal::Git(GitConcern::NotIgnored)
    );
}

#[test]
fn a_build_folder_un_ignored_with_a_bang_is_skipped() {
    let home = Home::new();
    home.repo("Developer/app");
    home.file("Developer/app/.gitignore", b"target\n!target\n");
    home.age_everything();
    assert_eq!(
        home.check("Developer/app/target", &[], &standard()),
        Err(ItemRefusal::Git(GitConcern::NotIgnored))
    );
}

#[test]
fn ignore_files_outside_the_project_count() {
    // info/exclude
    let home = Home::new();
    home.repo("Developer/app");
    home.file("Developer/app/.git/info/exclude", b"target/\n");
    home.age_everything();
    assert!(home.check("Developer/app/target", &[], &standard()).is_ok());

    // The user's global ignore file, in its default place and via ~/.gitconfig.
    let home = Home::new();
    home.repo("Developer/app");
    home.file(".config/git/ignore", b"target\n");
    home.age_everything();
    assert!(home.check("Developer/app/target", &[], &standard()).is_ok());

    let home = Home::new();
    home.repo("Developer/app");
    home.file(
        ".gitconfig",
        b"[user]\n\tname = Sam\n[core]\n\texcludesFile = ~/.my-ignores\n",
    );
    home.file(".my-ignores", b"/target/\n");
    home.age_everything();
    assert!(home.check("Developer/app/target", &[], &standard()).is_ok());
}

#[test]
fn a_symlinked_gitignore_is_not_followed() {
    let home = Home::new();
    home.repo("Developer/app");
    home.file("elsewhere/ignore", b"target\n");
    std::os::unix::fs::symlink(
        home.path("elsewhere/ignore"),
        home.path("Developer/app/.gitignore"),
    )
    .unwrap();
    home.age_everything();
    assert_eq!(
        home.check("Developer/app/target", &[], &standard()),
        Err(ItemRefusal::Git(GitConcern::NotIgnored))
    );
}

#[test]
fn a_build_folder_with_files_saved_in_git_is_skipped() {
    let home = Home::new();
    home.repo("Developer/app");
    home.file("Developer/app/.gitignore", b"target/\n");
    home.file(
        "Developer/app/.git/index",
        &index(&["src/main.rs", "target/debug/app"], 0),
    );
    home.age_everything();
    assert_eq!(
        home.refused_by_cleanup("Developer/app/target", &[], &standard()),
        ItemRefusal::Git(GitConcern::Tracked)
    );
}

#[test]
fn a_project_partway_through_a_merge_is_left_alone() {
    for marker in ["MERGE_HEAD", "rebase-merge/head-name", "CHERRY_PICK_HEAD"] {
        let home = Home::new();
        home.repo("Developer/app");
        home.file("Developer/app/.gitignore", b"target/\n");
        home.file(&format!("Developer/app/.git/{marker}"), b"0123\n");
        home.age_everything();
        assert_eq!(
            home.check("Developer/app/target", &[], &standard()),
            Err(ItemRefusal::Git(GitConcern::Unfinished)),
            "{marker}"
        );
    }
    let home = Home::new();
    home.repo("Developer/app");
    home.file("Developer/app/.gitignore", b"target/\n");
    home.file("Developer/app/.git/index", &index(&["src/main.rs"], 2));
    home.age_everything();
    assert_eq!(
        home.check("Developer/app/target", &[], &standard()),
        Err(ItemRefusal::Git(GitConcern::Unfinished))
    );
}

#[test]
fn an_index_that_cannot_be_read_skips_the_item() {
    let home = Home::new();
    home.repo("Developer/app");
    home.file("Developer/app/.gitignore", b"target/\n");
    home.file("Developer/app/.git/index", b"DIRC garbage");
    home.age_everything();
    assert_eq!(
        home.check("Developer/app/target", &[], &standard()),
        Err(ItemRefusal::Git(GitConcern::Unchecked))
    );
}

#[test]
fn a_linked_worktree_is_checked_through_its_git_file() {
    let home = Home::new();
    // The main repository, and a worktree whose .git is a file.
    home.file("Developer/app/.git/HEAD", b"ref: refs/heads/main\n");
    let gitdir = home.path("Developer/app/.git/worktrees/feature");
    home.file(
        "Developer/app/.git/worktrees/feature/HEAD",
        b"ref: refs/heads/feature\n",
    );
    home.file("Developer/app/.git/worktrees/feature/commondir", b"../..\n");
    home.file(
        "Developer/feature/.git",
        format!("gitdir: {}\n", gitdir.display()).as_bytes(),
    );
    home.file("Developer/feature/target/debug/app", &[1u8; 4096]);
    home.file("Developer/app/.git/info/exclude", b"/target\n");
    home.age_everything();
    assert!(home
        .check("Developer/feature/target", &[], &standard())
        .is_ok());

    // A merge underway in that worktree's own git folder.
    home.file("Developer/app/.git/worktrees/feature/MERGE_HEAD", b"0123\n");
    home.age_everything();
    assert_eq!(
        home.check("Developer/feature/target", &[], &standard()),
        Err(ItemRefusal::Git(GitConcern::Unfinished))
    );
}

#[test]
fn reasons_are_plain_words_without_paths() {
    let all = [
        ItemRefusal::InFolderYouKeep,
        ItemRefusal::HoldsFolderYouKeep,
        ItemRefusal::AppOpen {
            app: "Xcode".into(),
        },
        ItemRefusal::AppNotChecked {
            app: "Xcode".into(),
        },
        ItemRefusal::ChangedRecently,
        ItemRefusal::Git(GitConcern::Unfinished),
        ItemRefusal::Git(GitConcern::Tracked),
        ItemRefusal::Git(GitConcern::NotIgnored),
        ItemRefusal::Git(GitConcern::Unchecked),
    ];
    for r in all {
        let text = r.to_string();
        assert!(!text.contains('/'), "{text}");
        assert!(text.len() < 120, "{text}");
    }
}
