//! T11 check: prints what the app's Overview and category screens would show for a folder, in the
//! same units as `scripts/mac-storage-survey.sh`, so the two can be compared line by line.
//!
//!   cargo run --release -p ffi --example overview_report -- <home> [simple|developer]
//!
//! Read only: it runs the same scan the app runs, through the same `RoomyCore` commands, and
//! changes nothing on disk. It prints rule ids and sizes only, never a path.
//!
//! The survey's "GB" is 1024 × 1024 × 1024 bytes (it divides `du -sk` by 1,048,576). The app shows
//! sizes the way Finder does, where 1 GB is 1,000,000,000 bytes. Both are printed here.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use roomy_ffi::{CoreError, Mode, RoomyCore, ScanPhase, Sort};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let home = args
        .next()
        .ok_or("usage: overview_report <home> [simple|developer]")?;
    let mode = match args.next().as_deref() {
        None | Some("developer") => Mode::Developer,
        Some("simple") => Mode::Simple,
        Some(other) => return Err(format!("unknown mode {other:?}: use simple or developer")),
    };
    let core = RoomyCore::with_home(PathBuf::from(home)).map_err(describe)?;

    let started = Instant::now();
    let handle = core.start_scan(mode);
    loop {
        match core.scan_progress(handle).map_err(describe)?.phase {
            ScanPhase::Scanning | ScanPhase::Classifying => {
                std::thread::sleep(Duration::from_millis(100))
            }
            ScanPhase::Finished => break,
            ScanPhase::Cancelled => return Err("the scan was cancelled".to_string()),
            ScanPhase::Failed { reason } => return Err(reason),
        }
    }
    let overview = core.overview(handle).map_err(describe)?;

    println!(
        "Roomy overview report ({mode:?} mode, scan took {:.1} s)",
        started.elapsed().as_secs_f64()
    );
    println!();
    row("Disk total", overview.disk_total);
    row("Disk free", overview.disk_free);
    row("Home folder (scanned)", overview.scanned_bytes);
    row("Headline: likely safe", overview.likely_safe_bytes);
    println!(
        "  {:<40} {:>12}",
        "Folders not measured", overview.unreadable_count
    );
    println!();
    println!("[Categories]");
    for c in &overview.categories {
        row(
            &format!("{:?} ({} items)", c.category, c.item_count),
            c.bytes,
        );
    }
    println!();
    println!("[Rules: compare with the survey rows of the same name]");
    let mut by_rule: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for item in core
        .list_items(handle, None, None, Sort::LargestFirst)
        .map_err(describe)?
    {
        let detail = core.item_detail(handle, item.id).map_err(describe)?;
        let entry = by_rule.entry(detail.rule_id).or_default();
        entry.0 = entry.0.saturating_add(detail.size_bytes);
        entry.1 += 1;
    }
    for (rule, (bytes, count)) in &by_rule {
        row(&format!("{rule} ({count} items)"), *bytes);
    }
    core.close_scan(handle);
    Ok(())
}

/// One line: label, size in the survey's GB, size in the app's GB.
fn row(label: &str, bytes: u64) {
    #[allow(clippy::cast_precision_loss)] // display only
    let b = bytes as f64;
    println!(
        "  {label:<40} {:>9.1} GB (survey)  {:>9.1} GB (app)",
        b / 1_073_741_824.0,
        b / 1_000_000_000.0
    );
}

fn describe(err: CoreError) -> String {
    err.to_string()
}
