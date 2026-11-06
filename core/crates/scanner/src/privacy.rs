//! Read-only checks of the folders macOS keeps private.
//!
//! macOS privacy protection (TCC) stops an app listing some folders inside
//! home, such as Mail, Messages, Safari data and the Trash, until the user
//! turns on Full Disk Access for it. The folders themselves still exist and
//! `lstat` works on them; only listing their contents fails with "operation
//! not permitted".
//!
//! [`full_disk_access`] lists a few of those folders to find out whether
//! the app has Full Disk Access right now. [`area_of`] names the private
//! area an unreadable folder belongs to, so the app can say in plain words
//! what was not scanned.
//!
//! Everything here only reads metadata and folder listings. Nothing is
//! opened for writing, and no symlink is followed.

use std::fs;
use std::io;
use std::path::Path;

/// Whether the app can read the folders macOS keeps private.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Every private folder that exists could be listed.
    Granted,
    /// At least one private folder that exists could not be listed.
    NotGranted,
    /// None of the folders checked exist (not a Mac, or a test home), so
    /// there is nothing to tell either way.
    Unknown,
}

/// Folders, relative to home, that macOS lets an app list only with Full
/// Disk Access. A folder missing from a Mac is skipped. The first one is
/// on every Mac since macOS 10.14.
pub const PROBES: &[&str] = &[
    "Library/Application Support/com.apple.TCC",
    "Library/Safari",
    "Library/Mail",
    "Library/Messages",
    ".Trash",
];

/// What one probe found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe {
    /// The folder is missing, or is not a real folder (a symlink, a file).
    Missing,
    /// The folder could be listed.
    Readable,
    /// Listing the folder was refused.
    Refused,
}

/// Checks whether the app has Full Disk Access, by listing each of
/// [`PROBES`] inside `home`. Reads only.
pub fn full_disk_access(home: &Path) -> Access {
    let mut readable = false;
    for probe in PROBES {
        match probe_one(&home.join(probe)) {
            Probe::Refused => return Access::NotGranted,
            Probe::Readable => readable = true,
            Probe::Missing => {}
        }
    }
    if readable {
        Access::Granted
    } else {
        Access::Unknown
    }
}

/// Lists one folder without following a symlink.
fn probe_one(path: &Path) -> Probe {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {}
        _ => return Probe::Missing,
    }
    match fs::read_dir(path) {
        Ok(mut entries) => match entries.next() {
            Some(Err(err)) if refused(&err) => Probe::Refused,
            _ => Probe::Readable,
        },
        Err(err) if refused(&err) => Probe::Refused,
        // Gone between the two calls, or some other error: not a privacy
        // refusal, so it says nothing about Full Disk Access.
        Err(_) => Probe::Missing,
    }
}

/// Whether an error is a permission refusal (EPERM or EACCES).
fn refused(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied
}

/// A private area of home that a scan may not be able to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Area {
    /// `~/Library/Mail`
    Mail,
    /// `~/Library/Messages`
    Messages,
    /// `~/Library/Safari` and Safari's own container.
    Safari,
    /// iPhone and iPad backups, `~/Library/Application Support/MobileSync`.
    DeviceBackups,
    /// Photos libraries in `~/Pictures`.
    Photos,
    /// Contacts, Calendars and Reminders data.
    ContactsAndCalendars,
    /// The Trash, `~/.Trash`.
    Trash,
    /// Other apps' private data: `~/Library/Containers` and
    /// `~/Library/Group Containers`.
    AppData,
    /// Anything else.
    Other,
}

/// Names the private area `path` belongs to. `path` is absolute and inside
/// `home`; a path outside `home` is [`Area::Other`]. Pure: no filesystem
/// access.
pub fn area_of(home: &Path, path: &Path) -> Area {
    let Ok(rel) = path.strip_prefix(home) else {
        return Area::Other;
    };
    let parts: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    match parts.as_slice() {
        [".Trash", ..] => Area::Trash,
        ["Library", "Mail", ..] => Area::Mail,
        ["Library", "Messages", ..] => Area::Messages,
        ["Library", "Safari", ..] => Area::Safari,
        ["Library", "Containers", c, ..] if c.starts_with("com.apple.Safari") => Area::Safari,
        ["Library", "Application Support", "MobileSync", ..] => Area::DeviceBackups,
        ["Library", "Application Support", "AddressBook", ..]
        | ["Library", "Calendars", ..]
        | ["Library", "Reminders", ..] => Area::ContactsAndCalendars,
        ["Library", "Containers" | "Group Containers", _, ..] => Area::AppData,
        ["Pictures", lib, ..] if lib.ends_with(".photoslibrary") => Area::Photos,
        _ => Area::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn home() -> PathBuf {
        PathBuf::from("/Users/sam")
    }

    #[test]
    fn names_each_private_area() {
        let cases = [
            (".Trash/old", Area::Trash),
            ("Library/Mail/V10", Area::Mail),
            ("Library/Messages", Area::Messages),
            ("Library/Safari/Favicon Cache", Area::Safari),
            ("Library/Containers/com.apple.Safari/Data", Area::Safari),
            (
                "Library/Application Support/MobileSync/Backup",
                Area::DeviceBackups,
            ),
            (
                "Library/Application Support/AddressBook",
                Area::ContactsAndCalendars,
            ),
            ("Library/Calendars", Area::ContactsAndCalendars),
            ("Library/Reminders", Area::ContactsAndCalendars),
            ("Library/Containers/com.apple.stocks/Data", Area::AppData),
            (
                "Library/Group Containers/group.com.apple.notes",
                Area::AppData,
            ),
            (
                "Pictures/Photos Library.photoslibrary/database",
                Area::Photos,
            ),
            ("Pictures/Holiday", Area::Other),
            ("Library/Caches/com.example", Area::Other),
            ("Library/Containers", Area::Other),
        ];
        for (rel, want) in cases {
            assert_eq!(area_of(&home(), &home().join(rel)), want, "{rel}");
        }
    }

    #[test]
    fn a_path_outside_home_is_other() {
        assert_eq!(area_of(&home(), Path::new("/Library/Mail")), Area::Other);
    }

    #[test]
    fn a_home_with_none_of_the_folders_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(full_disk_access(dir.path()), Access::Unknown);
    }

    #[test]
    fn readable_folders_mean_granted() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Library/Mail")).unwrap();
        fs::create_dir_all(dir.path().join(".Trash")).unwrap();
        assert_eq!(full_disk_access(dir.path()), Access::Granted);
    }

    #[test]
    fn a_symlink_is_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Elsewhere")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("Elsewhere"), dir.path().join(".Trash"))
            .unwrap();
        assert_eq!(full_disk_access(dir.path()), Access::Unknown);
    }

    #[test]
    fn a_refused_folder_means_not_granted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mail = dir.path().join("Library/Mail");
        fs::create_dir_all(&mail).unwrap();
        fs::create_dir_all(dir.path().join(".Trash")).unwrap();
        fs::set_permissions(&mail, fs::Permissions::from_mode(0o000)).unwrap();
        let still_readable = fs::read_dir(&mail).is_ok();
        let got = full_disk_access(dir.path());
        fs::set_permissions(&mail, fs::Permissions::from_mode(0o755)).unwrap();
        if still_readable {
            eprintln!("skipped: running as root, so the folder is still readable");
            return;
        }
        assert_eq!(got, Access::NotGranted);
    }
}
