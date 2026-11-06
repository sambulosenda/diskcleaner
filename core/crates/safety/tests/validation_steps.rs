//! Tests for each step of "Validation order" in `docs/safety/protected-paths.md`.

mod common;

use std::os::unix::fs::{symlink, MetadataExt};
use std::path::{Path, PathBuf};

use common::FakeHome;
use safety::protected::{BlockReason, ReviewReason, BUILD_OUTPUT_EXCEPTIONS};
use safety::{validate, Refusal};

// ---------------------------------------------------------------------------
// The home path itself must be usable.
// ---------------------------------------------------------------------------

#[test]
fn home_must_be_absolute_canonical_and_not_root() {
    let h = FakeHome::new();
    let target = h.file("Library/Caches/x");
    let trailing = PathBuf::from(format!("{}/", h.home.display()));
    for home in [
        Path::new("relative/home"),
        Path::new("/"),
        trailing.as_path(),
    ] {
        assert_eq!(
            validate(home, h.dev, &target, true),
            Err(Refusal::InvalidHome),
            "{}",
            home.display()
        );
    }
}

#[test]
fn home_must_not_be_a_symlink() {
    let h = FakeHome::new();
    h.file("Library/Caches/x");
    let link = h.home.parent().expect("parent").join("home-link");
    symlink(&h.home, &link).expect("symlink");
    let target = link.join("Library/Caches/x");
    assert_eq!(
        validate(&link, h.dev, &target, true),
        Err(Refusal::InvalidHome)
    );
}

#[test]
fn home_must_exist() {
    let h = FakeHome::new();
    let missing = h.home.parent().expect("parent").join("nobody");
    let target = missing.join("Library/Caches/x");
    assert_eq!(
        validate(&missing, h.dev, &target, true),
        Err(Refusal::InvalidHome)
    );
}

// ---------------------------------------------------------------------------
// 1. "Path is absolute and canonical (no `..`, no trailing tricks)."
// ---------------------------------------------------------------------------

#[test]
fn step1_relative_path() {
    let h = FakeHome::new();
    h.file("Library/Caches/x");
    for rel in [
        "Library/Caches/x",
        "./Library/Caches/x",
        "~/Library/Caches/x",
        "",
    ] {
        for result in h.check_both(Path::new(rel)) {
            assert_eq!(result, Err(Refusal::NotAbsolute), "{rel:?}");
        }
    }
}

#[test]
fn step1_dot_dot_segments() {
    let h = FakeHome::new();
    h.file("Library/Caches/x");
    h.dir("Library/Keychains");
    let home = h.home.display();
    for raw in [
        format!("{home}/Library/Caches/../Keychains"),
        format!("{home}/Library/Caches/x/.."),
        format!("{home}/../home/Library/Caches/x"),
        format!("{home}/.."),
    ] {
        for result in h.check_both(Path::new(&raw)) {
            assert_eq!(result, Err(Refusal::NotCanonical), "{raw}");
        }
    }
}

#[test]
fn step1_trailing_tricks() {
    let h = FakeHome::new();
    h.file("Library/Caches/x");
    let home = h.home.display();
    for raw in [
        format!("{home}/Library/Caches/x/"),
        format!("{home}/Library/Caches/./x"),
        format!("{home}/Library/Caches/x/."),
        format!("{home}/Library//Caches/x"),
        format!("{home}//Library/Caches/x"),
    ] {
        for result in h.check_both(Path::new(&raw)) {
            assert_eq!(result, Err(Refusal::NotCanonical), "{raw}");
        }
    }
}

// ---------------------------------------------------------------------------
// 2. "Path starts with the home path."
// ---------------------------------------------------------------------------

#[test]
fn step2_path_outside_home() {
    let h = FakeHome::new();
    let parent = h.home.parent().expect("parent").to_path_buf();
    // A sibling whose name starts with the home folder's name is still outside.
    let lookalike = parent.join("home2");
    std::fs::create_dir(&lookalike).expect("make lookalike");
    for path in [
        parent.clone(),
        lookalike,
        PathBuf::from("/"),
        PathBuf::from("/tmp"),
    ] {
        for result in h.check_both(&path) {
            assert_eq!(result, Err(Refusal::OutsideHome), "{}", path.display());
        }
    }
}

