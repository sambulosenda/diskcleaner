//! The git check: skips an item inside a git project when removing it could
//! lose work that isn't saved in git.
//!
//! The safety crate already refuses a git project itself and everything in
//! it except named build-output folders (`node_modules`, `target` and so on)
//! that sit right inside the project. This check is for those folders. For
//! each folder above the item that holds `.git`, the item is skipped when:
//!
//! - the project is partway through a merge, rebase, cherry-pick or revert,
//!   or its index has unmerged files ([`GitConcern::Unfinished`]);
//! - git tracks a file inside the item, so removing it would delete
//!   committed or staged work ([`GitConcern::Tracked`]);
//! - the project does not ignore the item, so anything in it would show up
//!   as untracked work ([`GitConcern::NotIgnored`]);
//! - any of this can't be read for sure ([`GitConcern::Unchecked`]).
//!
//! Read only, and git itself is never run: this reads `.git/index` and the
//! ignore files directly. Ignore files that are symlinks are not followed
//! and count as having no patterns, which can only make the check refuse
//! more. The user's global ignore file is read from `~/.config/git/ignore`
//! and from `core.excludesFile` in `~/.gitconfig`.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;

/// The largest ignore or `.git` pointer file read. Bigger ones count as
/// unreadable.
const MAX_SMALL_FILE: u64 = 4 * 1024 * 1024;
/// The largest index read. Bigger ones count as unreadable.
const MAX_INDEX: u64 = 1024 * 1024 * 1024;
/// Files and folders in the git folder that mean an operation is underway.
const IN_PROGRESS: &[&str] = &[
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "rebase-merge",
    "rebase-apply",
];

/// Why an item inside a git project is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitConcern {
    /// A merge, rebase, cherry-pick or revert is not finished.
    Unfinished,
    /// Git tracks files inside the item.
    Tracked,
    /// The project does not ignore the item.
    NotIgnored,
    /// The project's git data could not be read for sure.
    Unchecked,
}

/// Checks `item` (inside `home`) against every git project above it.
pub fn check(home: &Path, item: &Path, is_dir: bool) -> Result<(), GitConcern> {
    let Ok(rel) = item.strip_prefix(home) else {
        return Err(GitConcern::Unchecked);
    };
    let parts: Vec<&OsStr> = rel.iter().collect();
    // Every folder from home down to the item's parent.
    let mut folder = home.to_path_buf();
    for i in 0..parts.len() {
        if i > 0 {
            folder.push(parts[i - 1]);
        }
        let dot_git = folder.join(".git");
        match dot_git.symlink_metadata() {
            Ok(_) => check_project(home, &folder, &parts[i..], is_dir)?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(GitConcern::Unchecked),
        }
    }
    Ok(())
}

/// Checks the item at `rel` (relative to the work tree `top`) in that project.
fn check_project(home: &Path, top: &Path, rel: &[&OsStr], is_dir: bool) -> Result<(), GitConcern> {
    let (git_dir, common_dir) = git_dirs(top)?;
    for name in IN_PROGRESS {
        match git_dir.join(name).symlink_metadata() {
            Ok(_) => return Err(GitConcern::Unfinished),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(GitConcern::Unchecked),
        }
    }
    let hash_len = hash_len(&common_dir)?;
    let rel_bytes = join_rel(rel);
    if let Some(index) = read_capped(&git_dir.join("index"), MAX_INDEX)? {
        index_concern(&index, hash_len, &rel_bytes)?;
    }
    if !is_ignored(home, top, &common_dir, rel, is_dir)? {
        return Err(GitConcern::NotIgnored);
    }
    Ok(())
}

