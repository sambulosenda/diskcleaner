//! How much space the Mac's own media and mail libraries take: Photos,
//! Music, TV, Mail, Messages attachments and Final Cut Pro libraries.
//! PRD E6, E7 and C1.
//!
//! Pure: works only on a finished [`ScanResult`], so nothing here opens,
//! reads or changes a file. The app shows these sizes and points the user to
//! each library's own app. Roomy never cleans inside a library: every
//! library here is hard-blocked or review-only in
//! `docs/safety/protected-paths.md`, and nothing in this module produces a
//! cleanup item or a path for the app.

use std::collections::HashSet;
use std::path::Path;

use scanner::{DirId, ScanResult};

/// Which library a row is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LibraryKind {
    /// Every `*.photoslibrary` bundle in the home folder.
    Photos,
    /// Every `*.musiclibrary` bundle, plus the Music app's media folder and
    /// the older iTunes folder.
    Music,
    /// Every `*.tvlibrary` bundle, plus the TV app's media folder.
    Tv,
    /// Mail's mailboxes and the attachments Mail has downloaded.
    Mail,
    /// The photos, videos and files sent or received in Messages.
    MessagesAttachments,
    /// Every Final Cut Pro library (`*.fcpbundle`).
    FinalCut,
}

impl LibraryKind {
    /// Every kind, in the order the app lists them.
    pub const ALL: [LibraryKind; 6] = [
        LibraryKind::Photos,
        LibraryKind::Music,
        LibraryKind::Tv,
        LibraryKind::Mail,
        LibraryKind::MessagesAttachments,
        LibraryKind::FinalCut,
    ];

    /// Bundle extensions (without the dot) that are libraries of this kind,
    /// wherever they are in the home folder.
    fn bundle_extensions(self) -> &'static [&'static str] {
        match self {
            LibraryKind::Photos => &["photoslibrary"],
            LibraryKind::Music => &["musiclibrary"],
            LibraryKind::Tv => &["tvlibrary"],
            LibraryKind::FinalCut => &["fcpbundle"],
            LibraryKind::Mail | LibraryKind::MessagesAttachments => &[],
        }
    }

    /// Fixed folders, relative to home, that belong to this kind.
    fn folders(self) -> &'static [&'static [&'static str]] {
        match self {
            LibraryKind::Photos | LibraryKind::FinalCut => &[],
            LibraryKind::Music => &[
                &["Music", "Music", "Media"],
                &["Music", "Music", "Media.localized"],
                &["Music", "iTunes"],
            ],
            LibraryKind::Tv => &[
                &["Movies", "TV", "Media"],
                &["Movies", "TV", "Media.localized"],
            ],
            LibraryKind::Mail => &[
                &["Library", "Mail"],
                &["Library", "Containers", "com.apple.mail"],
            ],
            LibraryKind::MessagesAttachments => &[&["Library", "Messages", "Attachments"]],
        }
    }

    /// Fixed folders whose absence can only mean "not readable", because
    /// macOS keeps their parent private. Used to tell "none" from "Roomy
    /// can't see it".
    fn private_parents(self) -> &'static [&'static [&'static str]] {
        match self {
            LibraryKind::Mail => &[&["Library", "Mail"]],
            LibraryKind::MessagesAttachments => &[&["Library", "Messages"]],
            _ => &[],
        }
    }
}

/// How much one kind of library takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySize {
    /// Which library.
    pub kind: LibraryKind,
    /// Allocated bytes of everything found, counted once.
    pub bytes: u64,
    /// How many library bundles were found (Photos, Music, TV and Final
    /// Cut). 0 for the kinds that are folders, such as Mail.
    pub bundle_count: u32,
    /// Whether anything of this kind was found.
    pub found: bool,
    /// Whether part of it could not be read (usually because Full Disk
    /// Access is off), so the real size may be larger.
    pub needs_access: bool,
}