// ---------------------------------------------------------------------------
// 3. "No component of the path, or any parent up to home, is a symlink
// (`symlink_metadata`, never `metadata`)."
// ---------------------------------------------------------------------------

#[test]
fn step3_symlinked_parent() {
    let h = FakeHome::new();
    let real = h.dir("Library/Caches/real");
    h.file("Library/Caches/real/file");
    let link = h.path("Library/Caches/link");
    symlink(&real, &link).expect("symlink");
    let target = link.join("file");
    for result in h.check_both(&target) {
        assert_eq!(result, Err(Refusal::Symlink { at: link.clone() }));
    }
}

#[test]
fn step3_symlinked_parent_pointing_at_protected_folder() {
    // A link inside an allowed folder that leads into Keychains.
    let h = FakeHome::new();
    let keychains = h.dir("Library/Keychains");
    h.file("Library/Keychains/login.keychain-db");
    let link = h.path("Library/Caches/sneaky");
    std::fs::create_dir_all(link.parent().expect("parent")).expect("make parent");
    symlink(&keychains, &link).expect("symlink");
    for result in h.check_both(&link.join("login.keychain-db")) {
        assert_eq!(result, Err(Refusal::Symlink { at: link.clone() }));
    }
}

#[test]
fn step3_symlink_as_final_component() {
    let h = FakeHome::new();
    let real = h.file("Library/Caches/real-file");
    let link = h.path("Library/Caches/link-file");
    symlink(&real, &link).expect("symlink");
    for result in h.check_both(&link) {
        assert_eq!(result, Err(Refusal::Symlink { at: link.clone() }));
    }
    // A dangling link is refused the same way.
    let dangling = h.path("Library/Caches/dangling");
    symlink(h.path("Library/Caches/nowhere"), &dangling).expect("symlink");
    assert_eq!(
        h.check(&dangling, true),
        Err(Refusal::Symlink {
            at: dangling.clone()
        })
    );
}

#[test]
fn step3_missing_path_is_refused() {
    let h = FakeHome::new();
    h.dir("Library/Caches");
    let missing = h.path("Library/Caches/not-there");
    assert_eq!(
        h.check(&missing, true),
        Err(Refusal::Unreadable {
            at: missing.clone()
        })
    );
}

// ---------------------------------------------------------------------------
// 4. "Path is on the same volume (`st_dev`) as the scan root."
// ---------------------------------------------------------------------------

#[test]
fn step4_other_volume_mocked_device_id() {
    // The scan root's device id is passed in, so a different id stands in
    // for a path on another disk.
    let h = FakeHome::new();
    let target = h.file("Library/Caches/x");
    for other_dev in [h.dev.wrapping_add(1), h.dev ^ 0xFFFF, 0, u64::MAX] {
        if other_dev == h.dev {
            continue;
        }
        for selected in [false, true] {
            assert_eq!(
                validate(&h.home, other_dev, &target, selected),
                Err(Refusal::OtherVolume)
            );
        }
    }
    assert!(validate(&h.home, h.dev, &target, false).is_ok());
}

