//! When a JavaScript project was last worked on, and which package manager
//! it uses, for the JavaScript screen.
//!
//! Read only. Nothing here opens a file: it reads the metadata (modified
//! time and file type) of a fixed list of names in the project folder, and
//! of `.git/index` and `.git/logs/HEAD` in the nearest git repository. It
//! never follows a symlink: every name is read with `symlink_metadata`, and
//! only real files and folders count.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

/// The package manifest every JavaScript project has.
pub const MANIFEST: &str = "package.json";

/// Lock files, with the package manager each belongs to. When a project
/// has more than one, the most recently changed one names the manager.
pub const LOCK_FILES: &[(&str, PackageManager)] = &[
    ("package-lock.json", PackageManager::Npm),
    ("npm-shrinkwrap.json", PackageManager::Npm),
    ("pnpm-lock.yaml", PackageManager::Pnpm),
    ("yarn.lock", PackageManager::Yarn),
    ("bun.lock", PackageManager::Bun),
    ("bun.lockb", PackageManager::Bun),
];

/// Files inside `.git` that change when someone commits, stages or
/// switches branch.
const GIT_FILES: &[&str] = &["index", "logs/HEAD"];

/// How many folders up from the project the search for `.git` goes, at
/// most. Deep enough for any real monorepo; it also stops at the home
/// folder.
const MAX_GIT_SEARCH_DEPTH: usize = 12;

/// The package manager a project's lock file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    /// npm (`package-lock.json`, `npm-shrinkwrap.json`).
    Npm,
    /// pnpm (`pnpm-lock.yaml`).
    Pnpm,
    /// Yarn (`yarn.lock`).
    Yarn,
    /// Bun (`bun.lock`, `bun.lockb`).
    Bun,
}

impl PackageManager {
    /// The command that fetches a project's packages again.
    pub fn install_command(self) -> &'static str {
        match self {
            Self::Npm => "npm install",
            Self::Pnpm => "pnpm install",
            Self::Yarn => "yarn install",
            Self::Bun => "bun install",
        }
    }
}

/// Which file gave a project's last-activity date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivitySource {
    /// The project's git repository (a commit, a staged change or a branch
    /// switch).
    Git,
    /// A lock file (packages added, removed or updated).
    LockFile,
    /// `package.json`.
    Manifest,
}

/// What the project folder says about recent work. Every part may be
/// missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProjectActivity {
    /// The newest modified time among the manifest, the lock files and the
    /// git files.
    pub last_activity: Option<SystemTime>,
    /// Which kind of file gave `last_activity`.
    pub source: Option<ActivitySource>,
    /// The package manager, from the newest lock file.
    pub package_manager: Option<PackageManager>,
    /// Whether the folder has a `package.json`.
    pub has_manifest: bool,
}

/// Reads when the project in `project_dir` was last worked on.
///
/// `project_dir` is the folder that holds a `node_modules` item, already
/// checked by `safety::validate` (no symlink in it or its parents). The
/// search for `.git` goes up from `project_dir` but never reaches `home`
/// or leaves it. Read only; a name that is missing, unreadable or a
/// symlink is simply not counted.
pub fn project_activity(home: &Path, project_dir: &Path) -> ProjectActivity {
    let mut activity = ProjectActivity::default();

    if let Some(t) = file_modified(&project_dir.join(MANIFEST)) {
        activity.has_manifest = true;
        consider(&mut activity, t, ActivitySource::Manifest);
    }

    let mut newest_lock: Option<SystemTime> = None;
    for (name, manager) in LOCK_FILES {
        if let Some(t) = file_modified(&project_dir.join(name)) {
            consider(&mut activity, t, ActivitySource::LockFile);
            if newest_lock.is_none_or(|n| t > n) {
                newest_lock = Some(t);
                activity.package_manager = Some(*manager);
            }
        }
    }

    if let Some(git_dir) = find_git_dir(home, project_dir) {
        for name in GIT_FILES {
            if let Some(t) = file_modified(&git_dir.join(name)) {
                consider(&mut activity, t, ActivitySource::Git);
            }
        }
    }

    activity
}

/// Keeps `t` if it is newer than what `activity` holds.
fn consider(activity: &mut ProjectActivity, t: SystemTime, source: ActivitySource) {
    if activity.last_activity.is_none_or(|current| t > current) {
        activity.last_activity = Some(t);
        activity.source = Some(source);
    }
}

