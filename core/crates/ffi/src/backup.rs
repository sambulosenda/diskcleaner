//! Which device an iPhone or iPad backup belongs to, for the warning the app
//! shows before such a backup goes to the Trash.
//!
//! Read only. The backup folder is checked by `safety::validate` first (no
//! symlink anywhere in the path, same volume, not blocked), and then
//! `analysers::ios_backup` reads its `Info.plist` without following a
//! symlink.

use classifier::ClassifiedItem;

use crate::run::unix_seconds;
use crate::types::DeviceBackup;
use crate::views::Finished;

/// The rule for iPhone and iPad backups (`rules/everyday.toml`).
pub(crate) const DEVICE_BACKUP_RULE: &str = "ios.device_backup";

/// For an iPhone or iPad backup, what its `Info.plist` says about the
/// device. `None` for every other item.
///
/// A backup whose `Info.plist` can't be read still gets `Some`, with both
/// parts empty, so the app always shows the warning for a backup.
pub(crate) fn device_backup(f: &Finished, item: &ClassifiedItem) -> Option<DeviceBackup> {
    if item.rule_id != DEVICE_BACKUP_RULE {
        return None;
    }
    let unknown = DeviceBackup {
        device_name: None,
        last_backup_unix_seconds: None,
    };
    // Chosen alone: this check only decides whether the folder is safe to
    // read, not whether it may move.
    if let Err(refusal) = safety::validate(&f.home, f.root_dev, &item.path, true) {
        tracing::warn!(%refusal, "backup folder not read");
        return Some(unknown);
    }
    match analysers::ios_backup::read_backup_info(&item.path) {
        Ok(info) => Some(DeviceBackup {
            device_name: info.device_name,
            last_backup_unix_seconds: info.last_backup.map(unix_seconds),
        }),
        Err(err) => {
            tracing::warn!(%err, "backup Info.plist not read");
            Some(unknown)
        }
    }
}
