//! One test per entry in `docs/safety/protected-paths.md`.
//!
//! Hard-blocked entries must be refused whether or not the item was selected
//! individually. Review-only entries must be refused unless selected
//! individually. Each test names the line of the doc it proves.

mod common;

use std::fs;
use std::path::Path;

use common::FakeHome;
use safety::protected::{
    self, BlockReason, ReviewReason, BUILD_OUTPUT_EXCEPTIONS, CREATIVE_PROJECT_EXTENSIONS,
    MEDIA_LIBRARY_EXTENSIONS, OUTSIDE_HOME_EXAMPLES, SIGNING_EXTENSIONS,
};
use safety::{check_docker_volume, validate, Refusal};

/// Asserts the path is hard-blocked for `reason`, selected or not.
fn assert_hard(h: &FakeHome, path: &Path, reason: BlockReason) {
    for result in h.check_both(path) {
        assert_eq!(
            result,
            Err(Refusal::HardBlocked(reason)),
            "{}",
            path.display()
        );
    }
}

/// Asserts the path is review-only for `reason`: refused unless selected
/// individually, and marked review-only when allowed.
fn assert_review(h: &FakeHome, path: &Path, reason: ReviewReason) {
    let [unselected, selected] = h.check_both(path);
    assert_eq!(
        unselected,
        Err(Refusal::NeedsIndividualSelection(reason)),
        "{}",
        path.display()
    );
    let ok = selected.unwrap_or_else(|e| panic!("{} refused: {e:?}", path.display()));
    assert_eq!(ok.review_only(), Some(reason));
    assert_eq!(ok.path(), path);
}

// ---------------------------------------------------------------------------
// Control: an ordinary cache is allowed without individual selection, so the
// refusals below are not just "everything is refused".
// ---------------------------------------------------------------------------

#[test]
fn control_ordinary_cache_is_allowed() {
    let h = FakeHome::new();
    let path = h.file("Library/Caches/com.example.app/cache.db-not-messages");
    let ok = h.check(&path, false).expect("ordinary cache should pass");
    assert_eq!(ok.review_only(), None);
}

// ---------------------------------------------------------------------------
// "Anything outside the home folder, including `/System`, `/Library`,
// `/usr`, `/bin`, `/sbin`, `/private`, `/Applications`, `/Volumes`."
// ---------------------------------------------------------------------------

macro_rules! outside_home_test {
    ($name:ident, $root:literal) => {
        #[test]
        fn $name() {
            let h = FakeHome::new();
            for path in [$root.to_string(), format!("{}/anything", $root)] {
                for selected in [false, true] {
                    let result = validate(&h.home, h.dev, Path::new(&path), selected);
                    assert_eq!(result, Err(Refusal::OutsideHome), "{path}");
                }
            }
        }
    };
}

outside_home_test!(outside_home_system, "/System");
outside_home_test!(outside_home_library, "/Library");
outside_home_test!(outside_home_usr, "/usr");
outside_home_test!(outside_home_bin, "/bin");
outside_home_test!(outside_home_sbin, "/sbin");
outside_home_test!(outside_home_private, "/private");
outside_home_test!(outside_home_applications, "/Applications");
outside_home_test!(outside_home_volumes, "/Volumes");

#[test]
fn outside_home_list_matches_doc() {
    assert_eq!(
        OUTSIDE_HOME_EXAMPLES,
        &[
            "/System",
            "/Library",
            "/usr",
            "/bin",
            "/sbin",
            "/private",
            "/Applications",
            "/Volumes"
        ]
    );
}

#[test]
fn outside_home_existing_sibling_folder() {
    let h = FakeHome::new();
    let sibling = h.home.parent().expect("home has a parent").join("other");
    fs::create_dir(&sibling).expect("make sibling");
    for result in h.check_both(&sibling) {
        assert_eq!(result, Err(Refusal::OutsideHome));
    }
}

// ---------------------------------------------------------------------------
// "The home folder itself, and these folders themselves (their contents may
// be targeted, the folders never): `~/Desktop`, `~/Documents`, `~/Downloads`,
// `~/Pictures`, `~/Movies`, `~/Music`, `~/Library`, `~/Applications`, `~/Public`."
// ---------------------------------------------------------------------------

#[test]
fn home_folder_itself() {
    let h = FakeHome::new();
    assert_hard(&h, &h.home.clone(), BlockReason::HomeFolder);
}

