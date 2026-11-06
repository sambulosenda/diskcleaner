//! The Docker commands end to end, against a stand-in `docker` program.
//!
//! The stand-in is a small script in a temporary folder that answers each
//! query from a file and logs every call, so the tests can check exactly
//! which commands ran, with which arguments and what went to stdin.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use roomy_ffi::{CoreError, DockerCleanupKind, DockerPlanHandle, DockerStatus, RoomyCore};

const SYSTEM_DF: &str = r#"{"Active":"2","Reclaimable":"9.618GB (78%)","Size":"12.33GB","TotalCount":"14","Type":"Images"}
{"Active":"1","Reclaimable":"1.2MB (50%)","Size":"2.4MB","TotalCount":"3","Type":"Containers"}
{"Active":"1","Reclaimable":"512.3MB (40%)","Size":"1.28GB","TotalCount":"4","Type":"Local Volumes"}
{"Active":"0","Reclaimable":"3.1GB","Size":"3.1GB","TotalCount":"57","Type":"Build Cache"}
"#;
const IMAGES: &str = r#"{"CreatedSince":"5 weeks ago","ID":"aaa111","Repository":"<none>","Size":"1GB","Tag":"<none>"}
{"CreatedSince":"2 days ago","ID":"bbb222","Repository":"<none>","Size":"500MB","Tag":"<none>"}
"#;
const BUILDER_DU: &str = "ID  RECLAIMABLE  SIZE  LAST ACCESSED
abc123*  true  2GB  3 weeks ago
def456  false  1GB  1 minute ago
Reclaimable: 2GB
Total: 3GB
";

/// A home folder, a stand-in `docker`, and a core wired to both.
struct Setup {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    home: PathBuf,
    core: Arc<RoomyCore>,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("Library/Application Support")).unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let script = format!(
            "#!/bin/sh
d='{d}'
echo \"$*\" >> \"$d/calls.txt\"
case \"$1 $2\" in
  'system df') /bin/cat \"$d/df.txt\" ;;
  'image ls') /bin/cat \"$d/images.txt\" ;;
  'builder du') /bin/cat \"$d/du.txt\" ;;
  'image prune'|'builder prune')
    read answer
    echo \"stdin:$answer\" >> \"$d/calls.txt\"
    echo 'Deleted Images:'
    echo 'Total reclaimed space: 1.5GB' ;;
  *) echo 'unexpected command' >&2; exit 1 ;;
esac
",
            d = bin.display()
        );
        let docker = bin.join("docker");
        fs::write(&docker, script).unwrap();
        fs::set_permissions(&docker, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(bin.join("df.txt"), SYSTEM_DF).unwrap();
        fs::write(bin.join("images.txt"), IMAGES).unwrap();
        fs::write(bin.join("du.txt"), BUILDER_DU).unwrap();
        let core = RoomyCore::with_home(home.clone()).unwrap();
        core.set_docker_search_path_for_testing(bin.as_os_str().to_owned());
        Self {
            _dir: dir,
            bin,
            home,
            core,
        }
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.bin.join("calls.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn answer(&self, file: &str, text: &str) {
        fs::write(self.bin.join(file), text).unwrap();
    }
}

fn history_runs(home: &Path) -> Vec<history::CommandRun> {
    history::History::open_for_home(home)
        .unwrap()
        .recent_command_runs(10)
        .unwrap()
}

#[test]
fn overview_reads_system_df() {
    let s = Setup::new();
    let o = s.core.docker_overview();
    assert_eq!(o.status, DockerStatus::Ready);
    assert_eq!(o.images.count, 14);
    assert_eq!(o.images.reclaimable_bytes, 9_618_000_000);
    assert_eq!(o.volumes.size_bytes, 1_280_000_000);
    assert_eq!(o.build_cache.size_bytes, 3_100_000_000);
    assert_eq!(o.disk_image_bytes, None);
    assert_eq!(s.calls(), ["system df --format {{json .}}"]);
}

#[test]
fn overview_without_docker_says_so() {
    let s = Setup::new();
    let empty = tempfile::tempdir().unwrap();
    s.core
        .set_docker_search_path_for_testing(empty.path().as_os_str().to_owned());
    let o = s.core.docker_overview();
    assert_eq!(o.status, DockerStatus::NotInstalled);
    assert_eq!(o.images.size_bytes, 0);
}

