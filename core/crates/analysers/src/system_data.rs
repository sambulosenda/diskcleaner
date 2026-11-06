//! What sits in the "System Data" bar of macOS's storage settings (PRD F9).
//!
//! macOS lumps a lot together as System Data and says nothing more. This
//! module names the parts Roomy can measure: Time Machine local snapshots,
//! caches, logs, downloaded software updates, the sleep image, swap files
//! and the temporary files macOS keeps for the user.
//!
//! Everything here is read only:
//!
//! - Folders are measured with the read-only [`scanner`] (allocated size,
//!   no symlink followed, one volume, hard links once). Some of them are
//!   outside the home folder, such as `/Library/Caches`. Roomy only ever
//!   reads their size: the executor refuses anything outside the home
//!   folder, so none of them can be cleaned.
//! - Single files (the sleep image, swap files) are read with `lstat` only.
//! - Local snapshots are listed with the read-only `tmutil listlocalsnapshots /`.
//!   macOS does not say how much space a snapshot takes, so Roomy only
//!   counts them and gives their dates.
//!
//! Every location is built from a [`Roots`], so tests point the whole thing
//! at temporary folders and a stand-in `tmutil`.

use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

pub use crate::process::{Output, RunError};

/// How long `tmutil` may take before Roomy gives up on it.
pub const SNAPSHOT_QUERY_TIMEOUT: Duration = Duration::from_secs(15);

/// The folder searched for the `tmutil` program. It ships with macOS there.
pub const TMUTIL_FOLDER: &str = "/usr/bin";

/// One part of System Data, as the app shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    /// Time Machine local snapshots.
    LocalSnapshots,
    /// App and system caches.
    Caches,
    /// Log files.
    Logs,
    /// Downloaded iPhone, iPad and macOS updates.
    SoftwareUpdates,
    /// The file macOS writes memory into while the Mac sleeps.
    SleepImage,
    /// Swap files: memory macOS moved to disk.
    Swap,
    /// The temporary files and caches macOS keeps for the user.
    TemporaryFiles,
}

impl Part {
    /// Every part, in the order the app lists them.
    pub const ALL: [Part; 7] = [
        Part::LocalSnapshots,
        Part::Caches,
        Part::Logs,
        Part::SoftwareUpdates,
        Part::TemporaryFiles,
        Part::Swap,
        Part::SleepImage,
    ];
}

/// Whose a location is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Owner {
    /// Inside the user's home folder.
    You,
    /// Shared by the whole Mac, outside the home folder. Never cleaned.
    System,
}

/// What a location is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A folder, measured with everything inside it.
    Folder,
    /// One file.
    File,
    /// The plain files directly in a folder whose names start with `prefix`.
    FilesNamed {
        /// The start of the names, such as `swapfile`.
        prefix: &'static str,
    },
}

/// One location that holds part of System Data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The part it belongs to.
    pub part: Part,
    /// A stable id for the location, such as `user_caches`. The app picks
    /// its words from it.
    pub id: &'static str,
    /// Whose it is.
    pub owner: Owner,
    /// Where it is.
    pub path: PathBuf,
    /// What it is on disk.
    pub shape: Shape,
}

/// The roots every location is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    /// The user's home folder.
    pub home: PathBuf,
    /// The root of the startup disk: `/` on a real Mac.
    pub system: PathBuf,
    /// The user's own temporary folder under `/private/var/folders`, if
    /// macOS said where it is.
    pub user_temp: Option<PathBuf>,
}

impl Roots {
    /// The real locations for `home`: the startup disk at `/`, and the
    /// temporary folder macOS gives this user.
    pub fn for_home(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            system: PathBuf::from("/"),
            user_temp: user_temp_folder(),
        }
    }
}