/// The git folder of the work tree `top`, and its common folder (where
/// `config` and `info/exclude` live). They differ for a linked worktree.
fn git_dirs(top: &Path) -> Result<(PathBuf, PathBuf), GitConcern> {
    let dot_git = top.join(".git");
    let meta = dot_git
        .symlink_metadata()
        .map_err(|_| GitConcern::Unchecked)?;
    let git_dir = if meta.is_dir() {
        dot_git
    } else if meta.is_file() {
        let text = read_capped(&dot_git, MAX_SMALL_FILE)?.ok_or(GitConcern::Unchecked)?;
        let text = std::str::from_utf8(&text).map_err(|_| GitConcern::Unchecked)?;
        let target = text
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("gitdir:"))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or(GitConcern::Unchecked)?;
        top.join(target)
    } else {
        return Err(GitConcern::Unchecked);
    };
    let real = git_dir
        .symlink_metadata()
        .map_err(|_| GitConcern::Unchecked)?;
    if !real.is_dir() {
        return Err(GitConcern::Unchecked);
    }
    let common_dir = match read_capped(&git_dir.join("commondir"), MAX_SMALL_FILE)? {
        None => git_dir.clone(),
        Some(text) => {
            let text = std::str::from_utf8(&text).map_err(|_| GitConcern::Unchecked)?;
            let common = text.trim();
            if common.is_empty() {
                return Err(GitConcern::Unchecked);
            }
            git_dir.join(common)
        }
    };
    Ok((git_dir, common_dir))
}

/// The object id length: 32 bytes for SHA-256 repositories, 20 otherwise.
fn hash_len(common_dir: &Path) -> Result<usize, GitConcern> {
    let Some(config) = read_capped(&common_dir.join("config"), MAX_SMALL_FILE)? else {
        return Ok(20);
    };
    let text = String::from_utf8_lossy(&config).to_ascii_lowercase();
    let sha256 = text.lines().any(|line| {
        let line: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        line.starts_with("objectformat=sha256")
    });
    Ok(if sha256 { 32 } else { 20 })
}

/// `rel` as git writes paths: names joined by `/`.
fn join_rel(rel: &[&OsStr]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, part) in rel.iter().enumerate() {
        if i > 0 {
            out.push(b'/');
        }
        out.extend_from_slice(part.as_bytes());
    }
    out
}

/// Reads the index and refuses if it has unmerged entries, or an entry
/// inside `rel` (or a sparse folder entry holding `rel`).
///
/// Names are compared ignoring ASCII case, as on a Mac disk.
fn index_concern(index: &[u8], hash_len: usize, rel: &[u8]) -> Result<(), GitConcern> {
    let rel = rel.to_ascii_lowercase();
    for entry in parse_index(index, hash_len)? {
        if entry.stage != 0 {
            return Err(GitConcern::Unfinished);
        }
        let path = entry.path.to_ascii_lowercase();
        if is_within(&path, &rel) || (path.ends_with(b"/") && rel.starts_with(&path)) {
            return Err(GitConcern::Tracked);
        }
    }
    Ok(())
}

/// Whether `path` is `dir` itself or inside it.
fn is_within(path: &[u8], dir: &[u8]) -> bool {
    path == dir || (path.starts_with(dir) && path.get(dir.len()) == Some(&b'/'))
}

/// One index entry: its path and merge stage.
#[derive(Debug, PartialEq, Eq)]
struct IndexEntry {
    path: Vec<u8>,
    stage: u16,
}