#[test]
fn image_cleanup_previews_then_runs_once_and_is_recorded() {
    let s = Setup::new();
    let preview = s
        .core
        .preview_docker_cleanup(DockerCleanupKind::UnusedImages)
        .unwrap();
    assert_eq!(preview.command_line, "docker image prune");
    assert_eq!(preview.items.len(), 2);
    assert_eq!(preview.total_bytes, 1_500_000_000);
    assert_eq!(preview.expires_in_seconds, 600);
    let plan = preview.plan.unwrap();
    // Nothing ran but the read-only preview.
    assert_eq!(s.calls(), ["image ls --filter dangling=true --format json"]);

    let result = s.core.run_docker_cleanup(plan).unwrap();
    assert!(result.success);
    assert_eq!(result.reclaimed_bytes, Some(1_500_000_000));
    assert!(result.saved_to_history);
    assert_eq!(
        s.calls(),
        [
            "image ls --filter dangling=true --format json",
            "image ls --filter dangling=true --format json",
            "image prune",
            "stdin:y",
        ]
    );

    // Single use.
    assert!(matches!(
        s.core.run_docker_cleanup(plan),
        Err(CoreError::CleanupUnavailable { .. })
    ));
    assert_eq!(s.calls().len(), 4);

    let runs = history_runs(&s.home);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].command_id, "docker_image_prune");
    assert_eq!(runs[0].command_line, "docker image prune");
    assert!(runs[0].stdout.contains("Total reclaimed space"));
}

#[test]
fn build_cache_cleanup_runs_builder_prune_without_flags() {
    let s = Setup::new();
    let preview = s
        .core
        .preview_docker_cleanup(DockerCleanupKind::BuildCache)
        .unwrap();
    assert_eq!(preview.items.len(), 1);
    assert_eq!(preview.items[0].id, "abc123");
    s.core.run_docker_cleanup(preview.plan.unwrap()).unwrap();
    let calls = s.calls();
    assert_eq!(calls[2], "builder prune");
    assert!(calls
        .iter()
        .all(|c| !c.contains("-f") && !c.contains("--all") && !c.contains("volume")));
}

#[test]
fn something_new_since_the_review_stops_the_run() {
    let s = Setup::new();
    let preview = s
        .core
        .preview_docker_cleanup(DockerCleanupKind::UnusedImages)
        .unwrap();
    s.answer(
        "images.txt",
        &format!(
            "{IMAGES}{}\n",
            r#"{"CreatedSince":"now","ID":"ccc333","Repository":"<none>","Size":"9GB","Tag":"<none>"}"#
        ),
    );
    assert!(matches!(
        s.core.run_docker_cleanup(preview.plan.unwrap()),
        Err(CoreError::DockerUnavailable { .. })
    ));
    assert!(!s.calls().iter().any(|c| c.contains("prune")));
    assert!(history_runs(&s.home).is_empty());
}

#[test]
fn nothing_to_remove_gives_no_plan() {
    let s = Setup::new();
    s.answer("images.txt", "");
    let preview = s
        .core
        .preview_docker_cleanup(DockerCleanupKind::UnusedImages)
        .unwrap();
    assert_eq!(preview.plan, None);
    assert!(preview.items.is_empty());
}

#[test]
fn a_new_preview_or_discard_drops_the_old_plan() {
    let s = Setup::new();
    let first = s
        .core
        .preview_docker_cleanup(DockerCleanupKind::UnusedImages)
        .unwrap()
        .plan
        .unwrap();
    let second = s
        .core
        .preview_docker_cleanup(DockerCleanupKind::BuildCache)
        .unwrap()
        .plan
        .unwrap();
    assert_ne!(first, second);
    assert!(s.core.run_docker_cleanup(first).is_err());
    s.core.discard_docker_cleanup(second);
    assert!(s.core.run_docker_cleanup(second).is_err());
    s.core.discard_docker_cleanup(DockerPlanHandle { id: 999 });
    assert!(!s.calls().iter().any(|c| c.contains("prune")));
}

#[test]
fn a_stopped_docker_is_reported() {
    let s = Setup::new();
    let docker = s.bin.join("docker");
    fs::write(
        &docker,
        "#!/bin/sh\necho 'Cannot connect to the Docker daemon at unix:///x/docker.sock. Is the docker daemon running?' >&2\nexit 1\n",
    )
    .unwrap();
    assert_eq!(s.core.docker_overview().status, DockerStatus::NotRunning);
    let err = s
        .core
        .preview_docker_cleanup(DockerCleanupKind::BuildCache)
        .unwrap_err();
    let CoreError::DockerUnavailable { reason } = err else {
        panic!("expected docker_unavailable");
    };
    assert!(reason.contains("isn't running"));
    assert!(!reason.contains('/'));
}