/// Every location Roomy measures, in a fixed order.
pub fn places(roots: &Roots) -> Vec<Place> {
    let home = |rel: &str| roots.home.join(rel);
    let sys = |rel: &str| roots.system.join(rel);
    let mut list = vec![
        place(
            Part::Caches,
            "user_caches",
            Owner::You,
            home("Library/Caches"),
        ),
        place(
            Part::Caches,
            "shared_caches",
            Owner::System,
            sys("Library/Caches"),
        ),
        place(Part::Logs, "user_logs", Owner::You, home("Library/Logs")),
        place(
            Part::Logs,
            "shared_logs",
            Owner::System,
            sys("Library/Logs"),
        ),
        place(
            Part::Logs,
            "system_logs",
            Owner::System,
            sys("private/var/log"),
        ),
        place(
            Part::Logs,
            "unified_logs",
            Owner::System,
            sys("private/var/db/diagnostics"),
        ),
        place(
            Part::SoftwareUpdates,
            "iphone_updates",
            Owner::You,
            home("Library/iTunes/iPhone Software Updates"),
        ),
        place(
            Part::SoftwareUpdates,
            "ipad_updates",
            Owner::You,
            home("Library/iTunes/iPad Software Updates"),
        ),
        place(
            Part::SoftwareUpdates,
            "mac_updates",
            Owner::System,
            sys("Library/Updates"),
        ),
    ];
    if let Some(temp) = &roots.user_temp {
        list.push(place(
            Part::TemporaryFiles,
            "user_temporary",
            Owner::System,
            temp.clone(),
        ));
    }
    list.push(Place {
        part: Part::Swap,
        id: "swap_files",
        owner: Owner::System,
        path: sys("private/var/vm"),
        shape: Shape::FilesNamed { prefix: "swapfile" },
    });
    list.push(Place {
        part: Part::SleepImage,
        id: "sleep_image",
        owner: Owner::System,
        path: sys("private/var/vm/sleepimage"),
        shape: Shape::File,
    });
    list
}

/// A folder location.
fn place(part: Part, id: &'static str, owner: Owner, path: PathBuf) -> Place {
    Place {
        part,
        id,
        owner,
        path,
        shape: Shape::Folder,
    }
}

/// What measuring one location found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// It is not on this Mac.
    Missing,
    /// It is there but Roomy could not read it (permission, or a symlink,
    /// which is never followed).
    Unreadable,
    /// Its size.
    Measured {
        /// Allocated bytes.
        bytes: u64,
        /// Files counted (hard links once).
        files: u64,
        /// Whether some folders inside could not be read, so the real size
        /// is larger.
        partly_unreadable: bool,
    },
}

/// Measures one location. Reads metadata and folder listings only.
pub fn measure(place: &Place) -> Found {
    match place.shape {
        Shape::Folder => measure_folder(&place.path),
        Shape::File => measure_file(&place.path),
        Shape::FilesNamed { prefix } => measure_named_files(&place.path, prefix),
    }
}

/// A folder and everything in it, through the read-only scanner.
fn measure_folder(path: &Path) -> Found {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Found::Missing,
        Err(_) => return Found::Unreadable,
    };
    if !meta.is_dir() {
        // A symlink (never followed) or something that is not a folder.
        return Found::Unreadable;
    }
    let opts = scanner::ScanOptions {
        cancel: Arc::new(AtomicBool::new(false)),
        largest_files: 0,
        ..scanner::ScanOptions::default()
    };
    match scanner::scan(path, opts, |_| {}) {
        Ok(scan) if scan.unreadable.iter().any(|u| u.path == path) => Found::Unreadable,
        Ok(scan) => Found::Measured {
            bytes: scan.total_bytes(),
            files: scan.total_files(),
            partly_unreadable: !scan.unreadable.is_empty(),
        },
        Err(err) => {
            tracing::warn!(%err, "system data folder not measured");
            Found::Unreadable
        }
    }
}

/// One plain file. A symlink is never followed.
fn measure_file(path: &Path) -> Found {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Found::Measured {
            bytes: allocated(&meta),
            files: 1,
            partly_unreadable: false,
        },
        Ok(_) => Found::Unreadable,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Found::Missing,
        Err(_) => Found::Unreadable,
    }
}

