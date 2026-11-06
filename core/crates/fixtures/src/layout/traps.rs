//! Traps: things the engine must never follow, double count or remove.

use super::KB;
use crate::builder::Builder;
use crate::paths;
use crate::FixtureError;

/// Builds the traps. Runs last, so the folders the traps sit in already exist.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    // A symlink out of the home folder, to a system folder.
    b.symlink(paths::TRAP_SYMLINK_ETC, "/etc")?;
    // A symlink from a cache folder into personal documents. Relative, so the
    // tree is the same wherever it is built: Library/Caches/<id>/ -> ~/Documents.
    b.symlink(paths::TRAP_SYMLINK_DOCUMENTS, "../../../Documents")?;
    // A hard link pair: one file, two names. Its size must be counted once.
    b.bytes(paths::TRAP_HARD_LINK_A, 80 * KB)?;
    b.hard_link(paths::TRAP_HARD_LINK_B, paths::TRAP_HARD_LINK_A)?;
    git_repo(b)?;
    // Fake secrets: obviously not real values.
    b.text(
        paths::TRAP_ENV_FILE,
        "API_KEY=fixture-not-a-real-key\nDATABASE_URL=postgres://fixture:fixture@localhost/fixture\n",
    )?;
    // Signing key: deterministic bytes, not a real keystore.
    b.bytes(paths::TRAP_KEYSTORE, 3 * KB)
}

/// A minimal `.git` folder in project a, beside its `node_modules`, and a
/// `.gitignore` that ignores `node_modules` as real projects do.
fn git_repo(b: &mut Builder) -> Result<(), FixtureError> {
    b.text(
        paths::TRAP_GIT_IGNORE,
        "# Dependencies\nnode_modules/\n.DS_Store\n",
    )?;
    let git = paths::TRAP_GIT_DIR;
    b.text(&format!("{git}/HEAD"), "ref: refs/heads/main\n")?;
    b.text(
        &format!("{git}/config"),
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n",
    )?;
    b.text(
        &format!("{git}/refs/heads/main"),
        "0123456789abcdef0123456789abcdef01234567\n",
    )?;
    b.bytes(
        &format!("{git}/objects/01/23456789abcdef0123456789abcdef01234567"),
        2 * KB,
    )?;
    b.dir(&format!("{git}/refs/tags"))
}
