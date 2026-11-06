//! Large files and old Downloads (E2). Read only.
//!
//! Two things live here:
//! - [`may_list_large`], a pure check of which folders' files may be shown
//!   as large files at all. It keeps out places where a single file is part
//!   of something bigger (an app, a Photos library, an app's own data in
//!   `~/Library`, a tool's hidden folder), so removing it alone would break
//!   that thing.
//! - [`look`], which reads one file's or folder's metadata and when macOS
//!   last recorded it being opened. It reads metadata and one extended
//!   attribute only, never the file's contents, and never follows a
//!   symlink.
//!
//! macOS records "last opened" in the `com.apple.lastuseddate#PS` extended
//! attribute when a file is opened from Finder or an app (it is what Finder
//! shows as "Last opened"). A file opened only from the command line may
//! have none, so it is `None`, not "never".

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The extended attribute macOS keeps the last-opened date in.
pub const LAST_USED_ATTR: &str = "com.apple.lastuseddate#PS";

/// Folder names that are really one thing (an app, a library, a project),
/// shown by Finder as a single file. A file inside one is never listed on
/// its own. Compared ignoring ASCII case.
const PACKAGE_EXTENSIONS: &[&str] = &[
    "app",
    "appex",
    "band",
    "bundle",
    "dvdmedia",
    "fcpbundle",
    "framework",
    "imovielibrary",
    "kext",
    "logicx",
    "lrlibrary",
    "musiclibrary",
    "photoboothlibrary",
    "photoslibrary",
    "plugin",
    "pvm",
    "rtfd",
    "sparsebundle",
    "theater",
    "tvlibrary",
    "utm",
    "vmwarevm",
    "xcarchive",
    "xcodeproj",
    "xcworkspace",
];

/// Folders, relative to home, whose files belong to an app's own library
/// (Music and TV keep their media here and would show broken entries).
const MEDIA_APP_FOLDERS: &[&str] = &["Music/Music", "Music/iTunes", "Movies/TV"];

/// Folder names whose contents a tool rebuilds or manages, listed on their
/// own screens instead.
const TOOL_FOLDERS: &[&str] = &["node_modules"];

/// Whether files directly in the folder `rel` (relative to home, empty for
/// home itself) may be shown as large files. Pure.
///
/// Leaves out `~/Library` (app data, explained on other screens), every
/// hidden folder, `~/Applications`, anything inside a package such as an
/// `.app` or a Photos library, the Music and TV apps' media folders, and
/// `node_modules`. When unsure, a folder is left out.
pub fn may_list_large(rel: &Path) -> bool {
    let mut names = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(name) => names.push(name),
            // Anything but plain names (`..`, a root) is not a home folder.
            _ => return false,
        }
    }
    if let Some(first) = names.first() {
        let first = first.as_bytes();
        if first.eq_ignore_ascii_case(b"Library") || first.eq_ignore_ascii_case(b"Applications") {
            return false;
        }
    }
    for name in &names {
        let bytes = name.as_bytes();
        if bytes.starts_with(b".") || is_package(bytes) {
            return false;
        }
        if TOOL_FOLDERS
            .iter()
            .any(|t| bytes.eq_ignore_ascii_case(t.as_bytes()))
        {
            return false;
        }
    }
    !MEDIA_APP_FOLDERS
        .iter()
        .any(|m| starts_with_ignoring_case(rel, Path::new(m)))
}

/// Whether a folder name ends in one of [`PACKAGE_EXTENSIONS`].
fn is_package(name: &[u8]) -> bool {
    let Some(dot) = name.iter().rposition(|&b| b == b'.') else {
        return false;
    };
    let ext = &name[dot + 1..];
    PACKAGE_EXTENSIONS
        .iter()
        .any(|p| ext.eq_ignore_ascii_case(p.as_bytes()))
}