/// Parses a git index (versions 2, 3 and 4). Anything unexpected, including
/// a split index whose entries live in another file, is an error.
fn parse_index(data: &[u8], hash_len: usize) -> Result<Vec<IndexEntry>, GitConcern> {
    let bad = || GitConcern::Unchecked;
    if data.get(0..4) != Some(b"DIRC") {
        return Err(bad());
    }
    let version = be32(data, 4).ok_or_else(bad)?;
    if !(2..=4).contains(&version) {
        return Err(bad());
    }
    let count = be32(data, 8).ok_or_else(bad)?;
    let mut at = 12usize;
    let mut entries = Vec::new();
    let mut previous: Vec<u8> = Vec::new();
    // 40 bytes of stat data, then the object id, then 2 bytes of flags.
    let fixed = 40 + hash_len + 2;
    for _ in 0..count {
        let flags_at = at.checked_add(40 + hash_len).ok_or_else(bad)?;
        let flags = be16(data, flags_at).ok_or_else(bad)?;
        let stage = (flags >> 12) & 0x3;
        let extended = flags & 0x4000 != 0;
        if extended && version < 3 {
            return Err(bad());
        }
        let mut path_at = at + fixed + if extended { 2 } else { 0 };
        if version == 4 {
            let (strip, used) = varint(data, path_at).ok_or_else(bad)?;
            path_at += used;
            let suffix = nul_terminated(data, path_at).ok_or_else(bad)?;
            let keep = previous.len().checked_sub(strip).ok_or_else(bad)?;
            let mut path = previous[..keep].to_vec();
            path.extend_from_slice(suffix);
            at = path_at + suffix.len() + 1;
            previous = path.clone();
            entries.push(IndexEntry { path, stage });
        } else {
            let path = nul_terminated(data, path_at).ok_or_else(bad)?.to_vec();
            let used = (path_at - at) + path.len();
            // Entries are padded with 1 to 8 NULs to a multiple of 8 bytes.
            at += (used + 8) & !7;
            entries.push(IndexEntry { path, stage });
        }
    }
    // Extensions follow, then the checksum. A split index ("link") keeps
    // entries in a shared file, so the entries above may be incomplete.
    let end = data.len().checked_sub(hash_len).ok_or_else(bad)?;
    while at + 8 <= end {
        let signature = &data[at..at + 4];
        let size = usize::try_from(be32(data, at + 4).ok_or_else(bad)?).map_err(|_| bad())?;
        if signature == b"link" {
            return Err(bad());
        }
        at = at.checked_add(8 + size).ok_or_else(bad)?;
    }
    if at != end {
        return Err(bad());
    }
    Ok(entries)
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

/// The bytes from `at` up to (not including) the next NUL.
fn nul_terminated(data: &[u8], at: usize) -> Option<&[u8]> {
    let rest = data.get(at..)?;
    let len = rest.iter().position(|b| *b == 0)?;
    Some(&rest[..len])
}

/// Git's offset varint (index v4 path prefix length). Returns the value and
/// the number of bytes used.
fn varint(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let mut used = 0;
    let mut byte = *data.get(at)?;
    used += 1;
    let mut value = usize::from(byte & 0x7f);
    while byte & 0x80 != 0 {
        if used >= 9 {
            return None;
        }
        byte = *data.get(at + used)?;
        used += 1;
        value = value.checked_add(1)?.checked_shl(7)? | usize::from(byte & 0x7f);
    }
    Some((value, used))
}

/// Whether git ignores the item at `rel` inside the work tree `top`.
///
/// Git never looks inside an ignored folder, so the item counts as ignored
/// if it, or any folder between `top` and it, is ignored. Each one is
/// checked against the ignore files git would use, closest first: the
/// `.gitignore` files from its own folder up to `top`, then
/// `info/exclude`, then the user's global ignore file.
fn is_ignored(
    home: &Path,
    top: &Path,
    common_dir: &Path,
    rel: &[&OsStr],
    is_dir: bool,
) -> Result<bool, GitConcern> {
    let shared = shared_ignores(home, top, common_dir)?;
    let mut per_folder: Vec<Option<Gitignore>> = Vec::with_capacity(rel.len());
    let mut folder = top.to_path_buf();
    for (i, part) in rel.iter().enumerate() {
        per_folder.push(ignore_file(&folder, &folder.join(".gitignore"))?);
        folder.push(part);
        let path = folder.as_path();
        let last = i + 1 == rel.len();
        let this_is_dir = !last || is_dir;
        let mut decided = None;
        for matcher in per_folder.iter().rev().flatten() {
            match matcher.matched(path, this_is_dir) {
                Match::Ignore(_) => decided = Some(true),
                Match::Whitelist(_) => decided = Some(false),
                Match::None => continue,
            }
            break;
        }
        if decided.is_none() {
            for matcher in shared.iter().flatten() {
                match matcher.matched(path, this_is_dir) {
                    Match::Ignore(_) => decided = Some(true),
                    Match::Whitelist(_) => decided = Some(false),
                    Match::None => continue,
                }
                break;
            }
        }
        if decided == Some(true) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `info/exclude`, then the global ignore files, as matchers rooted at `top`.
fn shared_ignores(
    home: &Path,
    top: &Path,
    common_dir: &Path,
) -> Result<Vec<Option<Gitignore>>, GitConcern> {
    let mut out = vec![ignore_file(top, &common_dir.join("info").join("exclude"))?];
    if let Some(path) = configured_excludes_file(home)? {
        out.push(ignore_file(top, &path)?);
    }
    out.push(ignore_file(
        top,
        &home.join(".config").join("git").join("ignore"),
    )?);
    Ok(out)
}

/// `core.excludesFile` from `~/.gitconfig`, if set. Only `~/` and absolute
/// paths are understood; anything else is left out.
fn configured_excludes_file(home: &Path) -> Result<Option<PathBuf>, GitConcern> {
    let Some(config) = read_capped(&home.join(".gitconfig"), MAX_SMALL_FILE)? else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&config);
    let mut in_core = false;
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_core = line
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
                .eq_ignore_ascii_case("core");
            continue;
        }
        if !in_core {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("excludesfile") {
            continue;
        }
        let value = value.trim().trim_matches('"');
        found = if let Some(rest) = value.strip_prefix("~/") {
            Some(home.join(rest))
        } else if value.starts_with('/') {
            Some(PathBuf::from(value))
        } else {
            None
        };
    }
    Ok(found)
}

/// A matcher for one ignore file, with patterns relative to `root`. `None`
/// if the file doesn't exist, is a symlink, or has no patterns.
fn ignore_file(root: &Path, file: &Path) -> Result<Option<Gitignore>, GitConcern> {
    let Some(data) = read_capped(file, MAX_SMALL_FILE)? else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&data);
    let mut builder = GitignoreBuilder::new(root);
    for line in text.lines() {
        // Git skips a pattern it can't read; so do we. Fewer patterns can
        // only make the check refuse more.
        let _ = builder.add_line(None, line);
    }
    let matcher = builder.build().map_err(|_| GitConcern::Unchecked)?;
    Ok((!matcher.is_empty()).then_some(matcher))
}