macro_rules! main_folder_test {
    ($name:ident, $folder:literal) => {
        #[test]
        fn $name() {
            let h = FakeHome::new();
            let path = h.dir($folder);
            assert_hard(&h, &path, BlockReason::MainFolder);
            // A different letter case is the same folder on a macOS volume.
            let lower = h.dir(&$folder.to_lowercase());
            assert_hard(&h, &lower, BlockReason::MainFolder);
        }
    };
}

main_folder_test!(main_folder_desktop, "Desktop");
main_folder_test!(main_folder_documents, "Documents");
main_folder_test!(main_folder_downloads, "Downloads");
main_folder_test!(main_folder_pictures, "Pictures");
main_folder_test!(main_folder_movies, "Movies");
main_folder_test!(main_folder_music, "Music");
main_folder_test!(main_folder_library, "Library");
main_folder_test!(main_folder_applications, "Applications");
main_folder_test!(main_folder_public, "Public");

#[test]
fn main_folder_contents_may_be_targeted() {
    // "their contents may be targeted"
    let h = FakeHome::new();
    let pictures = h.file("Pictures/old-export.zip");
    assert!(h.check(&pictures, false).is_ok());
    let library = h.dir("Library/Caches");
    assert!(h.check(&library, false).is_ok());
}

// ---------------------------------------------------------------------------
// "`~/Library/Keychains/**`, `~/Library/Mail/**`, `~/Library/Messages/*.db*`
// and `~/Library/Messages/` itself, `~/Library/Mobile Documents/**` (iCloud
// Drive), `~/Library/CloudStorage/**`."
// ---------------------------------------------------------------------------

/// Asserts the folder, a file inside it and a nested file are all blocked.
fn assert_subtree_blocked(h: &FakeHome, root: &str, reason: BlockReason) {
    let folder = h.dir(root);
    assert_hard(h, &folder, reason);
    assert_hard(h, &h.file(&format!("{root}/item")), reason);
    assert_hard(h, &h.file(&format!("{root}/deep/er/item.txt")), reason);
}

#[test]
fn library_keychains_subtree() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, "Library/Keychains", BlockReason::Keychains);
}

#[test]
fn library_keychains_other_case() {
    let h = FakeHome::new();
    let path = h.file("library/KEYCHAINS/login.keychain-db");
    assert_hard(&h, &path, BlockReason::Keychains);
}

#[test]
fn library_mail_subtree() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, "Library/Mail", BlockReason::Mail);
}

#[test]
fn library_messages_folder_itself() {
    let h = FakeHome::new();
    let path = h.dir("Library/Messages");
    assert_hard(&h, &path, BlockReason::MessagesFolder);
}

#[test]
fn library_messages_databases() {
    let h = FakeHome::new();
    for name in [
        "chat.db",
        "chat.db-wal",
        "chat.db-shm",
        "prewarmsg.db",
        ".db",
    ] {
        let path = h.file(&format!("Library/Messages/{name}"));
        assert_hard(&h, &path, BlockReason::MessagesDatabase);
    }
    // Safer than written: a `.db` name deeper inside Messages is blocked too.
    let deep = h.file("Library/Messages/Archive/old.db");
    assert_hard(&h, &deep, BlockReason::MessagesDatabase);
}

#[test]
fn library_mobile_documents_subtree() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, "Library/Mobile Documents", BlockReason::ICloudDrive);
}

#[test]
fn library_cloud_storage_subtree() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, "Library/CloudStorage", BlockReason::CloudStorage);
}

// ---------------------------------------------------------------------------
// "Any `*.photoslibrary`, `*.musiclibrary`, `*.tvlibrary` bundle and
// everything inside it."
// ---------------------------------------------------------------------------

macro_rules! media_library_test {
    ($name:ident, $ext:literal) => {
        #[test]
        fn $name() {
            let h = FakeHome::new();
            for root in [
                concat!("Pictures/Library.", $ext),
                concat!("Old Stuff/Copy.", $ext),
            ] {
                assert_subtree_blocked(&h, root, BlockReason::MediaLibrary);
            }
            let upper = h.dir(&concat!("Music/Lib.", $ext).to_uppercase());
            assert_hard(&h, &upper, BlockReason::MediaLibrary);
        }
    };
}

media_library_test!(media_photoslibrary, "photoslibrary");
media_library_test!(media_musiclibrary, "musiclibrary");
media_library_test!(media_tvlibrary, "tvlibrary");

#[test]
fn media_library_list_matches_doc() {
    assert_eq!(
        MEDIA_LIBRARY_EXTENSIONS,
        &["photoslibrary", "musiclibrary", "tvlibrary"]
    );
}

