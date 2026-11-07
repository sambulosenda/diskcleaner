//! Large files and old Downloads (E2): what the Large files screen lists,
//! and how a ticked row becomes a cleanup request.
//!
//! Nothing here changes a file. Rows are found once, on the scan thread,
//! from the scanner's own list of the largest files and its record of
//! `~/Downloads`; each is then looked at again (metadata and macOS's
//! last-opened record only, via `analysers::files`) and checked by the
//! safety rules. The app gets an id per row and sends ids back, never
//! paths.
//!
//! Safety, on top of everything every cleanup gets:
//! - Nothing is ever ticked for the user; every row is Review first.
//! - Rows are Trash only: someone's own file is never something a tool
//!   rebuilds, so permanent delete is never offered.
//! - Files inside `~/Library`, hidden folders, apps and libraries (see
//!   `analysers::files::may_list_large`) are never listed, so a single
//!   file is never pulled out of something bigger.
//! - Anything already explained by a rule (or inside such an item) is left
//!   to its own screen, so it is not offered twice.
//! - A file with more than one hard link is left out: removing one name
//!   frees nothing.
//! - A path the safety rules would refuse is not listed at all.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use classifier::{ClassifiedItem, UNKNOWN_RULE_ID};
use planner::{Expected, ItemRequest};
use scanner::ScanResult;

use crate::types::{FileRow, LargeFiles, Mode};
use crate::views::display_path;

/// How many rows the Large files list holds at most (PRD E2).
pub const LARGE_FILES: usize = 500;

/// How many of the largest files the scanner keeps. More than
/// [`LARGE_FILES`], because some are left out after the scan (inside a
/// rule's item, hard-linked, refused by the safety rules).
pub const SCANNED_LARGE_FILES: usize = 1000;

/// The name the app shows for the home folder itself as a place.
const HOME_PLACE: &str = "Home folder";

/// One file or folder the Large files screen can list.
#[derive(Debug, Clone)]
pub(crate) struct LooseFile {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub place: String,
    pub is_folder: bool,
    pub expected: Expected,
    pub last_opened: Option<SystemTime>,
}

/// Everything the Large files screen lists for one scan.
#[derive(Debug, Default)]
pub(crate) struct Loose {
    files: Vec<LooseFile>,
    /// Indexes into `files`, largest first.
    large: Vec<usize>,
    /// Indexes into `files`, largest first.
    old_downloads: Vec<usize>,
    by_id: HashMap<String, usize>,
}

impl Loose {
    /// The row with this id, if this scan has one.
    pub fn get(&self, id: &str) -> Option<&LooseFile> {
        self.by_id.get(id).and_then(|&i| self.files.get(i))
    }

    /// What the app sees. Paths only in Developer mode.
    pub fn view(&self, home: &Path, mode: Mode) -> LargeFiles {
        let rows = |list: &[usize]| -> Vec<FileRow> {
            list.iter()
                .filter_map(|&i| self.files.get(i))
                .map(|f| row(f, home, mode))
                .collect()
        };
        LargeFiles {
            large_files: rows(&self.large),
            old_downloads: rows(&self.old_downloads),
            old_after_days: old_after_days(),
        }
    }
}

/// [`analysers::files::OLD_DOWNLOAD_AFTER`] in whole days.
fn old_after_days() -> u32 {
    u32::try_from(analysers::files::OLD_DOWNLOAD_AFTER.as_secs() / 86_400).unwrap_or(u32::MAX)
}

/// The list row for one file. The path only in Developer mode.
fn row(f: &LooseFile, home: &Path, mode: Mode) -> FileRow {
    FileRow {
        item_id: f.id.clone(),
        name: f.name.clone(),
        place: f.place.clone(),
        size_bytes: f.expected.size_bytes,
        is_folder: f.is_folder,
        last_opened_unix_seconds: f.last_opened.map(crate::run::unix_seconds),
        modified_unix_seconds: crate::run::unix_seconds(f.expected.modified),
        path: match mode {
            Mode::Developer => display_path(home, &f.path),
            Mode::Simple => None,
        },
    }
}

