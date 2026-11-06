//! Faster rescans (F12): read again only the folders that changed since an
//! earlier scan, and reuse that scan's figures for everything else.
//!
//! The caller says what changed (on macOS, [`crate::fsevents`] asks the
//! system's file event history). Every folder named there is listed again;
//! everything else is copied from the earlier result. The same read-only
//! rules as a full scan apply to what is read: `lstat` only, no symlinks
//! followed, one volume.
//!
//! A rescan refuses ([`RescanError::NeedsFullScan`]) rather than guess:
//!
//! - when a change is outside the scanned folder, or the folder itself
//!   moved or is on another volume now,
//! - when a folder it would read again holds (or held) a file with more
//!   than one hard link, because which name such a file is counted under
//!   depends on the whole tree,
//! - when the largest files are asked for, because the earlier scan did not
//!   keep the files below the top list,
//! - when a folder the earlier scan knew is missing, or no longer a folder,
//!   although nothing said its parent changed.
//!
//! Whenever a folder's fresh `lstat` is at hand (its parent was listed
//! again, or it was named itself), it is also checked against the earlier
//! record: a different inode means a different folder took the name (for
//! example one was moved away and another moved in), so it is read in full;
//! a different modification time means its entries changed, so it is
//! listed again.
//!
//! Folders the earlier scan could not read are always read again (Full Disk
//! Access may have been turned on since), and so are the folders that hold
//! a mount point it skipped.
//!
//! If the list of changes is complete, the result is the same as a full
//! scan's (tests check this). The caller must use the same
//! [`crate::ScanOptions::record_files`] as for the earlier scan.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use rayon::prelude::*;

use crate::result::{Dir, DirId, ScanResult};
use crate::walk::{self, Node, Shared};
use crate::{check_root, ScanError, ScanOptions, ScanProgress};

/// What changed since the earlier scan, as absolute paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Folders whose own entries changed: something directly in them was
    /// added, removed, renamed or written to. Each is listed again; its
    /// sub-folders are reused unless they are named too.
    pub folders: Vec<PathBuf>,
    /// Folders whose whole contents must be read again.
    pub subtrees: Vec<PathBuf>,
}

/// Why a rescan did not give a result.
#[derive(Debug, thiserror::Error)]
pub enum RescanError {
    /// The earlier scan cannot safely be reused; do a full scan instead.
    #[error("a full scan is needed: {0}")]
    NeedsFullScan(&'static str),
    /// The scan could not run, as for a full scan.
    #[error(transparent)]
    Scan(#[from] ScanError),
}

/// How a folder of the earlier scan is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// Reused as it was.
    Clean,
    /// Listed again; its sub-folders are visited in turn.
    Relist,
    /// Read again in full, like a new folder.
    Subtree,
}

/// Why a visit stopped. Turned into [`RescanError::NeedsFullScan`].
type Bail = &'static str;

/// What the visits share: the earlier scan and how each folder is treated.
struct Plan<'a> {
    previous: &'a ScanResult,
    marks: Vec<Mark>,
    /// Some folder below this one is not [`Mark::Clean`].
    dirty_below: Vec<bool>,
    /// This folder or one below it held a hard-linked entry.
    links_below: Vec<bool>,
}

