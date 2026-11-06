//! The JavaScript screen: every `node_modules` under the scan root with
//! when its project was last worked on (PRD D2), and cleaning one through
//! the usual preview, Trash move and put back.

use std::collections::BTreeSet;
use std::fs::{self, File, FileTimes};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fixtures::paths::{
    JS_PROJECTS, JS_PROJECTS_DOCS_AND_HOME, JS_PROJECTS_ELSEWHERE, JS_STALE_AGE_DAYS,
};
use roomy_ffi::{
    ActivitySource, CleanupCheck, CleanupChoice, Mode, PackageManager, RoomyCore, ScanHandle,
    ScanPhase, Sort,
};

struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    core: Arc<RoomyCore>,
}

fn setup() -> Setup {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    fs::create_dir(root.join(".Trash")).unwrap();
    let core = RoomyCore::with_home(root.clone()).unwrap();
    // Fixture homes are made just now, and the test machine runs its own
    // programs: switch off those two extra checks (tested in extra_checks.rs).
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

fn days_ago(unix: i64) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    ((now - unix).max(0) as u64) / 86_400
}

fn set_modified(path: &std::path::Path, days: u64) {
    let file = File::options().write(true).open(path).unwrap();
    let t = SystemTime::now() - Duration::from_secs(days * 86_400);
    file.set_times(FileTimes::new().set_modified(t)).unwrap();
}

#[test]
fn every_node_modules_folder_is_a_project_row() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let js = s.core.javascript_projects(h).unwrap();

    let folders: BTreeSet<String> = js
        .projects
        .iter()
        .map(|p| p.folder.clone().unwrap())
        .collect();
    let want: BTreeSet<String> = JS_PROJECTS
        .iter()
        .chain(JS_PROJECTS_ELSEWHERE.iter())
        .chain(JS_PROJECTS_DOCS_AND_HOME.iter())
        .map(|p| format!("~/{p}"))
        .collect();
    assert_eq!(folders, want);

    // Each row is an item of the scan, with the same summary.
    let all = s
        .core
        .list_items(h, None, None, Sort::LargestFirst)
        .unwrap();
    for p in &js.projects {
        assert!(all.contains(&p.item), "{p:?}");
        assert!(p.has_manifest, "{p:?}");
        assert!(p.last_activity_unix_seconds.is_some(), "{p:?}");
        assert_eq!(
            &p.name,
            p.folder.as_deref().unwrap().rsplit('/').next().unwrap()
        );
    }
    // Largest first.
    assert!(js
        .projects
        .windows(2)
        .all(|w| w[0].item.size_bytes >= w[1].item.size_bytes));

    // Caches are the other JavaScript items, and the total is exact.
    assert!(!js.caches.is_empty());
    let rows: u64 = js.projects.iter().map(|p| p.item.size_bytes).sum::<u64>()
        + js.caches.iter().map(|c| c.size_bytes).sum::<u64>();
    assert_eq!(js.total_bytes, rows);
    for c in &js.caches {
        let d = s.core.item_detail(h, c.id.clone()).unwrap();
        assert!(d.rule_id.starts_with("js."), "{}", d.rule_id);
        assert!(!d.rule_id.starts_with("js.node_modules"), "{}", d.rule_id);
    }
}

#[test]
fn the_stale_project_shows_its_age_from_package_json() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let js = s.core.javascript_projects(h).unwrap();
    let c = js
        .projects
        .iter()
        .find(|p| p.folder.as_deref() == Some("~/projects/c"))
        .unwrap();
    assert_eq!(c.activity_source, Some(ActivitySource::Manifest));
    assert_eq!(
        days_ago(c.last_activity_unix_seconds.unwrap()),
        JS_STALE_AGE_DAYS
    );
    assert_eq!(c.package_manager, None);
    assert_eq!(c.install_command, None);
}

#[test]
fn lock_files_and_git_move_the_date_and_name_the_tool() {
    let s = setup();
    let b = s.root.join("projects/b");
    set_modified(&b.join("package.json"), 300);
    fs::write(b.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
    set_modified(&b.join("pnpm-lock.yaml"), 40);
    fs::create_dir_all(b.join(".git")).unwrap();
    fs::write(b.join(".git/index"), b"DIRC").unwrap();
    set_modified(&b.join(".git/index"), 3);

    let h = finished_scan(&s.core, Mode::Developer);
    let js = s.core.javascript_projects(h).unwrap();
    let p = js
        .projects
        .iter()
        .find(|p| p.folder.as_deref() == Some("~/projects/b"))
        .unwrap();
    assert_eq!(p.activity_source, Some(ActivitySource::Git));
    assert_eq!(days_ago(p.last_activity_unix_seconds.unwrap()), 3);
    assert_eq!(p.package_manager, Some(PackageManager::Pnpm));
    assert_eq!(p.install_command.as_deref(), Some("pnpm install"));
}

#[test]
fn simple_mode_shows_no_project_folders() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Simple);
    let js = s.core.javascript_projects(h).unwrap();
    // The JavaScript rules are Developer mode only, and no row may carry a
    // path in simple mode anyway.
    assert!(js.projects.iter().all(|p| p.folder.is_none()));
    assert!(js.projects.is_empty());
}

#[test]
fn a_project_row_cleans_up_like_any_item_and_comes_back() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    let js = s.core.javascript_projects(h).unwrap();
    let c = js
        .projects
        .iter()
        .find(|p| p.folder.as_deref() == Some("~/projects/c"))
        .unwrap();
    let modules = s.root.join("projects/c/node_modules");
    assert!(modules.is_dir());

    let preview = s
        .core
        .preview_cleanup(
            h,
            vec![CleanupChoice {
                item_id: c.item.id.clone(),
                chosen_alone: true,
            }],
        )
        .unwrap();
    assert_eq!(preview.items.len(), 1);
    assert_eq!(preview.items[0].check, CleanupCheck::WillMove);
    assert!(modules.is_dir(), "the preview moved something");

    let result = s.core.run_cleanup(preview.plan.unwrap()).unwrap();
    assert!(!modules.exists());
    // The project itself stays.
    assert!(s.root.join("projects/c/package.json").is_file());
    let history = s.core.cleanup_history(10);
    assert_eq!(history.len(), 1);

    let back = s.core.put_back_cleanup(result.cleanup.unwrap()).unwrap();
    assert!(back.items.iter().all(|i| i.done), "{back:?}");
    assert!(modules.is_dir());
}
