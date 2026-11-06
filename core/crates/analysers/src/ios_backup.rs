//! Reads the device name and last backup date from an iPhone or iPad
//! backup's `Info.plist`, so the app can say which device a backup belongs
//! to before the user moves it to the Trash.
//!
//! Read only. Nothing here changes a file. The reader never follows a
//! symlink: the backup folder must be a real folder, and `Info.plist` is
//! opened with `O_NOFOLLOW` and must be a regular file of a sensible size.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::SystemTime;

/// The file inside each backup folder that names the device.
pub const INFO_PLIST: &str = "Info.plist";

/// The largest `Info.plist` the reader opens. Real ones are well under
/// this; anything bigger is not read, so a strange file can't make the app
/// read gigabytes just to show a name.
pub const INFO_PLIST_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The longest device name the app is given, in characters.
pub const DEVICE_NAME_MAX_CHARS: usize = 60;

/// What a backup's `Info.plist` says about the device. Either part may be
/// missing if the file doesn't hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    /// The device's name, as the user set it on the device, made safe to
    /// show (no control or text-direction characters, at most
    /// [`DEVICE_NAME_MAX_CHARS`] characters).
    pub device_name: Option<String>,
    /// When the backup last finished.
    pub last_backup: Option<SystemTime>,
}

/// Why a backup's `Info.plist` could not be read.
#[derive(Debug, thiserror::Error)]
pub enum BackupInfoError {
    /// The backup folder is missing, is a symlink, or is not a folder.
    #[error("the backup folder is not a real folder")]
    NotAFolder,
    /// `Info.plist` is a symlink, a folder or some other special file.
    #[error("Info.plist is not a regular file")]
    NotAFile,
    /// `Info.plist` is bigger than [`INFO_PLIST_MAX_BYTES`].
    #[error("Info.plist is too large to read")]
    TooLarge,
    /// The file could not be opened or read.
    #[error("Info.plist could not be read: {0}")]
    Io(#[from] io::Error),
    /// The file is not a property list holding a dictionary.
    #[error("Info.plist is not a valid property list")]
    NotAPlist,
}

/// Reads the device name and last backup date from `backup_dir/Info.plist`.
///
/// `backup_dir` is one folder under `MobileSync/Backup`. Read only, and
/// never follows a symlink.
pub fn read_backup_info(backup_dir: &Path) -> Result<BackupInfo, BackupInfoError> {
    let dir_meta = fs::symlink_metadata(backup_dir).map_err(|_| BackupInfoError::NotAFolder)?;
    if !dir_meta.is_dir() {
        return Err(BackupInfoError::NotAFolder);
    }
    let bytes = read_capped(&backup_dir.join(INFO_PLIST))?;
    parse_backup_info(&bytes)
}

/// Opens `path` without following a symlink and reads it whole, refusing
/// anything that is not a regular file or is too large.
pub(crate) fn read_capped(path: &Path) -> Result<Vec<u8>, BackupInfoError> {
    let file = open_no_follow(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(BackupInfoError::NotAFile);
    }
    if meta.len() > INFO_PLIST_MAX_BYTES {
        return Err(BackupInfoError::TooLarge);
    }
    let mut bytes = Vec::new();
    // `take` guards against a file that grows after the size check.
    file.take(INFO_PLIST_MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > INFO_PLIST_MAX_BYTES {
        return Err(BackupInfoError::TooLarge);
    }
    Ok(bytes)
}

/// Opens `path` for reading. `O_NOFOLLOW` makes a symlink fail to open, and
/// `O_NONBLOCK` stops a named pipe from hanging the open; the regular-file
/// check after opening refuses both kinds of special file.
fn open_no_follow(path: &Path) -> Result<File, BackupInfoError> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|err| {
            if err.raw_os_error() == Some(libc::ELOOP) {
                BackupInfoError::NotAFile
            } else {
                BackupInfoError::Io(err)
            }
        })
}

/// Pulls the device name and last backup date out of `Info.plist` bytes
/// (XML or binary property list).
pub fn parse_backup_info(bytes: &[u8]) -> Result<BackupInfo, BackupInfoError> {
    let value =
        plist::Value::from_reader(Cursor::new(bytes)).map_err(|_| BackupInfoError::NotAPlist)?;
    let dict = value.as_dictionary().ok_or(BackupInfoError::NotAPlist)?;
    let name_of = |key: &str| {
        dict.get(key)
            .and_then(plist::Value::as_string)
            .and_then(clean_device_name)
    };
    let device_name = name_of("Device Name").or_else(|| name_of("Display Name"));
    let last_backup = dict
        .get("Last Backup Date")
        .and_then(plist::Value::as_date)
        .map(SystemTime::from);
    Ok(BackupInfo {
        device_name,
        last_backup,
    })
}

/// The device name made safe to show: control characters and characters
/// that change text direction are dropped (so a name can't make the
/// warning read differently), runs of spaces become one, and long names are
/// cut to [`DEVICE_NAME_MAX_CHARS`] characters with an ellipsis. Gives
/// `None` if nothing is left.
pub fn clean_device_name(raw: &str) -> Option<String> {
    let kept: String = raw
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|&c| !c.is_control() && !is_direction_or_invisible(c))
        .collect();
    let words: Vec<&str> = kept.split(' ').filter(|w| !w.is_empty()).collect();
    let joined = words.join(" ");
    if joined.is_empty() {
        return None;
    }
    if joined.chars().count() <= DEVICE_NAME_MAX_CHARS {
        return Some(joined);
    }
    let cut: String = joined.chars().take(DEVICE_NAME_MAX_CHARS - 1).collect();
    Some(format!("{}\u{2026}", cut.trim_end()))
}

