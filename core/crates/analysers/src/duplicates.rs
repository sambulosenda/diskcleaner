//! Finds files that are exact copies of each other, for the duplicate
//! finder (PRD E5).
//!
//! Read only. Nothing here changes a file, starts a program or follows a
//! symlink. Files are matched by content, never by name or date:
//!
//! 1. **Walk** the given folders and keep every plain file of at least
//!    [`MIN_FILE_BYTES`]. Every entry is read with `symlink_metadata`; a
//!    symlink, anything on another volume, hidden files and folders, app
//!    and document bundles (folders with an extension, such as `.app` or
//!    `.photoslibrary`), code projects (a folder holding `.git`) and
//!    `node_modules` are skipped. A file stored only in iCloud (not
//!    downloaded) is skipped, because reading it would download it.
//! 2. **Hard links once**: names of the same file (`st_dev`, `st_ino`) are
//!    one file, not copies, since removing one frees nothing.
//! 3. **Size**: only files with exactly the same length can be copies.
//! 4. **First bytes**: files of the same length are compared by the SHA-256
//!    of their first [`PREFIX_BYTES`].
//! 5. **Whole content**: files that still match are compared by the SHA-256
//!    of their whole content.
//!
//! Each file is opened with `O_NOFOLLOW | O_NONBLOCK`, and must still be the
//! same file (device, inode, length and modification time) after it was read,
//! or it is left out.
//!
//! The executor compares the bytes again, directly against the copy the user
//! keeps, just before it moves anything. The hashes here only find
//! candidates.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

/// Files smaller than this are not looked at: they free too little to be
/// worth the user's time, and there are far too many of them.
pub const MIN_FILE_BYTES: u64 = 1024 * 1024;

/// How many bytes from the start of each file are compared first.
pub const PREFIX_BYTES: usize = 64 * 1024;

/// How much is read at a time.
const CHUNK_BYTES: usize = 1024 * 1024;

/// The most groups returned, largest first. A Mac with more than this has
/// bigger problems than one screen can show.
pub const MAX_GROUPS: usize = 500;

/// One file found by the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    /// Absolute path.
    pub path: PathBuf,
    /// Device id (`st_dev`).
    pub dev: u64,
    /// Inode number (`st_ino`).
    pub ino: u64,
    /// Length in bytes (`st_size`).
    pub len: u64,
    /// Allocated bytes (`st_blocks * 512`), the way the scanner measures.
    pub allocated_bytes: u64,
    /// Modification time (`lstat`).
    pub modified: SystemTime,
    /// macOS only: the bytes only this file uses, which is what removing it
    /// would free. Less than `allocated_bytes` for an APFS clone, which
    /// shares its space with the file it was copied from. `None` where the
    /// system can't say.
    pub private_bytes: Option<u64>,
}

impl Copy {
    /// What removing this file would free, as far as is known.
    pub fn frees_bytes(&self) -> u64 {
        self.private_bytes
            .map_or(self.allocated_bytes, |p| p.min(self.allocated_bytes))
    }

    /// Whether this file shares most of its space with another file (an
    /// APFS clone), so removing it frees little.
    pub fn shares_space(&self) -> bool {
        self.frees_bytes() < self.allocated_bytes / 2
    }
}

/// Files with the same content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The length of each file, in bytes.
    pub len: u64,
    /// The SHA-256 of the content.
    pub sha256: [u8; 32],
    /// At least two different files (never two names of one file), sorted
    /// oldest first, then by path.
    pub copies: Vec<Copy>,
}

/// Where the search has got to. Written by the search, read from any thread.
#[derive(Debug, Default)]
pub struct Progress {
    /// Set once the walk is done and comparing has started.
    pub comparing: AtomicBool,
    /// Files looked at so far.
    pub files_seen: AtomicU64,
    /// Bytes to read while comparing, known once comparing starts.
    pub bytes_to_compare: AtomicU64,
    /// Bytes read so far while comparing.
    pub bytes_compared: AtomicU64,
}

/// The search was stopped by the cancel flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the duplicate search was stopped")]
pub struct Cancelled;

