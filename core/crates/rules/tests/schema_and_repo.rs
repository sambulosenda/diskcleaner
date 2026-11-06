//! The schema document's examples load, the repository's `rules/` folder
//! loads with no errors (so a bad rule blocks the build), and the command
//! allow-list matches `docs/safety/allowed-commands.md`.

mod common;

use std::collections::BTreeSet;
use std::fs;

use common::{parse, repo_root};
use rules::commands::ALLOWED_COMMAND_IDS;
use rules::{load_dir, Action, Category, Confidence, Mode, Tier};

/// The ```toml blocks of `docs/safety/rule-schema.md`, joined.
fn schema_examples() -> String {
    let doc = fs::read_to_string(repo_root().join("docs/safety/rule-schema.md"))
        .expect("read rule-schema.md");
    let mut out = String::new();
    let mut inside = false;
    for line in doc.lines() {
        if line.starts_with("```toml") {
            inside = true;
        } else if line.starts_with("```") {
            inside = false;
        } else if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[test]
fn the_three_schema_examples_load() {
    let (rules, errors) = parse(&schema_examples());
    assert!(errors.is_empty(), "{errors:#?}");
    let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "xcode.derived_data",
            "ios.device_backup",
            "ios.simulators_unavailable"
        ]
    );

    let derived = &rules[0];
    assert_eq!(
        derived.matcher.path,
        "Library/Developer/Xcode/DerivedData/*"
    );
    assert_eq!(derived.category, Category::Build);
    assert_eq!(derived.tier, Tier::LikelySafe);
    assert_eq!(derived.confidence, Confidence::High);
    assert_eq!(derived.mode, Mode::Developer);
    assert_eq!(derived.action, Action::Trash);
    assert!(derived.per_item && derived.regenerable);

    let backup = &rules[1];
    assert_eq!(backup.matcher.requires, ["Info.plist"]);
    assert_eq!(backup.tier, Tier::ReviewFirst);
    assert!(!backup.allows_permanent_delete());
    assert_eq!(backup.restore, None);

    let sims = &rules[2];
    assert_eq!(sims.action, Action::Command);
    assert_eq!(
        sims.command.as_deref(),
        Some("xcrun_simctl_delete_unavailable")
    );
    assert!(!sims.per_item, "per_item defaults to false");
}

#[test]
fn every_rule_in_the_repository_loads() {
    let (rules, errors) = load_dir(&repo_root().join("rules"));
    let report: Vec<String> = errors.iter().map(ToString::to_string).collect();
    assert!(
        errors.is_empty(),
        "rules/ has {} problem(s):\n{}",
        errors.len(),
        report.join("\n")
    );
    let _ = rules;
}

#[test]
fn allow_list_matches_allowed_commands_doc() {
    let doc = fs::read_to_string(repo_root().join("docs/safety/allowed-commands.md"))
        .expect("read allowed-commands.md");
    // Table rows look like: | `id` | preview | command | notes |
    let from_doc: BTreeSet<&str> = doc
        .lines()
        .filter_map(|l| l.strip_prefix("| `"))
        .filter_map(|l| l.split_once('`').map(|(id, _)| id))
        .collect();
    let in_code: BTreeSet<&str> = ALLOWED_COMMAND_IDS.iter().copied().collect();
    assert!(!from_doc.is_empty(), "no command ids found in the doc");
    assert_eq!(in_code, from_doc);
}

#[test]
fn enum_names_match_serde() {
    // Every name listed in error messages is accepted, so the lists cannot drift.
    let base = common::GOOD;
    for (field, names) in [
        ("category", Category::NAMES),
        ("tier", Tier::NAMES),
        ("confidence", Confidence::NAMES),
        ("mode", Mode::NAMES),
    ] {
        for name in names {
            let text = common::with(field, &format!("{field} = \"{name}\""));
            let (_, errors) = parse(&text);
            // Some combinations fail later checks (low + likely_safe); none may
            // fail on the value itself.
            assert!(
                errors.iter().all(|e| e.field.as_deref() != Some(field)
                    || matches!(e.problem, rules::Problem::LowConfidenceLikelySafe)),
                "{field} = {name}: {errors:#?}"
            );
        }
    }
    assert!(!base.is_empty());
    assert_eq!(Action::NAMES, ["trash", "command", "show_only"]);
}