/// Reads a whole file without following a symlink. `None` if it doesn't
/// exist or is a symlink; an error if it can't be read or is too big.
fn read_capped(path: &Path, cap: u64) -> Result<Option<Vec<u8>>, GitConcern> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path);
    let mut file: File = match file {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) if err.raw_os_error() == Some(libc::ELOOP) => return Ok(None),
        Err(_) => return Err(GitConcern::Unchecked),
    };
    let meta = file.metadata().map_err(|_| GitConcern::Unchecked)?;
    if !meta.is_file() || meta.len() > cap {
        return Err(GitConcern::Unchecked);
    }
    let mut data = Vec::new();
    file.by_ref()
        .take(cap + 1)
        .read_to_end(&mut data)
        .map_err(|_| GitConcern::Unchecked)?;
    if data.len() as u64 > cap {
        return Err(GitConcern::Unchecked);
    }
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a v2 index with the given (path, stage) entries.
    pub(crate) fn index_v2(entries: &[(&str, u16)]) -> Vec<u8> {
        let mut out = b"DIRC".to_vec();
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for (path, stage) in entries {
            let start = out.len();
            out.extend_from_slice(&[0u8; 40]);
            out.extend_from_slice(&[0xab; 20]);
            let flags = (stage << 12) | (path.len().min(0xfff) as u16);
            out.extend_from_slice(&flags.to_be_bytes());
            out.extend_from_slice(path.as_bytes());
            let used = out.len() - start;
            let padded = (used + 8) & !7;
            out.resize(start + padded, 0);
        }
        out.extend_from_slice(&[0u8; 20]);
        out
    }

    /// Builds a v4 index (prefix-compressed paths).
    fn index_v4(paths: &[&str]) -> Vec<u8> {
        let mut out = b"DIRC".to_vec();
        out.extend_from_slice(&4u32.to_be_bytes());
        out.extend_from_slice(&(paths.len() as u32).to_be_bytes());
        let mut previous = "";
        for path in paths {
            out.extend_from_slice(&[0u8; 40]);
            out.extend_from_slice(&[0xab; 20]);
            out.extend_from_slice(&(path.len() as u16).to_be_bytes());
            let common = previous
                .bytes()
                .zip(path.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            let strip = previous.len() - common;
            assert!(strip < 128, "test helper only writes one-byte varints");
            out.push(strip as u8);
            out.extend_from_slice(&path.as_bytes()[common..]);
            out.push(0);
            previous = path;
        }
        out.extend_from_slice(&[0u8; 20]);
        out
    }

    #[test]
    fn parses_v2_and_v4_indexes() {
        let v2 = parse_index(&index_v2(&[("a.txt", 0), ("src/main.rs", 0)]), 20).unwrap();
        assert_eq!(v2[1].path, b"src/main.rs");
        let v4 = parse_index(&index_v4(&["src/lib.rs", "src/main.rs", "target/x"]), 20).unwrap();
        let paths: Vec<_> = v4.iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                b"src/lib.rs".to_vec(),
                b"src/main.rs".to_vec(),
                b"target/x".to_vec()
            ]
        );
    }

    #[test]
    fn broken_indexes_are_unchecked() {
        let good = index_v2(&[("a", 0)]);
        assert_eq!(parse_index(b"NOPE", 20), Err(GitConcern::Unchecked));
        assert_eq!(
            parse_index(&good[..good.len() - 30], 20),
            Err(GitConcern::Unchecked)
        );
        let mut split = good.clone();
        let tail = split.split_off(split.len() - 20);
        split.extend_from_slice(b"link");
        split.extend_from_slice(&0u32.to_be_bytes());
        split.extend_from_slice(&tail);
        assert_eq!(parse_index(&split, 20), Err(GitConcern::Unchecked));
    }

    #[test]
    fn tracked_and_unmerged_entries_are_found() {
        let index = index_v2(&[("src/main.rs", 0), ("target/keep.txt", 0)]);
        assert_eq!(
            index_concern(&index, 20, b"target"),
            Err(GitConcern::Tracked)
        );
        assert_eq!(
            index_concern(&index, 20, b"Target"),
            Err(GitConcern::Tracked)
        );
        assert_eq!(index_concern(&index, 20, b"node_modules"), Ok(()));
        let near = index_v2(&[("targets.txt", 0)]);
        assert_eq!(index_concern(&near, 20, b"target"), Ok(()));
        let conflict = index_v2(&[("src/main.rs", 2)]);
        assert_eq!(
            index_concern(&conflict, 20, b"target"),
            Err(GitConcern::Unfinished)
        );
        let sparse = index_v2(&[("build/", 0)]);
        assert_eq!(
            index_concern(&sparse, 20, b"build/out"),
            Err(GitConcern::Tracked)
        );
    }

    #[test]
    fn varints_follow_gits_offset_encoding() {
        assert_eq!(varint(&[0x05], 0), Some((5, 1)));
        // 0x80 0x00 is 128 in git's encoding.
        assert_eq!(varint(&[0x80, 0x00], 0), Some((128, 2)));
        assert_eq!(varint(&[0x80], 0), None);
    }
}
