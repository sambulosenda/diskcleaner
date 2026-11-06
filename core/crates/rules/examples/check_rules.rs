//! Prints every problem in the rule files of a folder, one per line.
//!
//! Usage: `cargo run -p rules --example check_rules -- <folder>`
//! (defaults to the repository's `rules/` folder).

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../rules")));
    let (rules, errors) = rules::load_dir(&dir);
    for err in &errors {
        println!("{err}");
    }
    println!(
        "{} rule(s) loaded, {} problem(s).",
        rules.len(),
        errors.len()
    );
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