/// The plain files directly in `folder` whose names start with `prefix`.
fn measure_named_files(folder: &Path, prefix: &str) -> Found {
    match std::fs::symlink_metadata(folder) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Found::Unreadable,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Found::Missing,
        Err(_) => return Found::Unreadable,
    }
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(_) => return Found::Unreadable,
    };
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut partly_unreadable = false;
    for entry in entries {
        let Ok(entry) = entry else {
            partly_unreadable = true;
            continue;
        };
        let named = entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with(prefix));
        if !named {
            continue;
        }
        // `DirEntry::metadata` does not follow a symlink.
        match entry.metadata() {
            Ok(meta) if meta.file_type().is_file() => {
                bytes = bytes.saturating_add(allocated(&meta));
                files += 1;
            }
            Ok(_) => {}
            Err(_) => partly_unreadable = true,
        }
    }
    if files == 0 && !partly_unreadable {
        return Found::Missing;
    }
    Found::Measured {
        bytes,
        files,
        partly_unreadable,
    }
}

/// The space a file really takes on disk.
fn allocated(meta: &std::fs::Metadata) -> u64 {
    meta.blocks().saturating_mul(512)
}

/// The user's own temporary folder, `/private/var/folders/<xx>/<id>`, which
/// holds the `C` (caches) and `T` (temporary) folders macOS gives each
/// user. `None` off macOS or if macOS gives something unexpected.
pub fn user_temp_folder() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        darwin_user_cache_dir().and_then(|dir| temp_folder_from(&dir))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Asks macOS for the user's cache folder (`_CS_DARWIN_USER_CACHE_DIR`).
#[cfg(target_os = "macos")]
fn darwin_user_cache_dir() -> Option<String> {
    let mut buf = vec![0u8; 1024];
    // SAFETY: `buf` is a writable buffer of `buf.len()` bytes; confstr
    // writes at most that many bytes, including the closing NUL.
    let needed = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_CACHE_DIR,
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    if needed == 0 || needed > buf.len() {
        return None;
    }
    let text = std::ffi::CStr::from_bytes_until_nul(&buf).ok()?;
    text.to_str().ok().map(str::to_string)
}

/// Turns the cache folder macOS gives (`/var/folders/xx/id/C/`) into the
/// user's temporary folder (`/private/var/folders/xx/id`). Fails closed:
/// anything not shaped like that gives `None`.
pub fn temp_folder_from(cache_dir: &str) -> Option<PathBuf> {
    let trimmed = cache_dir.trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("/private/var/folders/")
        .or_else(|| trimmed.strip_prefix("/var/folders/"))?;
    let parts: Vec<&str> = rest.split('/').collect();
    let [bucket, id, "C"] = parts.as_slice() else {
        return None;
    };
    let plain = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-')
    };
    if !plain(bucket) || !plain(id) {
        return None;
    }
    Some(PathBuf::from(format!("/private/var/folders/{bucket}/{id}")))
}

// ---------------------------------------------------------------------------
// Time Machine local snapshots
// ---------------------------------------------------------------------------

/// Runs the read-only `tmutil listlocalsnapshots /` query. A trait so tests
/// can answer with recorded output instead of a real Mac.
pub trait SnapshotQuery {
    /// Runs `tmutil listlocalsnapshots /`.
    fn list_local_snapshots(&self) -> Result<Output, RunError>;
}

/// The real `tmutil` program.
#[derive(Debug, Clone)]
pub struct RealTmutil {
    path: OsString,
}

impl Default for RealTmutil {
    fn default() -> Self {
        Self {
            path: OsString::from(TMUTIL_FOLDER),
        }
    }
}

impl RealTmutil {
    /// Looks for `tmutil` in exactly these folders (a `PATH`-style list).
    /// For tests that put a stand-in `tmutil` in a temporary folder.
    pub fn with_search_path(path: OsString) -> Self {
        Self { path }
    }
}