/// The cleanup request for a ticked row. Trash only, never regenerable.
pub(crate) fn request_for(f: &LooseFile, chosen_alone: bool) -> ItemRequest {
    ItemRequest {
        recommendation_id: f.id.clone(),
        path: f.path.clone(),
        expected: f.expected,
        regenerable: false,
        selected_individually: chosen_alone,
        owners: &[],
        belongs_to: None,
        keep_copy: None,
    }
}

/// Which folders' files may be kept by the scanner as large files.
pub(crate) fn largest_in() -> scanner::RecordFiles {
    scanner::RecordFiles(std::sync::Arc::new(analysers::files::may_list_large))
}

/// Whether the scanner should list the files directly in `rel`: always for
/// `~/Downloads`, otherwise as `rules` says.
pub(crate) fn record_files(rules: scanner::RecordFiles) -> scanner::RecordFiles {
    scanner::RecordFiles(std::sync::Arc::new(move |rel: &Path| {
        rel == Path::new("Downloads") || (rules.0)(rel)
    }))
}

/// Finds the rows for a finished scan. Reads metadata of at most a few
/// thousand entries; changes nothing.
pub(crate) fn find(
    scan: &ScanResult,
    items: &[ClassifiedItem],
    home: &Path,
    now: SystemTime,
) -> Loose {
    let explained: HashSet<&Path> = items
        .iter()
        .filter(|i| i.rule_id != UNKNOWN_RULE_ID)
        .map(|i| i.path.as_path())
        .collect();
    let mut loose = Loose::default();
    let mut by_path: HashMap<PathBuf, usize> = HashMap::new();
    for file in &scan.largest_files {
        if loose.large.len() >= LARGE_FILES {
            break;
        }
        if is_explained(&file.path, home, &explained) {
            continue;
        }
        let Some(modified) = file.modified else {
            continue;
        };
        let expected = Expected {
            size_bytes: file.allocated_bytes,
            modified,
        };
        if let Some(f) = checked(home, scan.root_dev, &file.path, true, expected) {
            let i = add(&mut loose, &mut by_path, f);
            loose.large.push(i);
        }
    }
    for f in downloads(scan, home, &explained) {
        if !analysers::files::is_old(f.last_opened, Some(f.expected.modified), now) {
            continue;
        }
        let i = match by_path.get(&f.path) {
            Some(&i) => i,
            None => add(&mut loose, &mut by_path, f),
        };
        loose.old_downloads.push(i);
    }
    let files = &loose.files;
    loose.old_downloads.sort_by(|&a, &b| {
        files[b]
            .expected
            .size_bytes
            .cmp(&files[a].expected.size_bytes)
            .then_with(|| files[a].path.cmp(&files[b].path))
    });
    loose
}

/// Adds `f` to the table with the next id and returns its index.
fn add(loose: &mut Loose, by_path: &mut HashMap<PathBuf, usize>, mut f: LooseFile) -> usize {
    let i = loose.files.len();
    f.id = format!("file-{i}");
    loose.by_id.insert(f.id.clone(), i);
    by_path.insert(f.path.clone(), i);
    loose.files.push(f);
    i
}

/// Every file and folder directly in `~/Downloads` that passes the checks,
/// in no particular order.
fn downloads(scan: &ScanResult, home: &Path, explained: &HashSet<&Path>) -> Vec<LooseFile> {
    let Some(id) = scan.find(Path::new("Downloads")) else {
        return Vec::new();
    };
    let Some(dir) = scan.dirs.get(id) else {
        return Vec::new();
    };
    let folder = home.join("Downloads");
    let mut found = Vec::new();
    for file in &dir.files {
        let path = folder.join(&file.name);
        let Some(modified) = file.modified else {
            continue;
        };
        let expected = Expected {
            size_bytes: file.allocated_bytes,
            modified,
        };
        if is_explained(&path, home, explained) {
            continue;
        }
        found.extend(checked(home, scan.root_dev, &path, true, expected));
    }
    for &child in &dir.children {
        let Some(sub) = scan.dirs.get(child) else {
            continue;
        };
        // A folder that couldn't be read has an unknown size.
        if sub.unreadable || has_unreadable(scan, child) {
            continue;
        }
        let Some(modified) = sub.modified else {
            continue;
        };
        let path = folder.join(&sub.name);
        if is_explained(&path, home, explained) {
            continue;
        }
        let expected = Expected {
            size_bytes: sub.total_bytes,
            modified,
        };
        found.extend(checked(home, scan.root_dev, &path, false, expected));
    }
    found
}

