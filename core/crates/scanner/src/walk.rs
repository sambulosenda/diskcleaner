//! The parallel walk. Reads folder listings and `lstat` metadata only.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};

use rayon::prelude::*;

use crate::largest::Largest;
use crate::result::{Dir, FileEntry, ScanResult, Unreadable, UnreadableReason};
use crate::{RecordFiles, ScanError, ScanOptions, ScanProgress};

/// Stack size for walk threads. The walk recurses once per folder level and
/// rayon may stack stolen work on top, so leave plenty of room.
const WALK_STACK_BYTES: usize = 16 * 1024 * 1024;

/// How many entries to read between checks of the cancel flag.
const CANCEL_CHECK_EVERY: usize = 256;

/// One folder while the walk runs. Turned into flat [`Dir`] records after.
struct Node {
    name: OsString,
    own_bytes: u64,
    own_logical: u64,
    own_files: u64,
    unreadable: bool,
    has_git: bool,
    files: Vec<FileEntry>,
    /// Files in this folder with more than one hard link. Counted or skipped
    /// after the walk, by [`settle_links`].
    links: Vec<LinkedFile>,
    children: Vec<Node>,
}

/// A file with more than one hard link, waiting for the walk to finish.
///
/// Threads reach the names of a hard-linked file in no fixed order, so
/// counting the first name seen would put its bytes in a different folder
/// from one scan to the next. Instead every name is noted, and after the
/// walk the file is counted once, under the name whose full path sorts
/// first. The same tree then always gives the same sizes.
struct LinkedFile {
    key: (u64, u64),
    path: PathBuf,
    name: OsString,
    allocated: u64,
    logical: u64,
    modified: Option<std::time::SystemTime>,
    is_file: bool,
    is_symlink: bool,
    record: bool,
}

/// State shared by every worker thread.
struct Shared<'a> {
    root: &'a Path,
    record_files: Option<&'a RecordFiles>,
    dev: u64,
    cancel: &'a AtomicBool,
    largest: Largest,
    /// For each hard-linked inode, the name that sorts first so far.
    link_owner: Mutex<HashMap<(u64, u64), PathBuf>>,
    unreadable: Mutex<Vec<Unreadable>>,
    other_volumes: Mutex<Vec<PathBuf>>,
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    symlinks: AtomicU64,
    hard_links_skipped: AtomicU64,
    entries_skipped: AtomicU64,
}

/// A sub-folder found in a listing, to be walked next.
struct SubDir {
    path: PathBuf,
    name: OsString,
    meta: Metadata,
}

/// Runs the walk on its own thread pool and reports progress on this thread.
pub fn run(
    root: &Path,
    root_meta: &Metadata,
    opts: &ScanOptions,
    progress: &impl Fn(ScanProgress),
) -> Result<ScanResult, ScanError> {
    let pool = build_pool(opts.threads)?;
    let shared = Shared::new(
        root,
        opts.record_files.as_ref(),
        root_meta.dev(),
        &opts.cancel,
        opts.largest_files,
    );
    let (tx, rx) = mpsc::channel();
    let root_node = pool.in_place_scope(|scope| {
        let shared = &shared;
        scope.spawn(move |_| {
            let node = walk_dir(root, OsString::new(), root_meta, shared);
            // The receiver only goes away if this thread panicked first.
            let _ = tx.send(node);
        });
        wait_with_progress(&rx, shared, opts, progress)
    });
    let mut root_node = root_node.ok_or(ScanError::Internal)?;
    if opts.cancel.load(Ordering::Relaxed) {
        return Err(ScanError::Cancelled);
    }
    settle_links(&mut root_node, &shared);
    progress(shared.snapshot(true));
    Ok(shared.into_result(root, root_node))
}

fn build_pool(threads: Option<usize>) -> Result<rayon::ThreadPool, ScanError> {
    let mut builder = rayon::ThreadPoolBuilder::new()
        .stack_size(WALK_STACK_BYTES)
        .thread_name(|i| format!("roomy-scan-{i}"));
    if let Some(n) = threads {
        builder = builder.num_threads(n.max(1));
    }
    builder
        .build()
        .map_err(|e| ScanError::Threads(e.to_string()))
}