/// Characters that change text direction or are invisible, which have no
/// place in a name shown in a warning.
fn is_direction_or_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn xml(entries: &str) -> Vec<u8> {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>{entries}</dict></plist>\n"
        )
        .into_bytes()
    }

    #[test]
    fn reads_name_and_date_from_xml() {
        let info = parse_backup_info(&xml("<key>Device Name</key><string>Sam's iPhone</string>\
             <key>Last Backup Date</key><date>2026-10-30T08:15:00Z</date>"))
        .unwrap();
        assert_eq!(info.device_name.as_deref(), Some("Sam's iPhone"));
        assert_eq!(
            info.last_backup,
            Some(UNIX_EPOCH + Duration::from_secs(1_793_348_100))
        );
    }

    #[test]
    fn reads_binary_plists() {
        let mut dict = plist::Dictionary::new();
        dict.insert("Device Name".into(), plist::Value::String("iPad".into()));
        let mut bytes = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_binary(&mut bytes)
            .unwrap();
        let info = parse_backup_info(&bytes).unwrap();
        assert_eq!(info.device_name.as_deref(), Some("iPad"));
        assert_eq!(info.last_backup, None);
    }

    #[test]
    fn falls_back_to_display_name() {
        let info = parse_backup_info(&xml("<key>Device Name</key><string>  </string>\
             <key>Display Name</key><string>Work iPhone</string>"))
        .unwrap();
        assert_eq!(info.device_name.as_deref(), Some("Work iPhone"));
    }

    #[test]
    fn missing_fields_are_none() {
        let info = parse_backup_info(&xml("<key>Other</key><integer>3</integer>")).unwrap();
        assert_eq!(
            info,
            BackupInfo {
                device_name: None,
                last_backup: None
            }
        );
    }

    #[test]
    fn wrong_types_are_none() {
        let info = parse_backup_info(&xml("<key>Device Name</key><integer>3</integer>\
             <key>Last Backup Date</key><string>yesterday</string>"))
        .unwrap();
        assert_eq!(info.device_name, None);
        assert_eq!(info.last_backup, None);
    }

    #[test]
    fn refuses_what_is_not_a_dictionary_plist() {
        assert!(matches!(
            parse_backup_info(b"not a plist"),
            Err(BackupInfoError::NotAPlist)
        ));
        let array = b"<?xml version=\"1.0\"?><plist version=\"1.0\"><array/></plist>";
        assert!(matches!(
            parse_backup_info(array),
            Err(BackupInfoError::NotAPlist)
        ));
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(
            clean_device_name("  Sam's\n\tiPhone \u{0007}").as_deref(),
            Some("Sam's iPhone")
        );
        assert_eq!(
            clean_device_name("enohP\u{202E}i s'maS").as_deref(),
            Some("enohPi s'maS")
        );
        assert_eq!(clean_device_name("\u{200B}\u{FEFF}"), None);
        assert_eq!(clean_device_name(""), None);
        let long = "x".repeat(200);
        let cut = clean_device_name(&long).unwrap();
        assert_eq!(cut.chars().count(), DEVICE_NAME_MAX_CHARS);
        assert!(cut.ends_with('\u{2026}'));
    }

    #[test]
    fn reads_a_real_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(INFO_PLIST),
            xml("<key>Device Name</key><string>Family iPad</string>"),
        )
        .unwrap();
        let info = read_backup_info(dir.path()).unwrap();
        assert_eq!(info.device_name.as_deref(), Some("Family iPad"));
    }

    #[test]
    fn never_follows_a_symlinked_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere.plist");
        fs::write(
            &elsewhere,
            xml("<key>Device Name</key><string>Not me</string>"),
        )
        .unwrap();
        let backup = dir.path().join("backup");
        fs::create_dir(&backup).unwrap();
        std::os::unix::fs::symlink(&elsewhere, backup.join(INFO_PLIST)).unwrap();
        assert!(matches!(
            read_backup_info(&backup),
            Err(BackupInfoError::NotAFile)
        ));
    }

    #[test]
    fn never_follows_a_symlinked_backup_folder() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::write(
            real.join(INFO_PLIST),
            xml("<key>Device Name</key><string>Not me</string>"),
        )
        .unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(matches!(
            read_backup_info(&link),
            Err(BackupInfoError::NotAFolder)
        ));
    }

    #[test]
    fn refuses_a_folder_named_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(INFO_PLIST)).unwrap();
        assert!(read_backup_info(dir.path()).is_err());
    }

    #[test]
    fn refuses_a_named_pipe_without_hanging() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join(INFO_PLIST);
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `c_path` is a valid NUL-terminated path for the call.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            read_backup_info(dir.path()),
            Err(BackupInfoError::NotAFile)
        ));
    }

    #[test]
    fn refuses_a_file_that_is_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let file = File::create(dir.path().join(INFO_PLIST)).unwrap();
        file.set_len(INFO_PLIST_MAX_BYTES + 1).unwrap();
        assert!(matches!(
            read_backup_info(dir.path()),
            Err(BackupInfoError::TooLarge)
        ));
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            read_backup_info(dir.path()),
            Err(BackupInfoError::Io(_))
        ));
    }
}