/// Finds files with the same content in `folders`, which must be absolute
/// and on the volume `dev`. A folder that is missing, a symlink or on
/// another volume is skipped.
///
/// `cancel` is checked often; when set, the search stops with [`Cancelled`].
/// Groups come back with the most space to free first, at most
/// [`MAX_GROUPS`].
pub fn find(
    folders: &[PathBuf],
    dev: u64,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<Vec<Group>, Cancelled> {
    let mut files = Vec::new();
    for folder in folders {
        walk_root(folder, dev, &mut files, progress, cancel)?;
    }
    let candidates = same_length(one_name_each(files));
    let total: u64 = candidates
        .iter()
        .flatten()
        .fold(0u64, |sum, c| sum.saturating_add(c.len));
    progress.bytes_to_compare.store(total, Ordering::Relaxed);
    progress.comparing.store(true, Ordering::Relaxed);
    let mut groups = Vec::new();
    for same_len in candidates {
        groups.extend(compare(same_len, progress, cancel)?);
    }
    groups.sort_by(|a, b| {
        could_free(b)
            .cmp(&could_free(a))
            .then_with(|| a.copies[0].path.cmp(&b.copies[0].path))
    });
    groups.truncate(MAX_GROUPS);
    Ok(groups)
}

/// The most removing copies of `group` could free while keeping one: every
/// copy but the one that frees least.
pub fn could_free(group: &Group) -> u64 {
    let frees: Vec<u64> = group.copies.iter().map(Copy::frees_bytes).collect();
    let sum = frees.iter().fold(0u64, |s, f| s.saturating_add(*f));
    sum.saturating_sub(frees.iter().copied().min().unwrap_or(0))
}

/// Walks one root folder, if it is a real folder on `dev`.
fn walk_root(
    root: &Path,
    dev: u64,
    files: &mut Vec<Copy>,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<(), Cancelled> {
    let Ok(meta) = fs::symlink_metadata(root) else {
        return Ok(());
    };
    if !root.is_absolute() || !meta.is_dir() || meta.dev() != dev {
        return Ok(());
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled);
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.dev() != dev || is_hidden(&path) {
                continue;
            }
            if meta.is_dir() {
                if enters(&path) {
                    stack.push(path);
                }
            } else if meta.is_file() {
                progress.files_seen.fetch_add(1, Ordering::Relaxed);
                if let Some(copy) = candidate(path, &meta) {
                    files.push(copy);
                }
            }
        }
    }
    Ok(())
}

/// Whether the name starts with a dot.
fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.as_encoded_bytes().first() == Some(&b'.'))
}

/// Whether the walk goes into this folder. Not into bundles (any folder
/// whose name has an extension, such as `.app`, `.photoslibrary` or
/// `.fcpbundle`: Finder shows most of them as one file, and removing one file
/// from inside would break it), `node_modules`, or a code project.
fn enters(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        // A name that isn't valid text is left alone.
        return false;
    };
    if name == "node_modules" || has_extension(name) {
        return false;
    }
    // A folder holding `.git` (a folder or, for a worktree, a file).
    fs::symlink_metadata(dir.join(".git")).is_err()
}

/// Whether a name ends in `.something`, the way bundles are named.
fn has_extension(name: &str) -> bool {
    match name.rfind('.') {
        Some(i) => i > 0 && i + 1 < name.len(),
        None => false,
    }
}

/// The file as a [`Copy`], if it is big enough and its content is on this Mac.
fn candidate(path: PathBuf, meta: &fs::Metadata) -> Option<Copy> {
    let len = meta.len();
    let allocated_bytes = meta.blocks().saturating_mul(512);
    // No blocks means nothing would be freed, or the content is elsewhere.
    if len < MIN_FILE_BYTES || allocated_bytes == 0 || in_cloud_only(meta) {
        return None;
    }
    Some(Copy {
        private_bytes: clones::private_bytes(&path),
        path,
        dev: meta.dev(),
        ino: meta.ino(),
        len,
        allocated_bytes,
        modified: meta.modified().ok()?,
    })
}

/// macOS: whether the file's content is in iCloud only (`SF_DATALESS`), so
/// reading it would download it.
#[cfg(target_os = "macos")]
fn in_cloud_only(meta: &fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt as _;
    /// `SF_DATALESS` from `<sys/stat.h>`.
    const SF_DATALESS: u32 = 0x4000_0000;
    meta.st_flags() & SF_DATALESS != 0
}

/// Other systems have no such files.
#[cfg(not(target_os = "macos"))]
fn in_cloud_only(_meta: &fs::Metadata) -> bool {
    false
}

/// Keeps one name of each file (the first by path), so hard links are
/// never offered as copies of each other.
fn one_name_each(mut files: Vec<Copy>) -> Vec<Copy> {
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut seen = std::collections::HashSet::new();
    files.retain(|f| seen.insert((f.dev, f.ino)));
    files
}