impl SnapshotQuery for RealTmutil {
    fn list_local_snapshots(&self) -> Result<Output, RunError> {
        // Arguments are an array, never a shell line. This only lists.
        let mut command = Command::new("tmutil");
        command
            .args(["listlocalsnapshots", "/"])
            .env("PATH", &self.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::process::wait_with_timeout(command, SNAPSHOT_QUERY_TIMEOUT, "Time Machine")
    }
}

/// The local snapshots on the startup disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Snapshots {
    /// How many there are.
    pub count: u64,
    /// When the oldest Time Machine snapshot was taken, as seconds since
    /// 1970 read from its name as if it were UTC (macOS names them in local
    /// time). Show it as a date in UTC to get the day the name says.
    pub oldest: Option<i64>,
    /// When the newest one was taken, the same way.
    pub newest: Option<i64>,
}

/// What Roomy could find out about local snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotState {
    /// The list.
    Listed(Snapshots),
    /// They could not be listed, in plain words with no path.
    Unavailable {
        /// Why.
        reason: String,
    },
}

/// `tmutil`'s output could not be understood. The parser fails closed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unexpected tmutil output: {0}")]
pub struct ParseError(pub String);

/// Lists the local snapshots. Never fails: every problem becomes a
/// [`SnapshotState::Unavailable`] the app can explain.
pub fn local_snapshots(query: &dyn SnapshotQuery) -> SnapshotState {
    let unavailable = |reason: &str| SnapshotState::Unavailable {
        reason: reason.to_string(),
    };
    match query.list_local_snapshots() {
        Err(RunError::NotFound) => unavailable("Roomy couldn't ask Time Machine on this Mac."),
        Err(RunError::TimedOut) => {
            unavailable("Time Machine took too long to answer. Try again in a moment.")
        }
        Err(RunError::Other(reason)) => SnapshotState::Unavailable { reason },
        Ok(out) if !out.success => {
            tracing::warn!(stderr = %out.stderr.trim(), "tmutil listlocalsnapshots failed");
            unavailable("Time Machine reported a problem, so Roomy couldn't list snapshots.")
        }
        Ok(out) => match parse_snapshot_list(&out.stdout) {
            Ok(snapshots) => SnapshotState::Listed(snapshots),
            Err(err) => {
                tracing::warn!(%err, "tmutil output not understood");
                unavailable("Roomy couldn't read Time Machine's answer.")
            }
        },
    }
}

/// Reads `tmutil listlocalsnapshots /` output:
///
/// ```text
/// Snapshots for disk /:
/// com.apple.TimeMachine.2026-11-05-093012.local
/// com.apple.os.update-4F2A...
/// ```
///
/// The header line is optional (older macOS leaves it out). Every other
/// line must be one snapshot name with no spaces; anything else is an
/// error, never a guess.
pub fn parse_snapshot_list(text: &str) -> Result<Snapshots, ParseError> {
    let mut snapshots = Snapshots::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || (line.starts_with("Snapshots for disk") && line.ends_with(':')) {
            continue;
        }
        if !line.starts_with("com.apple.") || line.contains(char::is_whitespace) {
            return Err(ParseError(format!("line {line:?}")));
        }
        snapshots.count += 1;
        if let Some(at) = time_machine_time(line) {
            snapshots.oldest = Some(snapshots.oldest.map_or(at, |o| o.min(at)));
            snapshots.newest = Some(snapshots.newest.map_or(at, |n| n.max(at)));
        }
    }
    Ok(snapshots)
}

