//! The undo for a move to the Trash.
//!
//! Only puts back what this executor moved: the caller passes entries made
//! from an [`crate::ItemReport`] of [`crate::execute`], never paths from
//! anywhere else. For each one, immediately before acting, it:
//!
//! 1. checks the original path is inside home, written plainly, and that its
//!    Trash name is one plain name directly in `~/.Trash`;
//! 2. checks the Trash still holds the same file or folder (same device and
//!    inode, not a symlink);
//! 3. opens every folder from home down to the original parent with
//!    `O_NOFOLLOW`, each on the scan volume;
//! 4. moves it back with a move that never replaces anything, so if
//!    something new is in its old place, it stays in the Trash.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::{sys, Context, ExecuteError};

/// One item to put back, made from an execute report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutBack {
    /// The item's recommendation.
    pub recommendation_id: String,
    /// Where it was before it moved (`ItemReport::path`).
    pub original: PathBuf,
    /// Where it is in the Trash (`Outcome::MovedToTrash::to`).
    pub in_trash: PathBuf,
    /// The inode that was moved (`ItemReport::moved_ino`).
    pub ino: u64,
}

/// Why one item was not put back. Nothing was changed for it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PutBackRefusal {
    /// The entry is not one this executor could have made.
    #[error("Roomy can't put this back")]
    NotOurs,
    /// The Trash no longer holds what was moved there.
    #[error("it is no longer in the Trash as Roomy left it")]
    NotInTrash,
    /// A folder on the way to the old place is gone, is a link, or is on
    /// another disk.
    #[error("the folder it came from has changed or is gone")]
    FolderChanged,
    /// Something new is in its old place.
    #[error("something new is in its old place, so it stays in the Trash")]
    PlaceTaken,
    /// The move back failed.
    #[error("the move back did not work")]
    Failed,
}

/// Puts each entry back where it was. One refused entry never stops the
/// others. Returns one result per entry, in order.
///
/// - `home`: the home path passed to the core.
/// - `dev`: the device id of the scan root.
pub fn put_back(
    home: &Path,
    dev: u64,
    items: &[PutBack],
) -> Result<Vec<Result<(), PutBackRefusal>>, ExecuteError> {
    let ctx = Context::open(home, dev)?;
    let results = items
        .iter()
        .map(|item| {
            let result = put_back_one(&ctx, item);
            tracing::info!(
                recommendation_id = %item.recommendation_id,
                ok = result.is_ok(),
                "put back"
            );
            result
        })
        .collect();
    Ok(results)
}

/// Checks and moves one entry back.
fn put_back_one(ctx: &Context, item: &PutBack) -> Result<(), PutBackRefusal> {
    if !is_plain(&ctx.home, &item.original) {
        return Err(PutBackRefusal::NotOurs);
    }
    let trash_name = trash_name(&ctx.trash_path, &item.in_trash).ok_or(PutBackRefusal::NotOurs)?;
    let in_trash =
        sys::stat_at(&ctx.trash_fd, &trash_name).map_err(|_| PutBackRefusal::NotInTrash)?;
    if in_trash.kind == sys::Kind::Symlink || in_trash.dev != ctx.dev || in_trash.ino != item.ino {
        return Err(PutBackRefusal::NotInTrash);
    }
    let (parent_fd, name) = ctx
        .open_parent(&item.original)
        .map_err(|_| PutBackRefusal::FolderChanged)?;
    match sys::stat_at(&parent_fd, &name) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(PutBackRefusal::PlaceTaken),
    }
    match sys::rename_no_replace(&ctx.trash_fd, &trash_name, &parent_fd, &name) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(PutBackRefusal::PlaceTaken)
        }
        Err(err) => {
            tracing::warn!(%err, "put back failed");
            Err(PutBackRefusal::Failed)
        }
    }
}

/// Whether `path` is strictly inside `home` and has only plain names (no
/// `.`, `..` or root parts after home).
fn is_plain(home: &Path, path: &Path) -> bool {
    match path.strip_prefix(home) {
        Ok(rest) => {
            rest.components().count() > 0
                && rest.components().all(|c| matches!(c, Component::Normal(_)))
        }
        Err(_) => false,
    }
}

/// The single name of `in_trash` directly inside `trash`, if it is one.
fn trash_name(trash: &Path, in_trash: &Path) -> Option<OsString> {
    let rest = in_trash.strip_prefix(trash).ok()?;
    let mut parts = rest.components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(name)), None) => Some(name.to_os_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_paths_inside_home_count() {
        let home = Path::new("/Users/sam");
        assert!(is_plain(home, Path::new("/Users/sam/Library/Caches/x")));
        assert!(!is_plain(home, Path::new("/Users/sam")));
        assert!(!is_plain(home, Path::new("/Users/sam/../eve/x")));
        assert!(!is_plain(home, Path::new("/etc/passwd")));
    }

    #[test]
    fn trash_names_are_one_plain_name() {
        let trash = Path::new("/Users/sam/.Trash");
        assert_eq!(
            trash_name(trash, Path::new("/Users/sam/.Trash/x 2")),
            Some(OsString::from("x 2"))
        );
        assert_eq!(trash_name(trash, Path::new("/Users/sam/.Trash/a/b")), None);
        assert_eq!(trash_name(trash, Path::new("/Users/sam/.Trash")), None);
        assert_eq!(trash_name(trash, Path::new("/Users/sam/x")), None);
        assert_eq!(trash_name(trash, Path::new("/Users/sam/.Trash/..")), None);
    }
}
