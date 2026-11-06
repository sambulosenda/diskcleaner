//! The rule files built into the core.
//!
//! The app never reads rules from a path the UI gives it: the files in the
//! repository's `rules/` folder are compiled in here. A test checks that
//! every `*.toml` file in that folder is listed, so a new rule file cannot
//! be forgotten.

use rules::Rule;

use crate::CoreError;

/// Every rule file, as `(file name, text)`, in file-name order.
pub(crate) const FILES: &[(&str, &str)] = &[
    (
        "ai_tools.toml",
        include_str!("../../../../rules/ai_tools.toml"),
    ),
    (
        "android.toml",
        include_str!("../../../../rules/android.toml"),
    ),
    (
        "editors.toml",
        include_str!("../../../../rules/editors.toml"),
    ),
    (
        "everyday.toml",
        include_str!("../../../../rules/everyday.toml"),
    ),
    ("js.toml", include_str!("../../../../rules/js.toml")),
    ("rust.toml", include_str!("../../../../rules/rust.toml")),
    ("xcode.toml", include_str!("../../../../rules/xcode.toml")),
];

/// Loads and validates the built-in rules.
///
/// Any rule error is refused as `rules_invalid`: tests catch this long
/// before a release, and a core with a broken rule file should not scan at
/// all. Rules that tie are not refused here: the classifier already treats
/// whatever they match as unknown with tier `keep`, and `check_ties` logs
/// them.
pub(crate) fn load() -> Result<Vec<Rule>, CoreError> {
    let (rules, errors) = rules::load_texts(FILES);
    if let Some(first) = errors.first() {
        return Err(CoreError::RulesInvalid {
            reason: first.to_string(),
        });
    }
    for mode in [rules::Mode::Simple, rules::Mode::Developer] {
        // Logs each tie as an error.
        let _ = classifier::check_ties(&rules, mode);
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn rules_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rules")
    }

    #[test]
    fn every_rule_file_is_built_in() {
        let mut on_disk: Vec<String> = std::fs::read_dir(rules_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".toml"))
            .collect();
        on_disk.sort();
        let built_in: Vec<&str> = FILES.iter().map(|(n, _)| *n).collect();
        assert_eq!(on_disk, built_in);
    }

    #[test]
    fn built_in_rules_match_the_rules_folder() {
        let built_in = load().unwrap();
        let (from_dir, errors) = rules::load_dir(&rules_dir());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(built_in, from_dir);
        assert!(!built_in.is_empty());
    }

    /// No two built-in rules tie. A tie makes the classifier show what
    /// both rules match as unknown `keep`, so any tie is a rule bug.
    #[test]
    fn no_built_in_rules_tie() {
        let rules = load().unwrap();
        assert!(classifier::check_ties(&rules, rules::Mode::Simple).is_empty());
        assert!(classifier::check_ties(&rules, rules::Mode::Developer).is_empty());
    }
}
