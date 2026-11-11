//! The live matcher: what it reports while a scan runs must agree with what
//! the full classifier says once the scan is done, and it must stay quiet
//! whenever it cannot be sure from the path alone.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use classifier::{classify, record_files, LiveMatcher, Look};
use rules::{Mode, Rule};
use scanner::{scan, ScanOptions, Watch, Watched};

fn rule(id: &str, path: &str, tier: &str, title: &str, extra: &str) -> String {
    format!(
        r#"
[[rule]]
id = "{id}"
version = 1
match.path = "{path}"
{extra}
category = "cache"
tier = "{tier}"
confidence = "high"
regenerable = true
mode = "simple"
title = "{title}"
explain = "A test item."
risk = "None."
action = "trash"
"#
    )
}

fn parse(text: &str) -> Vec<Rule> {
    let (rules, errors) = rules::parse_file(Path::new("test.toml"), text);
    assert!(errors.is_empty(), "{errors:?}");
    rules.into_iter().map(|(_, r)| r).collect()
}

fn home(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for f in files {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, vec![7u8; 5000]).unwrap();
    }
    (dir, root)
}

fn look(rules: &[Rule], rel: &str) -> Look {
    LiveMatcher::new(rules, Mode::Developer).look(Path::new(rel))
}

fn found_title(look: Look) -> Option<String> {
    match look {
        Look::Found(kind) => Some(kind.title),
        _ => None,
    }
}

#[test]
fn a_plain_rule_match_is_found_and_other_folders_are_descended() {
    let rules = parse(&rule(
        "a.cache",
        "Library/Caches/*",
        "likely_safe",
        "App cache",
        "",
    ));
    assert_eq!(
        found_title(look(&rules, "Library/Caches/com.example")).as_deref(),
        Some("App cache")
    );
    assert_eq!(look(&rules, "Library/Caches"), Look::Descend);
    assert_eq!(look(&rules, "Library"), Look::Descend);
    // Matching ignores case, like the classifier.
    assert!(matches!(look(&rules, "library/caches/X"), Look::Found(_)));
}

#[test]
fn a_rule_that_needs_to_see_inside_is_never_guessed() {
    let rules = parse(&rule(
        "a.sim",
        "Devices/*",
        "likely_safe",
        "Simulator",
        r#"match.requires = ["device.plist"]"#,
    ));
    assert_eq!(look(&rules, "Devices/abc"), Look::Skip);
}

#[test]
fn a_requires_rule_also_silences_a_plain_rule_on_the_same_path() {
    let text = rule("a.plain", "work/*", "likely_safe", "Plain", "")
        + &rule(
            "a.needs",
            "work/build",
            "likely_safe",
            "Needs",
            r#"match.requires = ["marker"]"#,
        );
    let rules = parse(&text);
    assert_eq!(look(&rules, "work/build"), Look::Skip);
    assert!(matches!(look(&rules, "work/other"), Look::Found(_)));
}

#[test]
fn the_most_specific_rule_names_the_folder() {
    let text = rule("a.any", "Library/Caches/*", "likely_safe", "Any cache", "")
        + &rule("a.one", "Library/Caches/one", "review_first", "The one", "");
    let rules = parse(&text);
    assert_eq!(
        found_title(look(&rules, "Library/Caches/one")).as_deref(),
        Some("The one")
    );
}

#[test]
fn tied_rules_and_keep_rules_say_nothing() {
    let tied = parse(
        &(rule("a.x", "tmp/*", "likely_safe", "X", "")
            + &rule("a.y", "tmp/*", "likely_safe", "Y", "")),
    );
    assert_eq!(look(&tied, "tmp/thing"), Look::Skip);
    let keep = parse(&rule("a.keep", "Photos", "keep", "Photos", ""));
    assert_eq!(look(&keep, "Photos"), Look::Skip);
}

#[test]
fn simple_mode_leaves_out_developer_rules() {
    let text = rule("a.dev", "DerivedData", "likely_safe", "Build files", "")
        .replace("mode = \"simple\"", "mode = \"developer\"");
    let rules = parse(&text);
    let simple = LiveMatcher::new(&rules, Mode::Simple);
    assert_eq!(simple.look(Path::new("DerivedData")), Look::Descend);
}

