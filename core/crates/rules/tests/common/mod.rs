//! Shared helpers for the rules tests.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use rules::{parse_file, Problem, Rule, RuleError};

/// The repository root (three levels above this crate).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("find repo root")
}

/// A valid rule (the schema's first example), used as the starting point
/// for tests that break one thing at a time.
pub const GOOD: &str = r#"
[[rule]]
id = "xcode.derived_data"
version = 1
match.path = "Library/Developer/Xcode/DerivedData/*"
category = "build"
tier = "likely_safe"
confidence = "high"
regenerable = true
mode = "developer"
per_item = true
title = "Xcode build data"
explain = "Temporary files Xcode creates while building a project."
risk = "The next build of this project will take longer."
restore = "Rebuilds automatically on the next Xcode build."
action = "trash"
"#;

/// `GOOD` with the line starting `key =` replaced by `line`, or with `line`
/// added at the end when no such line exists. `line` may be empty to
/// remove the setting.
pub fn with(key: &str, line: &str) -> String {
    let prefix = format!("{key} =");
    let mut found = false;
    let mut out: Vec<String> = GOOD
        .lines()
        .map(|l| {
            if l.starts_with(&prefix) {
                found = true;
                line.to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(line.to_string());
    }
    out.join("\n")
}

/// Parses `text` as `test.toml`.
pub fn parse(text: &str) -> (Vec<Rule>, Vec<RuleError>) {
    let (rules, errors) = parse_file(Path::new("test.toml"), text);
    (rules.into_iter().map(|(_, r)| r).collect(), errors)
}

/// Asserts that `text` is rejected with exactly one error, on `field`,
/// with a problem accepted by `is_expected`. Returns the error.
pub fn assert_rejected(
    text: &str,
    field: &str,
    is_expected: impl Fn(&Problem) -> bool,
) -> RuleError {
    let (rules, errors) = parse(text);
    assert!(rules.is_empty(), "rule should be rejected:\n{text}");
    assert_eq!(errors.len(), 1, "expected one error, got {errors:#?}");
    let err = errors.into_iter().next().expect("one error");
    assert_eq!(err.field.as_deref(), Some(field), "wrong field: {err}");
    assert!(is_expected(&err.problem), "unexpected problem: {err}");
    err
}