/// Files grouped by length, only lengths with more than one file.
fn same_length(files: Vec<Copy>) -> Vec<Vec<Copy>> {
    let mut by_len: HashMap<u64, Vec<Copy>> = HashMap::new();
    for f in files {
        by_len.entry(f.len).or_default().push(f);
    }
    let mut groups: Vec<Vec<Copy>> = by_len.into_values().filter(|g| g.len() > 1).collect();
    groups.sort_by_key(|g| g[0].len);
    groups
}

/// Splits files of one length into groups with the same content.
fn compare(
    files: Vec<Copy>,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<Vec<Group>, Cancelled> {
    let len = files.first().map_or(0, |f| f.len);
    let mut by_prefix: HashMap<[u8; 32], Vec<Copy>> = HashMap::new();
    for f in files {
        if let Some(h) = hash(&f, Some(PREFIX_BYTES), progress, cancel)? {
            by_prefix.entry(h).or_default().push(f);
        }
    }
    let mut groups = Vec::new();
    for (_, same_start) in by_prefix {
        if same_start.len() < 2 {
            continue;
        }
        let mut by_hash: HashMap<[u8; 32], Vec<Copy>> = HashMap::new();
        for f in same_start {
            // The prefix already covered a short file; reading it again
            // still checks it hasn't changed.
            if let Some(h) = hash(&f, None, progress, cancel)? {
                by_hash.entry(h).or_default().push(f);
            }
        }
        for (sha256, mut copies) in by_hash {
            if copies.len() < 2 {
                continue;
            }
            copies.sort_by(|a, b| a.modified.cmp(&b.modified).then(a.path.cmp(&b.path)));
            groups.push(Group {
                len,
                sha256,
                copies,
            });
        }
    }
    Ok(groups)
}

/// The SHA-256 of the file's first `limit` bytes, or all of it. `None` if
/// it can't be read, or is no longer the file that was found.
fn hash(
    file: &Copy,
    limit: Option<usize>,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<Option<[u8; 32]>, Cancelled> {
    let Ok(mut f) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&file.path)
    else {
        return Ok(None);
    };
    if !same_file(f.metadata().ok().as_ref(), file) {
        return Ok(None);
    }
    let want = limit.map_or(file.len, |l| (l as u64).min(file.len));
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut done: u64 = 0;
    while done < want {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled);
        }
        let room = usize::try_from(want - done).map_or(buf.len(), |r| r.min(buf.len()));
        let n = match f.read(&mut buf[..room]) {
            Ok(0) => return Ok(None),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Ok(None),
        };
        hasher.update(&buf[..n]);
        done += n as u64;
        if limit.is_none() {
            progress
                .bytes_compared
                .fetch_add(n as u64, Ordering::Relaxed);
        }
    }
    // Still the same file, unchanged, after reading it.
    if !same_file(f.metadata().ok().as_ref(), file)
        || !same_file(fs::symlink_metadata(&file.path).ok().as_ref(), file)
    {
        return Ok(None);
    }
    Ok(Some(hasher.finalize().into()))
}

/// Whether `meta` is the plain file `file`, with the same length and date.
fn same_file(meta: Option<&fs::Metadata>, file: &Copy) -> bool {
    meta.is_some_and(|m| {
        m.is_file()
            && m.dev() == file.dev
            && m.ino() == file.ino
            && m.len() == file.len
            && m.modified().ok() == Some(file.modified)
    })
}

/// How much of a file's space is its own, on macOS.
mod clones {
    use std::path::Path;

    /// The bytes only this file uses (`ATTR_CMNEXT_PRIVATESIZE`), read with
    /// `getattrlist` without following a symlink. `None` if the system
    /// doesn't say.
    #[cfg(target_os = "macos")]
    pub fn private_bytes(path: &Path) -> Option<u64> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        /// `ATTR_CMNEXT_PRIVATESIZE` from `<sys/attr.h>`.
        const ATTR_CMNEXT_PRIVATESIZE: u32 = 0x0000_0008;
        /// `FSOPT_ATTR_CMN_EXTENDED` from `<sys/attr.h>`.
        const FSOPT_ATTR_CMN_EXTENDED: u32 = 0x0000_0020;

