//! Classifier behaviour on small hand-made homes: specificity, ties,
//! `match.requires`, and the checks that only ever make a tier more careful.

use std::path::{Path, PathBuf};

use classifier::{classify, record_files, ClassifiedItem, Lowered, UNKNOWN_RULE_ID};
use rules::{Action, Mode, Rule, Tier};
use safety::protected::BlockReason;
use scanner::{scan, ScanOptions};

/// One rule in TOML, with the settings that the tests vary.
fn rule(id: &str, path: &str, tier: &str, extra: &str) -> String {
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
title = "Test item"
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

/// A temp home holding the given files (each with a little content).
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

fn run(root: &Path, rules: &[Rule]) -> Vec<ClassifiedItem> {
    let opts = ScanOptions {
        record_files: Some(record_files(rules, Mode::Developer)),
        ..ScanOptions::default()
    };
    let result = scan(root, opts, |_| {}).unwrap();
    classify(&result, rules, Mode::Developer)
}

fn item<'a>(items: &'a [ClassifiedItem], root: &Path, rel: &str) -> Option<&'a ClassifiedItem> {
    items.iter().find(|i| i.path == root.join(rel))
}

#[test]
fn the_longest_literal_prefix_wins() {
    let text = rule(
        "t.any",
        "Library/Caches/*",
        "review_first",
        "per_item = true",
    ) + &rule(
        "t.safari",
        "Library/Caches/com.apple.Safari",
        "likely_safe",
        "",
    );
    let rules = parse(&text);
    let (_d, root) = home(&[
        "Library/Caches/com.apple.Safari/a",
        "Library/Caches/Other/b",
    ]);
    let items = run(&root, &rules);
    let safari = item(&items, &root, "Library/Caches/com.apple.Safari").unwrap();
    assert_eq!(safari.rule_id, "t.safari");
    assert_eq!(safari.tier, Tier::LikelySafe);
    let other = item(&items, &root, "Library/Caches/Other").unwrap();
    assert_eq!(other.rule_id, "t.any");
}

#[test]
fn tied_rules_classify_nothing() {
    let text = rule("t.one", "Library/Caches/A?", "likely_safe", "")
        + &rule("t.two", "Library/Caches/A*", "likely_safe", "");
    let rules = parse(&text);
    assert_eq!(classifier::check_ties(&rules, Mode::Developer).len(), 1);
    let (_d, root) = home(&["Library/Caches/AB/x"]);
    let items = run(&root, &rules);
    assert!(items.iter().all(|i| i.rule_id == UNKNOWN_RULE_ID));
    assert!(items.iter().all(|i| i.tier == Tier::Keep));
}

#[test]
fn requires_must_be_present_inside() {
    let rules = parse(&rule(
        "t.marked",
        "Library/Caches/*",
        "likely_safe",
        "match.requires = [\"marker.txt\"]\nper_item = true",
    ));
    let (_d, root) = home(&[
        "Library/Caches/Yes/marker.txt",
        "Library/Caches/No/other.txt",
    ]);
    let items = run(&root, &rules);
    assert_eq!(
        item(&items, &root, "Library/Caches/Yes").unwrap().rule_id,
        "t.marked"
    );
    assert!(item(&items, &root, "Library/Caches/No").is_none());
}

#[test]
fn a_rule_inside_an_item_makes_it_more_careful() {
    let text = rule("t.outer", "Library/Caches/Tool", "likely_safe", "")
        + &rule("t.inner", "Library/Caches/Tool/Keep", "keep", "");
    let rules = parse(&text);
    let (_d, root) = home(&["Library/Caches/Tool/Keep/x", "Library/Caches/Tool/y"]);
    let items = run(&root, &rules);
    let outer = item(&items, &root, "Library/Caches/Tool").unwrap();
    assert_eq!(outer.rule_id, "t.outer");
    assert_eq!(outer.tier, Tier::Keep);
    assert_eq!(outer.action, Action::ShowOnly);
    assert!(!outer.regenerable);
    assert_eq!(
        outer.lowered,
        vec![Lowered::NestedRule {
            rule_id: "t.inner".to_string()
        }]
    );
    // The inner folder is part of the outer item, not an item of its own.
    assert!(item(&items, &root, "Library/Caches/Tool/Keep").is_none());
}

#[test]
fn a_git_repository_inside_an_item_makes_it_keep() {
    let rules = parse(&rule("t.cache", "Library/Caches/Tool", "likely_safe", ""));
    let (_d, root) = home(&["Library/Caches/Tool/checkout/.git/HEAD"]);
    let items = run(&root, &rules);
    let it = item(&items, &root, "Library/Caches/Tool").unwrap();
    assert_eq!(it.tier, Tier::Keep);
    assert!(it
        .lowered
        .contains(&Lowered::Protected(BlockReason::GitRepository)));
}

#[test]
fn a_media_library_inside_an_item_makes_it_keep() {
    let rules = parse(&rule("t.cache", "Library/Caches/Tool", "likely_safe", ""));
    let (_d, root) = home(&["Library/Caches/Tool/Old.photoslibrary/database/x"]);
    let items = run(&root, &rules);
    let it = item(&items, &root, "Library/Caches/Tool").unwrap();
    assert_eq!(it.tier, Tier::Keep);
    assert!(it
        .lowered
        .contains(&Lowered::Protected(BlockReason::MediaLibrary)));
}

#[test]
fn an_odd_name_inside_an_item_needs_review() {
    use std::os::unix::ffi::OsStrExt;
    let rules = parse(&rule("t.cache", "Library/Caches/Tool", "likely_safe", ""));
    let (_d, root) = home(&["Library/Caches/Tool/a"]);
    let odd = std::ffi::OsStr::from_bytes(b"bad\xffname");
    if let Err(e) = std::fs::create_dir(root.join("Library/Caches/Tool").join(odd)) {
        // APFS refuses names that are not valid UTF-8 (EILSEQ, "Illegal byte
        // sequence"), so this case can only be built on other file systems.
        // CI also runs the core tests on Linux, where it is built and checked.
        const EILSEQ_MACOS: i32 = 92;
        if cfg!(target_os = "macos") && e.raw_os_error() == Some(EILSEQ_MACOS) {
            eprintln!("skipped: this file system does not allow non-UTF-8 names");
            return;
        }
        panic!("could not create the odd name: {e}");
    }
    let items = run(&root, &rules);
    let it = item(&items, &root, "Library/Caches/Tool").unwrap();
    assert_eq!(it.tier, Tier::ReviewFirst);
    assert_eq!(it.lowered, vec![Lowered::UnusualName]);
}

#[test]
fn developer_rules_are_left_out_in_simple_mode() {
    let text = rule("t.dev", "Library/Caches/Tool", "likely_safe", "")
        .replace("mode = \"simple\"", "mode = \"developer\"");
    let rules = parse(&text);
    let (_d, root) = home(&["Library/Caches/Tool/a"]);
    let opts = ScanOptions {
        record_files: Some(record_files(&rules, Mode::Simple)),
        ..ScanOptions::default()
    };
    let result = scan(&root, opts, |_| {}).unwrap();
    let simple = classify(&result, &rules, Mode::Simple);
    assert!(simple.iter().all(|i| i.rule_id == UNKNOWN_RULE_ID));
    let developer = classify(&result, &rules, Mode::Developer);
    assert!(developer.iter().any(|i| i.rule_id == "t.dev"));
}
