//! The other allow-listed tool commands of `docs/safety/allowed-commands.md`:
//!
//! | Id | Preview (read-only) | Command |
//! |---|---|---|
//! | `brew_cleanup` | `brew cleanup --dry-run` | `brew cleanup` |
//! | `xcrun_simctl_delete_unavailable` | `xcrun simctl list devices unavailable -j` | `xcrun simctl delete unavailable` |
//! | `sdkmanager_uninstall` | `sdkmanager --list_installed` | `sdkmanager --uninstall <package>` |
//!
//! They work the way the Docker clean-ups in [`crate::commands`] do:
//!
//! 1. [`preview_tool`] runs the read-only preview and keeps what it listed in
//!    a [`ToolPlan`]. The user sees that list before anything runs.
//! 2. [`run_tool`] takes the plan by value (so it is used once), refuses it if
//!    it is older than [`planner::PLAN_LIFETIME`], runs the preview again and
//!    refuses if anything is listed now that was not listed before. Only then
//!    does it run the command.
//!
//! For the Android SDK the user picks one package from the preview, and the
//! command removes only that one. The package id must exactly match one the
//! preview listed, and must still be installed when the preview runs again.
//! The command-line tools package itself is never offered: removing it would
//! remove `sdkmanager`.
//!
//! A Mac app starts with a short `PATH`, so [`ToolLocations::find`] looks for
//! each program in the places it is installed: Homebrew in `/opt/homebrew/bin`
//! or `/usr/local/bin`, `xcrun` with the Xcode found in `/Applications`, and
//! `sdkmanager` inside `~/Library/Android/sdk`, with Android Studio's own Java.
//!
//! Every program runs with an argument array, never through a shell, and
//! [`check_run_args`] refuses any argument list that is not exactly one of
//! the three commands. Sizes are measured by reading metadata only, without
//! following symlinks or crossing volumes, and hard links count once.

use std::collections::{BTreeSet, HashSet};
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use analysers::docker::{Output, RunError};

use crate::commands::{cut, run_program};

/// How long a read-only preview may take. `sdkmanager` can be slow to start.
pub const PREVIEW_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a command may take.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The `PATH` every tool runs with. Homebrew's own folders come first.
const BASE_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// Where the Android SDK lives, relative to the home folder. Android Studio
/// puts it here; Roomy looks nowhere else.
pub const ANDROID_SDK: &str = "Library/Android/sdk";

/// Where the simulators keep their data, relative to the home folder.
pub const SIMULATOR_DEVICES: &str = "Library/Developer/CoreSimulator/Devices";

/// A program one of the commands runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// Homebrew's `brew`.
    Brew,
    /// Apple's `xcrun`, which finds `simctl` in Xcode.
    Xcrun,
    /// The Android SDK's `sdkmanager`.
    Sdkmanager,
}

impl Tool {
    /// The program's name as typed.
    pub fn program(self) -> &'static str {
        match self {
            Self::Brew => "brew",
            Self::Xcrun => "xcrun",
            Self::Sdkmanager => "sdkmanager",
        }
    }

    /// Its name for people.
    fn name(self) -> &'static str {
        match self {
            Self::Brew => "Homebrew",
            Self::Xcrun => "Xcode",
            Self::Sdkmanager => "sdkmanager",
        }
    }

    /// What the user sees when it is missing.
    fn not_installed(self) -> &'static str {
        match self {
            Self::Brew => "Homebrew isn't installed, so there is nothing for it to clean up.",
            Self::Xcrun => "Xcode isn't installed in Applications, so there are no simulators to delete.",
            Self::Sdkmanager => "Android's sdkmanager isn't installed. Add \"Android SDK Command-line Tools\" in Android Studio's SDK Manager, then try again.",
        }
    }
}

/// One of the tool clean-ups on the allow-list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCleanup {
    /// `brew cleanup`: old versions and downloads Homebrew no longer needs.
    Homebrew,
    /// `xcrun simctl delete unavailable`: simulators whose iOS version is gone.
    UnavailableSimulators,
    /// `sdkmanager --uninstall <package>`: one installed SDK package.
    AndroidSdkPackage,
}

impl ToolCleanup {
    /// All three.
    pub const ALL: [ToolCleanup; 3] = [
        Self::Homebrew,
        Self::UnavailableSimulators,
        Self::AndroidSdkPackage,
    ];

    /// Its id in `docs/safety/allowed-commands.md`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Homebrew => "brew_cleanup",
            Self::UnavailableSimulators => "xcrun_simctl_delete_unavailable",
            Self::AndroidSdkPackage => "sdkmanager_uninstall",
        }
    }

    /// The program it runs.
    pub fn tool(self) -> Tool {
        match self {
            Self::Homebrew => Tool::Brew,
            Self::UnavailableSimulators => Tool::Xcrun,
            Self::AndroidSdkPackage => Tool::Sdkmanager,
        }
    }

    /// The read-only preview's arguments, exactly as the allow-list gives them.
    pub fn preview_args(self) -> &'static [&'static str] {
        match self {
            Self::Homebrew => &["cleanup", "--dry-run"],
            Self::UnavailableSimulators => &["simctl", "list", "devices", "unavailable", "-j"],
            Self::AndroidSdkPackage => &["--list_installed"],
        }
    }

    /// The command's arguments. `package` is used only by the Android SDK
    /// clean-up, and must already have been checked against the preview.
    pub fn run_args(self, package: Option<&str>) -> Vec<String> {
        let fixed: &[&str] = match self {
            Self::Homebrew => &["cleanup"],
            Self::UnavailableSimulators => &["simctl", "delete", "unavailable"],
            Self::AndroidSdkPackage => &["--uninstall"],
        };
        let mut args: Vec<String> = fixed.iter().map(|s| s.to_string()).collect();
        if self == Self::AndroidSdkPackage {
            args.push(package.unwrap_or_default().to_string());
        }
        args
    }

    /// The command as a person would type it, for History and the app.
    /// For the Android SDK without a package, shows `<package>`.
    pub fn command_line(self, package: Option<&str>) -> String {
        let mut args = self.run_args(package);
        if self == Self::AndroidSdkPackage && package.is_none() {
            if let Some(last) = args.last_mut() {
                *last = "<package>".to_string();
            }
        }
        format!("{} {}", self.tool().program(), args.join(" "))
    }
}

/// Whether `id` looks like an Android SDK package id, such as
/// `build-tools;34.0.0` or `system-images;android-34;google_apis;arm64-v8a`.
/// Never starts with `-`, so it can never be read as a flag.
pub fn is_sdk_package_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ';' | '.' | '_' | '-'))
}

/// Packages Roomy never removes: the command-line tools hold `sdkmanager`
/// itself, so removing them would leave the user unable to manage the SDK.
pub fn is_kept_sdk_package(id: &str) -> bool {
    id == "cmdline-tools" || id.starts_with("cmdline-tools;") || id == "tools"
}

