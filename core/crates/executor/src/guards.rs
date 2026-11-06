//! Extra checks that skip an item which is probably in use or that the user
//! wants kept, on top of the protected-path rules. They only ever add
//! reasons to leave something alone.
//!
//! - **Folders you keep**: the user's own protected folders. An item inside
//!   one, or holding one, is skipped.
//! - **App open**: the item's owner app (from `planner::owner_apps`) is
//!   running. If the running programs can't be listed, items with an owner
//!   are skipped.
//! - **Git**: see [`crate::git`].
//! - **Changed recently**: the item, or anything inside it, was modified in
//!   the last [`RECENT_CHANGE_WINDOW`].
//!
//! All read only. They run in [`crate::preview`] (the dry run) and again in
//! [`crate::execute`] immediately before each item is moved.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use planner::{AppBundle, OwnerApp};

use crate::ItemRefusal;

/// An item changed less than this long ago is skipped.
pub const RECENT_CHANGE_WINDOW: Duration = Duration::from_secs(10 * 60);

/// What is known about the programs running now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Running {
    /// The executable path (or, if hidden, the name) of each running program.
    Known(Vec<PathBuf>),
    /// The list could not be read. Every item with an owner app is skipped.
    Unknown,
}

impl Running {
    /// Lists the programs running now.
    pub fn now() -> Running {
        match crate::processes::running_programs() {
            Ok(programs) => Running::Known(programs),
            Err(err) => {
                tracing::warn!(%err, "could not list running programs");
                Running::Unknown
            }
        }
    }
}

/// The settings for the extra checks, for one preview or one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guards {
    /// Skip an item changed less than this long ago. The app uses
    /// [`RECENT_CHANGE_WINDOW`].
    pub recent_window: Duration,
    /// The user's protected folders: absolute paths inside home.
    pub protected_folders: Vec<PathBuf>,
    /// The programs running now.
    pub running: Running,
}

impl Guards {
    /// The checks the app runs: the standard window, the user's folders, and
    /// the programs running right now.
    pub fn standard(protected_folders: Vec<PathBuf>) -> Guards {
        Guards {
            recent_window: RECENT_CHANGE_WINDOW,
            protected_folders,
            running: Running::now(),
        }
    }

    /// Refuses `path` if it is inside, or holds, one of the user's folders.
    pub(crate) fn check_folders(&self, path: &Path) -> Result<(), ItemRefusal> {
        for folder in &self.protected_folders {
            if is_within(path, folder) {
                return Err(ItemRefusal::InFolderYouKeep);
            }
            if is_within(folder, path) {
                return Err(ItemRefusal::HoldsFolderYouKeep);
            }
        }
        Ok(())
    }

    /// Refuses the item if one of its owner apps is running, or if that
    /// can't be told.
    pub(crate) fn check_owners(&self, owners: &[OwnerApp]) -> Result<(), ItemRefusal> {
        let Some(first) = owners.first() else {
            return Ok(());
        };
        let programs = match &self.running {
            Running::Known(programs) => programs,
            Running::Unknown => {
                return Err(ItemRefusal::AppNotChecked {
                    app: first.name.to_string(),
                })
            }
        };
        for app in owners {
            if programs.iter().any(|p| is_app(p, app)) {
                return Err(ItemRefusal::AppOpen {
                    app: app.name.to_string(),
                });
            }
        }
        Ok(())
    }

    /// For an item of the app uninstaller: refuses it if any program inside
    /// the app's bundle, or inside the item itself (a helper the app keeps
    /// in its data folder), is running, or if that can't be told.
    pub(crate) fn check_app(
        &self,
        app: Option<&AppBundle>,
        item: &Path,
    ) -> Result<(), ItemRefusal> {
        let Some(app) = app else {
            return Ok(());
        };
        let programs = match &self.running {
            Running::Known(programs) => programs,
            Running::Unknown => {
                return Err(ItemRefusal::AppNotChecked {
                    app: app.name.clone(),
                })
            }
        };
        let open = programs
            .iter()
            .any(|p| is_within(p, &app.bundle) || is_within(p, item));
        if open {
            return Err(ItemRefusal::AppOpen {
                app: app.name.clone(),
            });
        }
        Ok(())
    }

    /// Refuses the item if anything in it changed within the window before
    /// `now`, or has a time after `now` (the clock may be wrong).
    pub(crate) fn check_recent(
        &self,
        newest: SystemTime,
        now: SystemTime,
    ) -> Result<(), ItemRefusal> {
        match now.duration_since(newest) {
            Ok(age) if age >= self.recent_window => Ok(()),
            _ => Err(ItemRefusal::ChangedRecently),
        }
    }
}

/// Whether the program at `program` is `app`: its file name is one of the
/// app's programs, or its path goes through one of the app's bundles.
pub fn is_app(program: &Path, app: &OwnerApp) -> bool {
    let named = program
        .file_name()
        .is_some_and(|name| app.programs.iter().any(|p| OsStr::new(p) == name));
    named
        || program
            .iter()
            .any(|part| app.bundles.iter().any(|b| OsStr::new(b) == part))
}