/// The modified time of `path` if it is a regular file (not a symlink).
fn file_modified(path: &Path) -> Option<SystemTime> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    meta.modified().ok()
}

/// The nearest `.git` folder at or above `project_dir`, strictly inside
/// `home`. A `.git` that is a file (a worktree or submodule) or a symlink
/// is not used. Walking stops at the first ancestor that is a symlink.
fn find_git_dir(home: &Path, project_dir: &Path) -> Option<std::path::PathBuf> {
    let mut dir = project_dir;
    for _ in 0..MAX_GIT_SEARCH_DEPTH {
        if dir == home || !dir.starts_with(home) {
            return None;
        }
        let meta = fs::symlink_metadata(dir).ok()?;
        if !meta.file_type().is_dir() {
            return None;
        }
        let git = dir.join(".git");
        if fs::symlink_metadata(&git).is_ok_and(|m| m.file_type().is_dir()) {
            return Some(git);
        }
        dir = dir.parent()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, FileTimes};
    use std::time::Duration;

    /// Makes `path` (and its parents) with a modified time `days_ago`.
    fn touch(path: &Path, days_ago: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(path).unwrap();
        let t = SystemTime::now() - Duration::from_secs(days_ago * 86_400);
        file.set_times(FileTimes::new().set_modified(t)).unwrap();
    }

    fn days_ago(activity: &ProjectActivity) -> u64 {
        let t = activity.last_activity.unwrap();
        SystemTime::now().duration_since(t).unwrap().as_secs() / 86_400
    }

    #[test]
    fn an_empty_folder_says_nothing() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("projects/empty");
        fs::create_dir_all(&project).unwrap();
        assert_eq!(
            project_activity(home.path(), &project),
            ProjectActivity::default()
        );
    }

    #[test]
    fn the_manifest_alone_gives_the_date() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("projects/site");
        touch(&project.join("package.json"), 30);
        let a = project_activity(home.path(), &project);
        assert!(a.has_manifest);
        assert_eq!(a.source, Some(ActivitySource::Manifest));
        assert_eq!(a.package_manager, None);
        assert_eq!(days_ago(&a), 30);
    }

    #[test]
    fn the_newest_lock_file_names_the_package_manager() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("projects/site");
        touch(&project.join("package.json"), 90);
        touch(&project.join("package-lock.json"), 60);
        touch(&project.join("pnpm-lock.yaml"), 10);
        let a = project_activity(home.path(), &project);
        assert_eq!(a.package_manager, Some(PackageManager::Pnpm));
        assert_eq!(a.source, Some(ActivitySource::LockFile));
        assert_eq!(days_ago(&a), 10);
    }

    #[test]
    fn git_activity_counts_and_is_found_above_the_project() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("Developer/mono");
        let project = repo.join("packages/web");
        touch(&project.join("package.json"), 200);
        touch(&project.join("yarn.lock"), 150);
        touch(&repo.join(".git/index"), 2);
        let a = project_activity(home.path(), &project);
        assert_eq!(a.source, Some(ActivitySource::Git));
        assert_eq!(a.package_manager, Some(PackageManager::Yarn));
        assert_eq!(days_ago(&a), 2);
    }

    #[test]
    fn a_git_folder_in_home_itself_is_never_used() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("site");
        touch(&project.join("package.json"), 40);
        touch(&home.path().join(".git/index"), 1);
        let a = project_activity(home.path(), &project);
        assert_eq!(a.source, Some(ActivitySource::Manifest));
        assert_eq!(days_ago(&a), 40);
    }

    #[test]
    fn symlinks_are_never_followed() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let real = elsewhere.path().join("package.json");
        touch(&real, 1);
        let project = home.path().join("projects/linked");
        fs::create_dir_all(&project).unwrap();
        std::os::unix::fs::symlink(&real, project.join("package.json")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), project.join(".git")).unwrap();
        let a = project_activity(home.path(), &project);
        assert_eq!(a, ProjectActivity::default());
    }

    #[test]
    fn a_folder_named_like_a_lock_file_is_not_counted() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("projects/odd");
        fs::create_dir_all(project.join("yarn.lock")).unwrap();
        let a = project_activity(home.path(), &project);
        assert_eq!(a.package_manager, None);
        assert_eq!(a.last_activity, None);
    }

    #[test]
    fn install_commands_name_the_tool() {
        assert_eq!(PackageManager::Npm.install_command(), "npm install");
        assert_eq!(PackageManager::Bun.install_command(), "bun install");
    }
}