/// Calls `progress` every interval until the walk sends its result.
fn wait_with_progress(
    rx: &mpsc::Receiver<Node>,
    shared: &Shared<'_>,
    opts: &ScanOptions,
    progress: &impl Fn(ScanProgress),
) -> Option<Node> {
    loop {
        match rx.recv_timeout(opts.progress_interval) {
            Ok(node) => return Some(node),
            Err(mpsc::RecvTimeoutError::Timeout) => progress(shared.snapshot(false)),
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Walks one folder: counts its entries, then walks its sub-folders in parallel.
fn walk_dir(path: &Path, name: OsString, meta: &Metadata, shared: &Shared<'_>) -> Node {
    let mut node = Node {
        name,
        own_bytes: allocated(meta),
        own_logical: 0,
        own_files: 0,
        unreadable: false,
        has_git: false,
        files: Vec::new(),
        links: Vec::new(),
        children: Vec::new(),
    };
    shared.dirs.fetch_add(1, Ordering::Relaxed);
    shared.add_bytes(node.own_bytes);
    if shared.cancelled() {
        return node;
    }
    let subdirs = match list_dir(path, &mut node, shared) {
        Ok(subdirs) => subdirs,
        Err(err) => {
            node.unreadable = true;
            shared.record_unreadable(path, &err);
            return node;
        }
    };
    node.children = subdirs
        .into_par_iter()
        .map(|sub| walk_dir(&sub.path, sub.name, &sub.meta, shared))
        .collect();
    node
}

/// Counts each hard-linked file once, in the folder of the name its inode is
/// owned by (see [`LinkedFile`]); its other names are skipped. Iterative, so
/// a deep tree cannot overflow the stack.
fn settle_links(root: &mut Node, shared: &Shared<'_>) {
    let mut stack: Vec<&mut Node> = vec![root];
    while let Some(node) = stack.pop() {
        let links = std::mem::take(&mut node.links);
        let mut recorded = false;
        for link in links {
            if !shared.owns_link(link.key, &link.path) {
                shared.hard_links_skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            node.own_bytes += link.allocated;
            if link.is_symlink {
                shared.symlinks.fetch_add(1, Ordering::Relaxed);
            } else if link.is_file {
                node.own_files += 1;
                node.own_logical += link.logical;
                shared.files.fetch_add(1, Ordering::Relaxed);
                if link.record {
                    node.files.push(FileEntry {
                        name: link.name,
                        allocated_bytes: link.allocated,
                        logical_bytes: link.logical,
                    });
                    recorded = true;
                }
                let path = link.path;
                shared
                    .largest
                    .offer(link.allocated, link.logical, link.modified, || path);
            }
        }
        if recorded {
            node.files.sort_by(|a, b| a.name.cmp(&b.name));
        }
        stack.extend(node.children.iter_mut());
    }
}

/// Reads one listing. Counts files and symlinks into `node` and returns the
/// sub-folders on the same volume, sorted by name.
fn list_dir(path: &Path, node: &mut Node, shared: &Shared<'_>) -> io::Result<Vec<SubDir>> {
    let mut subdirs = Vec::new();
    let record = shared.wants_files(path);
    for (i, entry) in fs::read_dir(path)?.enumerate() {
        if i % CANCEL_CHECK_EVERY == 0 && shared.cancelled() {
            break;
        }
        let entry = entry?;
        // DirEntry::metadata is lstat: it never follows a symlink.
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(err) => {
                tracing::debug!(path = %entry.path().display(), %err, "entry skipped");
                shared.entries_skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        let file_type = meta.file_type();
        if is_git_name(&entry.file_name()) {
            node.has_git = true;
        }
        if file_type.is_dir() {
            if meta.dev() != shared.dev {
                shared.record_other_volume(entry.path());
                continue;
            }
            subdirs.push(SubDir {
                path: entry.path(),
                name: entry.file_name(),
                meta,
            });
            continue;
        }
        let bytes = allocated(&meta);
        if meta.nlink() > 1 {
            let path = entry.path();
            let key = (meta.dev(), meta.ino());
            if shared.note_link(key, &path) {
                shared.add_bytes(bytes);
            }
            node.links.push(LinkedFile {
                key,
                path,
                name: entry.file_name(),
                allocated: bytes,
                logical: meta.len(),
                modified: meta.modified().ok(),
                is_file: file_type.is_file(),
                is_symlink: file_type.is_symlink(),
                record,
            });
            continue;
        }
        node.own_bytes += bytes;
        shared.add_bytes(bytes);
        if file_type.is_symlink() {
            shared.symlinks.fetch_add(1, Ordering::Relaxed);
        } else if file_type.is_file() {
            node.own_files += 1;
            node.own_logical += meta.len();
            shared.files.fetch_add(1, Ordering::Relaxed);
            if record {
                node.files.push(FileEntry {
                    name: entry.file_name(),
                    allocated_bytes: bytes,
                    logical_bytes: meta.len(),
                });
            }
            shared
                .largest
                .offer(bytes, meta.len(), meta.modified().ok(), || entry.path());
        }
    }
    subdirs.sort_by(|a, b| a.name.cmp(&b.name));
    node.files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(subdirs)
}

/// Whether a name is `.git`, ignoring ASCII case.
fn is_git_name(name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().eq_ignore_ascii_case(b".git")
}

/// Space on disk: `st_blocks` is always in 512-byte units.
fn allocated(meta: &Metadata) -> u64 {
    meta.blocks().saturating_mul(512)
}

impl<'a> Shared<'a> {
    fn new(
        root: &'a Path,
        record_files: Option<&'a RecordFiles>,
        dev: u64,
        cancel: &'a AtomicBool,
        largest_files: usize,
    ) -> Self {
        Self {
            root,
            record_files,
            dev,
            cancel,
            largest: Largest::new(largest_files),
            link_owner: Mutex::new(HashMap::new()),
            unreadable: Mutex::new(Vec::new()),
            other_volumes: Mutex::new(Vec::new()),
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            symlinks: AtomicU64::new(0),
            hard_links_skipped: AtomicU64::new(0),
            entries_skipped: AtomicU64::new(0),
        }
    }

    /// Whether the caller asked for the files in folder `path` to be listed.
    fn wants_files(&self, path: &Path) -> bool {
        let Some(RecordFiles(want)) = self.record_files else {
            return false;
        };
        match path.strip_prefix(self.root) {
            Ok(rel) => want(rel),
            Err(_) => false,
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn add_bytes(&self, bytes: u64) {
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Notes one name of a hard-linked inode, keeping the name that sorts
    /// first. Returns `true` the first time the inode is seen (for progress
    /// only; which name is counted is decided after the walk).
    fn note_link(&self, key: (u64, u64), path: &Path) -> bool {
        match self.link_owner.lock() {
            Ok(mut owners) => match owners.get_mut(&key) {
                Some(owner) => {
                    if path < owner.as_path() {
                        *owner = path.to_path_buf();
                    }
                    false
                }
                None => {
                    owners.insert(key, path.to_path_buf());
                    true
                }
            },
            // A poisoned lock means another worker panicked; the scan is
            // failing anyway.
            Err(_) => false,
        }
    }

    /// Whether `path` is the name a hard-linked inode is counted under. If
    /// the owner list is unusable (a worker panicked), every name counts:
    /// counting too much is safer than hiding a file.
    fn owns_link(&self, key: (u64, u64), path: &Path) -> bool {
        match self.link_owner.lock() {
            Ok(owners) => owners.get(&key).is_none_or(|owner| owner == path),
            Err(_) => true,
        }
    }

    fn record_unreadable(&self, path: &Path, err: &io::Error) {
        tracing::debug!(path = %path.display(), %err, "folder unreadable");
        let reason = match err.kind() {
            io::ErrorKind::PermissionDenied => UnreadableReason::PermissionDenied,
            io::ErrorKind::NotFound => UnreadableReason::Vanished,
            _ => UnreadableReason::Other(err.to_string()),
        };
        if let Ok(mut list) = self.unreadable.lock() {
            list.push(Unreadable {
                path: path.to_path_buf(),
                reason,
            });
        }
    }

    fn record_other_volume(&self, path: PathBuf) {
        tracing::debug!(path = %path.display(), "other volume skipped");
        if let Ok(mut list) = self.other_volumes.lock() {
            list.push(path);
        }
    }

    fn snapshot(&self, finished: bool) -> ScanProgress {
        ScanProgress {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            finished,
        }
    }

    /// Flattens the tree and gathers the shared lists into the result.
    fn into_result(self, root: &Path, root_node: Node) -> ScanResult {
        let mut unreadable = take(self.unreadable);
        unreadable.sort_by(|a, b| a.path.cmp(&b.path));
        let mut other_volumes = take(self.other_volumes);
        other_volumes.sort();
        ScanResult {
            root: root.to_path_buf(),
            dirs: flatten(root_node),
            largest_files: self.largest.into_sorted(),
            unreadable,
            other_volumes,
            symlinks: self.symlinks.into_inner(),
            hard_links_skipped: self.hard_links_skipped.into_inner(),
            entries_skipped: self.entries_skipped.into_inner(),
        }
    }
}

fn take<T>(list: Mutex<Vec<T>>) -> Vec<T> {
    match list.into_inner() {
        Ok(list) => list,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Turns the nested tree into flat records (parents before children), then
/// adds each folder's totals into its parent, deepest first. Iterative, so a
/// deep tree cannot overflow the stack.
fn flatten(root: Node) -> Vec<Dir> {
    let mut dirs: Vec<Dir> = Vec::new();
    let mut stack: Vec<(Node, Option<usize>)> = vec![(root, None)];
    while let Some((node, parent)) = stack.pop() {
        let id = dirs.len();
        dirs.push(Dir {
            name: node.name,
            parent,
            children: Vec::with_capacity(node.children.len()),
            own_bytes: node.own_bytes,
            total_bytes: node.own_bytes,
            total_logical_bytes: node.own_logical,
            total_files: node.own_files,
            unreadable: node.unreadable,
            has_git: node.has_git,
            files: node.files,
        });
        if let Some(p) = parent {
            dirs[p].children.push(id);
        }
        // Reversed so children are popped, and numbered, in name order.
        for child in node.children.into_iter().rev() {
            stack.push((child, Some(id)));
        }
    }
    for id in (1..dirs.len()).rev() {
        if let Some(p) = dirs[id].parent {
            let (bytes, logical, files) = (
                dirs[id].total_bytes,
                dirs[id].total_logical_bytes,
                dirs[id].total_files,
            );
            dirs[p].total_bytes += bytes;
            dirs[p].total_logical_bytes += logical;
            dirs[p].total_files += files;
        }
    }
    dirs
}