/// Whether `path` is `folder` or inside it, comparing names without regard
/// to ASCII case (Mac disks usually ignore case), part by part.
fn is_within(path: &Path, folder: &Path) -> bool {
    let mut path_parts = path.iter();
    for part in folder.iter() {
        match path_parts.next() {
            Some(p) if p.as_bytes().eq_ignore_ascii_case(part.as_bytes()) => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const XCODE: OwnerApp = OwnerApp {
        name: "Xcode",
        programs: &["Xcode"],
        bundles: &["Xcode.app"],
    };

    fn guards(running: Running, folders: &[&str]) -> Guards {
        Guards {
            recent_window: RECENT_CHANGE_WINDOW,
            protected_folders: folders.iter().map(PathBuf::from).collect(),
            running,
        }
    }

    #[test]
    fn folders_you_keep_cover_inside_and_holding() {
        let g = guards(Running::Known(vec![]), &["/Users/sam/Developer/keep"]);
        let check = |p: &str| g.check_folders(Path::new(p));
        assert_eq!(
            check("/Users/sam/Developer/keep"),
            Err(ItemRefusal::InFolderYouKeep)
        );
        assert_eq!(
            check("/Users/sam/Developer/KEEP/target"),
            Err(ItemRefusal::InFolderYouKeep)
        );
        assert_eq!(
            check("/Users/sam/Developer"),
            Err(ItemRefusal::HoldsFolderYouKeep)
        );
        assert_eq!(check("/Users/sam/Developer/keeper"), Ok(()));
        assert_eq!(check("/Users/sam/Library/Caches"), Ok(()));
    }

    #[test]
    fn an_open_owner_app_skips_the_item() {
        let open = guards(
            Running::Known(vec![PathBuf::from(
                "/Applications/Xcode.app/Contents/MacOS/Xcode",
            )]),
            &[],
        );
        assert_eq!(
            open.check_owners(&[XCODE]),
            Err(ItemRefusal::AppOpen {
                app: "Xcode".into()
            })
        );
        let helper = guards(
            Running::Known(vec![PathBuf::from(
                "/Applications/Xcode.app/Contents/Developer/usr/bin/xcdevice",
            )]),
            &[],
        );
        assert!(helper.check_owners(&[XCODE]).is_err());
        let other = guards(
            Running::Known(vec![PathBuf::from("/usr/bin/XcodeHelper")]),
            &[],
        );
        assert_eq!(other.check_owners(&[XCODE]), Ok(()));
        assert_eq!(open.check_owners(&[]), Ok(()));
    }

    #[test]
    fn unknown_programs_skip_only_items_with_an_owner() {
        let g = guards(Running::Unknown, &[]);
        assert_eq!(
            g.check_owners(&[XCODE]),
            Err(ItemRefusal::AppNotChecked {
                app: "Xcode".into()
            })
        );
        assert_eq!(g.check_owners(&[]), Ok(()));
    }

    #[test]
    fn an_app_being_removed_is_open_if_anything_in_its_bundle_or_item_runs() {
        let app = AppBundle {
            name: "Slack".into(),
            bundle: PathBuf::from("/Applications/Slack.app"),
        };
        let item = Path::new("/Users/sam/Library/Application Support/Slack");
        let running = |p: &str| guards(Running::Known(vec![PathBuf::from(p)]), &[]);
        let open = Err(ItemRefusal::AppOpen {
            app: "Slack".into(),
        });
        assert_eq!(
            running("/Applications/Slack.app/Contents/MacOS/Slack").check_app(Some(&app), item),
            open
        );
        assert_eq!(
            running("/applications/slack.app/Contents/Frameworks/Helper")
                .check_app(Some(&app), item),
            open
        );
        assert_eq!(
            running("/Users/sam/Library/Application Support/Slack/bin/agent")
                .check_app(Some(&app), item),
            open
        );
        let other = running("/Applications/Slack Helper.app/Contents/MacOS/x");
        assert_eq!(other.check_app(Some(&app), item), Ok(()));
        assert_eq!(other.check_app(None, item), Ok(()));
        assert_eq!(
            guards(Running::Unknown, &[]).check_app(Some(&app), item),
            Err(ItemRefusal::AppNotChecked {
                app: "Slack".into()
            })
        );
        assert_eq!(guards(Running::Unknown, &[]).check_app(None, item), Ok(()));
    }

    #[test]
    fn recent_changes_and_future_times_are_skipped() {
        let g = guards(Running::Known(vec![]), &[]);
        let now = SystemTime::now();
        let ago = |s| now - Duration::from_secs(s);
        assert_eq!(
            g.check_recent(ago(60), now),
            Err(ItemRefusal::ChangedRecently)
        );
        assert_eq!(
            g.check_recent(ago(599), now),
            Err(ItemRefusal::ChangedRecently)
        );
        assert_eq!(g.check_recent(ago(600), now), Ok(()));
        assert_eq!(
            g.check_recent(now + Duration::from_secs(5), now),
            Err(ItemRefusal::ChangedRecently)
        );
    }
}