/// Rescans `previous.root`, reading again only what `changes` names (plus
/// the folders the earlier scan could not read), and reusing `previous`
/// for the rest. See the module notes for when it refuses.
///
/// `progress` is called as for [`crate::scan`]; reused folders count
/// straight away.
pub fn rescan(
    previous: &ScanResult,
    changes: &Changes,
    opts: ScanOptions,
    progress: impl Fn(ScanProgress),
) -> Result<ScanResult, RescanError> {
    if opts.largest_files > 0 {
        return Err(RescanError::NeedsFullScan(
            "the largest files list needs a full scan",
        ));
    }
    let root = previous.root.as_path();
    let root_meta = check_root(root)?;
    if root_meta.dev() != previous.root_dev {
        return Err(RescanError::NeedsFullScan(
            "the folder is on another volume now",
        ));
    }
    if previous.dirs.is_empty() {
        return Err(RescanError::NeedsFullScan("the earlier scan is empty"));
    }
    let plan = Plan::new(previous, changes)?;
    let pool = walk::build_pool(opts.threads)?;
    let shared = Shared::new(
        root,
        opts.record_files.as_ref(),
        root_meta.dev(),
        &opts.cancel,
        0,
    );
    let (tx, rx) = mpsc::channel();
    let outcome = pool.in_place_scope(|scope| {
        let (shared, plan) = (&shared, &plan);
        let root_meta = &root_meta;
        scope.spawn(move |_| {
            let node = visit(0, root, Some(root_meta), plan, shared);
            // The receiver only goes away if this thread panicked first.
            let _ = tx.send(node);
        });
        walk::wait_with_progress(&rx, shared, &opts, &progress)
    });
    let root_node = outcome.ok_or(ScanError::Internal)?;
    if opts.cancel.load(Ordering::Relaxed) {
        return Err(ScanError::Cancelled.into());
    }
    let root_node = root_node.map_err(RescanError::NeedsFullScan)?;
    if any_unsettled_links(&root_node) {
        return Err(RescanError::NeedsFullScan(
            "a folder read again holds hard-linked files",
        ));
    }
    progress(shared.snapshot(true));
    Ok(shared.into_rescan_result(root, root_node, previous.hard_links_skipped))
}

impl<'a> Plan<'a> {
    /// Marks each folder of `previous` from `changes`.
    fn new(previous: &'a ScanResult, changes: &Changes) -> Result<Self, RescanError> {
        let n = previous.dirs.len();
        let mut plan = Self {
            previous,
            marks: vec![Mark::Clean; n],
            dirty_below: vec![false; n],
            links_below: links_below(&previous.dirs),
        };
        for path in &changes.folders {
            plan.mark_path(path, Mark::Relist)?;
        }
        for path in &changes.subtrees {
            plan.mark_path(path, Mark::Subtree)?;
        }
        // Folders that could not be read: perhaps they can be now.
        for id in 0..n {
            if previous.dirs[id].unreadable {
                plan.mark(id, Mark::Relist);
            }
        }
        // A skipped mount point may have gone, leaving a plain folder.
        for mount in &previous.other_volumes {
            let parent = mount.parent().unwrap_or(mount);
            plan.mark_path(parent, Mark::Relist)?;
        }
        Ok(plan)
    }

    /// Marks the folder at `path`. A path the earlier scan does not know
    /// marks its deepest known folder for listing again, which finds it.
    fn mark_path(&mut self, path: &Path, mark: Mark) -> Result<(), RescanError> {
        let rel = path
            .strip_prefix(&self.previous.root)
            .map_err(|_| RescanError::NeedsFullScan("a change is outside the scanned folder"))?;
        let mut id: DirId = 0;
        let mut exact = true;
        for part in rel.components() {
            let Component::Normal(name) = part else {
                return Err(RescanError::NeedsFullScan("a changed path is not plain"));
            };
            let child = self.previous.dirs[id]
                .children
                .iter()
                .copied()
                .find(|&c| self.previous.dirs.get(c).is_some_and(|d| d.name == name));
            match child {
                Some(c) => id = c,
                None => {
                    exact = false;
                    break;
                }
            }
        }
        self.mark(id, if exact { mark } else { Mark::Relist });
        Ok(())
    }

    /// Marks folder `id`, never less than it already is, and notes the
    /// folders above it.
    fn mark(&mut self, id: DirId, mark: Mark) {
        let current = self.marks[id];
        self.marks[id] = match (current, mark) {
            (Mark::Subtree, _) | (_, Mark::Subtree) => Mark::Subtree,
            (Mark::Relist, _) | (_, Mark::Relist) => Mark::Relist,
            _ => Mark::Clean,
        };
        let mut parent = self.previous.dirs[id].parent;
        while let Some(p) = parent {
            if self.dirty_below[p] {
                break;
            }
            self.dirty_below[p] = true;
            parent = self.previous.dirs[p].parent;
        }
    }
}

