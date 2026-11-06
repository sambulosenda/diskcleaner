//! The System Data breakdown end to end: a pretend startup disk in a
//! temporary folder, and a stand-in `tmutil` that answers from a file and
//! logs every call.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use roomy_ffi::{
    Mode, RoomyCore, SnapshotStatus, SystemData, SystemDataKind, SystemDataOwner,
    SystemDataPlaceState,
};

const SNAPSHOTS: &str = "Snapshots for disk /:
com.apple.TimeMachine.2026-11-04-080000.local
com.apple.TimeMachine.2026-11-06-093012.local
";

struct Setup {
    _dir: tempfile::TempDir,
    base: PathBuf,
    bin: PathBuf,
    core: Arc<RoomyCore>,
}

fn write(path: &Path, len: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![1u8; len]).unwrap();
}

impl Setup {
    fn new(tmutil_answer: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        let home = base.join("home");
        let disk = base.join("disk");
        let temp = disk.join("private/var/folders/zz/me");
        write(&home.join("Library/Caches/com.example/cache.db"), 40_000);
        write(&home.join("Library/Logs/app.log"), 8_000);
        write(
            &home.join("Library/iTunes/iPhone Software Updates/iPhone.ipsw"),
            30_000,
        );
        write(&disk.join("Library/Caches/com.apple.shared/blob"), 12_000);
        write(&disk.join("private/var/log/system.log"), 4_000);
        write(&disk.join("private/var/vm/sleepimage"), 64_000);
        write(&disk.join("private/var/vm/swapfile0"), 16_000);
        write(&temp.join("C/com.example/x"), 4_000);
        write(&temp.join("T/tmp.bin"), 4_000);

        let bin = base.join("bin");
        fs::create_dir(&bin).unwrap();
        if let Some(answer) = tmutil_answer {
            fs::write(bin.join("answer.txt"), answer).unwrap();
            let script = format!(
                "#!/bin/sh\nd='{d}'\necho \"$*\" >> \"$d/calls.txt\"\n/bin/cat \"$d/answer.txt\"\n",
                d = bin.display()
            );
            let tmutil = bin.join("tmutil");
            fs::write(&tmutil, script).unwrap();
            fs::set_permissions(&tmutil, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let core = RoomyCore::with_home(home).unwrap();
        core.set_system_roots_for_testing(disk, Some(temp));
        core.set_tmutil_search_path_for_testing(bin.as_os_str().to_owned());
        Self {
            _dir: dir,
            base,
            bin,
            core,
        }
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.bin.join("calls.txt")).unwrap_or_default()
    }

    /// Every file under the temporary folder with its length and contents,
    /// to show nothing changed. Leaves out the stand-in's own call log.
    fn snapshot_tree(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![self.base.clone()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    stack.push(path);
                } else if path.file_name().is_some_and(|n| n != "calls.txt") {
                    out.insert(path.clone(), fs::read(&path).unwrap());
                }
            }
        }
        out
    }
}

fn part(data: &SystemData, kind: SystemDataKind) -> &roomy_ffi::SystemDataPart {
    data.parts.iter().find(|p| p.kind == kind).unwrap()
}

#[test]
fn every_part_is_measured_and_nothing_changes() {
    let s = Setup::new(Some(SNAPSHOTS));
    let before = s.snapshot_tree();
    let data = s.core.system_data(Mode::Simple);
    assert_eq!(s.snapshot_tree(), before, "the breakdown changed a file");

    let kinds: Vec<SystemDataKind> = data.parts.iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SystemDataKind::Caches,
            SystemDataKind::Logs,
            SystemDataKind::SoftwareUpdates,
            SystemDataKind::TemporaryFiles,
            SystemDataKind::Swap,
            SystemDataKind::SleepImage,
        ]
    );
    assert!(part(&data, SystemDataKind::Caches).size_bytes >= 52_000);
    assert!(part(&data, SystemDataKind::Logs).size_bytes >= 12_000);
    assert!(part(&data, SystemDataKind::SoftwareUpdates).size_bytes >= 30_000);
    assert!(part(&data, SystemDataKind::TemporaryFiles).size_bytes >= 8_000);
    assert!(part(&data, SystemDataKind::Swap).size_bytes >= 16_000);
    assert!(part(&data, SystemDataKind::SleepImage).size_bytes >= 64_000);
    let sum: u64 = data.parts.iter().map(|p| p.size_bytes).sum();
    assert_eq!(data.total_bytes, sum);

    let caches = part(&data, SystemDataKind::Caches);
    assert_eq!(caches.places[0].owner, SystemDataOwner::You);
    assert_eq!(caches.places[1].owner, SystemDataOwner::System);
    // Not on the pretend disk.
    let logs = part(&data, SystemDataKind::Logs);
    let shared = logs.places.iter().find(|p| p.id == "shared_logs").unwrap();
    assert_eq!(shared.state, SystemDataPlaceState::Missing);
    assert_eq!(shared.size_bytes, 0);

    assert_eq!(data.snapshots.status, SnapshotStatus::Listed);
    assert_eq!(data.snapshots.count, 2);
    // 2026-11-04 08:00:00 and 2026-11-06 09:30:12, read as UTC.
    assert_eq!(data.snapshots.oldest_unix_seconds, Some(1_793_779_200));
    assert_eq!(data.snapshots.newest_unix_seconds, Some(1_793_957_412));
    assert_eq!(s.calls().trim(), "listlocalsnapshots /");
}

#[test]
fn paths_are_shown_in_developer_mode_only() {
    let s = Setup::new(Some(SNAPSHOTS));
    let simple = s.core.system_data(Mode::Simple);
    assert!(simple
        .parts
        .iter()
        .flat_map(|p| &p.places)
        .all(|p| p.path.is_none()));

    let dev = s.core.system_data(Mode::Developer);
    let paths: Vec<String> = dev
        .parts
        .iter()
        .flat_map(|p| &p.places)
        .filter_map(|p| p.path.clone())
        .collect();
    assert!(paths.contains(&"~/Library/Caches".to_string()));
    assert!(paths.contains(&"/Library/Caches".to_string()));
    assert!(paths.contains(&"/private/var/vm/sleepimage".to_string()));
    // Nothing leaks the real location of the temporary folders.
    let base = s.base.to_string_lossy().to_string();
    assert!(paths.iter().all(|p| !p.contains(&base)), "{paths:?}");
}

#[test]
fn snapshots_that_cannot_be_listed_are_explained() {
    let missing = Setup::new(None);
    let data = missing.core.system_data(Mode::Simple);
    let SnapshotStatus::Unavailable { reason } = &data.snapshots.status else {
        panic!("expected unavailable");
    };
    assert!(!reason.is_empty() && !reason.contains('/'));
    assert_eq!(data.snapshots.count, 0);
    // The rest is still measured.
    assert!(data.total_bytes > 0);

    let odd = Setup::new(Some("Something Roomy has never seen\n"));
    let data = odd.core.system_data(Mode::Simple);
    assert!(matches!(
        data.snapshots.status,
        SnapshotStatus::Unavailable { .. }
    ));
}
