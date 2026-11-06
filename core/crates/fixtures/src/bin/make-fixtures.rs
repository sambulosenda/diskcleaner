//! `make-fixtures <dir>`: builds a fixture home in `<dir>`.
//!
//! `<dir>` must be empty or not exist. The tool refuses the real home folder
//! and any folder that contains it. Prints the manifest path and totals.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, dir] = args.as_slice() else {
        eprintln!("usage: make-fixtures <dir>");
        return ExitCode::from(2);
    };
    let target = PathBuf::from(dir);
    if is_real_home_or_parent(&target) {
        eprintln!("refusing to build fixtures in or above the real home folder: {dir}");
        return ExitCode::from(2);
    }
    match fixtures::build(&target, SystemTime::now()) {
        Ok(manifest) => {
            println!(
                "built {} entries, {} bytes; manifest at {}",
                manifest.entries.len(),
                manifest.total_file_bytes(),
                target.join(fixtures::MANIFEST_FILE).display()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("make-fixtures: {err}");
            ExitCode::FAILURE
        }
    }
}

/// True if `target` is the user's real home folder or one of its parents.
///
/// `$HOME` is read only to refuse it. A target that does not exist yet
/// cannot be the home folder, but its nearest existing parent is checked.
fn is_real_home_or_parent(target: &Path) -> bool {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    let Ok(home) = std::fs::canonicalize(home) else {
        return false;
    };
    match std::fs::canonicalize(target) {
        Ok(t) => home.starts_with(&t),
        Err(_) => false,
    }
}