// ---------------------------------------------------------------------------
// "`~/.ssh/**`, `~/.gnupg/**`, `~/.aws/**`, `~/.netrc`,
// `~/.docker/config.json`, `~/.kube/**`, `~/.config/gh/**`."
// ---------------------------------------------------------------------------

#[test]
fn credentials_ssh() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, ".ssh", BlockReason::Credentials);
}

#[test]
fn credentials_gnupg() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, ".gnupg", BlockReason::Credentials);
}

#[test]
fn credentials_aws() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, ".aws", BlockReason::Credentials);
}

#[test]
fn credentials_netrc() {
    let h = FakeHome::new();
    let path = h.file(".netrc");
    assert_hard(&h, &path, BlockReason::Credentials);
}

#[test]
fn credentials_netrc_hard_link_elsewhere() {
    // The same file under another name is still `~/.netrc`.
    let h = FakeHome::new();
    let netrc = h.file(".netrc");
    let link = h.path("Library/Caches/innocent-looking");
    fs::create_dir_all(link.parent().expect("parent")).expect("make parent");
    fs::hard_link(&netrc, &link).expect("hard link");
    assert_hard(&h, &link, BlockReason::Credentials);
}

#[test]
fn credentials_docker_config() {
    let h = FakeHome::new();
    let path = h.file(".docker/config.json");
    assert_hard(&h, &path, BlockReason::Credentials);
    // Other Docker files are not blocked by this line.
    let other = h.file(".docker/buildx/activity/default");
    assert!(h.check(&other, false).is_ok());
}

#[test]
fn credentials_kube() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, ".kube", BlockReason::Credentials);
}

#[test]
fn credentials_config_gh() {
    let h = FakeHome::new();
    assert_subtree_blocked(&h, ".config/gh", BlockReason::Credentials);
}

#[test]
fn folder_holding_a_blocked_path_is_blocked() {
    // Removing a parent would remove the protected item inside it.
    let h = FakeHome::new();
    h.file(".docker/config.json");
    assert_hard(&h, &h.path(".docker"), BlockReason::ContainsProtected);
    h.dir(".config/gh");
    assert_hard(&h, &h.path(".config"), BlockReason::ContainsProtected);
    h.dir("Library/MobileDevice/Provisioning Profiles");
    assert_hard(
        &h,
        &h.path("Library/MobileDevice"),
        BlockReason::ContainsProtected,
    );
}

// ---------------------------------------------------------------------------
// "Any `.git` directory, and any directory that contains a `.git` directory,
// **except** these named build-output children inside a repository:
// `node_modules`, `build`, `target`, `DerivedData`, `.gradle`, `Pods`,
// `.next`, `dist`, `.venv`, `venv`, `__pycache__`."
// ---------------------------------------------------------------------------

#[test]
fn git_directory_itself_and_inside() {
    let h = FakeHome::new();
    h.repo("code/app");
    assert_hard(&h, &h.path("code/app/.git"), BlockReason::GitRepository);
    assert_hard(
        &h,
        &h.file("code/app/.git/objects/ab/cdef"),
        BlockReason::GitRepository,
    );
    // A stray `.git` folder with no repository around it.
    assert_hard(
        &h,
        &h.dir("Library/Caches/x/.git"),
        BlockReason::GitRepository,
    );
    assert_hard(
        &h,
        &h.dir("Library/Caches/y/.GIT"),
        BlockReason::GitRepository,
    );
}

#[test]
fn git_repository_root() {
    let h = FakeHome::new();
    let root = h.repo("code/app");
    assert_hard(&h, &root, BlockReason::GitRepository);
}

#[test]
fn git_worktree_with_git_file_is_a_repository() {
    // Worktrees and submodules have a `.git` file, not a folder. Treated the same.
    let h = FakeHome::new();
    h.file("code/wt/.git");
    assert_hard(&h, &h.path("code/wt"), BlockReason::GitRepository);
    assert_hard(
        &h,
        &h.file("code/wt/src/main.rs"),
        BlockReason::GitRepository,
    );
}

#[test]
fn git_source_inside_repository() {
    let h = FakeHome::new();
    h.repo("code/app");
    assert_hard(
        &h,
        &h.file("code/app/src/main.rs"),
        BlockReason::GitRepository,
    );
    assert_hard(&h, &h.dir("code/app/assets"), BlockReason::GitRepository);
}

