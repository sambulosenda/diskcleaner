//! Well-known paths inside a fixture home, relative to its root.
//!
//! Tests should use these names instead of repeating strings, so a layout
//! change shows up as a compile error rather than a silently passing test.

/// iPhone backups: one folder per device id.
pub const MOBILESYNC_BACKUP: &str = "Library/Application Support/MobileSync/Backup";
/// The two fake device ids, each with an `Info.plist`.
pub const BACKUP_DEVICE_IDS: [&str; 2] = ["00008110-001A2B3C4D5E6F70", "00008030-000B1C2D3E4F5A60"];
/// Downloads folder.
pub const DOWNLOADS: &str = "Downloads";
/// Disk image installer in Downloads.
pub const DOWNLOAD_DMG: &str = "Downloads/Installer-2.4.dmg";
/// Package installer in Downloads.
pub const DOWNLOAD_PKG: &str = "Downloads/PrinterDriver.pkg";
/// Zip archive in Downloads.
pub const DOWNLOAD_ZIP: &str = "Downloads/Photos-from-trip.zip";
/// Ordinary document in Downloads.
pub const DOWNLOAD_NORMAL: &str = "Downloads/Meeting notes.txt";
/// App caches root.
pub const CACHES: &str = "Library/Caches";
/// The three app cache folders (bundle ids).
pub const CACHE_APP_IDS: [&str; 3] = [
    "com.google.Chrome",
    "com.spotify.client",
    "com.tinyspeck.slackmacgap",
];
/// Photos library bundle (hard-blocked, everything inside it too).
pub const PHOTOS_LIBRARY: &str = "Pictures/Test.photoslibrary";
/// Messages attachments (review-only, never removed in v1).
pub const MESSAGES_ATTACHMENTS: &str = "Library/Messages/Attachments";
/// Safari's cache folder.
pub const SAFARI_CACHE: &str = "Library/Caches/com.apple.Safari";
/// Google Chrome's main cache folder (the large one, separate from the
/// small `com.google.Chrome` folder above).
pub const CHROME_CACHE: &str = "Library/Caches/Google/Chrome";
/// Firefox's cache folder.
pub const FIREFOX_CACHE: &str = "Library/Caches/Firefox";
/// Slack's cache folder (direct download version).
pub const SLACK_CACHE: &str = "Library/Application Support/Slack/Cache";
/// Slack data next to the cache that no cache rule may match.
pub const SLACK_LOCAL_STORAGE: &str = "Library/Application Support/Slack/Local Storage";
/// The user's log folder.
pub const USER_LOGS: &str = "Library/Logs";
/// The two app log folders inside it.
pub const USER_LOG_FOLDERS: [&str; 2] = ["DiagnosticReports", "Zoom"];
/// An iPhone software update file Finder downloaded.
pub const IPHONE_SOFTWARE_UPDATE: &str =
    "Library/iTunes/iPhone Software Updates/iPhone15,2_18.1_22B83_Restore.ipsw";

/// Xcode DerivedData root.
pub const DERIVED_DATA: &str = "Library/Developer/Xcode/DerivedData";
/// DerivedData folder whose `WorkspacePath` exists in the fixture.
pub const DERIVED_DATA_LIVE_A: &str =
    "Library/Developer/Xcode/DerivedData/Weather-abcdefghijklmnopqrstuvwxyzab";
/// Second DerivedData folder whose `WorkspacePath` exists in the fixture.
pub const DERIVED_DATA_LIVE_B: &str =
    "Library/Developer/Xcode/DerivedData/Notes-bcdefghijklmnopqrstuvwxyzabc";
/// DerivedData folder whose `WorkspacePath` does not exist (orphaned).
pub const DERIVED_DATA_ORPHAN: &str =
    "Library/Developer/Xcode/DerivedData/OldGame-cdefghijklmnopqrstuvwxyzabcd";
/// The Xcode projects the live DerivedData folders point at.
pub const XCODE_PROJECTS: [&str; 2] = [
    "Developer/Weather/Weather.xcodeproj",
    "Developer/Notes/Notes.xcworkspace",
];
/// The workspace path the orphaned DerivedData points at (never created).
pub const XCODE_MISSING_PROJECT: &str = "Developer/OldGame/OldGame.xcodeproj";
/// iOS DeviceSupport root.
pub const IOS_DEVICE_SUPPORT: &str = "Library/Developer/Xcode/iOS DeviceSupport";
/// The three DeviceSupport versions.
pub const IOS_DEVICE_SUPPORT_VERSIONS: [&str; 3] =
    ["16.7.10 (20H350)", "17.6.1 (21G101)", "18.1 (22B83)"];
