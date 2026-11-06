//! The device details shown in the warning before an iPhone or iPad backup
//! goes to the Trash: the device name and last backup date, read from the
//! backup's `Info.plist`, in the detail panel and in the cleanup preview.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fixtures::paths::{BACKUP_DEVICE_IDS, MOBILESYNC_BACKUP};
use roomy_ffi::{
    CleanupCheck, CleanupChoice, DeviceBackup, Mode, RoomyCore, ScanHandle, ScanPhase, Sort,
};

const RULE: &str = "ios.device_backup";

struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    core: Arc<RoomyCore>,
}

fn setup() -> Setup {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    // The cleanup preview needs a Trash to check against.
    fs::create_dir(root.join(".Trash")).unwrap();
    let core = RoomyCore::with_home(root.clone()).unwrap();
    // Fixture homes are made just now, and the test machine runs its own
    // programs: switch off those two extra checks (tested in extra_checks.rs).
    core.set_recent_change_window_for_testing(std::time::Duration::ZERO);
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

/// Every item's id with its rule id and device details.
fn all(core: &RoomyCore, h: ScanHandle) -> Vec<(String, String, Option<DeviceBackup>)> {
    core.list_items(h, None, None, Sort::LargestFirst)
        .unwrap()
        .into_iter()
        .map(|i| {
            let d = core.item_detail(h, i.id.clone()).unwrap();
            (i.id, d.rule_id, d.device_backup)
        })
        .collect()
}

fn expected() -> BTreeMap<String, DeviceBackup> {
    BTreeMap::from([
        (
            "Family iPad".to_string(),
            DeviceBackup {
                device_name: Some("Family iPad".into()),
                last_backup_unix_seconds: Some(1_764_704_400),
            },
        ),
        (
            "Sam's iPhone".to_string(),
            DeviceBackup {
                device_name: Some("Sam's iPhone".into()),
                last_backup_unix_seconds: Some(1_793_348_100),
            },
        ),
    ])
}

#[test]
fn backups_name_their_device_in_both_modes() {
    let s = setup();
    for mode in [Mode::Simple, Mode::Developer] {
        let h = finished_scan(&s.core, mode);
        let mut found = BTreeMap::new();
        for (_, rule, device) in all(&s.core, h) {
            if rule == RULE {
                let device = device.expect("a backup always has device details");
                found.insert(device.device_name.clone().unwrap(), device);
            } else {
                assert_eq!(device, None, "{rule} is not a backup");
            }
        }
        assert_eq!(found, expected(), "{mode:?}");
    }
}

#[test]
fn the_cleanup_preview_names_the_device_too() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Simple);
    let items = all(&s.core, h);
    let choices: Vec<CleanupChoice> = items
        .iter()
        .filter(|(_, rule, _)| rule == RULE || rule == "browser.safari_cache")
        .map(|(id, _, _)| CleanupChoice {
            item_id: id.clone(),
            chosen_alone: true,
        })
        .collect();
    assert_eq!(choices.len(), 3);
    let preview = s.core.preview_cleanup(h, choices).unwrap();
    let mut found = BTreeMap::new();
    for row in preview.items {
        let rule = &items
            .iter()
            .find(|(id, _, _)| *id == row.item_id)
            .unwrap()
            .1;
        if rule == RULE {
            assert_eq!(row.check, CleanupCheck::WillMove);
            let device = row.device_backup.unwrap();
            found.insert(device.device_name.clone().unwrap(), device);
        } else {
            assert_eq!(row.device_backup, None);
        }
    }
    assert_eq!(found, expected());
    s.core.discard_cleanup(preview.plan.unwrap());
}

#[test]
fn an_unreadable_backup_still_gets_the_warning() {
    let s = setup();
    let h = finished_scan(&s.core, Mode::Developer);
    // Changed after the scan, so the backup is still matched by its rule.
    // (A symlinked Info.plist is covered by the analysers tests.)
    let info = s
        .root
        .join(MOBILESYNC_BACKUP)
        .join(BACKUP_DEVICE_IDS[1])
        .join("Info.plist");
    fs::write(info, b"not a plist").unwrap();
    let devices: Vec<DeviceBackup> = all(&s.core, h)
        .into_iter()
        .filter(|(_, rule, _)| rule == RULE)
        .map(|(_, _, d)| d.expect("a backup always has device details"))
        .collect();
    assert_eq!(devices.len(), 2);
    assert!(devices.contains(&DeviceBackup {
        device_name: None,
        last_backup_unix_seconds: None,
    }));
    assert_eq!(
        devices.iter().filter(|d| d.device_name.is_some()).count(),
        1
    );
}
