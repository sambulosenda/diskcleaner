//! `load_texts` applies the same checks as `load_dir`.

use std::path::Path;

use rules::{load_dir, load_texts, Problem};

fn repo_rules_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rules")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(repo_rules_dir().join(name)).unwrap()
}

#[test]
fn gives_the_same_rules_as_load_dir() {
    let names = [
        "ai_models.toml",
        "ai_tools.toml",
        "android.toml",
        "editors.toml",
        "everyday.toml",
        "go.toml",
        "homebrew.toml",
        "js.toml",
        "jvm.toml",
        "python.toml",
        "rust.toml",
        "web_tools.toml",
        "xcode.toml",
    ];
    let texts: Vec<String> = names.iter().map(|n| read(n)).collect();
    let files: Vec<(&str, &str)> = names
        .iter()
        .zip(&texts)
        .map(|(n, t)| (*n, t.as_str()))
        .collect();
    let (from_texts, errors) = load_texts(&files);
    assert!(errors.is_empty(), "{errors:?}");
    let (from_dir, dir_errors) = load_dir(&repo_rules_dir());
    assert!(dir_errors.is_empty(), "{dir_errors:?}");
    assert_eq!(from_texts, from_dir);
}

#[test]
fn drops_ids_used_in_two_files() {
    let text = read("xcode.toml");
    let (rules, errors) = load_texts(&[("a.toml", &text), ("b.toml", &text)]);
    assert!(rules.is_empty(), "duplicated rules must all be dropped");
    assert!(!errors.is_empty());
    assert!(errors
        .iter()
        .all(|e| matches!(e.problem, Problem::DuplicateId { .. })));
}

#[test]
fn reports_a_bad_file_by_its_name() {
    let (rules, errors) = load_texts(&[("broken.toml", "[[rule]]\nid = 3\n")]);
    assert!(rules.is_empty());
    assert!(errors.iter().any(|e| e.file == Path::new("broken.toml")));
}