/// Needs a real second volume, so it is ignored by default. On a Mac:
///
/// ```sh
/// mkdir -p /tmp/roomy-volume-test/home/mnt
/// hdiutil create -size 20m -fs APFS -volname RoomyTest /tmp/roomy-volume-test/disk.dmg
/// hdiutil attach /tmp/roomy-volume-test/disk.dmg -mountpoint /tmp/roomy-volume-test/home/mnt
/// cd core
/// ROOMY_TEST_HOME=/private/tmp/roomy-volume-test/home \
/// ROOMY_TEST_MOUNT=/private/tmp/roomy-volume-test/home/mnt \
///   cargo test -p safety -- --ignored step4_other_volume_real_disk_image
/// hdiutil detach /tmp/roomy-volume-test/home/mnt
/// ```
///
/// Both paths must be canonical (`/private/tmp`, not `/tmp`). The fake home
/// is a throwaway folder, never the real home.
#[test]
#[ignore = "needs a mounted disk image; see the doc comment for steps"]
fn step4_other_volume_real_disk_image() {
    let home = PathBuf::from(std::env::var("ROOMY_TEST_HOME").expect("set ROOMY_TEST_HOME"));
    let mount = PathBuf::from(std::env::var("ROOMY_TEST_MOUNT").expect("set ROOMY_TEST_MOUNT"));
    assert!(
        mount.starts_with(&home),
        "the mount point must be inside the fake home"
    );
    let home_dev = std::fs::symlink_metadata(&home).expect("stat home").dev();
    let mount_dev = std::fs::symlink_metadata(&mount).expect("stat mount").dev();
    assert_ne!(
        home_dev, mount_dev,
        "the mount point is not a separate volume"
    );
    let target = mount.join("roomy-volume-test-file");
    std::fs::write(&target, b"data").expect("write on the disk image");
    for selected in [false, true] {
        assert_eq!(
            validate(&home, home_dev, &target, selected),
            Err(Refusal::OtherVolume)
        );
    }
}

// ---------------------------------------------------------------------------
// 5. "Path is not hard-blocked." (each entry is in protected_paths.rs; these
// cover the `.git` rule from the validation side)
// ---------------------------------------------------------------------------

#[test]
fn step5_folder_containing_git() {
    let h = FakeHome::new();
    let repo = h.repo("Projects/site");
    for result in h.check_both(&repo) {
        assert_eq!(
            result,
            Err(Refusal::HardBlocked(BlockReason::GitRepository))
        );
    }
}

#[test]
fn step5_allowed_build_output_inside_repo() {
    let h = FakeHome::new();
    h.repo("Projects/site");
    for name in BUILD_OUTPUT_EXCEPTIONS {
        let path = h.dir(&format!("Projects/site/{name}"));
        let ok = h
            .check(&path, false)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(ok.review_only(), None);
    }
}

#[test]
fn step5_build_output_in_repo_inside_documents_is_still_review_only() {
    // The exception lifts the `.git` block only; step 6 still applies.
    let h = FakeHome::new();
    h.repo("Documents/code/app");
    let target = h.dir("Documents/code/app/node_modules");
    assert_eq!(
        h.check(&target, false),
        Err(Refusal::NeedsIndividualSelection(
            ReviewReason::PersonalFile
        ))
    );
    assert!(h.check(&target, true).is_ok());
}

#[test]
fn step5_hard_block_wins_over_selection_and_review() {
    // A signing key inside Downloads is review-only by place but hard-blocked
    // by type. Hard-blocked wins.
    let h = FakeHome::new();
    let key = h.file("Downloads/AuthKey_ABC123.p8");
    for result in h.check_both(&key) {
        assert_eq!(
            result,
            Err(Refusal::HardBlocked(BlockReason::SigningMaterial))
        );
    }
}

// ---------------------------------------------------------------------------
// 6. "If path is review-only, the plan item must have been individually
// selected by the user."
// ---------------------------------------------------------------------------

#[test]
fn step6_review_only_needs_individual_selection() {
    let h = FakeHome::new();
    let target = h.file("Downloads/installer.dmg");
    assert_eq!(
        h.check(&target, false),
        Err(Refusal::NeedsIndividualSelection(
            ReviewReason::PersonalFile
        ))
    );
    let ok = h.check(&target, true).expect("allowed when selected");
    assert_eq!(ok.review_only(), Some(ReviewReason::PersonalFile));
}

#[test]
fn step6_ordinary_path_needs_no_selection() {
    let h = FakeHome::new();
    let target = h.dir("Library/Developer/Xcode/DerivedData/App-abc");
    let ok = h.check(&target, false).expect("allowed");
    assert_eq!(ok.review_only(), None);
    assert_eq!(ok.path(), target.as_path());
}