/// Refuses any argument list that is not exactly the command for `cleanup`.
/// For the Android SDK, the package must be a valid id that Roomy may
/// remove. Called just before running.
pub fn check_run_args(cleanup: ToolCleanup, args: &[String]) -> Result<(), ToolError> {
    let ok = match cleanup {
        ToolCleanup::AndroidSdkPackage => match args {
            [flag, package] => {
                flag == "--uninstall" && is_sdk_package_id(package) && !is_kept_sdk_package(package)
            }
            _ => false,
        },
        _ => args == cleanup.run_args(None).as_slice(),
    };
    if ok {
        Ok(())
    } else {
        Err(ToolError::NotAllowed)
    }
}

/// Where each program is on this Mac. `None` means not installed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolLocations {
    /// The `brew` program.
    pub brew: Option<PathBuf>,
    /// The `xcrun` program.
    pub xcrun: Option<PathBuf>,
    /// The Xcode `Contents/Developer` folder `xcrun` should use.
    pub developer_dir: Option<PathBuf>,
    /// The `sdkmanager` program.
    pub sdkmanager: Option<PathBuf>,
    /// A Java home for `sdkmanager`, usually Android Studio's own.
    pub java_home: Option<PathBuf>,
}

impl ToolLocations {
    /// Looks in the usual places for `home`. Reads metadata only.
    pub fn find(home: &Path) -> Self {
        let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file());
        let developer_dir = [
            PathBuf::from("/Applications/Xcode.app"),
            PathBuf::from("/Applications/Xcode-beta.app"),
            home.join("Applications/Xcode.app"),
        ]
        .into_iter()
        .map(|app| app.join("Contents/Developer"))
        .find(|dev| dev.join("usr/bin/simctl").is_file());
        let xcrun = Some(PathBuf::from("/usr/bin/xcrun")).filter(|p| p.is_file());
        let java_home = [
            PathBuf::from("/Applications/Android Studio.app/Contents/jbr/Contents/Home"),
            PathBuf::from("/Applications/Android Studio.app/Contents/jre/Contents/Home"),
            home.join("Applications/Android Studio.app/Contents/jbr/Contents/Home"),
        ]
        .into_iter()
        .find(|j| j.join("bin/java").is_file());
        Self {
            brew,
            // Without Xcode, `xcrun simctl` would ask the user to install
            // developer tools, so Roomy treats it as not installed.
            xcrun: developer_dir.as_ref().and(xcrun),
            developer_dir,
            sdkmanager: find_sdkmanager(&home.join(ANDROID_SDK)),
            java_home,
        }
    }

    fn program(&self, tool: Tool) -> Option<&Path> {
        match tool {
            Tool::Brew => self.brew.as_deref(),
            Tool::Xcrun => self.xcrun.as_deref(),
            Tool::Sdkmanager => self.sdkmanager.as_deref(),
        }
    }
}

/// `sdkmanager` inside the SDK: `cmdline-tools/latest`, else the newest
/// other `cmdline-tools` version, else the old `tools` folder. Only a
/// regular file counts, never a symlink.
fn find_sdkmanager(sdk: &Path) -> Option<PathBuf> {
    let is_program = |p: &Path| {
        std::fs::symlink_metadata(p)
            .map(|m| m.file_type().is_file())
            .unwrap_or(false)
    };
    let latest = sdk.join("cmdline-tools/latest/bin/sdkmanager");
    if is_program(&latest) {
        return Some(latest);
    }
    let mut versions: Vec<PathBuf> = std::fs::read_dir(sdk.join("cmdline-tools"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("bin/sdkmanager"))
        .filter(|p| is_program(p))
        .collect();
    versions.sort();
    if let Some(newest) = versions.pop() {
        return Some(newest);
    }
    Some(sdk.join("tools/bin/sdkmanager")).filter(|p| is_program(p))
}

/// Runs one of the three programs. A trait so tests can stand in for them.
pub trait ToolRunner {
    /// Runs `<tool> <args>` and waits at most `timeout`.
    fn run(&self, tool: Tool, args: &[&str], timeout: Duration) -> Result<Output, RunError>;
}

/// The real programs, at the [`ToolLocations`] given.
#[derive(Debug, Clone)]
pub struct RealToolRunner {
    locations: ToolLocations,
}

impl RealToolRunner {
    /// Uses exactly these locations.
    pub fn new(locations: ToolLocations) -> Self {
        Self { locations }
    }
}

impl ToolRunner for RealToolRunner {
    fn run(&self, tool: Tool, args: &[&str], timeout: Duration) -> Result<Output, RunError> {
        let program = self.locations.program(tool).ok_or(RunError::NotFound)?;
        let mut command = Command::new(program);
        command.args(args);
        let mut path = OsString::from(BASE_PATH);
        match tool {
            Tool::Brew => {
                // Nothing here may update Homebrew or send analytics.
                command
                    .env("HOMEBREW_NO_AUTO_UPDATE", "1")
                    .env("HOMEBREW_NO_ANALYTICS", "1")
                    .env("HOMEBREW_NO_ENV_HINTS", "1")
                    .env("HOMEBREW_NO_COLOR", "1");
            }
            Tool::Xcrun => {
                if let Some(dev) = &self.locations.developer_dir {
                    command.env("DEVELOPER_DIR", dev);
                }
            }
            Tool::Sdkmanager => {
                if let Some(java) = &self.locations.java_home {
                    command.env("JAVA_HOME", java);
                    let mut with_java = java.join("bin").into_os_string();
                    with_java.push(":");
                    with_java.push(&path);
                    path = with_java;
                }
            }
        }
        command.env("PATH", path);
        run_program(command, b"", timeout, tool.name())
    }
}

/// Why a tool clean-up was not previewed or not run. Plain words, no paths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    /// The program is not installed.
    #[error("{}", .0.not_installed())]
    NotInstalled(Tool),
    /// `sdkmanager` could not find Java.
    #[error("Android's sdkmanager needs Java to run. Install Android Studio, which includes it, then try again.")]
    NeedsJava,
    /// The plan is older than the plan lifetime, or from the future.
    #[error("This review has expired, so please review again.")]
    Expired,
    /// Something is listed now that the user did not see, or the chosen
    /// package is no longer installed.
    #[error("Something changed since the review, so nothing was removed. Please review again.")]
    Changed,
    /// The preview listed nothing, so there is nothing to run.
    #[error("There is nothing to remove.")]
    NothingToClean,
    /// The package was not one the preview listed.
    #[error("That package wasn't in the review, so nothing was removed. Please review again.")]
    NotInPreview,
    /// The arguments are not one of the allow-listed commands. A bug.
    #[error("{product} refused to run this command.", product = brand::PRODUCT_NAME)]
    NotAllowed,
    /// The preview could not be read, so Roomy did not act on it.
    #[error("{product} could not read the tool's answer, so nothing was removed.", product = brand::PRODUCT_NAME)]
    Unreadable,
    /// The program took too long.
    #[error("The tool took too long to answer. It may still be finishing.")]
    TimedOut,
    /// The program reported a problem.
    #[error("{reason}")]
    Failed {
        /// Plain words.
        reason: String,
    },
}

