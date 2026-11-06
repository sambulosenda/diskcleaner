//! T04 "Done when": determinism, the test helper, size and speed, and that
//! every item the task lists is really in the tree.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use fixtures::{paths, Entry, EntryKind, Manifest, MANIFEST_FILE};

/// An independent snapshot of a tree: path -> (kind, size, checksum or target).
/// Written without the crate's own walker, so the manifest is checked too.
fn snapshot(root: &Path, home_files: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let root_text = root.to_string_lossy().into_owned();
    snap_dir(root, root, &root_text, home_files, &mut out);
    out
}

fn snap_dir(
    root: &Path,
    dir: &Path,
    root_text: &str,
    home_files: &[String],
    out: &mut BTreeMap<String, String>,
) {
    for item in fs::read_dir(dir).unwrap() {
        let path = item.unwrap().path();
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let meta = fs::symlink_metadata(&path).unwrap();
        let value = if meta.file_type().is_symlink() {
            format!("symlink -> {}", fs::read_link(&path).unwrap().display())
        } else if meta.is_dir() {
            snap_dir(root, &path, root_text, home_files, out);
            "dir".to_string()
        } else {
            let mut data = fs::read(&path).unwrap();
            if home_files.contains(&rel) {
                // These files hold the absolute root by design; compare them
                // with the root written as a placeholder.
                let text = String::from_utf8(data)
                    .unwrap()
                    .replace(root_text, "{HOME}");
                data = text.into_bytes();
            } else if rel == MANIFEST_FILE {
                // The manifest records checksums of the home-path files, which
                // differ between roots; compare it with those lines removed.
                let text = String::from_utf8(data).unwrap();
                data = text
                    .lines()
                    .filter(|l| !l.contains("\"sha256\""))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes();
            }
            format!("file {} {}", data.len(), sha(&data))
        };
        out.insert(rel, value);
    }
}

fn sha(data: &[u8]) -> String {
    use std::fmt::Write;
    // Tiny independent check: FNV is enough to detect differences here; the
    // manifest's SHA-256 is compared separately.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    let mut s = String::new();
    write!(s, "{h:016x}").unwrap();
    s
}

fn home_files(m: &Manifest) -> Vec<String> {
    m.entries
        .iter()
        .filter(|e| e.contains_home_path)
        .map(|e| e.path.clone())
        .collect()
}

fn read_manifest(root: &Path) -> String {
    fs::read_to_string(root.join(MANIFEST_FILE)).unwrap()
}

fn entry<'a>(m: &'a Manifest, path: &str) -> &'a Entry {
    m.get(path)
        .unwrap_or_else(|| panic!("missing from manifest: {path}"))
}

#[test]
fn two_builds_give_identical_trees() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let ma = fixtures::build(a.path(), SystemTime::now()).unwrap();
    let mb = fixtures::build(b.path(), SystemTime::now()).unwrap();

    // Same paths, kinds, sizes and links.
    assert_eq!(ma.entries.len(), mb.entries.len());
    for (ea, eb) in ma.entries.iter().zip(&mb.entries) {
        assert_eq!(ea.path, eb.path);
        assert_eq!(ea.kind, eb.kind);
        assert_eq!(ea.size, eb.size, "{}", ea.path);
        assert_eq!(ea.target, eb.target);
        assert_eq!(ea.hard_link_of, eb.hard_link_of);
        assert_eq!(ea.contains_home_path, eb.contains_home_path);
        // Every file without the home path in it is byte-identical.
        if !ea.contains_home_path {
            assert_eq!(ea.sha256, eb.sha256, "{}", ea.path);
        }
    }

    // Independent walk: identical once the root path itself is normalised.
    let ra = fs::canonicalize(a.path()).unwrap();
    let rb = fs::canonicalize(b.path()).unwrap();
    assert_eq!(
        snapshot(&ra, &home_files(&ma)),
        snapshot(&rb, &home_files(&mb))
    );
}

#[test]
fn manifest_matches_disk() {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    let json: serde_json::Value = serde_json::from_str(&read_manifest(&root)).unwrap();
    let listed = json["entries"].as_array().unwrap();
    let snap = snapshot(&root, &[]);
    // Every path on disk except the manifest itself is listed, and nothing more.
    assert_eq!(listed.len(), snap.len() - 1);
    for e in listed {
        let path = e["path"].as_str().unwrap();
        let full = root.join(path);
        let meta = fs::symlink_metadata(&full).unwrap();
        match e["kind"].as_str().unwrap() {
            "file" => {
                assert!(meta.is_file(), "{path}");
                assert_eq!(e["size"].as_u64().unwrap(), meta.len(), "{path}");
                let data = fs::read(&full).unwrap();
                use sha2::Digest;
                let digest = sha2::Sha256::digest(&data);
                let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
                assert_eq!(e["sha256"].as_str().unwrap(), hex, "{path}");
            }
            "dir" => assert!(meta.is_dir(), "{path}"),
            "symlink" => assert!(meta.file_type().is_symlink(), "{path}"),
            other => panic!("unknown kind {other}"),
        }
    }
}