/// Whether folder `id`, or anything inside it, could not be read.
fn has_unreadable(scan: &ScanResult, id: scanner::DirId) -> bool {
    let mut stack = vec![id];
    while let Some(i) = stack.pop() {
        let Some(dir) = scan.dirs.get(i) else {
            return true;
        };
        if dir.unreadable {
            return true;
        }
        stack.extend(dir.children.iter().copied());
    }
    false
}

/// Whether `path` is, or is inside, an item a rule explains.
fn is_explained(path: &Path, home: &Path, explained: &HashSet<&Path>) -> bool {
    path.ancestors()
        .take_while(|a| a.starts_with(home) && *a != home)
        .any(|a| explained.contains(a))
}

/// Looks at `path` again and returns its row, or `None` if it should not be
/// listed: it changed since the scan, isn't the expected kind, is on
/// another volume, has more than one hard link, or the safety rules would
/// refuse it. The row's id is filled in by [`add`].
fn checked(
    home: &Path,
    dev: u64,
    path: &Path,
    is_file: bool,
    expected: Expected,
) -> Option<LooseFile> {
    let facts = analysers::files::look(path)?;
    if facts.is_file != is_file || facts.dev != dev || facts.modified != Some(expected.modified) {
        return None;
    }
    if is_file && (facts.links != 1 || facts.allocated_bytes != expected.size_bytes) {
        return None;
    }
    if expected.size_bytes == 0 {
        return None;
    }
    safety::validate(home, dev, path, true).ok()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(LooseFile {
        id: String::new(),
        path: path.to_path_buf(),
        name,
        place: place(home, path),
        is_folder: !is_file,
        expected,
        last_opened: facts.last_opened,
    })
}

/// The folder in home that `path` is in, such as "Downloads", or
/// [`HOME_PLACE`] for something directly in home.
fn place(home: &Path, path: &Path) -> String {
    let Ok(rel) = path.strip_prefix(home) else {
        return HOME_PLACE.to_string();
    };
    let mut names = rel.iter();
    match (names.next(), names.next()) {
        (Some(first), Some(_)) => first.to_string_lossy().into_owned(),
        _ => HOME_PLACE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_name_the_folder_in_home() {
        let home = Path::new("/Users/sam");
        assert_eq!(
            place(home, Path::new("/Users/sam/Movies/a/b.mov")),
            "Movies"
        );
        assert_eq!(place(home, Path::new("/Users/sam/b.mov")), HOME_PLACE);
    }

    #[test]
    fn explained_means_the_item_or_inside_it() {
        let home = Path::new("/Users/sam");
        let item = PathBuf::from("/Users/sam/Downloads/a.dmg");
        let folder = PathBuf::from("/Users/sam/Code/app/node_modules");
        let set: HashSet<&Path> = [item.as_path(), folder.as_path()].into_iter().collect();
        assert!(is_explained(&item, home, &set));
        assert!(is_explained(&folder.join("x/big.bin"), home, &set));
        assert!(!is_explained(
            Path::new("/Users/sam/Downloads/b.dmg"),
            home,
            &set
        ));
        assert!(!is_explained(
            Path::new("/Users/sam/Code/app/big.bin"),
            home,
            &set
        ));
    }

    #[test]
    fn ninety_days_is_old() {
        assert_eq!(old_after_days(), 90);
    }
}