#[test]
fn git_build_output_exceptions_allowed() {
    let h = FakeHome::new();
    h.repo("code/app");
    for name in BUILD_OUTPUT_EXCEPTIONS {
        let folder = h.dir(&format!("code/app/{name}"));
        assert!(
            h.check(&folder, false).is_ok(),
            "{name} folder should be allowed"
        );
        let inside = h.file(&format!("code/app/{name}/sub/file.o"));
        assert!(
            h.check(&inside, false).is_ok(),
            "{name} contents should be allowed"
        );
    }
}

#[test]
fn git_build_output_list_matches_doc() {
    assert_eq!(
        BUILD_OUTPUT_EXCEPTIONS,
        &[
            "node_modules",
            "build",
            "target",
            "DerivedData",
            ".gradle",
            "Pods",
            ".next",
            "dist",
            ".venv",
            "venv",
            "__pycache__"
        ]
    );
}

#[test]
fn git_build_output_exception_is_exact() {
    let h = FakeHome::new();
    h.repo("code/app");
    // A different case is not the exception (it may be a source folder).
    assert_hard(
        &h,
        &h.dir("code/app/Node_Modules"),
        BlockReason::GitRepository,
    );
    // Only as a direct child of the repository root.
    assert_hard(
        &h,
        &h.dir("code/app/packages/web/node_modules"),
        BlockReason::GitRepository,
    );
}

#[test]
fn git_nested_repository_inside_build_output() {
    let h = FakeHome::new();
    h.repo("code/app");
    h.repo("code/app/node_modules/some-dep");
    assert_hard(
        &h,
        &h.path("code/app/node_modules/some-dep"),
        BlockReason::GitRepository,
    );
    assert_hard(
        &h,
        &h.file("code/app/node_modules/some-dep/index.js"),
        BlockReason::GitRepository,
    );
}

// ---------------------------------------------------------------------------
// "Signing material: `~/Library/MobileDevice/Provisioning Profiles/**`, and
// any file matching `*.keystore`, `*.jks`, `*.p12`, `*.p8`,
// `*.mobileprovision`, `*.pem`, `*.key`."
// ---------------------------------------------------------------------------

#[test]
fn signing_provisioning_profiles() {
    let h = FakeHome::new();
    assert_subtree_blocked(
        &h,
        "Library/MobileDevice/Provisioning Profiles",
        BlockReason::SigningMaterial,
    );
}

macro_rules! signing_ext_test {
    ($name:ident, $ext:literal) => {
        #[test]
        fn $name() {
            let h = FakeHome::new();
            for rel in [
                concat!("release.", $ext),
                concat!("Library/Caches/tmp/upload.", $ext),
                concat!("old/KEYS/Dist.", $ext),
            ] {
                assert_hard(&h, &h.file(rel), BlockReason::SigningMaterial);
            }
            let upper = h.file(&concat!("Library/Caches/A.", $ext).to_uppercase());
            assert_hard(&h, &upper, BlockReason::SigningMaterial);
        }
    };
}

signing_ext_test!(signing_keystore, "keystore");
signing_ext_test!(signing_jks, "jks");
signing_ext_test!(signing_p12, "p12");
signing_ext_test!(signing_p8, "p8");
signing_ext_test!(signing_mobileprovision, "mobileprovision");
signing_ext_test!(signing_pem, "pem");
signing_ext_test!(signing_key, "key");

#[test]
fn signing_list_matches_doc() {
    assert_eq!(
        SIGNING_EXTENSIONS,
        &[
            "keystore",
            "jks",
            "p12",
            "p8",
            "mobileprovision",
            "pem",
            "key"
        ]
    );
}

// ---------------------------------------------------------------------------
// "Docker volumes, by identifier, in every Docker action."
// ---------------------------------------------------------------------------

#[test]
fn docker_volumes_always_refused() {
    for id in [
        "",
        "pgdata",
        "my_project_db",
        "0123456789abcdef",
        "anything at all",
    ] {
        assert_eq!(
            check_docker_volume(id),
            Err(Refusal::HardBlocked(BlockReason::DockerVolume))
        );
    }
}

// ---------------------------------------------------------------------------
// Review-only
// ---------------------------------------------------------------------------

#[test]
fn review_device_backups() {
    // "iPhone and iPad backups (`~/Library/Application Support/MobileSync/Backup/*`)."
    let h = FakeHome::new();
    let base = "Library/Application Support/MobileSync/Backup";
    assert_review(
        &h,
        &h.dir(&format!("{base}/00008030-001A2B3C4D5E")),
        ReviewReason::DeviceBackup,
    );
    assert_review(
        &h,
        &h.file(&format!("{base}/00008030-001A2B3C4D5E/Manifest.db")),
        ReviewReason::DeviceBackup,
    );
    assert_review(&h, &h.path(base), ReviewReason::DeviceBackup);
}