/// CoreSimulator devices root.
pub const SIM_DEVICES: &str = "Library/Developer/CoreSimulator/Devices";
/// The three simulator device UDIDs.
pub const SIM_DEVICE_IDS: [&str; 3] = [
    "1A2B3C4D-0000-4000-8000-000000000001",
    "1A2B3C4D-0000-4000-8000-000000000002",
    "1A2B3C4D-0000-4000-8000-000000000003",
];
/// The simulator device marked unavailable in its `device.plist`.
pub const SIM_DEVICE_UNAVAILABLE: &str = "1A2B3C4D-0000-4000-8000-000000000003";
/// watchOS DeviceSupport root.
pub const WATCHOS_DEVICE_SUPPORT: &str = "Library/Developer/Xcode/watchOS DeviceSupport";
/// The one watchOS DeviceSupport version.
pub const WATCHOS_DEVICE_SUPPORT_VERSIONS: [&str; 1] = ["Watch6,1 10.6.1 (21U580)"];
/// CocoaPods download cache.
pub const COCOAPODS_CACHE: &str = "Library/Caches/CocoaPods";
/// The one Xcode archive (review-only).
pub const XCODE_ARCHIVE: &str =
    "Library/Developer/Xcode/Archives/2026-03-14/Weather 14-03-2026, 10.42.xcarchive";

/// Android SDK root.
pub const ANDROID_SDK: &str = "Library/Android/sdk";
/// System image API levels present.
pub const ANDROID_SYSTEM_IMAGE_APIS: [u32; 3] = [33, 34, 35];
/// Build tools versions present.
pub const ANDROID_BUILD_TOOLS: [&str; 2] = ["34.0.0", "35.0.0"];
/// NDK version present.
pub const ANDROID_NDK: &str = "26.3.11579264";
/// AVD folder (review-only).
pub const ANDROID_AVD: &str = ".android/avd";
/// The two AVD names; each has `<name>.avd/` and `<name>.ini`.
pub const ANDROID_AVD_NAMES: [&str; 2] = ["Pixel_7_API_34", "Pixel_Tablet_API_33"];
/// The two Android projects; both `build.gradle` files target API 34 only.
pub const ANDROID_PROJECTS: [&str; 2] = [
    "AndroidStudioProjects/Shopper",
    "AndroidStudioProjects/Tracker",
];
/// Gradle wrapper distributions root.
pub const GRADLE_DISTS: &str = ".gradle/wrapper/dists";
/// Gradle wrapper versions present.
pub const GRADLE_VERSIONS: [&str; 3] = ["gradle-8.5-bin", "gradle-8.7-bin", "gradle-8.9-all"];

/// The three JavaScript projects.
pub const JS_PROJECTS: [&str; 3] = ["projects/a", "projects/b", "projects/c"];
/// The stale project's `package.json`, dated 120 days before build time.
pub const JS_STALE_PACKAGE_JSON: &str = "projects/c/package.json";
/// How old the stale project's `package.json` is, in days.
pub const JS_STALE_AGE_DAYS: u64 = 120;
/// JavaScript projects in the other code folders the node_modules rules
/// cover, one each, with `node_modules`. The code one is a package inside a
/// monorepo, three levels down.
pub const JS_PROJECTS_ELSEWHERE: [&str; 6] = [
    "Developer/web-shop",
    "code/monorepo/packages/app",
    "dev/cli-tool",
    "src/api",
    "repos/site",
    "GitHub/acme/landing",
];
/// A package nested inside a project's `node_modules`, which belongs to the
/// outer `node_modules` item.
pub const JS_NESTED_MODULES: &str = "Developer/web-shop/node_modules/left-pad/node_modules";
/// JavaScript projects in Documents (two levels down) and straight in home.
pub const JS_PROJECTS_DOCS_AND_HOME: [&str; 2] = ["Documents/clients/site", "my-app"];
/// An editor extension with its own `node_modules`, in a hidden folder. No
/// rule may match it: removing it breaks the editor.
pub const JS_EDITOR_EXTENSION: &str = ".vscode/extensions/acme.helper-1.0.0";
/// A JavaScript project on the Desktop. No rule matches its `node_modules`
/// yet, on purpose: the Desktop holds personal files.
pub const JS_PROJECT_ON_DESKTOP: &str = "Desktop/old-site";
/// npm cache.
pub const NPM_CACHE: &str = ".npm";
/// npx tool cache, inside the npm folder.
pub const NPX_CACHE: &str = ".npm/_npx";
/// The one tool in the npx cache.
pub const NPX_TOOL: &str = ".npm/_npx/0a1b2c3d4e5f6a7b";
/// pnpm store.
pub const PNPM_STORE: &str = "Library/pnpm";
/// Yarn cache.
pub const YARN_CACHE: &str = "Library/Caches/Yarn";
/// Bun download cache.
pub const BUN_CACHE: &str = ".bun/install/cache";
/// Expo tools home folder.
pub const EXPO_HOME: &str = ".expo";
/// The Expo cache folders inside it.
pub const EXPO_CACHES: [&str; 2] = ["versions-cache", "ios-simulator-app-cache"];
/// Expo's sign-in state, which no cache rule may match.
pub const EXPO_STATE: &str = ".expo/state.json";