/// One thing a clean-up would remove, as its preview listed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolItem {
    /// What the tool calls it: a path for Homebrew, a device id for a
    /// simulator, a package id for the Android SDK.
    pub id: String,
    /// A short name for people.
    pub title: String,
    /// One more line: a folder, an iOS version, a package version.
    pub detail: String,
    /// Bytes on disk, measured or as the tool reports it. An estimate.
    pub size_bytes: u64,
}

/// A previewed clean-up, waiting for the user. Single use: [`run_tool`]
/// takes it by value. Deliberately not `Clone`.
#[derive(Debug, PartialEq, Eq)]
pub struct ToolPlan {
    cleanup: ToolCleanup,
    made_at: SystemTime,
    items: Vec<ToolItem>,
    total_bytes: u64,
}

impl ToolPlan {
    /// Which clean-up this is.
    pub fn cleanup(&self) -> ToolCleanup {
        self.cleanup
    }

    /// What the preview listed.
    pub fn items(&self) -> &[ToolItem] {
        &self.items
    }

    /// The bytes the clean-up would free, as far as Roomy can tell.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

/// What a clean-up did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRun {
    /// Which clean-up ran.
    pub cleanup: ToolCleanup,
    /// The command as typed, for History.
    pub command_line: String,
    /// Whether the program exited with status 0.
    pub success: bool,
    /// Bytes freed: Homebrew's own figure, or for the others the size the
    /// preview measured, when the program succeeded.
    pub reclaimed_bytes: Option<u64>,
    /// What the program printed, cut to [`crate::commands::KEPT_OUTPUT_BYTES`].
    pub stdout: String,
    /// What it printed on stderr, cut the same way.
    pub stderr: String,
}

/// Turns a failed or unfinished run into the error the user sees.
fn run_failure(tool: Tool, err: RunError) -> ToolError {
    match err {
        RunError::NotFound => ToolError::NotInstalled(tool),
        RunError::TimedOut => ToolError::TimedOut,
        RunError::Other(reason) => ToolError::Failed { reason },
    }
}

/// Whether `sdkmanager`'s output says it could not find Java.
fn needs_java(out: &Output) -> bool {
    let text = format!("{}\n{}", out.stdout, out.stderr);
    text.contains("Unable to locate a Java Runtime")
        || text.contains("JAVA_HOME is not set")
        || text.contains("JAVA_HOME is set to an invalid directory")
        || text.contains("java: command not found")
}

/// Whether `xcrun`'s output says it could not find `simctl`.
fn simctl_missing(out: &Output) -> bool {
    out.stderr.contains("unable to find utility")
        || out.stderr.contains("invalid active developer path")
}

/// The problem a failed preview reports, in plain words.
fn preview_failed(tool: Tool, out: &Output) -> ToolError {
    if tool == Tool::Sdkmanager && needs_java(out) {
        return ToolError::NeedsJava;
    }
    if tool == Tool::Xcrun && simctl_missing(out) {
        return ToolError::NotInstalled(Tool::Xcrun);
    }
    tracing::warn!(tool = tool.program(), stderr = %out.stderr.trim(), "tool preview failed");
    ToolError::Failed {
        reason: format!(
            concat!("{} reported a problem, so ", brand::name!(), " could not list what it would remove."),
            tool.name()
        ),
    }
}

/// Runs a read-only preview and reads what it listed.
fn list(
    runner: &dyn ToolRunner,
    cleanup: ToolCleanup,
    home: &Path,
) -> Result<(Vec<ToolItem>, u64), ToolError> {
    let tool = cleanup.tool();
    let out = runner
        .run(tool, cleanup.preview_args(), PREVIEW_TIMEOUT)
        .map_err(|e| run_failure(tool, e))?;
    if !out.success {
        return Err(preview_failed(tool, &out));
    }
    match cleanup {
        ToolCleanup::Homebrew => {
            let dry = parse_brew_dry_run(&out.stdout)?;
            let items = dry
                .removals
                .into_iter()
                .map(|r| ToolItem {
                    title: last_component(&r.path),
                    detail: parent_for_people(&r.path, home),
                    id: r.path,
                    size_bytes: r.size_bytes,
                })
                .collect::<Vec<_>>();
            let summed = items
                .iter()
                .fold(0u64, |s, i| s.saturating_add(i.size_bytes));
            Ok((items, dry.total_bytes.unwrap_or(summed)))
        }
        ToolCleanup::UnavailableSimulators => {
            let devices = parse_unavailable_simulators(&out.stdout)?;
            let folder = home.join(SIMULATOR_DEVICES);
            let items: Vec<ToolItem> = devices
                .into_iter()
                .map(|d| {
                    let measured = folder_size(&folder.join(&d.udid));
                    ToolItem {
                        size_bytes: if measured > 0 {
                            measured
                        } else {
                            d.data_bytes.unwrap_or(0)
                        },
                        title: d.name,
                        detail: d.runtime,
                        id: d.udid,
                    }
                })
                .collect();
            let total = items
                .iter()
                .fold(0u64, |s, i| s.saturating_add(i.size_bytes));
            Ok((items, total))
        }
        ToolCleanup::AndroidSdkPackage => {
            let packages = parse_sdk_installed(&out.stdout)?;
            let sdk = home.join(ANDROID_SDK);
            let items: Vec<ToolItem> = packages
                .into_iter()
                .filter(|p| !is_kept_sdk_package(&p.id))
                .map(|p| ToolItem {
                    size_bytes: p
                        .location
                        .as_deref()
                        .and_then(|l| safe_relative(l))
                        .map(|l| folder_size(&sdk.join(l)))
                        .unwrap_or(0),
                    title: if p.description.is_empty() {
                        p.id.clone()
                    } else {
                        p.description
                    },
                    detail: format!("{}, version {}", p.id, p.version),
                    id: p.id,
                })
                .collect();
            let total = items
                .iter()
                .fold(0u64, |s, i| s.saturating_add(i.size_bytes));
            Ok((items, total))
        }
    }
}

/// The dry run: lists what `cleanup` would remove, with the read-only
/// preview from the allow-list. Changes nothing. `home` is used only to
/// measure sizes and show paths.
pub fn preview_tool(
    runner: &dyn ToolRunner,
    cleanup: ToolCleanup,
    home: &Path,
    now: SystemTime,
) -> Result<ToolPlan, ToolError> {
    let (items, total_bytes) = list(runner, cleanup, home)?;
    Ok(ToolPlan {
        cleanup,
        made_at: now,
        items,
        total_bytes,
    })
}