/// When a Time Machine snapshot was taken, from a name such as
/// `com.apple.TimeMachine.2026-11-05-093012.local`, read as UTC.
fn time_machine_time(name: &str) -> Option<i64> {
    let stamp = name.strip_prefix("com.apple.TimeMachine.")?;
    let stamp = stamp.strip_suffix(".local").unwrap_or(stamp);
    let digits = |s: &str| -> Option<i64> {
        (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    let mut fields = stamp.split('-');
    let year = digits(fields.next()?)?;
    let month = digits(fields.next()?)?;
    let day = digits(fields.next()?)?;
    let clock = fields.next()?;
    if fields.next().is_some() || clock.len() != 6 {
        return None;
    }
    let hour = digits(&clock[0..2])?;
    let minute = digits(&clock[2..4])?;
    let second = digits(&clock[4..6])?;
    let valid = (1970..=9999).contains(&year)
        && (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour < 24
        && minute < 60
        && second < 61;
    if !valid {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since 1970-01-01 for a date in the proleptic Gregorian calendar
/// (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Answer(Result<Output, RunError>);

    impl SnapshotQuery for Answer {
        fn list_local_snapshots(&self) -> Result<Output, RunError> {
            self.0.clone()
        }
    }

    fn printed(stdout: &str) -> Answer {
        Answer(Ok(Output {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }))
    }

    #[test]
    fn snapshot_list_is_counted_with_dates() {
        let text = "Snapshots for disk /:\n\
                    com.apple.TimeMachine.2026-11-05-093012.local\n\
                    com.apple.TimeMachine.2026-11-06-103000.local\n\
                    com.apple.os.update-4F2A9C1E7B3D\n";
        let s = parse_snapshot_list(text).unwrap();
        assert_eq!(s.count, 3);
        // 2026-11-05 09:30:12 UTC and 2026-11-06 10:30:00 UTC.
        assert_eq!(s.oldest, Some(1_793_871_012));
        assert_eq!(s.newest, Some(1_793_961_000));
    }

    #[test]
    fn no_snapshots_is_zero_not_an_error() {
        assert_eq!(
            parse_snapshot_list("Snapshots for disk /:\n").unwrap(),
            Snapshots::default()
        );
        assert_eq!(parse_snapshot_list("").unwrap(), Snapshots::default());
    }

    #[test]
    fn unknown_lines_fail_closed() {
        assert!(parse_snapshot_list("Something new happened\n").is_err());
        assert!(parse_snapshot_list("com.apple.TimeMachine.2026 extra\n").is_err());
        assert!(parse_snapshot_list("/Volumes/Data\n").is_err());
    }

    #[test]
    fn odd_time_machine_names_are_counted_without_a_date() {
        let s = parse_snapshot_list("com.apple.TimeMachine.2026-13-05-093012.local\n").unwrap();
        assert_eq!(s.count, 1);
        assert_eq!(s.oldest, None);
    }

    #[test]
    fn dates_match_known_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(
            time_machine_time("com.apple.TimeMachine.2024-02-29-000000.local"),
            Some(1_709_164_800)
        );
    }

    #[test]
    fn query_problems_become_plain_reasons() {
        let missing = local_snapshots(&Answer(Err(RunError::NotFound)));
        assert!(matches!(missing, SnapshotState::Unavailable { .. }));
        let failed = local_snapshots(&Answer(Ok(Output {
            success: false,
            stdout: String::new(),
            stderr: "boom".into(),
        })));
        assert!(matches!(failed, SnapshotState::Unavailable { .. }));
        let odd = local_snapshots(&printed("What is this?\n"));
        assert!(matches!(odd, SnapshotState::Unavailable { .. }));
        let fine = local_snapshots(&printed("com.apple.os.update-AB12\n"));
        assert_eq!(
            fine,
            SnapshotState::Listed(Snapshots {
                count: 1,
                oldest: None,
                newest: None
            })
        );
    }

    #[test]
    fn temp_folder_comes_from_the_cache_folder() {
        assert_eq!(
            temp_folder_from("/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/C/"),
            Some(PathBuf::from(
                "/private/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000"
            ))
        );
        assert_eq!(
            temp_folder_from("/private/var/folders/ab/cd+ef/C"),
            Some(PathBuf::from("/private/var/folders/ab/cd+ef"))
        );
        assert_eq!(temp_folder_from("/var/folders/zz/id/T/"), None);
        assert_eq!(temp_folder_from("/var/folders/zz/../C/"), None);
        assert_eq!(temp_folder_from("/var/folders/zz/C/"), None);
        assert_eq!(temp_folder_from("/Users/sam/C/"), None);
        assert_eq!(temp_folder_from(""), None);
    }

    fn roots() -> (tempfile::TempDir, Roots) {
        let dir = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        let roots = Roots {
            home: base.join("home"),
            system: base.join("disk"),
            user_temp: Some(base.join("disk/private/var/folders/zz/me")),
        };
        (dir, roots)
    }

    fn write(path: &Path, len: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; len]).unwrap();
    }

    fn find<'a>(list: &'a [Place], id: &str) -> &'a Place {
        list.iter().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn every_place_is_built_from_the_roots() {
        let (_dir, roots) = roots();
        let list = places(&roots);
        for p in &list {
            let inside = match p.owner {
                Owner::You => p.path.starts_with(&roots.home),
                Owner::System => p.path.starts_with(&roots.system),
            };
            assert!(inside, "{} is outside its root", p.id);
        }
        let parts: std::collections::BTreeSet<Part> = list.iter().map(|p| p.part).collect();
        // Every part but snapshots has at least one place.
        assert_eq!(parts.len(), Part::ALL.len() - 1);
        let no_temp = Roots {
            user_temp: None,
            ..roots
        };
        assert!(places(&no_temp).iter().all(|p| p.id != "user_temporary"));
    }

    #[test]
    fn folders_files_and_swap_are_measured() {
        let (_dir, roots) = roots();
        write(&roots.home.join("Library/Caches/app/cache.db"), 10_000);
        write(&roots.system.join("private/var/vm/sleepimage"), 20_000);
        write(&roots.system.join("private/var/vm/swapfile0"), 8_192);
        write(&roots.system.join("private/var/vm/swapfile1"), 8_192);
        write(&roots.system.join("private/var/vm/other"), 50_000);
        let list = places(&roots);

        let Found::Measured {
            bytes,
            files,
            partly_unreadable,
        } = measure(find(&list, "user_caches"))
        else {
            panic!("caches not measured");
        };
        assert!(bytes >= 10_000);
        assert_eq!(files, 1);
        assert!(!partly_unreadable);

        let Found::Measured { bytes, files, .. } = measure(find(&list, "sleep_image")) else {
            panic!("sleep image not measured");
        };
        assert!(bytes >= 20_000);
        assert_eq!(files, 1);

        let Found::Measured { bytes, files, .. } = measure(find(&list, "swap_files")) else {
            panic!("swap not measured");
        };
        assert_eq!(files, 2);
        assert!(
            (16_384..50_000).contains(&bytes),
            "other files counted: {bytes}"
        );

        assert_eq!(measure(find(&list, "shared_logs")), Found::Missing);
    }

    #[test]
    fn symlinks_are_never_followed() {
        let (_dir, roots) = roots();
        let elsewhere = roots.home.join("elsewhere");
        write(&elsewhere.join("big.bin"), 100_000);
        write(&elsewhere.join("swapfile9"), 100_000);
        fs::create_dir_all(roots.home.join("Library")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, roots.home.join("Library/Caches")).unwrap();
        fs::create_dir_all(roots.system.join("private/var/vm")).unwrap();
        std::os::unix::fs::symlink(
            elsewhere.join("big.bin"),
            roots.system.join("private/var/vm/sleepimage"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            elsewhere.join("swapfile9"),
            roots.system.join("private/var/vm/swapfile0"),
        )
        .unwrap();
        let list = places(&roots);
        assert_eq!(measure(find(&list, "user_caches")), Found::Unreadable);
        assert_eq!(measure(find(&list, "sleep_image")), Found::Unreadable);
        // The symlinked swap file is skipped, so there is nothing to count.
        assert_eq!(measure(find(&list, "swap_files")), Found::Missing);
    }

    #[test]
    fn an_unreadable_folder_says_so() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, roots) = roots();
        let logs = roots.system.join("Library/Logs");
        write(&logs.join("a.log"), 4_096);
        write(&logs.join("locked/b.log"), 4_096);
        fs::set_permissions(logs.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        let list = places(&roots);
        let found = measure(find(&list, "shared_logs"));
        fs::set_permissions(logs.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        // Root can read anything, so only check the flag when not root.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } != 0 {
            assert!(matches!(
                found,
                Found::Measured {
                    partly_unreadable: true,
                    ..
                }
            ));
        } else {
            assert!(matches!(found, Found::Measured { .. }));
        }
    }
}
