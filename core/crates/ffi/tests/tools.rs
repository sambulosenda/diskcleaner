//! The tool clean-ups end to end, against stand-in `brew`, `xcrun` and
//! `sdkmanager` programs.
//!
//! Each stand-in is a small script in a temporary folder that answers its
//! preview from a file and logs every call, so the tests can check exactly
//! which commands ran and with which arguments.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use roomy_ffi::{CoreError, RoomyCore, ToolCleanupKind, ToolPlanHandle};

const BREW_DRY: &str = "Would remove: /opt/homebrew/Cellar/node/20.1.0 (2,104 files, 61.2MB)
==> This operation would free approximately 61.2MB of disk space.
";
const SIMS: &str = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-16-0":[{"udid":"0A1B2C3D-0000-4000-8000-000000000001","isAvailable":false,"name":"iPhone 14","dataPathSize":1000}]}}"#;
const SDK: &str = "Installed packages:
  Path                 | Version | Description                    | Location
  -------              | ------- | -------                        | -------
  build-tools;33.0.1   | 33.0.1  | Android SDK Build-Tools 33.0.1 | build-tools/33.0.1
  cmdline-tools;latest | 12.0    | Android SDK Command-line Tools | cmdline-tools/latest
";

/// A home folder, the stand-in programs, and a core wired to them.
struct Setup {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    core: Arc<RoomyCore>,
}

