//! Times one scan of a folder and prints the numbers the T05 benchmark needs.
//!
//!   cargo run --release -p scanner --example scan_bench -- <absolute folder>
//!
//! Read only, like the scanner. `scripts/bench-scanner.sh` wraps this with
//! peak memory and a `du` comparison.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use scanner::{scan, ScanOptions};

fn main() -> ExitCode {
    let Some(arg) = std::env::args_os().nth(1) else {
        eprintln!("usage: scan_bench <absolute folder>");
        return ExitCode::from(2);
    };
    let root = PathBuf::from(arg);
    let started = Instant::now();
    let result = match scan(&root, ScanOptions::default(), |_| {}) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("scan failed: {err}");
            return ExitCode::FAILURE;
        }
    };
    let elapsed = started.elapsed();
    let total = result.total_bytes();
    println!("root:               {}", root.display());
    println!("elapsed_seconds:    {:.2}", elapsed.as_secs_f64());
    println!("folders:            {}", result.dirs.len());
    println!("files:              {}", result.total_files());
    println!("symlinks:           {}", result.symlinks);
    println!("hard_links_skipped: {}", result.hard_links_skipped);
    println!("unreadable_folders: {}", result.unreadable.len());
    println!("other_volumes:      {}", result.other_volumes.len());
    println!("entries_skipped:    {}", result.entries_skipped);
    println!("total_bytes:        {total}");
    println!("total_kib:          {}", total / 1024);
    ExitCode::SUCCESS
}