/// Measures every kind of library in `scan`. One row per kind, in
/// [`LibraryKind::ALL`] order, including kinds with nothing found.
pub fn library_sizes(scan: &ScanResult) -> Vec<LibrarySize> {
    let bundles = find_bundles(scan);
    LibraryKind::ALL
        .iter()
        .map(|&kind| measure(scan, kind, &bundles))
        .collect()
}

/// Measures one kind.
fn measure(scan: &ScanResult, kind: LibraryKind, bundles: &[(LibraryKind, DirId)]) -> LibrarySize {
    let mut roots: Vec<DirId> = bundles
        .iter()
        .filter(|(k, _)| *k == kind)
        .map(|&(_, id)| id)
        .collect();
    let bundle_count = u32::try_from(roots.len()).unwrap_or(u32::MAX);
    let mut needs_access = false;
    for rel in kind.folders() {
        match find_rel(scan, rel) {
            Some(id) => roots.push(id),
            None => needs_access |= hidden_by_unreadable(scan, rel),
        }
    }
    for rel in kind.private_parents() {
        if find_rel(scan, rel).is_none() {
            needs_access |= hidden_by_unreadable(scan, rel);
        }
    }
    let roots = outermost(scan, roots);
    let mut bytes = 0u64;
    for &id in &roots {
        if let Some(dir) = scan.dirs.get(id) {
            bytes = bytes.saturating_add(dir.total_bytes);
        }
        needs_access |= has_unreadable_inside(scan, id);
    }
    LibrarySize {
        kind,
        bytes,
        bundle_count,
        found: !roots.is_empty(),
        needs_access,
    }
}

/// Every folder whose name ends in a library bundle extension, with its
/// kind. A bundle inside another bundle of any kind is left out, so it is
/// never counted twice.
fn find_bundles(scan: &ScanResult) -> Vec<(LibraryKind, DirId)> {
    let mut found: Vec<(LibraryKind, DirId)> = Vec::new();
    let mut taken: HashSet<DirId> = HashSet::new();
    // Parents always come before children in `dirs`, so an outer bundle is
    // seen before anything inside it.
    for (id, dir) in scan.dirs.iter().enumerate() {
        let Some(kind) = bundle_kind(&dir.name.to_string_lossy()) else {
            continue;
        };
        if ancestors(scan, id).any(|a| taken.contains(&a)) {
            continue;
        }
        taken.insert(id);
        found.push((kind, id));
    }
    found
}

/// The kind of library a folder name is, by its extension (any case).
fn bundle_kind(name: &str) -> Option<LibraryKind> {
    let lower = name.to_lowercase();
    LibraryKind::ALL.into_iter().find(|kind| {
        kind.bundle_extensions()
            .iter()
            .any(|ext| lower.len() > ext.len() + 1 && lower.ends_with(&format!(".{ext}")))
    })
}

/// The strict ancestors of folder `id`, nearest first.
fn ancestors(scan: &ScanResult, id: DirId) -> impl Iterator<Item = DirId> + '_ {
    let mut current = scan.dirs.get(id).and_then(|d| d.parent);
    std::iter::from_fn(move || {
        let this = current?;
        current = scan.dirs.get(this).and_then(|d| d.parent);
        Some(this)
    })
}

/// `roots` without duplicates and without any folder inside another one in
/// the list, so each byte is counted once.
fn outermost(scan: &ScanResult, mut roots: Vec<DirId>) -> Vec<DirId> {
    roots.sort_unstable();
    roots.dedup();
    let set: HashSet<DirId> = roots.iter().copied().collect();
    roots
        .into_iter()
        .filter(|&id| !ancestors(scan, id).any(|a| set.contains(&a)))
        .collect()
}

/// The folder at `rel` (relative to home), matching names exactly.
fn find_rel(scan: &ScanResult, rel: &[&str]) -> Option<DirId> {
    let path: std::path::PathBuf = rel.iter().collect();
    scan.find(&path)
}