#[test]
fn review_messages_attachments_not_removable_in_v1() {
    // "Messages attachments (`~/Library/Messages/Attachments/**`) — shown and
    // opened in Finder only; the app does not delete them in v1."
    let h = FakeHome::new();
    for path in [
        h.dir("Library/Messages/Attachments"),
        h.dir("Library/Messages/Attachments/ab/12"),
        h.file("Library/Messages/Attachments/ab/12/IMG_0001.HEIC"),
    ] {
        for result in h.check_both(&path) {
            assert_eq!(
                result,
                Err(Refusal::NotRemovableInV1(ReviewReason::MessagesAttachment)),
                "{}",
                path.display()
            );
        }
    }
}

macro_rules! personal_folder_test {
    ($name:ident, $folder:literal) => {
        #[test]
        fn $name() {
            // "Any file directly in Downloads, Desktop or Documents, including
            // large files and duplicates." Safer than written: deeper too.
            let h = FakeHome::new();
            for rel in [
                concat!($folder, "/big-video.mov"),
                concat!($folder, "/copy of report (1).pdf"),
                concat!($folder, "/sub/folder/file.txt"),
            ] {
                assert_review(&h, &h.file(rel), ReviewReason::PersonalFile);
            }
            assert_review(
                &h,
                &h.dir(concat!($folder, "/a folder")),
                ReviewReason::PersonalFile,
            );
        }
    };
}

personal_folder_test!(review_personal_downloads, "Downloads");
personal_folder_test!(review_personal_desktop, "Desktop");
personal_folder_test!(review_personal_documents, "Documents");

#[test]
fn review_app_leftovers_containers() {
    // "App leftovers in `~/Library/Containers/*` ... for apps not currently installed."
    let h = FakeHome::new();
    assert_review(
        &h,
        &h.dir("Library/Containers/com.example.gone"),
        ReviewReason::AppLeftovers,
    );
    assert_review(
        &h,
        &h.path("Library/Containers"),
        ReviewReason::AppLeftovers,
    );
}

#[test]
fn review_app_leftovers_application_support() {
    // "... and `~/Library/Application Support/*` for apps not currently installed."
    let h = FakeHome::new();
    assert_review(
        &h,
        &h.dir("Library/Application Support/Some Old App"),
        ReviewReason::AppLeftovers,
    );
    assert_review(
        &h,
        &h.path("Library/Application Support"),
        ReviewReason::AppLeftovers,
    );
}

#[test]
fn review_android_avds_and_snapshots() {
    // "Android AVDs (`~/.android/avd/**`) and emulator snapshots"
    let h = FakeHome::new();
    assert_review(
        &h,
        &h.dir(".android/avd/Pixel_8.avd"),
        ReviewReason::AndroidEmulator,
    );
    assert_review(
        &h,
        &h.dir(".android/avd/Pixel_8.avd/snapshots/default_boot"),
        ReviewReason::AndroidEmulator,
    );
    assert_review(
        &h,
        &h.file(".android/avd/Pixel_8.ini"),
        ReviewReason::AndroidEmulator,
    );
}

#[test]
fn review_xcode_archives() {
    // "Xcode archives (`~/Library/Developer/Xcode/Archives/**`)."
    let h = FakeHome::new();
    assert_review(
        &h,
        &h.dir("Library/Developer/Xcode/Archives/2026-11-06/App.xcarchive"),
        ReviewReason::XcodeArchive,
    );
    assert_review(
        &h,
        &h.path("Library/Developer/Xcode/Archives"),
        ReviewReason::XcodeArchive,
    );
}

#[test]
fn review_env_files() {
    // "Any `.env` / `.env.*` file."
    let h = FakeHome::new();
    for rel in [
        "old/.env",
        "old/.env.local",
        "old/.env.production",
        "Library/Caches/x/.ENV.Dev",
    ] {
        assert_review(&h, &h.file(rel), ReviewReason::EnvFile);
    }
    // Not `.env`: these are ordinary names.
    assert!(h
        .check(&h.file("Library/Caches/x/.envrc-cache"), false)
        .is_ok());
}

