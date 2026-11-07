//! The System Data screen's command: what macOS's "System Data" holds
//! (PRD F9), measured just now.
//!
//! Read only. Folders are measured by the read-only scanner, single files
//! with `lstat`, and local snapshots are listed with the read-only
//! `tmutil listlocalsnapshots /` (see `analysers::system_data`). Several
//! locations are outside the home folder; the core only ever reads their
//! size and never offers them for cleanup (the executor refuses anything
//! outside the home folder anyway). What can be cleaned is already in the
//! scan's items, through the usual Trash-only cleanup.
//!
//! No path goes to the app in simple mode.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use analysers::system_data::{
    self as analysis, Found, Owner, Part, RealTmutil, Roots, SnapshotState,
};

use crate::types::Mode;
use crate::RoomyCore;

/// One part of System Data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SystemDataKind {
    /// App and system caches.
    Caches,
    /// Log files.
    Logs,
    /// Downloaded iPhone, iPad and macOS updates.
    SoftwareUpdates,
    /// Temporary files and caches macOS keeps for the user.
    TemporaryFiles,
    /// Swap files: memory macOS moved to disk.
    Swap,
    /// The sleep image.
    SleepImage,
}

impl SystemDataKind {
    /// The part for an analyser part, or `None` for snapshots, which have
    /// their own record.
    fn from_part(part: Part) -> Option<Self> {
        match part {
            Part::LocalSnapshots => None,
            Part::Caches => Some(Self::Caches),
            Part::Logs => Some(Self::Logs),
            Part::SoftwareUpdates => Some(Self::SoftwareUpdates),
            Part::TemporaryFiles => Some(Self::TemporaryFiles),
            Part::Swap => Some(Self::Swap),
            Part::SleepImage => Some(Self::SleepImage),
        }
    }
}

/// Whose a location is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SystemDataOwner {
    /// Inside the home folder. What Roomy can clean here is in the scan's items.
    You,
    /// Shared by the whole Mac. Roomy only shows its size.
    System,
}

/// What measuring one location found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SystemDataPlaceState {
    /// Not on this Mac.
    Missing,
    /// There, but Roomy could not read it.
    Unreadable,
    /// Measured; the size is in the record.
    Measured,
}

/// One location that holds part of System Data.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SystemDataPlace {
    /// A stable id, such as `user_caches`. The app picks its words from it.
    pub id: String,
    /// Whose it is.
    pub owner: SystemDataOwner,
    /// Whether it was measured.
    pub state: SystemDataPlaceState,
    /// Allocated bytes. 0 unless `Measured`.
    pub size_bytes: u64,
    /// Files counted (hard links once). 0 unless `Measured`.
    pub file_count: u64,
    /// Some folders inside could not be read, so the real size is larger.
    pub partly_unreadable: bool,
    /// Where it is, such as `~/Library/Caches` or `/Library/Caches`.
    /// Developer mode only; `None` in simple mode.
    pub path: Option<String>,
}

/// One part of System Data with the locations it was measured in.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SystemDataPart {
    /// Which part.
    pub kind: SystemDataKind,
    /// Allocated bytes across its measured locations.
    pub size_bytes: u64,
    /// Some of it could not be read (a location, or folders inside one),
    /// so the real size is larger.
    pub partly_unreadable: bool,
    /// Where it was measured, in a fixed order.
    pub places: Vec<SystemDataPlace>,
}

/// Whether local snapshots could be listed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SnapshotStatus {
    /// The count and dates are real.
    Listed,
    /// They could not be listed. `reason` is plain words with no path.
    Unavailable {
        /// Why.
        reason: String,
    },
}

/// Time Machine local snapshots on the startup disk. macOS does not say how
/// much space they take, so there is no size.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LocalSnapshots {
    /// Whether the numbers below are real. Both 0 / `None` unless `Listed`.
    pub status: SnapshotStatus,
    /// How many there are.
    pub count: u64,
    /// When the oldest Time Machine snapshot was taken, read from its name
    /// as if it were UTC: show it as a date in UTC.
    pub oldest_unix_seconds: Option<i64>,
    /// When the newest one was taken, the same way.
    pub newest_unix_seconds: Option<i64>,
}

/// What the System Data screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SystemData {
    /// Time Machine local snapshots.
    pub snapshots: LocalSnapshots,
    /// Every other part, in the order the app lists them.
    pub parts: Vec<SystemDataPart>,
    /// Sum of every part's size. Snapshots are not included (no size).
    pub total_bytes: u64,
}

/// Where the core looks, changed only by Rust tests.
pub(crate) struct SystemDataSlot {
    /// The roots every location is built from.
    roots: Mutex<Roots>,
    /// The folders searched for `tmutil`.
    tmutil_path: Mutex<OsString>,
}

impl SystemDataSlot {
    /// The real locations for `home`.
    pub(crate) fn new(home: &Path) -> Self {
        Self {
            roots: Mutex::new(Roots::for_home(home)),
            tmutil_path: Mutex::new(OsString::from(analysis::TMUTIL_FOLDER)),
        }
    }
}

/// Locks a mutex, carrying on if a thread panicked while holding it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl RoomyCore {
    /// Rust-only, for tests: measures the startup disk at `system` instead
    /// of `/`, with this user temporary folder.
    pub fn set_system_roots_for_testing(&self, system: PathBuf, user_temp: Option<PathBuf>) {
        let mut roots = lock(&self.system_data.roots);
        roots.system = system;
        roots.user_temp = user_temp;
    }

    /// Rust-only, for tests: looks for `tmutil` in exactly these folders.
    pub fn set_tmutil_search_path_for_testing(&self, path: OsString) {
        *lock(&self.system_data.tmutil_path) = path;
    }
}