/// For each folder, whether it or a folder below it held a hard-linked
/// entry. Children always come after their parent, so one backwards pass
/// is enough.
fn links_below(dirs: &[Dir]) -> Vec<bool> {
    let mut below: Vec<bool> = dirs.iter().map(|d| d.has_hard_links).collect();
    for id in (1..dirs.len()).rev() {
        if below[id] {
            if let Some(p) = dirs[id].parent {
                below[p] = true;
            }
        }
    }
    below
}

/// Builds the record for earlier folder `id` at `path`. `meta` is its fresh
/// `lstat` if the caller has one.
fn visit(
    id: DirId,
    path: &Path,
    meta: Option<&Metadata>,
    plan: &Plan<'_>,
    shared: &Shared<'_>,
) -> Result<Node, Bail> {
    if shared.cancelled() {
        return Ok(reused(id, plan, shared));
    }
    let dir = &plan.previous.dirs[id];
    let mut mark = plan.marks[id];
    // A folder nothing named, whose parent was not listed again, is not
    // looked at: the change list says it is as it was.
    let fresh = match (mark, meta) {
        (Mark::Clean, None) => None,
        (_, meta) => Some(fresh_meta(path, meta, shared)?),
    };
    if let Some(m) = &fresh {
        if m.ino() != dir.ino {
            mark = Mark::Subtree;
        } else if mark == Mark::Clean && m.modified().ok() != dir.modified {
            mark = Mark::Relist;
        }
    }
    match (mark, fresh) {
        (Mark::Clean, _) if !plan.dirty_below[id] => Ok(reused(id, plan, shared)),
        (Mark::Clean, _) => visit_children_only(id, path, plan, shared),
        (Mark::Relist, Some(m)) => relist(id, path, &m, plan, shared),
        (Mark::Subtree, Some(m)) => {
            if plan.links_below[id] {
                return Err("a folder read again held hard-linked files");
            }
            Ok(walk::walk_dir(path, name_of(id, plan), &m, shared))
        }
        // Not reached: every mark but `Clean` reads fresh metadata above.
        (_, None) => Err("a folder could not be checked"),
    }
}

/// A folder that did not change, but something below it did: its own
/// figures are reused and its sub-folders visited.
fn visit_children_only(
    id: DirId,
    path: &Path,
    plan: &Plan<'_>,
    shared: &Shared<'_>,
) -> Result<Node, Bail> {
    let mut node = own_figures(id, plan, shared);
    let dir = &plan.previous.dirs[id];
    node.children = dir
        .children
        .par_iter()
        .map(|&c| {
            let child_path = path.join(&plan.previous.dirs[c].name);
            visit(c, &child_path, None, plan, shared)
        })
        .collect::<Result<Vec<Node>, Bail>>()?;
    Ok(node)
}

/// A folder whose entries changed: listed again. Sub-folders the earlier
/// scan knew are visited (and mostly reused); new ones are read in full.
fn relist(
    id: DirId,
    path: &Path,
    meta: &Metadata,
    plan: &Plan<'_>,
    shared: &Shared<'_>,
) -> Result<Node, Bail> {
    let dir = &plan.previous.dirs[id];
    if dir.has_hard_links {
        return Err("a folder read again held hard-linked files");
    }
    let mut node = Node::empty(name_of(id, plan), meta);
    shared.dirs.fetch_add(1, Ordering::Relaxed);
    shared.add_bytes(node.own_bytes);
    let subdirs = walk::list_or_mark_unreadable(path, &mut node, shared).unwrap_or_default();
    let known: HashMap<&OsString, DirId> = dir
        .children
        .iter()
        .map(|&c| (&plan.previous.dirs[c].name, c))
        .collect();
    // A sub-folder that went away takes its hard-linked files with it,
    // which can change where their other names are counted.
    let still_here: std::collections::HashSet<&OsString> =
        subdirs.iter().map(|s| &s.name).collect();
    for (name, &c) in &known {
        if plan.links_below[c] && !still_here.contains(name) {
            return Err("a folder that went away held hard-linked files");
        }
    }
    node.children = subdirs
        .into_par_iter()
        .map(|sub| match known.get(&sub.name) {
            Some(&c) => visit(c, &sub.path, Some(&sub.meta), plan, shared),
            None => Ok(walk::walk_dir(&sub.path, sub.name, &sub.meta, shared)),
        })
        .collect::<Result<Vec<Node>, Bail>>()?;
    Ok(node)
}

