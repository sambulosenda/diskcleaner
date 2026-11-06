//! The fixture manifest: every path in the tree, read back from disk.
//!
//! The manifest is built by walking the finished tree (never following
//! symlinks), so it lists what is really there, not what the layout meant to
//! make. Entries are sorted by path and the JSON has no timestamps, so the
//! same tree always gives the same manifest.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::builder::{io_err, Note};
use crate::content::sha256_hex;
use crate::FixtureError;

/// Name of the manifest file written at the fixture root.
pub const MANIFEST_FILE: &str = "fixtures-manifest.json";

/// Bumped if the manifest shape changes.
const FORMAT_VERSION: u64 = 1;

/// What kind of thing a manifest entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A folder (including bundles such as `.photoslibrary`).
    Dir,
    /// A regular file.
    File,
    /// A symlink. Its target is recorded, never followed.
    Symlink,
}

impl EntryKind {
    fn as_str(self) -> &'static str {
        match self {
            EntryKind::Dir => "dir",
            EntryKind::File => "file",
            EntryKind::Symlink => "symlink",
        }
    }
}

/// One path in the fixture tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the fixture root, with `/` separators.
    pub path: String,
    /// Folder, file or symlink.
    pub kind: EntryKind,
    /// Size in bytes for files; 0 for folders and symlinks.
    pub size: u64,
    /// SHA-256 of the file bytes, lower-case hex. Files only.
    pub sha256: Option<String>,
    /// Symlink target exactly as stored. Symlinks only.
    pub target: Option<String>,
    /// For the second path of a hard link pair, the first path.
    pub hard_link_of: Option<String>,
    /// The bytes include the absolute fixture root path.
    pub contains_home_path: bool,
}

/// Every path the fixture created, sorted by path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The entries, sorted by `path`.
    pub entries: Vec<Entry>,
}

impl Manifest {
    /// Looks up an entry by its relative path.
    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|e| e.path.as_str().cmp(path))
            .ok()
            .and_then(|i| self.entries.get(i))
    }

    /// Sum of file sizes, counting each hard link pair once.
    pub fn total_file_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::File && e.hard_link_of.is_none())
            .map(|e| e.size)
            .sum()
    }

    /// The manifest as pretty JSON with a trailing newline.
    pub fn to_json(&self) -> String {
        let entries: Vec<Value> = self.entries.iter().map(entry_json).collect();
        let doc = json!({
            "format": FORMAT_VERSION,
            "entry_count": self.entries.len(),
            "total_file_bytes": self.total_file_bytes(),
            "entries": entries,
        });
        let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
        text.push('\n');
        text
    }
}

/// One entry as JSON. Optional fields are left out when empty.
fn entry_json(e: &Entry) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("path".into(), json!(e.path));
    map.insert("kind".into(), json!(e.kind.as_str()));
    map.insert("size".into(), json!(e.size));
    if let Some(sha) = &e.sha256 {
        map.insert("sha256".into(), json!(sha));
    }
    if let Some(target) = &e.target {
        map.insert("target".into(), json!(target));
    }
    if let Some(of) = &e.hard_link_of {
        map.insert("hard_link_of".into(), json!(of));
    }
    if e.contains_home_path {
        map.insert("contains_home_path".into(), json!(true));
    }
    Value::Object(map)
}

/// Walks `root` and builds the manifest, adding the builder's notes.
pub fn collect(root: &Path, notes: &BTreeMap<String, Note>) -> Result<Manifest, FixtureError> {
    let mut entries = Vec::new();
    walk(root, "", notes, &mut entries)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Manifest { entries })
}

/// Adds every child of `root/rel` to `out`, recursing into real folders only.
fn walk(
    root: &Path,
    rel: &str,
    notes: &BTreeMap<String, Note>,
    out: &mut Vec<Entry>,
) -> Result<(), FixtureError> {
    let dir = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    let mut names = Vec::new();
    for item in fs::read_dir(&dir).map_err(|e| io_err(&dir, e))? {
        let item = item.map_err(|e| io_err(&dir, e))?;
        names.push(item.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    for name in names {
        let child_rel = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        if child_rel == MANIFEST_FILE {
            continue;
        }
        let path = root.join(&child_rel);
        let meta = fs::symlink_metadata(&path).map_err(|e| io_err(&path, e))?;
        let note = notes.get(&child_rel).cloned().unwrap_or_default();
        let file_type = meta.file_type();
        let mut entry = Entry {
            path: child_rel.clone(),
            kind: EntryKind::Dir,
            size: 0,
            sha256: None,
            target: None,
            hard_link_of: note.hard_link_of,
            contains_home_path: note.contains_home_path,
        };
        if file_type.is_symlink() {
            let target = fs::read_link(&path).map_err(|e| io_err(&path, e))?;
            entry.kind = EntryKind::Symlink;
            entry.target = Some(target.to_string_lossy().into_owned());
            out.push(entry);
        } else if file_type.is_dir() {
            out.push(entry);
            walk(root, &child_rel, notes, out)?;
        } else {
            let data = fs::read(&path).map_err(|e| io_err(&path, e))?;
            entry.kind = EntryKind::File;
            entry.size = meta.len();
            entry.sha256 = Some(sha256_hex(&data));
            out.push(entry);
        }
    }
    Ok(())
}