/// Whether `path` starts with every name of `prefix`, ignoring ASCII case.
fn starts_with_ignoring_case(path: &Path, prefix: &Path) -> bool {
    let mut names = path.iter();
    prefix.iter().all(|p| {
        names
            .next()
            .is_some_and(|n| n.as_bytes().eq_ignore_ascii_case(p.as_bytes()))
    })
}

/// What [`look`] found about one file or folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// `true` for a plain file, `false` for a folder.
    pub is_file: bool,
    /// Space on disk of the entry itself (`st_blocks * 512`). For a folder
    /// this is only the folder entry, not what is inside it.
    pub allocated_bytes: u64,
    /// Number of hard links. A file with more than one is not freed by
    /// removing one name.
    pub links: u64,
    /// The volume it is on.
    pub dev: u64,
    /// Its own modification time, if the system reported one.
    pub modified: Option<SystemTime>,
    /// When macOS last recorded it being opened, if it did.
    pub last_opened: Option<SystemTime>,
}

/// Reads the metadata of `path` and its last-opened date. `None` if it is
/// a symlink, anything but a plain file or folder, or can't be read.
pub fn look(path: &Path) -> Option<Facts> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let file_type = meta.file_type();
    if !(file_type.is_file() || file_type.is_dir()) {
        return None;
    }
    Some(Facts {
        is_file: file_type.is_file(),
        allocated_bytes: meta.blocks().saturating_mul(512),
        links: meta.nlink(),
        dev: meta.dev(),
        modified: meta.modified().ok(),
        last_opened: last_opened(path),
    })
}

/// When macOS last recorded `path` being opened, read from
/// [`LAST_USED_ATTR`] without following a symlink. `None` if there is no
/// record or it can't be read.
pub fn last_opened(path: &Path) -> Option<SystemTime> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let c_name = CString::new(LAST_USED_ATTR).ok()?;
    let mut buf = [0u8; 32];
    let read = read_attr(&c_path, &c_name, &mut buf)?;
    decode_last_used(buf.get(..read)?)
}

/// Reads one extended attribute into `buf`, without following a symlink.
/// Returns how many bytes were read.
#[cfg(target_os = "macos")]
fn read_attr(path: &CString, name: &CString, buf: &mut [u8]) -> Option<usize> {
    // SAFETY: both strings are NUL-terminated and live for the call; `buf`
    // is valid for `buf.len()` bytes. getxattr only reads the file's
    // metadata into `buf`.
    let n = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            0,
            libc::XATTR_NOFOLLOW,
        )
    };
    usize::try_from(n).ok()
}