#[uniffi::export]
impl RoomyCore {
    /// What macOS's System Data holds, measured just now: local snapshots,
    /// caches, logs, software updates, temporary files, swap and the sleep
    /// image. Read only; never throws. Can take several seconds, so the app
    /// calls it off the main thread. Paths are filled in Developer mode only.
    pub fn system_data(&self, mode: Mode) -> SystemData {
        let roots = lock(&self.system_data.roots).clone();
        let tmutil = RealTmutil::with_search_path(lock(&self.system_data.tmutil_path).clone());
        let snapshots = snapshots_record(analysis::local_snapshots(&tmutil));
        let parts = measure_parts(&roots, mode);
        let total_bytes = parts
            .iter()
            .fold(0u64, |sum, p| sum.saturating_add(p.size_bytes));
        SystemData {
            snapshots,
            parts,
            total_bytes,
        }
    }
}

/// The snapshot record for what `tmutil` said.
fn snapshots_record(state: SnapshotState) -> LocalSnapshots {
    match state {
        SnapshotState::Listed(s) => LocalSnapshots {
            status: SnapshotStatus::Listed,
            count: s.count,
            oldest_unix_seconds: s.oldest,
            newest_unix_seconds: s.newest,
        },
        SnapshotState::Unavailable { reason } => LocalSnapshots {
            status: SnapshotStatus::Unavailable { reason },
            count: 0,
            oldest_unix_seconds: None,
            newest_unix_seconds: None,
        },
    }
}

/// Measures every location and groups them into parts, in the app's order.
fn measure_parts(roots: &Roots, mode: Mode) -> Vec<SystemDataPart> {
    let places = analysis::places(roots);
    analysis::Part::ALL
        .iter()
        .filter_map(|&part| {
            let kind = SystemDataKind::from_part(part)?;
            let measured: Vec<SystemDataPlace> = places
                .iter()
                .filter(|p| p.part == part)
                .map(|p| place_record(roots, p, mode))
                .collect();
            Some(part_record(kind, measured))
        })
        .collect()
}

/// One part from its measured locations.
fn part_record(kind: SystemDataKind, places: Vec<SystemDataPlace>) -> SystemDataPart {
    let size_bytes = places
        .iter()
        .fold(0u64, |sum, p| sum.saturating_add(p.size_bytes));
    let partly_unreadable = places
        .iter()
        .any(|p| p.partly_unreadable || p.state == SystemDataPlaceState::Unreadable);
    SystemDataPart {
        kind,
        size_bytes,
        partly_unreadable,
        places,
    }
}

/// Measures one location.
fn place_record(roots: &Roots, place: &analysis::Place, mode: Mode) -> SystemDataPlace {
    let (state, size_bytes, file_count, partly_unreadable) = match analysis::measure(place) {
        Found::Missing => (SystemDataPlaceState::Missing, 0, 0, false),
        Found::Unreadable => (SystemDataPlaceState::Unreadable, 0, 0, false),
        Found::Measured {
            bytes,
            files,
            partly_unreadable,
        } => (
            SystemDataPlaceState::Measured,
            bytes,
            files,
            partly_unreadable,
        ),
    };
    SystemDataPlace {
        id: place.id.to_string(),
        owner: match place.owner {
            Owner::You => SystemDataOwner::You,
            Owner::System => SystemDataOwner::System,
        },
        state,
        size_bytes,
        file_count,
        partly_unreadable,
        path: match mode {
            Mode::Developer => display(roots, place),
            Mode::Simple => None,
        },
    }
}

/// A location as Developer mode shows it: `~/…` inside the home folder,
/// `/…` on the startup disk.
fn display(roots: &Roots, place: &analysis::Place) -> Option<String> {
    match place.owner {
        Owner::You => crate::views::display_path(&roots.home, &place.path),
        Owner::System => {
            let rel = place.path.strip_prefix(&roots.system).ok()?;
            Some(format!("/{}", rel.to_string_lossy()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_paths_are_relative_to_their_root() {
        let roots = Roots {
            home: PathBuf::from("/Users/sam"),
            system: PathBuf::from("/"),
            user_temp: None,
        };
        let places = analysis::places(&roots);
        let shown = |id: &str| {
            let p = places.iter().find(|p| p.id == id).unwrap();
            display(&roots, p)
        };
        assert_eq!(shown("user_caches").as_deref(), Some("~/Library/Caches"));
        assert_eq!(shown("shared_caches").as_deref(), Some("/Library/Caches"));
        assert_eq!(
            shown("sleep_image").as_deref(),
            Some("/private/var/vm/sleepimage")
        );
    }

    #[test]
    fn a_part_with_an_unreadable_place_says_so() {
        let place = |state, partly| SystemDataPlace {
            id: "x".into(),
            owner: SystemDataOwner::System,
            state,
            size_bytes: 10,
            file_count: 1,
            partly_unreadable: partly,
            path: None,
        };
        let fine = part_record(
            SystemDataKind::Logs,
            vec![
                place(SystemDataPlaceState::Measured, false),
                place(SystemDataPlaceState::Missing, false),
            ],
        );
        assert!(!fine.partly_unreadable);
        assert_eq!(fine.size_bytes, 20);
        let blocked = part_record(
            SystemDataKind::Logs,
            vec![place(SystemDataPlaceState::Unreadable, false)],
        );
        assert!(blocked.partly_unreadable);
    }
}