        /// What `getattrlist` writes: its length, which attributes it
        /// returned, then the private size.
        #[repr(C, packed(4))]
        struct Reply {
            length: u32,
            returned: libc::attribute_set_t,
            private_size: libc::off_t,
        }

        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: an all-zero `attrlist` is a valid value of this C struct.
        let mut list: libc::attrlist = unsafe { std::mem::zeroed() };
        list.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
        list.commonattr = libc::ATTR_CMN_RETURNED_ATTRS;
        list.forkattr = ATTR_CMNEXT_PRIVATESIZE;
        // SAFETY: an all-zero `Reply` is a valid value of this plain struct.
        let mut reply: Reply = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is a valid C string, `list` a valid attrlist, and
        // `reply` is writable for the size given.
        let ret = unsafe {
            libc::getattrlist(
                c.as_ptr(),
                std::ptr::addr_of_mut!(list).cast(),
                std::ptr::addr_of_mut!(reply).cast(),
                std::mem::size_of::<Reply>(),
                libc::FSOPT_NOFOLLOW | FSOPT_ATTR_CMN_EXTENDED,
            )
        };
        if ret != 0 {
            return None;
        }
        let returned = reply.returned;
        let length = reply.length;
        if returned.forkattr & ATTR_CMNEXT_PRIVATESIZE == 0
            || (length as usize) < std::mem::size_of::<Reply>()
        {
            return None;
        }
        let size = reply.private_size;
        u64::try_from(size).ok()
    }

    /// Other systems have no clones to tell apart.
    #[cfg(not(target_os = "macos"))]
    pub fn private_bytes(_path: &Path) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp folder (canonical, so macOS's /var link is resolved) and its dev.
    fn folder() -> (tempfile::TempDir, PathBuf, u64) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let dev = fs::symlink_metadata(&root).unwrap().dev();
        (dir, root, dev)
    }

    /// `MIN_FILE_BYTES` plus `extra` bytes, all `fill`, except `last` at the end.
    fn content(fill: u8, extra: usize, last: u8) -> Vec<u8> {
        let mut v = vec![fill; MIN_FILE_BYTES as usize + extra];
        if let Some(b) = v.last_mut() {
            *b = last;
        }
        v
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    fn run(root: &Path, dev: u64) -> Vec<Group> {
        find(
            &[root.to_path_buf()],
            dev,
            &Progress::default(),
            &AtomicBool::new(false),
        )
        .unwrap()
    }

    fn names(g: &Group) -> Vec<String> {
        let mut n: Vec<String> = g
            .copies
            .iter()
            .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        n.sort();
        n
    }

    #[test]
    fn finds_copies_by_content_not_by_name() {
        let (_d, root, dev) = folder();
        let a = content(1, 10, 9);
        write(&root, "Trips/beach.mov", &a);
        write(&root, "Old/copy of something.bin", &a);
        // Same name and length, different last byte: not a copy.
        write(&root, "Other/beach.mov", &content(1, 10, 8));
        let groups = run(&root, dev);
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(names(&groups[0]), ["beach.mov", "copy of something.bin"]);
        assert_eq!(groups[0].len, a.len() as u64);
    }

    #[test]
    fn a_difference_after_the_first_bytes_is_found() {
        let (_d, root, dev) = folder();
        let mut b = content(3, PREFIX_BYTES * 3, 3);
        write(&root, "a.bin", &b);
        b[PREFIX_BYTES * 2] = 4;
        write(&root, "b.bin", &b);
        assert!(run(&root, dev).is_empty());
    }

    #[test]
    fn small_files_are_left_out() {
        let (_d, root, dev) = folder();
        let small = vec![7u8; (MIN_FILE_BYTES - 1) as usize];
        write(&root, "a.txt", &small);
        write(&root, "b.txt", &small);
        assert!(run(&root, dev).is_empty());
    }

    #[test]
    fn hard_links_are_one_file_not_copies() {
        let (_d, root, dev) = folder();
        let a = write(&root, "a.bin", &content(5, 0, 5));
        fs::hard_link(&a, root.join("link.bin")).unwrap();
        assert!(run(&root, dev).is_empty());
        // A real copy alongside: the group holds one name of the linked file.
        write(&root, "real copy.bin", &content(5, 0, 5));
        let groups = run(&root, dev);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].copies.len(), 2, "{groups:?}");
    }

    #[test]
    fn symlinks_hidden_items_bundles_projects_and_node_modules_are_skipped() {
        let (_d, root, dev) = folder();
        let a = content(6, 0, 6);
        let original = write(&root, "keep/original.bin", &a);
        std::os::unix::fs::symlink(&original, root.join("link.bin")).unwrap();
        std::os::unix::fs::symlink(root.join("keep"), root.join("linked folder")).unwrap();
        write(&root, ".hidden.bin", &a);
        write(&root, ".cache/x.bin", &a);
        write(&root, "Photos Library.photoslibrary/originals/x.bin", &a);
        write(&root, "Tool.app/Contents/x.bin", &a);
        write(&root, "project/.git/HEAD", b"ref");
        write(&root, "project/data/x.bin", &a);
        write(&root, "web/node_modules/x.bin", &a);
        assert!(run(&root, dev).is_empty(), "{:?}", run(&root, dev));
    }

    #[test]
    fn folders_with_dots_inside_their_name_are_still_entered_when_not_an_extension() {
        assert!(!has_extension("Photos"));
        assert!(!has_extension(".hidden"));
        assert!(!has_extension("trailing."));
        assert!(has_extension("Report.pages"));
        assert!(has_extension("v1.2"));
    }

    #[test]
    fn copies_are_oldest_first_and_groups_most_space_first() {
        let (_d, root, dev) = folder();
        let small = content(1, 0, 1);
        let big = content(2, MIN_FILE_BYTES as usize, 2);
        let newer = write(&root, "a/new.bin", &small);
        let older = write(&root, "b/old.bin", &small);
        let past = SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&older)
            .unwrap()
            .set_modified(past)
            .unwrap();
        write(&root, "c/big1.bin", &big);
        write(&root, "c/big2.bin", &big);
        let groups = run(&root, dev);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len, big.len() as u64);
        assert_eq!(groups[1].copies[0].path, older);
        assert_eq!(groups[1].copies[1].path, newer);
        assert!(could_free(&groups[0]) > 0);
    }

    #[test]
    fn a_missing_or_linked_root_is_skipped() {
        let (_d, root, dev) = folder();
        let a = content(8, 0, 8);
        write(&root, "real/a.bin", &a);
        write(&root, "real/b.bin", &a);
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        let groups = find(
            &[root.join("missing"), root.join("link")],
            dev,
            &Progress::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn the_cancel_flag_stops_the_search() {
        let (_d, root, dev) = folder();
        write(&root, "a.bin", &content(1, 0, 1));
        let err = find(
            std::slice::from_ref(&root),
            dev,
            &Progress::default(),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert_eq!(err, Cancelled);
    }

    #[test]
    fn progress_counts_files_and_bytes() {
        let (_d, root, dev) = folder();
        let a = content(1, 0, 1);
        write(&root, "a.bin", &a);
        write(&root, "b.bin", &a);
        let progress = Progress::default();
        find(
            std::slice::from_ref(&root),
            dev,
            &progress,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(progress.files_seen.load(Ordering::Relaxed), 2);
        assert_eq!(
            progress.bytes_to_compare.load(Ordering::Relaxed),
            2 * a.len() as u64
        );
        assert_eq!(
            progress.bytes_compared.load(Ordering::Relaxed),
            2 * a.len() as u64
        );
        assert!(progress.comparing.load(Ordering::Relaxed));
    }

    #[test]
    fn could_free_keeps_the_copy_that_frees_least() {
        let copy = |private| Copy {
            path: PathBuf::from("/x"),
            dev: 1,
            ino: 1,
            len: 100,
            allocated_bytes: 100,
            modified: SystemTime::UNIX_EPOCH,
            private_bytes: private,
        };
        let g = Group {
            len: 100,
            sha256: [0; 32],
            copies: vec![copy(None), copy(Some(0)), copy(None)],
        };
        assert_eq!(could_free(&g), 200);
        assert!(g.copies[1].shares_space());
        assert!(!g.copies[0].shares_space());
    }

    /// macOS on APFS: a clone made with `clonefile` shares its space, so
    /// removing it frees (almost) nothing.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_apfs_clone_is_flagged_as_sharing_space() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let (_d, root, dev) = folder();
        let a = write(&root, "a.bin", &content(9, MIN_FILE_BYTES as usize, 1));
        let b = root.join("b.bin");
        let ca = CString::new(a.as_os_str().as_bytes()).unwrap();
        let cb = CString::new(b.as_os_str().as_bytes()).unwrap();
        // SAFETY: both are valid C strings.
        if unsafe { libc::clonefile(ca.as_ptr(), cb.as_ptr(), 0) } != 0 {
            eprintln!("clonefile isn't supported here; skipping");
            return;
        }
        let groups = run(&root, dev);
        assert_eq!(groups.len(), 1);
        let private: Vec<Option<u64>> = groups[0].copies.iter().map(|c| c.private_bytes).collect();
        assert!(private.iter().all(Option::is_some), "{private:?}");
        assert!(
            groups[0].copies.iter().any(Copy::shares_space),
            "{:?}",
            groups[0].copies
        );
    }
}
