//! Prints how a folder classifies: `cargo run -p classifier --example
//! classify_dir -- <rules folder> <home folder> [simple|developer]`.
//! Read only; for checking rules against a fixture home.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: classify_dir <rules folder> <home folder> [simple|developer]");
        std::process::exit(2);
    }
    let mode = match args.get(3).map(String::as_str) {
        Some("simple") => rules::Mode::Simple,
        _ => rules::Mode::Developer,
    };
    let (rules, errors) = rules::load_dir(&PathBuf::from(&args[1]));
    for e in &errors {
        eprintln!("rule error: {e}");
    }
    for tie in classifier::check_ties(&rules, mode) {
        eprintln!("tie: {} and {}", tie.first, tie.second);
    }
    let opts = scanner::ScanOptions {
        record_files: Some(classifier::record_files(&rules, mode)),
        ..Default::default()
    };
    let scan = match scanner::scan(&PathBuf::from(&args[2]), opts, |_| {}) {
        Ok(scan) => scan,
        Err(e) => {
            eprintln!("scan failed: {e}");
            std::process::exit(1);
        }
    };
    for item in classifier::classify(&scan, &rules, mode) {
        println!(
            "{:>10}  {:<12} {:<30} {}  {:?}",
            item.size_bytes,
            item.tier.as_str(),
            item.rule_id,
            item.path.display(),
            item.lowered
        );
    }
}