/// Writes an executable stand-in called `name` that logs its arguments
/// and prints `preview.txt` for a preview, `run.txt` otherwise.
fn stand_in(bin: &Path, name: &str, preview_flag: &str) -> PathBuf {
    let script = format!(
        "#!/bin/sh
d='{d}'
echo \"{name} $*\" >> \"$d/calls.txt\"
for a in \"$@\"; do
  if [ \"$a\" = '{preview_flag}' ]; then /bin/cat \"$d/{name}-preview.txt\"; exit 0; fi
done
/bin/cat \"$d/{name}-run.txt\" 2>/dev/null
exit 0
",
        d = bin.display()
    );
    let path = bin.join(name);
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("Library/Application Support")).unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let brew = stand_in(&bin, "brew", "--dry-run");
        let xcrun = stand_in(&bin, "xcrun", "list");
        let sdk = stand_in(&bin, "sdkmanager", "--list_installed");
        fs::write(bin.join("brew-preview.txt"), BREW_DRY).unwrap();
        fs::write(
            bin.join("brew-run.txt"),
            "==> This operation has freed approximately 61.2MB of disk space.\n",
        )
        .unwrap();
        fs::write(bin.join("xcrun-preview.txt"), SIMS).unwrap();
        fs::write(bin.join("sdkmanager-preview.txt"), SDK).unwrap();
        let core = RoomyCore::with_home(home).unwrap();
        core.set_tool_programs_for_testing(Some(brew), Some(xcrun), Some(sdk));
        Self {
            _dir: dir,
            bin,
            core,
        }
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.bin.join("calls.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn brew_preview_then_run_saves_to_history() {
    let s = Setup::new();
    let preview = s
        .core
        .preview_tool_cleanup(ToolCleanupKind::Homebrew)
        .unwrap();
    assert_eq!(preview.command_line, "brew cleanup");
    assert_eq!(preview.items.len(), 1);
    assert_eq!(preview.items[0].title, "20.1.0");
    assert_eq!(
        s.calls(),
        ["brew cleanup --dry-run"],
        "a preview runs nothing else"
    );
    let result = s
        .core
        .run_tool_cleanup(preview.plan.unwrap(), None)
        .unwrap();
    assert!(result.success);
    assert_eq!(
        result.reclaimed_bytes,
        Some((61.2f64 * 1_048_576.0).round() as u64)
    );
    assert!(result.saved_to_history);
    assert_eq!(
        s.calls(),
        [
            "brew cleanup --dry-run",
            "brew cleanup --dry-run",
            "brew cleanup"
        ]
    );
}

#[test]
fn a_plan_runs_once() {
    let s = Setup::new();
    let preview = s
        .core
        .preview_tool_cleanup(ToolCleanupKind::Homebrew)
        .unwrap();
    let plan = preview.plan.unwrap();
    s.core.run_tool_cleanup(plan, None).unwrap();
    assert!(matches!(
        s.core.run_tool_cleanup(plan, None),
        Err(CoreError::CleanupUnavailable { .. })
    ));
    assert!(matches!(
        s.core.run_tool_cleanup(ToolPlanHandle { id: 999 }, None),
        Err(CoreError::CleanupUnavailable { .. })
    ));
}

#[test]
fn a_discarded_plan_never_runs() {
    let s = Setup::new();
    let preview = s
        .core
        .preview_tool_cleanup(ToolCleanupKind::UnavailableSimulators)
        .unwrap();
    let plan = preview.plan.unwrap();
    s.core.discard_tool_cleanup(plan);
    assert!(s.core.run_tool_cleanup(plan, None).is_err());
    assert_eq!(s.calls(), ["xcrun simctl list devices unavailable -j"]);
}

#[test]
fn simulators_are_deleted_with_the_exact_command() {
    let s = Setup::new();
    let preview = s
        .core
        .preview_tool_cleanup(ToolCleanupKind::UnavailableSimulators)
        .unwrap();
    assert_eq!(preview.items[0].title, "iPhone 14");
    assert_eq!(preview.items[0].detail, "iOS 16.0");
    let result = s
        .core
        .run_tool_cleanup(preview.plan.unwrap(), None)
        .unwrap();
    assert_eq!(result.command_line, "xcrun simctl delete unavailable");
    assert_eq!(s.calls().last().unwrap(), "xcrun simctl delete unavailable");
}

#[test]
fn sdk_uninstalls_only_a_package_from_the_review() {
    let s = Setup::new();
    let preview = s
        .core
        .preview_tool_cleanup(ToolCleanupKind::AndroidSdkPackage)
        .unwrap();
    assert_eq!(preview.command_line, "sdkmanager --uninstall <package>");
    let ids: Vec<&str> = preview.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        ["build-tools;33.0.1"],
        "the command-line tools are never offered"
    );
    let plan = preview.plan.unwrap();

    // A package the user did not see uses the plan up and runs nothing.
    assert!(matches!(
        s.core
            .run_tool_cleanup(plan, Some("cmdline-tools;latest".into())),
        Err(CoreError::ToolUnavailable { .. })
    ));
    assert!(!s.calls().iter().any(|c| c.contains("--uninstall")));

    let preview = s
        .core
        .preview_tool_cleanup(ToolCleanupKind::AndroidSdkPackage)
        .unwrap();
    let result = s
        .core
        .run_tool_cleanup(preview.plan.unwrap(), Some("build-tools;33.0.1".into()))
        .unwrap();
    assert!(result.success);
    assert_eq!(
        result.command_line,
        "sdkmanager --uninstall build-tools;33.0.1"
    );
    let uninstalls: Vec<String> = s
        .calls()
        .into_iter()
        .filter(|c| c.contains("--uninstall"))
        .collect();
    assert_eq!(uninstalls, ["sdkmanager --uninstall build-tools;33.0.1"]);
}

#[test]
fn a_missing_tool_is_a_plain_error() {
    let s = Setup::new();
    s.core.set_tool_programs_for_testing(None, None, None);
    for kind in [
        ToolCleanupKind::Homebrew,
        ToolCleanupKind::UnavailableSimulators,
        ToolCleanupKind::AndroidSdkPackage,
    ] {
        match s.core.preview_tool_cleanup(kind) {
            Err(CoreError::ToolUnavailable { reason }) => {
                assert!(reason.contains("isn't installed"), "{reason}");
                assert!(!reason.contains('/'), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }
}