/// The fresh `lstat` of a folder, checked to still be a plain folder on the
/// scanned volume.
fn fresh_meta(path: &Path, meta: Option<&Metadata>, shared: &Shared<'_>) -> Result<Metadata, Bail> {
    let meta = match meta {
        Some(m) => m.clone(),
        None => std::fs::symlink_metadata(path)
            .map_err(|_| "a folder went away although nothing said so")?,
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("a folder is no longer a folder");
    }
    if meta.dev() != shared.dev {
        return Err("a folder is on another volume now");
    }
    Ok(meta)
}

fn name_of(id: DirId, plan: &Plan<'_>) -> OsString {
    plan.previous.dirs[id].name.clone()
}

/// The earlier record of folder `id` alone, without its sub-folders, with
/// its own figures counted into progress.
fn own_figures(id: DirId, plan: &Plan<'_>, shared: &Shared<'_>) -> Node {
    let dirs = &plan.previous.dirs;
    let dir = &dirs[id];
    let (child_logical, child_files) = dir.children.iter().fold((0u64, 0u64), |(l, f), &c| {
        (
            l.saturating_add(dirs[c].total_logical_bytes),
            f.saturating_add(dirs[c].total_files),
        )
    });
    let own_files = dir.total_files.saturating_sub(child_files);
    shared.dirs.fetch_add(1, Ordering::Relaxed);
    shared.files.fetch_add(own_files, Ordering::Relaxed);
    shared.add_bytes(dir.own_bytes);
    Node {
        name: dir.name.clone(),
        modified: dir.modified,
        ino: dir.ino,
        own_bytes: dir.own_bytes,
        own_logical: dir.total_logical_bytes.saturating_sub(child_logical),
        own_files,
        own_symlinks: dir.own_symlinks,
        unreadable: dir.unreadable,
        has_git: dir.has_git,
        has_hard_links: dir.has_hard_links,
        files: dir.files.clone(),
        links: Vec::new(),
        children: Vec::new(),
    }
}

/// The earlier record of folder `id` and everything below it, unchanged.
/// Iterative, so a deep tree cannot overflow the stack.
fn reused(id: DirId, plan: &Plan<'_>, shared: &Shared<'_>) -> Node {
    // Build each node, then attach children deepest first.
    let dirs = &plan.previous.dirs;
    let mut order: Vec<DirId> = Vec::new();
    let mut stack = vec![id];
    while let Some(d) = stack.pop() {
        order.push(d);
        stack.extend(dirs[d].children.iter().copied());
    }
    let mut built: HashMap<DirId, Node> = HashMap::with_capacity(order.len());
    for &d in order.iter().rev() {
        let mut node = own_figures(d, plan, shared);
        node.children = dirs[d]
            .children
            .iter()
            .filter_map(|c| built.remove(c))
            .collect();
        built.insert(d, node);
    }
    built
        .remove(&id)
        .unwrap_or_else(|| own_figures(id, plan, shared))
}

/// Whether any folder that was read again found a hard-linked entry.
fn any_unsettled_links(root: &Node) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !node.links.is_empty() {
            return true;
        }
        stack.extend(node.children.iter());
    }
    false
}