/// Carries out a previewed clean-up, once.
///
/// `package` is required for the Android SDK, and must be one the preview
/// listed; it must be `None` for the others. Refuses, without running
/// anything, if the plan has expired, if the preview listed nothing, or if a
/// fresh preview lists anything the user did not see (for the Android SDK:
/// if the package is no longer installed).
pub fn run_tool(
    runner: &dyn ToolRunner,
    plan: ToolPlan,
    package: Option<&str>,
    home: &Path,
    now: SystemTime,
) -> Result<ToolRun, ToolError> {
    match now.duration_since(plan.made_at) {
        Ok(age) if age < planner::PLAN_LIFETIME => {}
        _ => return Err(ToolError::Expired),
    }
    if plan.items.is_empty() {
        return Err(ToolError::NothingToClean);
    }
    let cleanup = plan.cleanup;
    let chosen: Option<&ToolItem> = match (cleanup, package) {
        (ToolCleanup::AndroidSdkPackage, Some(id)) => Some(
            plan.items
                .iter()
                .find(|i| i.id == id)
                .ok_or(ToolError::NotInPreview)?,
        ),
        (ToolCleanup::AndroidSdkPackage, None) => return Err(ToolError::NotInPreview),
        (_, Some(_)) => return Err(ToolError::NotAllowed),
        (_, None) => None,
    };
    let args = cleanup.run_args(chosen.map(|i| i.id.as_str()));
    check_run_args(cleanup, &args)?;

    let (now_listed, _) = list(runner, cleanup, home)?;
    let now_ids: BTreeSet<&str> = now_listed.iter().map(|i| i.id.as_str()).collect();
    match chosen {
        Some(item) => {
            if !now_ids.contains(item.id.as_str()) {
                return Err(ToolError::Changed);
            }
        }
        None => {
            let seen: BTreeSet<&str> = plan.items.iter().map(|i| i.id.as_str()).collect();
            if now_ids.iter().any(|id| !seen.contains(id)) {
                return Err(ToolError::Changed);
            }
            if now_ids.is_empty() {
                return Err(ToolError::NothingToClean);
            }
        }
    }

    let command_line = cleanup.command_line(chosen.map(|i| i.id.as_str()));
    tracing::info!(command = %command_line, "running tool clean-up");
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let tool = cleanup.tool();
    let out = runner
        .run(tool, &arg_refs, RUN_TIMEOUT)
        .map_err(|e| run_failure(tool, e))?;
    if !out.success && tool == Tool::Sdkmanager && needs_java(&out) {
        return Err(ToolError::NeedsJava);
    }
    let reclaimed_bytes = match cleanup {
        ToolCleanup::Homebrew => brew_freed(&out.stdout),
        ToolCleanup::UnavailableSimulators => out.success.then_some(plan.total_bytes),
        ToolCleanup::AndroidSdkPackage => chosen.map(|i| i.size_bytes).filter(|_| out.success),
    };
    Ok(ToolRun {
        cleanup,
        command_line,
        success: out.success,
        reclaimed_bytes,
        stdout: cut(out.stdout),
        stderr: cut(out.stderr),
    })
}

// ---------------------------------------------------------------------------
// Reading the previews. Pure functions.
// ---------------------------------------------------------------------------

/// One line of `brew cleanup --dry-run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrewRemoval {
    /// The path Homebrew would remove.
    pub path: String,
    /// Its size, from Homebrew's own figure, or 0 if it gave none.
    pub size_bytes: u64,
}

/// Everything `brew cleanup --dry-run` said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrewDryRun {
    /// Each "Would remove" line.
    pub removals: Vec<BrewRemoval>,
    /// The "would free approximately" figure, if Homebrew printed one.
    pub total_bytes: Option<u64>,
}

/// Reads the output of `brew cleanup --dry-run`:
///
/// ```text
/// Would remove: /Users/me/Library/Caches/Homebrew/wget--1.21.bottle.tar.gz (1.5MB)
/// Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,104 files, 61.2MB)
/// ==> This operation would free approximately 62.7MB of disk space.
/// ```
///
/// Other lines (warnings, notes) are ignored. A "Would remove" line without
/// a path is an error, so Roomy never acts on output it misread.
pub fn parse_brew_dry_run(stdout: &str) -> Result<BrewDryRun, ToolError> {
    let mut dry = BrewDryRun::default();
    for line in stdout.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Would remove") {
            // "Would remove: /path (size)" or "Would remove (empty directory): /path".
            let (_, rest) = rest.split_once(": ").ok_or(ToolError::Unreadable)?;
            let (path, size) = split_brew_size(rest.trim());
            if !path.starts_with('/') {
                return Err(ToolError::Unreadable);
            }
            dry.removals.push(BrewRemoval {
                path: path.to_string(),
                size_bytes: size.unwrap_or(0),
            });
        } else if let Some(rest) = line.strip_prefix("==> This operation would free approximately ")
        {
            dry.total_bytes = rest.split_whitespace().next().and_then(parse_brew_size);
        }
    }
    Ok(dry)
}

/// Splits "/path (34 files, 1.2MB)" into the path and its size.
fn split_brew_size(text: &str) -> (&str, Option<u64>) {
    if let (Some(open), true) = (text.rfind(" ("), text.ends_with(')')) {
        let inside = &text[open + 2..text.len() - 1];
        let last = inside.rsplit(", ").next().unwrap_or(inside);
        if let Some(size) = parse_brew_size(last) {
            return (&text[..open], Some(size));
        }
    }
    (text, None)
}

/// Bytes freed, from "==> This operation has freed approximately 1.2GB of disk space."
fn brew_freed(stdout: &str) -> Option<u64> {
    stdout.lines().rev().find_map(|l| {
        l.trim()
            .strip_prefix("==> This operation has freed approximately ")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(parse_brew_size)
    })
}

/// Reads a Homebrew size such as "1.2MB", "512B" or "3GB". Homebrew counts
/// in steps of 1,024.
pub fn parse_brew_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| c.is_ascii_alphabetic())?;
    let (number, unit) = text.split_at(split);
    let number: f64 = number.trim().replace(',', "").parse().ok()?;
    if !number.is_finite() || number < 0.0 {
        return None;
    }
    let scale: f64 = match unit {
        "B" => 1.0,
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        "TB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let bytes = (number * scale).round();
    (bytes < u64::MAX as f64).then_some(bytes as u64)
}

/// One simulator `simctl` says is unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableSimulator {
    /// The device id (a UUID).
    pub udid: String,
    /// Its name, such as "iPhone 14".
    pub name: String,
    /// The system it was made for, such as "iOS 16.0".
    pub runtime: String,
    /// `dataPathSize`, when this Xcode reports it.
    pub data_bytes: Option<u64>,
}

/// Reads the output of `xcrun simctl list devices unavailable -j`. A device
/// marked available is skipped: `simctl delete unavailable` leaves it alone.
pub fn parse_unavailable_simulators(stdout: &str) -> Result<Vec<UnavailableSimulator>, ToolError> {
    let value: serde_json::Value = serde_json::from_str(stdout).map_err(|e| {
        tracing::warn!(error = %e, "simctl preview not understood");
        ToolError::Unreadable
    })?;
    let runtimes = value
        .get("devices")
        .and_then(|d| d.as_object())
        .ok_or(ToolError::Unreadable)?;
    let mut found = Vec::new();
    for (runtime, devices) in runtimes {
        for device in devices.as_array().ok_or(ToolError::Unreadable)? {
            if device.get("isAvailable").and_then(|a| a.as_bool()) == Some(true) {
                continue;
            }
            let udid = device
                .get("udid")
                .and_then(|u| u.as_str())
                .filter(|u| is_udid(u))
                .ok_or(ToolError::Unreadable)?;
            found.push(UnavailableSimulator {
                udid: udid.to_string(),
                name: device
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("Simulator")
                    .to_string(),
                runtime: runtime_for_people(runtime),
                data_bytes: device.get("dataPathSize").and_then(|s| s.as_u64()),
            });
        }
    }
    found.sort_by(|a, b| a.runtime.cmp(&b.runtime).then(a.name.cmp(&b.name)));
    Ok(found)
}