/// Reads one extended attribute into `buf`, without following a symlink.
/// Returns how many bytes were read. (Linux, for tests and CI.)
#[cfg(not(target_os = "macos"))]
fn read_attr(path: &CString, name: &CString, buf: &mut [u8]) -> Option<usize> {
    // SAFETY: both strings are NUL-terminated and live for the call; `buf`
    // is valid for `buf.len()` bytes. lgetxattr only reads metadata.
    let n = unsafe {
        libc::lgetxattr(
            path.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    usize::try_from(n).ok()
}

/// Decodes [`LAST_USED_ATTR`]: a `timespec`, seconds then nanoseconds,
/// each a little-endian 64-bit number. Pure. `None` for anything that
/// doesn't look like one, including a date before 1970.
pub fn decode_last_used(bytes: &[u8]) -> Option<SystemTime> {
    let secs = i64::from_le_bytes(bytes.get(0..8)?.try_into().ok()?);
    let nanos = i64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?);
    let secs = u64::try_from(secs).ok()?;
    let nanos = u32::try_from(nanos).ok().filter(|n| *n < 1_000_000_000)?;
    UNIX_EPOCH.checked_add(Duration::new(secs, nanos))
}

/// How long since a Downloads entry was last opened before it counts as
/// old.
pub const OLD_DOWNLOAD_AFTER: Duration = Duration::from_secs(90 * 24 * 60 * 60);

/// Whether a Downloads entry is old at `now`: last opened (or, with no
/// record of being opened, last changed) more than
/// [`OLD_DOWNLOAD_AFTER`] ago. Pure. An entry with neither date, or a date
/// in the future, is not old: when unsure, it is not offered.
pub fn is_old(
    last_opened: Option<SystemTime>,
    modified: Option<SystemTime>,
    now: SystemTime,
) -> bool {
    let Some(last) = last_opened.or(modified) else {
        return false;
    };
    now.duration_since(last)
        .is_ok_and(|age| age > OLD_DOWNLOAD_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(rel: &str) -> bool {
        may_list_large(Path::new(rel))
    }

    #[test]
    fn everyday_folders_may_list_large_files() {
        assert!(allowed(""));
        assert!(allowed("Downloads"));
        assert!(allowed("Movies/Exports"));
        assert!(allowed("Documents/Work 2024"));
        assert!(allowed("Desktop/my.project"));
        assert!(allowed("Music/GarageBand exports"));
    }

    #[test]
    fn app_data_hidden_folders_and_packages_never_do() {
        assert!(!allowed("Library"));
        assert!(!allowed("Library/Containers/com.docker.docker/Data"));
        assert!(!allowed("library/caches"));
        assert!(!allowed("Applications/Tool.app/Contents"));
        assert!(!allowed(".ollama/models/blobs"));
        assert!(!allowed("Documents/.hidden"));
        assert!(!allowed(
            "Pictures/Photos Library.photoslibrary/originals/A"
        ));
        assert!(!allowed("Movies/Holiday.fcpbundle/Render Files"));
        assert!(!allowed("Desktop/Thing.APP/Contents/MacOS"));
        assert!(!allowed("Music/Music/Media.localized/Music"));
        assert!(!allowed("music/itunes"));
        assert!(!allowed("Movies/TV/Media"));
        assert!(!allowed("Code/site/node_modules/esbuild"));
        assert!(!allowed("../elsewhere"));
    }

    #[test]
    fn last_used_dates_decode() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_700_000_000i64.to_le_bytes());
        bytes.extend_from_slice(&5i64.to_le_bytes());
        assert_eq!(
            decode_last_used(&bytes),
            Some(UNIX_EPOCH + Duration::new(1_700_000_000, 5))
        );
        assert_eq!(decode_last_used(&bytes[..8]), None);
        let mut before_1970 = (-1i64).to_le_bytes().to_vec();
        before_1970.extend_from_slice(&0i64.to_le_bytes());
        assert_eq!(decode_last_used(&before_1970), None);
        let mut bad_nanos = 1i64.to_le_bytes().to_vec();
        bad_nanos.extend_from_slice(&2_000_000_000i64.to_le_bytes());
        assert_eq!(decode_last_used(&bad_nanos), None);
    }

    #[test]
    fn downloads_are_old_after_ninety_days_of_not_being_opened() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let days = |d: u64| now - Duration::from_secs(d * 24 * 60 * 60);
        assert!(is_old(Some(days(91)), Some(days(400)), now));
        assert!(!is_old(Some(days(10)), Some(days(400)), now));
        // Opened recently counts, even if it changed long ago.
        assert!(!is_old(Some(days(1)), Some(days(1000)), now));
        // With no record of being opened, the change date is used.
        assert!(is_old(None, Some(days(120)), now));
        assert!(!is_old(None, Some(days(30)), now));
        assert!(!is_old(None, None, now));
        // A date in the future is not old.
        assert!(!is_old(Some(now + Duration::from_secs(60)), None, now));
    }

    #[test]
    fn look_reads_files_and_folders_but_not_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("film.mov");
        std::fs::write(&file, vec![1u8; 10_000]).unwrap();
        let facts = look(&file).unwrap();
        assert!(facts.is_file);
        assert_eq!(facts.links, 1);
        assert!(facts.modified.is_some());
        assert!(facts.last_opened.is_none());
        assert!(!look(dir.path()).unwrap().is_file);
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(look(&link).is_none());
        assert!(look(&dir.path().join("missing")).is_none());
    }
}