/// Whether the folder at `rel` is missing from the scan because a folder
/// above it could not be listed.
fn hidden_by_unreadable(scan: &ScanResult, rel: &[&str]) -> bool {
    let mut id = 0;
    for name in rel {
        let Some(dir) = scan.dirs.get(id) else {
            return false;
        };
        if dir.unreadable {
            return true;
        }
        let next = dir.children.iter().copied().find(|&c| {
            scan.dirs
                .get(c)
                .is_some_and(|d| d.name == std::ffi::OsStr::new(name))
        });
        match next {
            Some(c) => id = c,
            None => return false,
        }
    }
    scan.dirs.get(id).is_some_and(|d| d.unreadable)
}

/// Whether folder `id`, or any folder inside it, could not be listed.
fn has_unreadable_inside(scan: &ScanResult, id: DirId) -> bool {
    if scan.dirs.get(id).is_some_and(|d| d.unreadable) {
        return true;
    }
    let Some(path) = scan.path_of(id) else {
        return false;
    };
    scan.unreadable
        .iter()
        .any(|u| Path::new(&u.path).starts_with(&path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Writes a file of `len` bytes, making its folders.
    fn file(root: &Path, rel: &str, len: usize) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; len]).unwrap();
    }

    fn scan(root: &Path) -> ScanResult {
        scanner::scan(root, scanner::ScanOptions::default(), |_| {}).unwrap()
    }

    fn home() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        (dir, root)
    }

    fn row(rows: &[LibrarySize], kind: LibraryKind) -> &LibrarySize {
        rows.iter().find(|r| r.kind == kind).unwrap()
    }

    #[test]
    fn an_empty_home_has_one_empty_row_per_kind() {
        let (_d, root) = home();
        let rows = library_sizes(&scan(&root));
        assert_eq!(
            rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
            LibraryKind::ALL.to_vec()
        );
        for r in &rows {
            assert_eq!(r.bytes, 0);
            assert!(!r.found);
            assert!(!r.needs_access);
            assert_eq!(r.bundle_count, 0);
        }
    }

    #[test]
    fn photos_libraries_are_found_anywhere_and_counted_once() {
        let (_d, root) = home();
        file(
            &root,
            "Pictures/Photos Library.photoslibrary/database/Photos.sqlite",
            40_000,
        );
        file(
            &root,
            "Pictures/Photos Library.photoslibrary/originals/A/1.heic",
            100_000,
        );
        file(&root, "Documents/Old.PhotosLibrary/originals/2.jpg", 50_000);
        // A bundle inside a bundle is not counted again.
        file(
            &root,
            "Documents/Old.PhotosLibrary/inner.photoslibrary/x",
            10_000,
        );
        let s = scan(&root);
        let rows = library_sizes(&s);
        let photos = row(&rows, LibraryKind::Photos);
        assert!(photos.found);
        assert_eq!(photos.bundle_count, 2);
        let a = s.dirs[s
            .find(Path::new("Pictures/Photos Library.photoslibrary"))
            .unwrap()]
        .total_bytes;
        let b = s.dirs[s.find(Path::new("Documents/Old.PhotosLibrary")).unwrap()].total_bytes;
        assert_eq!(photos.bytes, a + b);
        assert!(photos.bytes >= 200_000);
    }

    #[test]
    fn music_counts_the_bundle_and_media_folders_without_overlap() {
        let (_d, root) = home();
        file(
            &root,
            "Music/Music/Music Library.musiclibrary/Library.musicdb",
            20_000,
        );
        file(
            &root,
            "Music/Music/Media.localized/Music/Artist/song.m4a",
            300_000,
        );
        file(&root, "Music/iTunes/iTunes Media/Music/old.mp3", 60_000);
        // A library inside the iTunes folder is inside a counted folder.
        file(&root, "Music/iTunes/Old.musiclibrary/db", 5_000);
        let s = scan(&root);
        let rows = library_sizes(&s);
        let music = row(&rows, LibraryKind::Music);
        let size = |rel: &str| s.dirs[s.find(Path::new(rel)).unwrap()].total_bytes;
        assert_eq!(
            music.bytes,
            size("Music/Music/Music Library.musiclibrary")
                + size("Music/Music/Media.localized")
                + size("Music/iTunes")
        );
        assert_eq!(music.bundle_count, 2);
        assert!(!row(&rows, LibraryKind::Tv).found);
    }

    #[test]
    fn mail_messages_and_final_cut_are_measured() {
        let (_d, root) = home();
        file(&root, "Library/Mail/V10/INBOX.mbox/1.emlx", 30_000);
        file(
            &root,
            "Library/Containers/com.apple.mail/Data/Library/Mail Downloads/a.pdf",
            70_000,
        );
        file(
            &root,
            "Library/Messages/Attachments/ab/01/X/IMG_1.heic",
            90_000,
        );
        file(&root, "Library/Messages/chat.db", 10_000);
        file(
            &root,
            "Movies/Holiday.fcpbundle/Event 1/Render Files/r.mov",
            120_000,
        );
        let s = scan(&root);
        let rows = library_sizes(&s);
        let size = |rel: &str| s.dirs[s.find(Path::new(rel)).unwrap()].total_bytes;
        assert_eq!(
            row(&rows, LibraryKind::Mail).bytes,
            size("Library/Mail") + size("Library/Containers/com.apple.mail")
        );
        // Only the attachments, never the chat database.
        assert_eq!(
            row(&rows, LibraryKind::MessagesAttachments).bytes,
            size("Library/Messages/Attachments")
        );
        let fcp = row(&rows, LibraryKind::FinalCut);
        assert_eq!(fcp.bundle_count, 1);
        assert_eq!(fcp.bytes, size("Movies/Holiday.fcpbundle"));
        assert!(rows.iter().all(|r| !r.needs_access));
    }

    #[test]
    fn a_name_that_is_only_the_extension_is_not_a_library() {
        assert_eq!(bundle_kind(".photoslibrary"), None);
        assert_eq!(bundle_kind("a.photoslibrary"), Some(LibraryKind::Photos));
        assert_eq!(bundle_kind("A.FCPBUNDLE"), Some(LibraryKind::FinalCut));
        assert_eq!(bundle_kind("photoslibrary"), None);
    }

    /// Marks the folder at `rel` as unlistable, the way the scanner leaves
    /// a folder macOS keeps private: no children, flagged, and listed.
    fn make_unreadable(s: &mut ScanResult, rel: &str) {
        let id = s.find(Path::new(rel)).unwrap();
        s.dirs[id].children.clear();
        s.dirs[id].unreadable = true;
        let path = s.path_of(id).unwrap();
        s.unreadable.push(scanner::Unreadable {
            path,
            reason: scanner::UnreadableReason::PermissionDenied,
        });
    }

    #[test]
    fn unreadable_private_folders_are_flagged() {
        let (_d, root) = home();
        file(&root, "Library/Messages/Attachments/ab/1.jpg", 10_000);
        file(&root, "Library/Mail/V10/x.emlx", 10_000);
        file(&root, "Library/Mail/V10/y/z.emlx", 10_000);
        let mut s = scan(&root);
        make_unreadable(&mut s, "Library/Messages");
        make_unreadable(&mut s, "Library/Mail/V10/y");
        let rows = library_sizes(&s);
        let attachments = row(&rows, LibraryKind::MessagesAttachments);
        assert!(!attachments.found);
        assert!(attachments.needs_access);
        let mail_row = row(&rows, LibraryKind::Mail);
        assert!(mail_row.found);
        assert!(mail_row.needs_access);
        assert!(!row(&rows, LibraryKind::Photos).needs_access);
    }

    #[test]
    fn an_unreadable_mail_folder_is_flagged_even_though_it_exists() {
        let (_d, root) = home();
        file(&root, "Library/Mail/V10/x.emlx", 10_000);
        let mut s = scan(&root);
        make_unreadable(&mut s, "Library/Mail");
        let rows = library_sizes(&s);
        let mail_row = row(&rows, LibraryKind::Mail);
        assert!(mail_row.needs_access);
    }
}