/// Scans `root` with the live matcher watching, then classifies. Returns
/// what was reported live and the final items.
fn scan_live(
    root: &Path,
    rules: &[Rule],
) -> (Vec<(PathBuf, String, u64)>, Vec<classifier::ClassifiedItem>) {
    let matcher = Arc::new(LiveMatcher::new(rules, Mode::Developer));
    let titles: Arc<Mutex<Vec<(PathBuf, String)>>> = Arc::default();
    let reported: Arc<Mutex<Vec<(PathBuf, u64)>>> = Arc::default();
    let (ask_titles, found_into) = (Arc::clone(&titles), Arc::clone(&reported));
    let watch = Watch {
        ask: Arc::new(move |rel: &Path| match matcher.look(rel) {
            Look::Descend => Watched::Descend,
            Look::Skip => Watched::Skip,
            Look::Found(kind) => {
                ask_titles
                    .lock()
                    .unwrap()
                    .push((rel.to_path_buf(), kind.title));
                Watched::Report
            }
        }),
        found: Arc::new(move |rel: &Path, bytes| {
            found_into.lock().unwrap().push((rel.to_path_buf(), bytes));
        }),
    };
    let opts = ScanOptions {
        record_files: Some(record_files(rules, Mode::Developer)),
        watch: Some(watch),
        ..ScanOptions::default()
    };
    let result = scan(root, opts, |_| {}).unwrap();
    let items = classify(&result, rules, Mode::Developer);
    let titles = titles.lock().unwrap().clone();
    let mut live: Vec<(PathBuf, String, u64)> = reported
        .lock()
        .unwrap()
        .iter()
        .map(|(rel, bytes)| {
            let title = titles.iter().find(|(p, _)| p == rel).unwrap().1.clone();
            (rel.clone(), title, *bytes)
        })
        .collect();
    live.sort();
    (live, items)
}

#[test]
fn everything_reported_live_is_an_item_of_the_same_kind_when_the_scan_ends() {
    let (_dir, root) = home(&[
        "Library/Caches/one/a.bin",
        "Library/Caches/one/deep/b.bin",
        "Library/Caches/two/c.bin",
        "Library/Caches/two/Library/Caches/inner/d.bin",
        "Devices/sim/device.plist",
        "Devices/sim/data/e.bin",
        "Devices/other/f.bin",
        "work/app/node_modules/pkg/g.js",
        "work/app/node_modules/pkg/node_modules/h.js",
        "Documents/letter.txt",
    ]);
    let text = rule(
        "a.cache",
        "Library/Caches/*",
        "likely_safe",
        "App cache",
        "",
    ) + &rule(
        "a.modules",
        "work/**/node_modules",
        "review_first",
        "Packages",
        "",
    ) + &rule(
        "a.sim",
        "Devices/*",
        "likely_safe",
        "Simulator",
        r#"match.requires = ["device.plist"]"#,
    );
    let rules = parse(&text);
    let (live, items) = scan_live(&root, &rules);

    let paths: Vec<&str> = live.iter().map(|(p, _, _)| p.to_str().unwrap()).collect();
    assert_eq!(
        paths,
        [
            "Library/Caches/one",
            "Library/Caches/two",
            "work/app/node_modules"
        ],
        "each item once, nothing inside an item, nothing that needs a look inside"
    );
    for (rel, title, bytes) in &live {
        let item = items
            .iter()
            .find(|i| i.path == root.join(rel))
            .unwrap_or_else(|| panic!("{} was reported live but is no item", rel.display()));
        assert_eq!(&item.title, title);
        assert_eq!(item.size_bytes, *bytes, "{}", rel.display());
    }
}

#[test]
fn a_cancelled_scan_reports_nothing() {
    let (_dir, root) = home(&["Library/Caches/one/a.bin"]);
    let reported: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let into = Arc::clone(&reported);
    let opts = ScanOptions {
        watch: Some(Watch {
            ask: Arc::new(|_: &Path| Watched::Report),
            found: Arc::new(move |rel: &Path, _| into.lock().unwrap().push(rel.to_path_buf())),
        }),
        ..ScanOptions::default()
    };
    opts.cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(scan(&root, opts, |_| {}).is_err());
    assert!(reported.lock().unwrap().is_empty());
}