#[test]
fn size_under_20_mb_and_build_under_5_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let start = Instant::now();
    let manifest = fixtures::build(dir.path(), SystemTime::now()).unwrap();
    let took = start.elapsed();
    assert!(took < Duration::from_secs(5), "took {took:?}");
    let total =
        manifest.total_file_bytes() + fs::metadata(dir.path().join(MANIFEST_FILE)).unwrap().len();
    assert!(total < 20 * 1024 * 1024, "total {total} bytes");
}

#[test]
fn refuses_a_folder_that_is_not_empty() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("precious.txt"), b"keep me").unwrap();
    let err = fixtures::build(dir.path(), SystemTime::now()).unwrap_err();
    assert!(matches!(err, fixtures::FixtureError::NotEmpty(_)), "{err}");
    assert_eq!(
        fs::read(dir.path().join("precious.txt")).unwrap(),
        b"keep me"
    );
}

#[test]
fn refuses_a_symlink_as_root() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let err = fixtures::build(&link, SystemTime::now()).unwrap_err();
    assert!(
        matches!(err, fixtures::FixtureError::NotAFolder(_)),
        "{err}"
    );
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

#[test]
fn everyday_items_exist() {
    let dir = tempfile::tempdir().unwrap();
    let m = fixtures::build(dir.path(), SystemTime::now()).unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    for id in paths::BACKUP_DEVICE_IDS {
        let info = fs::read_to_string(
            root.join(paths::MOBILESYNC_BACKUP)
                .join(id)
                .join("Info.plist"),
        )
        .unwrap();
        assert!(info.starts_with("<?xml"));
        for key in ["Device Name", "Product Version", "Last Backup Date"] {
            assert!(info.contains(&format!("<key>{key}</key>")), "{key}");
        }
        assert!(info.contains("<date>"));
    }
    for f in [
        paths::DOWNLOAD_DMG,
        paths::DOWNLOAD_PKG,
        paths::DOWNLOAD_ZIP,
        paths::DOWNLOAD_NORMAL,
    ] {
        assert_eq!(entry(&m, f).kind, EntryKind::File);
    }
    for id in paths::CACHE_APP_IDS {
        assert_eq!(
            entry(&m, &format!("{}/{id}", paths::CACHES)).kind,
            EntryKind::Dir
        );
    }
    assert_eq!(entry(&m, paths::PHOTOS_LIBRARY).kind, EntryKind::Dir);
    assert_eq!(entry(&m, paths::MESSAGES_ATTACHMENTS).kind, EntryKind::Dir);
}

#[test]
fn xcode_items_exist() {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    let workspace_of = |dd: &str| {
        let text = fs::read_to_string(root.join(dd).join("info.plist")).unwrap();
        let start = text.find("<key>WorkspacePath</key>\n\t<string>").unwrap() + 34;
        let end = text[start..].find("</string>").unwrap() + start;
        std::path::PathBuf::from(&text[start..end])
    };
    for dd in [paths::DERIVED_DATA_LIVE_A, paths::DERIVED_DATA_LIVE_B] {
        let ws = workspace_of(dd);
        assert!(ws.starts_with(&root), "{}", ws.display());
        assert!(ws.is_dir(), "workspace should exist: {}", ws.display());
    }
    let orphan = workspace_of(paths::DERIVED_DATA_ORPHAN);
    assert!(orphan.starts_with(&root));
    assert!(
        fs::symlink_metadata(&orphan).is_err(),
        "orphan workspace must not exist"
    );

    let versions = fs::read_dir(root.join(paths::IOS_DEVICE_SUPPORT))
        .unwrap()
        .count();
    assert_eq!(versions, 3);

    let mut unavailable = 0;
    for udid in paths::SIM_DEVICE_IDS {
        let plist = fs::read_to_string(
            root.join(paths::SIM_DEVICES)
                .join(udid)
                .join("device.plist"),
        )
        .unwrap();
        if plist.contains("<key>isAvailable</key>\n\t<false/>") {
            unavailable += 1;
            assert_eq!(udid, paths::SIM_DEVICE_UNAVAILABLE);
        }
    }
    assert_eq!(unavailable, 1);
    assert!(root.join(paths::XCODE_ARCHIVE).join("Info.plist").is_file());
}