/// Whether `id` is a UUID: 36 characters of hex digits and dashes.
fn is_udid(id: &str) -> bool {
    id.len() == 36
        && id.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// "com.apple.CoreSimulator.SimRuntime.iOS-16-0" as "iOS 16.0".
fn runtime_for_people(runtime: &str) -> String {
    let short = runtime.rsplit('.').next().unwrap_or(runtime);
    match short.split_once('-') {
        Some((os, version)) => format!("{os} {}", version.replace('-', ".")),
        None => short.to_string(),
    }
}

/// One row of `sdkmanager --list_installed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkPackage {
    /// The package id ("Path"), such as `build-tools;34.0.0`.
    pub id: String,
    /// Its version.
    pub version: String,
    /// Its description, such as "Android SDK Build-Tools 34".
    pub description: String,
    /// Its folder inside the SDK, when `sdkmanager` prints one.
    pub location: Option<String>,
}

/// Reads the output of `sdkmanager --list_installed`:
///
/// ```text
/// Installed packages:
///   Path               | Version | Description                | Location
///   -------            | ------- | -------                    | -------
///   build-tools;34.0.0 | 34.0.0  | Android SDK Build-Tools 34 | build-tools/34.0.0
/// ```
///
/// Only rows after "Installed packages:" are read, and only until another
/// heading. A row whose id does not look like a package id is an error.
pub fn parse_sdk_installed(stdout: &str) -> Result<Vec<SdkPackage>, ToolError> {
    let mut packages = Vec::new();
    let mut in_installed = false;
    // sdkmanager draws its progress bar with carriage returns.
    for raw in stdout.split(['\n', '\r']) {
        let line = raw.trim();
        if line.ends_with(':') && !line.contains('|') {
            in_installed = line.eq_ignore_ascii_case("Installed packages:");
            continue;
        }
        if !in_installed || !line.contains('|') {
            continue;
        }
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        if cols.len() < 3 || cols[0] == "Path" || cols[0].starts_with("---") {
            continue;
        }
        if !is_sdk_package_id(cols[0]) {
            return Err(ToolError::Unreadable);
        }
        packages.push(SdkPackage {
            id: cols[0].to_string(),
            version: cols[1].to_string(),
            description: cols[2].to_string(),
            location: cols.get(3).filter(|l| !l.is_empty()).map(|l| l.to_string()),
        });
    }
    Ok(packages)
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

/// `location` if it is a plain relative path with no `..`, for joining to
/// the SDK folder.
fn safe_relative(location: &str) -> Option<&Path> {
    let path = Path::new(location);
    path.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(path)
        .filter(|p| p.components().next().is_some())
}

/// The last part of a path, for a title.
fn last_component(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// The folder holding `path`, with the home folder shown as `~`.
fn parent_for_people(path: &str, home: &Path) -> String {
    let parent = Path::new(path).parent().unwrap_or(Path::new("/"));
    match parent.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => parent.display().to_string(),
    }
}

/// Bytes on disk under `path`, read with metadata only. Never follows a
/// symlink, never leaves the volume `path` is on, and counts a hard-linked
/// file once. 0 if `path` is missing.
pub fn folder_size(path: &Path) -> u64 {
    let Ok(top) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    let device = top.dev();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut total = 0u64;
    let mut stack = vec![(path.to_path_buf(), top)];
    while let Some((here, meta)) = stack.pop() {
        if meta.dev() != device || meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            for entry in std::fs::read_dir(&here).into_iter().flatten().flatten() {
                // `DirEntry::metadata` does not follow symlinks.
                if let Ok(m) = entry.metadata() {
                    stack.push((entry.path(), m));
                }
            }
        } else if meta.is_file() {
            if meta.nlink() > 1 && !seen.insert((meta.dev(), meta.ino())) {
                continue;
            }
            total = total.saturating_add(meta.blocks().saturating_mul(512));
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// Answers each call with the next recorded output and remembers the calls.
    #[derive(Default)]
    struct Recorded {
        answers: RefCell<VecDeque<Result<Output, RunError>>>,
        calls: RefCell<Vec<(Tool, Vec<String>)>>,
    }

    impl Recorded {
        fn answering(answers: Vec<Result<Output, RunError>>) -> Self {
            Self {
                answers: RefCell::new(answers.into()),
                calls: RefCell::default(),
            }
        }
        fn calls(&self) -> Vec<(Tool, Vec<String>)> {
            self.calls.borrow().clone()
        }
    }

    impl ToolRunner for Recorded {
        fn run(&self, tool: Tool, args: &[&str], _: Duration) -> Result<Output, RunError> {
            self.calls
                .borrow_mut()
                .push((tool, args.iter().map(|s| s.to_string()).collect()));
            self.answers
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(RunError::Other("no more answers".into())))
        }
    }

    fn ok(stdout: &str) -> Result<Output, RunError> {
        Ok(Output {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn failed(stdout: &str, stderr: &str) -> Result<Output, RunError> {
        Ok(Output {
            success: false,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        })
    }

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000)
    }

    fn home() -> PathBuf {
        PathBuf::from("/Users/me")
    }

    const BREW_DRY: &str =
        "Would remove: /Users/me/Library/Caches/Homebrew/wget--1.21.bottle.tar.gz (1.5MB)
Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,104 files, 61.2MB)
Warning: Skipping python@3.12: most recent version 3.12.1 not installed
==> This operation would free approximately 62.7MB of disk space.
";
    const BREW_ONE: &str = "Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,104 files, 61.2MB)\n";
    const BREW_NEW: &str = "Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,104 files, 61.2MB)
Would remove: /opt/homebrew/Cellar/git/2.40.0 (1,600 files, 50MB)
";
    const BREW_DONE: &str = "Removing: /opt/homebrew/Cellar/node/20.1.0... (2,104 files, 61.2MB)
==> This operation has freed approximately 62.7MB of disk space.
";

    const SIMS: &str = r#"{
  "devices" : {
    "com.apple.CoreSimulator.SimRuntime.iOS-16-0" : [
      {
        "dataPath" : "/Users/me/Library/Developer/CoreSimulator/Devices/0A1B2C3D-0000-4000-8000-000000000001/data",
        "dataPathSize" : 524288000,
        "udid" : "0A1B2C3D-0000-4000-8000-000000000001",
        "isAvailable" : false,
        "availabilityError" : "runtime profile not found",
        "name" : "iPhone 14",
        "state" : "Shutdown"
      }
    ],
    "com.apple.CoreSimulator.SimRuntime.watchOS-9-0" : [
      {
        "udid" : "0A1B2C3D-0000-4000-8000-000000000002",
        "isAvailable" : false,
        "name" : "Apple Watch Series 8",
        "state" : "Shutdown"
      }
    ]
  }
}"#;
    const NO_SIMS: &str = r#"{"devices":{}}"#;

    const SDK: &str = "[=======================================] 100% Computing updates...\r
Installed packages:
  Path                 | Version | Description                    | Location
  -------              | ------- | -------                        | -------
  build-tools;33.0.1   | 33.0.1  | Android SDK Build-Tools 33.0.1 | build-tools/33.0.1
  build-tools;34.0.0   | 34.0.0  | Android SDK Build-Tools 34     | build-tools/34.0.0
  cmdline-tools;latest | 12.0    | Android SDK Command-line Tools | cmdline-tools/latest
  platforms;android-34 | 3       | Android SDK Platform 34        | platforms/android-34
";

    #[test]
    fn command_arrays_match_the_allow_list_exactly() {
        let ids: Vec<&str> = ToolCleanup::ALL.iter().map(|c| c.id()).collect();
        assert_eq!(
            ids,
            [
                "brew_cleanup",
                "xcrun_simctl_delete_unavailable",
                "sdkmanager_uninstall"
            ]
        );
        let preview =
            |c: ToolCleanup| format!("{} {}", c.tool().program(), c.preview_args().join(" "));
        assert_eq!(preview(ToolCleanup::Homebrew), "brew cleanup --dry-run");
        assert_eq!(
            preview(ToolCleanup::UnavailableSimulators),
            "xcrun simctl list devices unavailable -j"
        );
        assert_eq!(
            preview(ToolCleanup::AndroidSdkPackage),
            "sdkmanager --list_installed"
        );
        assert_eq!(ToolCleanup::Homebrew.command_line(None), "brew cleanup");
        assert_eq!(
            ToolCleanup::UnavailableSimulators.command_line(None),
            "xcrun simctl delete unavailable"
        );
        assert_eq!(
            ToolCleanup::AndroidSdkPackage.command_line(None),
            "sdkmanager --uninstall <package>"
        );
        assert_eq!(
            ToolCleanup::AndroidSdkPackage.command_line(Some("build-tools;34.0.0")),
            "sdkmanager --uninstall build-tools;34.0.0"
        );
    }

    #[test]
    fn ids_match_the_rules_allow_list() {
        for c in ToolCleanup::ALL {
            assert!(rules::commands::is_allowed(c.id()), "{}", c.id());
        }
    }

    #[test]
    fn check_run_args_accepts_only_the_exact_commands() {
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(
            check_run_args(ToolCleanup::Homebrew, &s(&["cleanup"])),
            Ok(())
        );
        assert_eq!(
            check_run_args(
                ToolCleanup::UnavailableSimulators,
                &s(&["simctl", "delete", "unavailable"])
            ),
            Ok(())
        );
        assert_eq!(
            check_run_args(
                ToolCleanup::AndroidSdkPackage,
                &s(&["--uninstall", "build-tools;34.0.0"])
            ),
            Ok(())
        );
        for (cleanup, args) in [
            (ToolCleanup::Homebrew, &["cleanup", "--prune=all"][..]),
            (ToolCleanup::Homebrew, &["cleanup", "-s"]),
            (ToolCleanup::Homebrew, &["uninstall", "node"]),
            (ToolCleanup::Homebrew, &[]),
            (
                ToolCleanup::UnavailableSimulators,
                &["simctl", "delete", "all"],
            ),
            (
                ToolCleanup::UnavailableSimulators,
                &["simctl", "erase", "all"],
            ),
            (ToolCleanup::AndroidSdkPackage, &["--uninstall"]),
            (
                ToolCleanup::AndroidSdkPackage,
                &["--uninstall", "--sdk_root=/"],
            ),
            (ToolCleanup::AndroidSdkPackage, &["--uninstall", "a", "b"]),
            (
                ToolCleanup::AndroidSdkPackage,
                &["--uninstall", "cmdline-tools;latest"],
            ),
            (
                ToolCleanup::AndroidSdkPackage,
                &["--uninstall", "platforms;android 34"],
            ),
            (ToolCleanup::AndroidSdkPackage, &["--install", "emulator"]),
        ] {
            assert_eq!(
                check_run_args(cleanup, &s(args)),
                Err(ToolError::NotAllowed),
                "{args:?}"
            );
        }
    }

    #[test]
    fn package_ids_never_look_like_flags() {
        assert!(is_sdk_package_id(
            "system-images;android-34;google_apis;arm64-v8a"
        ));
        assert!(is_sdk_package_id("emulator"));
        for bad in [
            "",
            "-x",
            "--uninstall",
            "a b",
            "a/../b",
            "a|b",
            "a$b",
            "a\nb",
        ] {
            assert!(!is_sdk_package_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn reads_brew_dry_run() {
        let dry = parse_brew_dry_run(BREW_DRY).unwrap();
        assert_eq!(dry.removals.len(), 2);
        assert_eq!(
            dry.removals[0].path,
            "/Users/me/Library/Caches/Homebrew/wget--1.21.bottle.tar.gz"
        );
        assert_eq!(dry.removals[0].size_bytes, 1_572_864);
        assert_eq!(dry.removals[1].path, "/opt/homebrew/Cellar/node/20.1.0");
        assert_eq!(
            dry.removals[1].size_bytes,
            (61.2f64 * 1_048_576.0).round() as u64
        );
        assert_eq!(
            dry.total_bytes,
            Some((62.7f64 * 1_048_576.0).round() as u64)
        );
        assert_eq!(parse_brew_dry_run("").unwrap(), BrewDryRun::default());
        assert_eq!(
            parse_brew_dry_run("Would remove (empty directory): /opt/homebrew/share/foo\n")
                .unwrap()
                .removals[0]
                .path,
            "/opt/homebrew/share/foo"
        );
        assert_eq!(
            parse_brew_dry_run("Would remove: oops"),
            Err(ToolError::Unreadable)
        );
        assert_eq!(
            parse_brew_dry_run("Would remove everything"),
            Err(ToolError::Unreadable)
        );
    }

    #[test]
    fn reads_brew_sizes() {
        assert_eq!(parse_brew_size("512B"), Some(512));
        assert_eq!(parse_brew_size("1KB"), Some(1024));
        assert_eq!(parse_brew_size("1.5GB"), Some(1_610_612_736));
        assert_eq!(parse_brew_size("2,104 files"), None);
        assert_eq!(parse_brew_size("MB"), None);
        assert_eq!(parse_brew_size("-1MB"), None);
    }

    #[test]
    fn reads_unavailable_simulators() {
        let sims = parse_unavailable_simulators(SIMS).unwrap();
        assert_eq!(sims.len(), 2);
        assert_eq!(sims[0].name, "iPhone 14");
        assert_eq!(sims[0].runtime, "iOS 16.0");
        assert_eq!(sims[0].data_bytes, Some(524_288_000));
        assert_eq!(sims[1].runtime, "watchOS 9.0");
        assert_eq!(sims[1].data_bytes, None);
        assert!(parse_unavailable_simulators(NO_SIMS).unwrap().is_empty());
        let available = SIMS.replacen("\"isAvailable\" : false", "\"isAvailable\" : true", 1);
        assert_eq!(parse_unavailable_simulators(&available).unwrap().len(), 1);
        assert_eq!(
            parse_unavailable_simulators("not json"),
            Err(ToolError::Unreadable)
        );
        let bad_id = SIMS.replace("0A1B2C3D-0000-4000-8000-000000000002", "../../etc");
        assert_eq!(
            parse_unavailable_simulators(&bad_id),
            Err(ToolError::Unreadable)
        );
    }

    #[test]
    fn reads_installed_sdk_packages() {
        let packages = parse_sdk_installed(SDK).unwrap();
        let ids: Vec<&str> = packages.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "build-tools;33.0.1",
                "build-tools;34.0.0",
                "cmdline-tools;latest",
                "platforms;android-34"
            ]
        );
        assert_eq!(packages[1].location.as_deref(), Some("build-tools/34.0.0"));
        assert_eq!(packages[1].description, "Android SDK Build-Tools 34");
        let with_available =
            format!("{SDK}Available Packages:\n  emulator | 34.1 | Android Emulator\n");
        assert_eq!(parse_sdk_installed(&with_available).unwrap().len(), 4);
        let bad = SDK.replace("platforms;android-34 ", "--sdk_root=/ ");
        assert_eq!(parse_sdk_installed(&bad), Err(ToolError::Unreadable));
        assert!(parse_sdk_installed("").unwrap().is_empty());
    }

    #[test]
    fn locations_outside_the_sdk_are_never_measured() {
        assert_eq!(
            safe_relative("build-tools/34.0.0"),
            Some(Path::new("build-tools/34.0.0"))
        );
        assert_eq!(safe_relative("../Documents"), None);
        assert_eq!(safe_relative("/Users/me"), None);
        assert_eq!(safe_relative(""), None);
    }

    #[test]
    fn brew_preview_lists_what_brew_would_remove() {
        let runner = Recorded::answering(vec![ok(BREW_DRY)]);
        let plan = preview_tool(&runner, ToolCleanup::Homebrew, &home(), now()).unwrap();
        assert_eq!(plan.items().len(), 2);
        assert_eq!(plan.items()[0].title, "wget--1.21.bottle.tar.gz");
        assert_eq!(plan.items()[0].detail, "~/Library/Caches/Homebrew");
        assert_eq!(plan.items()[1].detail, "/opt/homebrew/Cellar/node");
        assert_eq!(plan.total_bytes(), (62.7f64 * 1_048_576.0).round() as u64);
        assert_eq!(
            runner.calls(),
            [(Tool::Brew, vec!["cleanup".into(), "--dry-run".into()])]
        );
    }

    #[test]
    fn brew_run_previews_again_then_cleans_up() {
        let runner = Recorded::answering(vec![ok(BREW_DRY), ok(BREW_ONE), ok(BREW_DONE)]);
        let plan = preview_tool(&runner, ToolCleanup::Homebrew, &home(), now()).unwrap();
        let ran = run_tool(
            &runner,
            plan,
            None,
            &home(),
            now() + Duration::from_secs(30),
        )
        .unwrap();
        assert!(ran.success);
        assert_eq!(ran.command_line, "brew cleanup");
        assert_eq!(
            ran.reclaimed_bytes,
            Some((62.7f64 * 1_048_576.0).round() as u64)
        );
        let calls = runner.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1], calls[0], "the preview runs again first");
        assert_eq!(calls[2], (Tool::Brew, vec!["cleanup".to_string()]));
    }

    #[test]
    fn brew_run_refuses_when_something_new_appeared() {
        let runner = Recorded::answering(vec![ok(BREW_ONE), ok(BREW_NEW)]);
        let plan = preview_tool(&runner, ToolCleanup::Homebrew, &home(), now()).unwrap();
        assert_eq!(
            run_tool(&runner, plan, None, &home(), now()),
            Err(ToolError::Changed)
        );
        assert_eq!(runner.calls().len(), 2, "brew cleanup never ran");
    }

    #[test]
    fn brew_run_refuses_a_package_argument() {
        let runner = Recorded::answering(vec![ok(BREW_DRY)]);
        let plan = preview_tool(&runner, ToolCleanup::Homebrew, &home(), now()).unwrap();
        assert_eq!(
            run_tool(&runner, plan, Some("node"), &home(), now()),
            Err(ToolError::NotAllowed)
        );
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn missing_tools_say_so_in_plain_words() {
        for cleanup in ToolCleanup::ALL {
            let runner = Recorded::answering(vec![Err(RunError::NotFound)]);
            let err = preview_tool(&runner, cleanup, &home(), now()).unwrap_err();
            assert_eq!(err, ToolError::NotInstalled(cleanup.tool()));
            assert!(!err.to_string().contains('/'), "{err}");
        }
        let runner = Recorded::answering(vec![failed(
            "",
            "xcrun: error: unable to find utility \"simctl\", not a developer tool or in PATH",
        )]);
        assert_eq!(
            preview_tool(&runner, ToolCleanup::UnavailableSimulators, &home(), now()),
            Err(ToolError::NotInstalled(Tool::Xcrun))
        );
    }

    #[test]
    fn sdkmanager_without_java_says_so() {
        let runner = Recorded::answering(vec![failed(
            "",
            "ERROR: JAVA_HOME is not set and no 'java' command could be found in your PATH.",
        )]);
        assert_eq!(
            preview_tool(&runner, ToolCleanup::AndroidSdkPackage, &home(), now()),
            Err(ToolError::NeedsJava)
        );
    }

    #[test]
    fn simulator_run_deletes_unavailable_only() {
        let runner = Recorded::answering(vec![ok(SIMS), ok(SIMS), ok("")]);
        let plan =
            preview_tool(&runner, ToolCleanup::UnavailableSimulators, &home(), now()).unwrap();
        assert_eq!(plan.items().len(), 2);
        assert_eq!(plan.items()[0].title, "iPhone 14");
        assert_eq!(plan.items()[0].detail, "iOS 16.0");
        assert_eq!(
            plan.items()[0].size_bytes,
            524_288_000,
            "falls back to dataPathSize"
        );
        let ran = run_tool(&runner, plan, None, &home(), now()).unwrap();
        assert_eq!(ran.command_line, "xcrun simctl delete unavailable");
        assert_eq!(ran.reclaimed_bytes, Some(524_288_000));
        assert_eq!(
            runner.calls()[2],
            (
                Tool::Xcrun,
                vec!["simctl".into(), "delete".into(), "unavailable".into()]
            )
        );
    }

    #[test]
    fn simulator_run_refuses_a_new_unavailable_device() {
        let first = SIMS.replacen("\"isAvailable\" : false", "\"isAvailable\" : true", 1);
        let runner = Recorded::answering(vec![ok(&first), ok(SIMS)]);
        let plan =
            preview_tool(&runner, ToolCleanup::UnavailableSimulators, &home(), now()).unwrap();
        assert_eq!(plan.items().len(), 1);
        assert_eq!(
            run_tool(&runner, plan, None, &home(), now()),
            Err(ToolError::Changed)
        );
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn empty_previews_run_nothing() {
        let runner = Recorded::answering(vec![ok(NO_SIMS)]);
        let plan =
            preview_tool(&runner, ToolCleanup::UnavailableSimulators, &home(), now()).unwrap();
        assert!(plan.items().is_empty());
        assert_eq!(
            run_tool(&runner, plan, None, &home(), now()),
            Err(ToolError::NothingToClean)
        );
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn plans_expire() {
        for when in [
            now() + planner::PLAN_LIFETIME,
            now() - Duration::from_secs(1),
        ] {
            let runner = Recorded::answering(vec![ok(BREW_DRY)]);
            let plan = preview_tool(&runner, ToolCleanup::Homebrew, &home(), now()).unwrap();
            assert_eq!(
                run_tool(&runner, plan, None, &home(), when),
                Err(ToolError::Expired)
            );
            assert_eq!(runner.calls().len(), 1);
        }
    }

    #[test]
    fn sdk_preview_never_offers_the_command_line_tools() {
        let runner = Recorded::answering(vec![ok(SDK)]);
        let plan = preview_tool(&runner, ToolCleanup::AndroidSdkPackage, &home(), now()).unwrap();
        let ids: Vec<&str> = plan.items().iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "build-tools;33.0.1",
                "build-tools;34.0.0",
                "platforms;android-34"
            ]
        );
        assert_eq!(plan.items()[1].title, "Android SDK Build-Tools 34");
        assert_eq!(plan.items()[1].detail, "build-tools;34.0.0, version 34.0.0");
    }

    #[test]
    fn sdk_run_removes_exactly_the_chosen_package() {
        let runner = Recorded::answering(vec![
            ok(SDK),
            ok(SDK),
            ok("[=====] 100% Fetch remote repository\n"),
        ]);
        let plan = preview_tool(&runner, ToolCleanup::AndroidSdkPackage, &home(), now()).unwrap();
        let ran = run_tool(&runner, plan, Some("build-tools;33.0.1"), &home(), now()).unwrap();
        assert!(ran.success);
        assert_eq!(
            ran.command_line,
            "sdkmanager --uninstall build-tools;33.0.1"
        );
        assert_eq!(
            runner.calls()[2],
            (
                Tool::Sdkmanager,
                vec!["--uninstall".into(), "build-tools;33.0.1".into()]
            )
        );
    }

    #[test]
    fn sdk_run_refuses_a_package_the_user_did_not_see() {
        for package in [
            None,
            Some("emulator"),
            Some("cmdline-tools;latest"),
            Some("--sdk_root=/"),
        ] {
            let runner = Recorded::answering(vec![ok(SDK)]);
            let plan =
                preview_tool(&runner, ToolCleanup::AndroidSdkPackage, &home(), now()).unwrap();
            assert_eq!(
                run_tool(&runner, plan, package, &home(), now()),
                Err(ToolError::NotInPreview),
                "{package:?}"
            );
            assert_eq!(runner.calls().len(), 1, "sdkmanager --uninstall never ran");
        }
    }

    #[test]
    fn sdk_run_refuses_a_package_that_is_gone() {
        let without = SDK.replace("  build-tools;33.0.1   | 33.0.1  | Android SDK Build-Tools 33.0.1 | build-tools/33.0.1\n", "");
        let runner = Recorded::answering(vec![ok(SDK), ok(&without)]);
        let plan = preview_tool(&runner, ToolCleanup::AndroidSdkPackage, &home(), now()).unwrap();
        assert_eq!(
            run_tool(&runner, plan, Some("build-tools;33.0.1"), &home(), now()),
            Err(ToolError::Changed)
        );
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn sdk_sizes_are_measured_inside_the_sdk() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let build = home.join(ANDROID_SDK).join("build-tools/34.0.0");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("aapt"), vec![7u8; 64 * 1024]).unwrap();
        let runner = Recorded::answering(vec![ok(SDK)]);
        let plan = preview_tool(&runner, ToolCleanup::AndroidSdkPackage, home, now()).unwrap();
        let item = plan
            .items()
            .iter()
            .find(|i| i.id == "build-tools;34.0.0")
            .unwrap();
        assert!(item.size_bytes >= 64 * 1024, "{}", item.size_bytes);
        let other = plan
            .items()
            .iter()
            .find(|i| i.id == "build-tools;33.0.1")
            .unwrap();
        assert_eq!(other.size_bytes, 0);
    }

    #[test]
    fn folder_size_skips_symlinks_and_counts_hard_links_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("big"), vec![1u8; 256 * 1024]).unwrap();
        std::fs::write(root.join("a"), vec![1u8; 32 * 1024]).unwrap();
        std::fs::hard_link(root.join("a"), root.join("b")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let size = folder_size(&root);
        assert!((32 * 1024..64 * 1024).contains(&size), "{size}");
        assert_eq!(folder_size(&dir.path().join("missing")), 0);
    }

    #[test]
    fn finds_sdkmanager_in_the_sdk_only_as_a_regular_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sdk = dir.path().join(ANDROID_SDK);
        assert_eq!(find_sdkmanager(&sdk), None);
        let old = sdk.join("cmdline-tools/9.0/bin");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("sdkmanager"), "").unwrap();
        assert_eq!(find_sdkmanager(&sdk), Some(old.join("sdkmanager")));
        let latest = sdk.join("cmdline-tools/latest/bin");
        std::fs::create_dir_all(&latest).unwrap();
        std::os::unix::fs::symlink(old.join("sdkmanager"), latest.join("sdkmanager")).unwrap();
        assert_eq!(
            find_sdkmanager(&sdk),
            Some(old.join("sdkmanager")),
            "a symlink is skipped"
        );
        std::fs::remove_file(latest.join("sdkmanager")).unwrap();
        std::fs::write(latest.join("sdkmanager"), "").unwrap();
        std::fs::set_permissions(
            latest.join("sdkmanager"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(find_sdkmanager(&sdk), Some(latest.join("sdkmanager")));
    }

    /// The real runner against a stand-in program: arguments arrive one by
    /// one, and Homebrew's settings are in its environment.
    #[test]
    fn real_runner_passes_an_argument_array_and_environment() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log.txt");
        let script = dir.path().join("brew");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"arg:$a\" >> '{log}'; done\necho \"auto:$HOMEBREW_NO_AUTO_UPDATE\" >> '{log}'\necho '==> This operation has freed approximately 2KB of disk space.'\n",
                log = log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runner = RealToolRunner::new(ToolLocations {
            brew: Some(script),
            ..Default::default()
        });
        let out = runner
            .run(Tool::Brew, &["cleanup"], Duration::from_secs(10))
            .unwrap();
        assert!(out.success);
        assert_eq!(brew_freed(&out.stdout), Some(2048));
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "arg:cleanup\nauto:1\n"
        );
    }

    #[test]
    fn real_runner_without_the_program_is_not_installed() {
        let runner = RealToolRunner::new(ToolLocations::default());
        for tool in [Tool::Brew, Tool::Xcrun, Tool::Sdkmanager] {
            assert_eq!(
                runner.run(tool, &["x"], Duration::from_secs(5)),
                Err(RunError::NotFound)
            );
        }
    }
}
