//! Property tests: no argument combination makes `validate` return `Ok` for
//! a path under any hard-blocked root.
//!
//! Paths are generated under each blocked root, with random extra parts and
//! random letter case on the root, then created inside a fake home so every
//! filesystem check really runs. Each one is validated with and without
//! individual selection and with the real and random device ids.

mod common;

use common::FakeHome;
use proptest::prelude::*;
use safety::protected::{
    self, Scope, BUILD_OUTPUT_EXCEPTIONS, MEDIA_LIBRARY_EXTENSIONS, SIGNING_EXTENSIONS,
};
use safety::{validate, Refusal};

/// The roots that block everything below them.
fn blocked_roots() -> Vec<Vec<String>> {
    let mut roots: Vec<Vec<String>> = protected::HARD_BLOCKED
        .iter()
        .filter(|e| e.scope == Scope::Subtree)
        .map(|e| e.components.iter().map(|s| s.to_string()).collect())
        .collect();
    for ext in MEDIA_LIBRARY_EXTENSIONS {
        roots.push(vec!["Pictures".into(), format!("Photos Library.{ext}")]);
    }
    for ext in SIGNING_EXTENSIONS {
        roots.push(vec![
            "Library".into(),
            "Caches".into(),
            format!("thing.{ext}"),
        ]);
    }
    roots.push(vec!["Library".into(), "Messages".into(), "chat.db".into()]);
    roots.push(vec!["Library".into(), "Caches".into(), ".git".into()]);
    roots
}

/// The paths blocked only as themselves (home's main folders and Messages).
fn blocked_exact() -> Vec<Vec<String>> {
    protected::HARD_BLOCKED
        .iter()
        .filter(|e| e.scope == Scope::Itself)
        .map(|e| e.components.iter().map(|s| s.to_string()).collect())
        .collect()
}

/// One extra path part: ordinary names, dot names, spaces and non-ASCII.
fn part() -> impl Strategy<Value = String> {
    "[A-Za-z0-9 ._~é-]{1,12}".prop_filter("not . or ..", |s| s != "." && s != "..")
}

/// Flips the letter case of each root part where the mask says so.
fn recase(root: &[String], mask: &[bool]) -> Vec<String> {
    root.iter()
        .zip(mask.iter().chain(std::iter::repeat(&false)))
        .map(|(p, flip)| if *flip { p.to_uppercase() } else { p.clone() })
        .collect()
}

/// Creates the path in the fake home (folders, then a file at the end) and
/// returns it.
fn create(h: &FakeHome, parts: &[String], last_is_file: bool) -> std::path::PathBuf {
    let path = h.path(&parts.join("/"));
    if parts.is_empty() {
        return path;
    }
    if last_is_file {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("make parents");
        // A part may already exist as a folder from an earlier root part.
        if !path.exists() {
            std::fs::write(&path, b"x").expect("write");
        }
    } else {
        std::fs::create_dir_all(&path).expect("make folders");
    }
    path
}

/// Validates with every argument combination and asserts none is `Ok`.
/// With the real device id the refusal must be a hard block.
fn assert_never_ok(h: &FakeHome, path: &std::path::Path, other_dev: u64) {
    for selected in [false, true] {
        let real = validate(&h.home, h.dev, path, selected);
        assert!(
            matches!(real, Err(Refusal::HardBlocked(_))),
            "{} gave {real:?}",
            path.display()
        );
        let other = validate(&h.home, other_dev, path, selected);
        assert!(other.is_err(), "{} gave {other:?}", path.display());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn nothing_under_a_blocked_root_is_ever_ok(
        root_index in any::<prop::sample::Index>(),
        mask in prop::collection::vec(any::<bool>(), 0..4),
        extra in prop::collection::vec(part(), 0..4),
        last_is_file in any::<bool>(),
        other_dev in any::<u64>(),
    ) {
        let roots = blocked_roots();
        let root = recase(roots[root_index.index(roots.len())].as_slice(), &mask);
        let mut parts = root;
        parts.extend(extra);
        let h = FakeHome::new();
        let path = create(&h, &parts, last_is_file);
        assert_never_ok(&h, &path, other_dev);
    }

    #[test]
    fn blocked_folders_themselves_are_never_ok(
        index in any::<prop::sample::Index>(),
        mask in prop::collection::vec(any::<bool>(), 0..3),
        other_dev in any::<u64>(),
    ) {
        let exact = blocked_exact();
        let parts = recase(exact[index.index(exact.len())].as_slice(), &mask);
        let h = FakeHome::new();
        let path = create(&h, &parts, false);
        assert_never_ok(&h, &path, other_dev);
    }

    #[test]
    fn nothing_inside_a_repository_is_ever_ok_outside_build_output(
        repo in prop::collection::vec(part(), 1..3),
        extra in prop::collection::vec(part(), 0..4),
        last_is_file in any::<bool>(),
        other_dev in any::<u64>(),
    ) {
        // The first part below the repository root must not be an exception.
        prop_assume!(extra.first().is_none_or(|p| !BUILD_OUTPUT_EXCEPTIONS.contains(&p.as_str())));
        // Keep the repository away from paths that are refused for other reasons.
        let mut parts = vec!["Library".to_string(), "Caches".to_string()];
        parts.extend(repo);
        let h = FakeHome::new();
        let root = create(&h, &parts, false);
        std::fs::create_dir_all(root.join(".git")).expect("make .git");
        parts.extend(extra);
        let path = create(&h, &parts, last_is_file && parts.len() > 2);
        assert_never_ok(&h, &path, other_dev);
    }

    #[test]
    fn pure_hard_block_matches_every_case_variant(
        root_index in any::<prop::sample::Index>(),
        mask in prop::collection::vec(any::<bool>(), 0..4),
        extra in prop::collection::vec(part(), 0..4),
    ) {
        let roots = blocked_roots();
        let mut parts = recase(roots[root_index.index(roots.len())].as_slice(), &mask);
        parts.extend(extra);
        let rel: Vec<&str> = parts.iter().map(String::as_str).collect();
        prop_assert!(protected::hard_block(&rel).is_some(), "{rel:?}");
    }
}