#[test]
fn android_items_exist() {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();
    let sdk = root.join(paths::ANDROID_SDK);
    for api in [33, 34, 35] {
        assert!(sdk.join(format!("system-images/android-{api}")).is_dir());
    }
    assert_eq!(fs::read_dir(sdk.join("build-tools")).unwrap().count(), 2);
    assert_eq!(fs::read_dir(sdk.join("ndk")).unwrap().count(), 1);
    for name in paths::ANDROID_AVD_NAMES {
        let avd = root.join(paths::ANDROID_AVD);
        assert!(avd.join(format!("{name}.avd")).is_dir());
        let ini = fs::read_to_string(avd.join(format!("{name}.ini"))).unwrap();
        let path_line = ini.lines().find(|l| l.starts_with("path=")).unwrap();
        assert!(Path::new(&path_line[5..]).is_dir(), "{path_line}");
    }
    for project in paths::ANDROID_PROJECTS {
        let gradle = fs::read_to_string(root.join(project).join("app/build.gradle")).unwrap();
        assert!(gradle.contains("compileSdk 34"));
        for other in ["33", "35"] {
            assert!(
                !gradle.contains(other),
                "{project} must reference API 34 only"
            );
        }
    }
    assert_eq!(
        fs::read_dir(root.join(paths::GRADLE_DISTS))
            .unwrap()
            .count(),
        3
    );
}

#[test]
fn js_items_exist_and_project_c_is_stale() {
    let now = SystemTime::now();
    let dir = tempfile::tempdir().unwrap();
    fixtures::build(dir.path(), now).unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    for project in paths::JS_PROJECTS {
        assert!(root.join(project).join("package.json").is_file());
        assert!(root.join(project).join("node_modules").is_dir());
    }
    let age = |rel: &str| {
        let modified = fs::metadata(root.join(rel)).unwrap().modified().unwrap();
        now.duration_since(modified).unwrap_or_default().as_secs() / 86_400
    };
    assert_eq!(age(paths::JS_STALE_PACKAGE_JSON), paths::JS_STALE_AGE_DAYS);
    assert!(age("projects/a/package.json") < 1);
    assert!(age("projects/b/package.json") < 1);
    for cache in [paths::NPM_CACHE, paths::PNPM_STORE, paths::YARN_CACHE] {
        assert!(root.join(cache).is_dir(), "{cache}");
    }
}

#[test]
fn traps_are_in_place() {
    let dir = fixtures::home();
    let root = fixtures::canonical_root(&dir).unwrap();

    let etc = root.join(paths::TRAP_SYMLINK_ETC);
    assert!(fs::symlink_metadata(&etc).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_link(&etc).unwrap(), Path::new("/etc"));

    let docs = root.join(paths::TRAP_SYMLINK_DOCUMENTS);
    assert!(fs::symlink_metadata(&docs)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::canonicalize(&docs).unwrap(),
        root.join(paths::DOCUMENTS),
        "cache symlink must resolve into Documents"
    );

    let a = fs::metadata(root.join(paths::TRAP_HARD_LINK_A)).unwrap();
    let b = fs::metadata(root.join(paths::TRAP_HARD_LINK_B)).unwrap();
    assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()));
    assert_eq!(a.nlink(), 2);

    assert!(root.join(paths::TRAP_GIT_DIR).join("HEAD").is_file());
    assert!(root.join(paths::TRAP_GIT_NODE_MODULES).is_dir());
    assert!(root.join(paths::TRAP_ENV_FILE).is_file());
    assert!(root.join(paths::TRAP_KEYSTORE).is_file());
}

#[test]
fn hard_link_pair_is_counted_once() {
    let dir = tempfile::tempdir().unwrap();
    let m = fixtures::build(dir.path(), SystemTime::now()).unwrap();
    let b = entry(&m, paths::TRAP_HARD_LINK_B);
    assert_eq!(b.hard_link_of.as_deref(), Some(paths::TRAP_HARD_LINK_A));
    let all: u64 = m
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::File)
        .map(|e| e.size)
        .sum();
    assert_eq!(all - m.total_file_bytes(), b.size);
}

#[test]
fn nothing_points_outside_except_the_etc_trap() {
    let dir = tempfile::tempdir().unwrap();
    let m = fixtures::build(dir.path(), SystemTime::now()).unwrap();
    let links: Vec<_> = m
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Symlink)
        .collect();
    assert_eq!(links.len(), 2);
    for link in links {
        let target = link.target.as_deref().unwrap();
        if target.starts_with('/') {
            assert_eq!(link.path, paths::TRAP_SYMLINK_ETC);
        }
    }
}