/// Trap: symlink to `/etc` from inside a cache folder.
pub const TRAP_SYMLINK_ETC: &str = "Library/Caches/com.google.Chrome/system-config";
/// Trap: symlink from a cache folder into `Documents/`.
pub const TRAP_SYMLINK_DOCUMENTS: &str = "Library/Caches/com.spotify.client/offline-docs";
/// Trap: first path of the hard link pair (a personal file).
pub const TRAP_HARD_LINK_A: &str = "Documents/Family video.mov";
/// Trap: second path of the hard link pair, same inode as the first.
pub const TRAP_HARD_LINK_B: &str = "Movies/Family video.mov";
/// Trap: git repository with `node_modules` beside `.git`.
pub const TRAP_GIT_DIR: &str = "projects/a/.git";

/// The `.gitignore` of the trap git repository; it ignores `node_modules`.
pub const TRAP_GIT_IGNORE: &str = "projects/a/.gitignore";
/// Trap: `node_modules` inside a git repository (the allowed child).
pub const TRAP_GIT_NODE_MODULES: &str = "projects/a/node_modules";
/// Trap: environment file with fake secrets (review-only).
pub const TRAP_ENV_FILE: &str = "projects/a/.env";
/// Trap: Android signing key (hard-blocked).
pub const TRAP_KEYSTORE: &str = "AndroidStudioProjects/Shopper/app/release.keystore";
/// Personal documents the Documents symlink trap points at.
pub const DOCUMENTS: &str = "Documents";

/// Gradle's shared cache (downloaded libraries and build results).
pub const GRADLE_CACHES: &str = ".gradle/caches";
/// Android Studio's cache folder for one version.
pub const ANDROID_STUDIO_CACHE: &str = "Library/Caches/Google/AndroidStudio2024.2";

/// Cargo's download cache.
pub const CARGO_REGISTRY_CACHE: &str = ".cargo/registry/cache";
/// Cargo's unpacked crates.
pub const CARGO_REGISTRY_SRC: &str = ".cargo/registry/src";
/// Tools installed with `cargo install`. No rule may match them.
pub const CARGO_BIN: &str = ".cargo/bin/cargo-watch";
/// Cargo's registry sign-in token file. No rule may match it.
pub const CARGO_CREDENTIALS: &str = ".cargo/credentials.toml";
/// A git dependency checkout, which is a git repository. No rule may match it.
pub const CARGO_GIT_CHECKOUT: &str = ".cargo/git/checkouts/tokio-1a2b3c4d5e6f7a8b/0123abc";
/// rustup toolchains root.
pub const RUSTUP_TOOLCHAINS: &str = ".rustup/toolchains";
/// The two installed toolchains.
pub const RUSTUP_TOOLCHAIN_NAMES: [&str; 2] =
    ["stable-aarch64-apple-darwin", "1.79.0-aarch64-apple-darwin"];
/// rustup's settings, which no rule may match.
pub const RUSTUP_SETTINGS: &str = ".rustup/settings.toml";
/// Rust projects, one in each code folder the `target` rules cover, in the
/// same order as the rules: projects, Developer, code, dev, src, repos,
/// GitHub, Documents and straight in home. Each has a Cargo `target` folder.
/// The Developer one is a git repository.
pub const RUST_PROJECTS: [&str; 9] = [
    "projects/rust-cli",
    "Developer/roomy-core",
    "code/engine",
    "dev/parser",
    "src/server",
    "repos/tools",
    "GitHub/acme/agent",
    "Documents/rust/book-examples",
    "rust-tool",
];
/// Trap: a personal folder that happens to be called `target`, with no
/// Cargo marker files. No rule may match it.
pub const TRAP_PLAIN_TARGET: &str = "Documents/Sales/target";
/// Trap: a `target` folder with Cargo's CACHEDIR.TAG but no
/// `.rustc_info.json`. No rule may match it.
pub const TRAP_HALF_TARGET: &str = "Developer/other-tool/target";

/// Claude Code's home folder.
pub const CLAUDE_HOME: &str = ".claude";
/// Claude Code conversations for one project folder.
pub const CLAUDE_CONVERSATIONS: &str = ".claude/projects/-Users-sam-Developer-roomy-core";
/// Claude Code's settings, which no rule may match.
pub const CLAUDE_SETTINGS: &str = ".claude/settings.json";
/// Claude Code's log folder.
pub const CLAUDE_LOGS: &str = "Library/Caches/claude-cli-nodejs";

/// Cursor's app data folder.
pub const CURSOR_DATA: &str = "Library/Application Support/Cursor";
/// VS Code's app data folder.
pub const VSCODE_DATA: &str = "Library/Application Support/Code";
/// The cache folders inside each of them that the editor rules match.
pub const EDITOR_CACHE_FOLDERS: [&str; 3] = ["Cache", "CachedData", "CachedExtensionVSIXs"];
/// Cursor's chat database for one workspace (review-only). No rule may match it.
pub const CURSOR_CHAT_DB: &str =
    "Library/Application Support/Cursor/User/workspaceStorage/4f1a2b3c/state.vscdb";
/// A JetBrains IDE cache folder for one version.
pub const JETBRAINS_CACHE: &str = "Library/Caches/JetBrains/RustRover2024.3";