#[test]
fn review_ai_tool_history_claude() {
    // "AI-tool session history: `~/.claude/projects/**`"
    let h = FakeHome::new();
    assert_review(
        &h,
        &h.file(".claude/projects/-Users-sam-code/abc.jsonl"),
        ReviewReason::AiToolHistory,
    );
    assert_review(&h, &h.path(".claude/projects"), ReviewReason::AiToolHistory);
}

#[test]
fn review_ai_tool_history_codex() {
    // "`~/.codex/sessions/**`"
    let h = FakeHome::new();
    assert_review(
        &h,
        &h.file(".codex/sessions/2026/11/06/rollout.jsonl"),
        ReviewReason::AiToolHistory,
    );
}

#[test]
fn review_ai_tool_history_cursor() {
    // "Cursor chat databases"
    let h = FakeHome::new();
    let base = "Library/Application Support/Cursor/User";
    assert_review(
        &h,
        &h.file(&format!("{base}/workspaceStorage/abc123/state.vscdb")),
        ReviewReason::AiToolHistory,
    );
    assert_review(
        &h,
        &h.file(&format!("{base}/globalStorage/state.vscdb")),
        ReviewReason::AiToolHistory,
    );
}

macro_rules! creative_test {
    ($name:ident, $ext:literal) => {
        #[test]
        fn $name() {
            // "Final Cut (`*.fcpbundle`), Logic, GarageBand and Adobe project libraries"
            let h = FakeHome::new();
            let bundle = concat!("Movies/Project.", $ext);
            assert_review(&h, &h.dir(bundle), ReviewReason::CreativeProject);
            assert_review(
                &h,
                &h.file(&format!("{bundle}/Render Files/a.mov")),
                ReviewReason::CreativeProject,
            );
        }
    };
}

creative_test!(review_final_cut, "fcpbundle");
creative_test!(review_logic, "logicx");
creative_test!(review_garageband, "band");
creative_test!(review_premiere, "prproj");
creative_test!(review_after_effects, "aep");
creative_test!(review_lightroom_catalog, "lrcat");
creative_test!(review_lightroom_library, "lrlibrary");

#[test]
fn creative_list_is_known() {
    assert_eq!(
        CREATIVE_PROJECT_EXTENSIONS,
        &[
            "fcpbundle",
            "logicx",
            "band",
            "prproj",
            "aep",
            "lrcat",
            "lrlibrary"
        ]
    );
}

#[test]
fn folder_holding_a_review_only_path_is_review_only() {
    let h = FakeHome::new();
    h.dir(".android/avd");
    assert_review(&h, &h.path(".android"), ReviewReason::ContainsReviewOnly);
    h.dir("Library/Developer/Xcode/Archives");
    assert_review(
        &h,
        &h.path("Library/Developer/Xcode"),
        ReviewReason::ContainsReviewOnly,
    );
}

// ---------------------------------------------------------------------------
// Completeness: every entry in the code lists is exercised above, so an entry
// added to `protected.rs` without a test fails here.
// ---------------------------------------------------------------------------

#[test]
fn every_hard_blocked_entry_is_refused() {
    for entry in protected::HARD_BLOCKED {
        let h = FakeHome::new();
        let rel = entry.components.join("/");
        let path = if rel.is_empty() {
            h.home.clone()
        } else {
            h.dir(&rel)
        };
        assert_hard(&h, &path, entry.reason);
    }
}

#[test]
fn every_review_only_entry_is_review_only() {
    for entry in protected::REVIEW_ONLY {
        let h = FakeHome::new();
        let path = h.file(&format!("{}/item", entry.components.join("/")));
        let [unselected, selected] = h.check_both(&path);
        if entry.reason.deletable_in_v1() {
            assert_eq!(
                unselected,
                Err(Refusal::NeedsIndividualSelection(entry.reason)),
                "{}",
                path.display()
            );
            assert!(selected.is_ok(), "{}", path.display());
        } else {
            assert_eq!(unselected, Err(Refusal::NotRemovableInV1(entry.reason)));
            assert_eq!(selected, Err(Refusal::NotRemovableInV1(entry.reason)));
        }
    }
}

#[test]
fn entry_counts_match_doc() {
    // 1 home + 9 main folders + Keychains, Mail, Messages, Mobile Documents,
    // CloudStorage + 7 credentials + Provisioning Profiles.
    assert_eq!(protected::HARD_BLOCKED.len(), 23);
    // Backup, Attachments, 3 personal folders, 2 leftovers, AVD, Archives,
    // Claude, Codex, 2 Cursor.
    assert_eq!(protected::REVIEW_ONLY.len(), 13);
}
