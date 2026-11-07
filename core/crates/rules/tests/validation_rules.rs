//! One test per validation rule: each breaks one thing in a good rule and
//! proves the rule is rejected, on the right setting, for the right reason.

mod common;

use std::fs;

use common::{assert_rejected, parse, with, GOOD};
use rules::{load_dir, GlobProblem, Problem};
use safety::protected::{BlockReason, ReviewReason};

#[test]
fn the_good_rule_loads() {
    let (rules, errors) = parse(GOOD);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(rules.len(), 1);
}

// ---------------------------------------------------------------------------
// File shape
// ---------------------------------------------------------------------------

#[test]
fn rejects_a_file_that_is_not_toml() {
    let (rules, errors) = parse("[[rule]\nid = ");
    assert!(rules.is_empty());
    assert_eq!(errors.len(), 1);
    assert!(matches!(errors[0].problem, Problem::NotToml(_)));
}

#[test]
fn rejects_unknown_sections_in_a_file() {
    let text = format!("{GOOD}\n[settings]\nx = 1\n");
    let (rules, errors) = parse(&text);
    assert_eq!(rules.len(), 1, "the good rule still loads");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].problem, Problem::UnknownSection);
    assert_eq!(errors[0].field.as_deref(), Some("settings"));
}

#[test]
fn rejects_rule_that_is_not_a_list_of_tables() {
    let (_, errors) = parse("rule = \"xcode.derived_data\"\n");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].problem, Problem::RuleListShape);
    let (_, errors) = parse("rule = [1]\n");
    assert_eq!(errors[0].problem, Problem::RuleListShape);
}

// ---------------------------------------------------------------------------
// Unknown fields (deny_unknown_fields)
// ---------------------------------------------------------------------------

#[test]
fn rejects_unknown_fields() {
    assert_rejected(&with("colour", "colour = \"red\""), "colour", |p| {
        *p == Problem::UnknownField
    });
}

#[test]
fn rejects_unknown_fields_inside_match() {
    assert_rejected(
        &with("match.glob", "match.glob = \"x\""),
        "match.glob",
        |p| *p == Problem::UnknownField,
    );
}

#[test]
fn rejects_misspelt_optional_fields() {
    // A typo in an optional setting must not silently fall back to the default.
    assert_rejected(&with("per_items", "per_items = true"), "per_items", |p| {
        *p == Problem::UnknownField
    });
}

// ---------------------------------------------------------------------------
// Required fields and types
// ---------------------------------------------------------------------------

#[test]
fn rejects_each_missing_required_field() {
    let required = [
        ("id", "id"),
        ("version", "version"),
        ("match.path", "match"),
        ("category", "category"),
        ("tier", "tier"),
        ("confidence", "confidence"),
        ("regenerable", "regenerable"),
        ("mode", "mode"),
        ("title", "title"),
        ("explain", "explain"),
        ("risk", "risk"),
        ("action", "action"),
    ];
    for (line_key, field) in required {
        assert_rejected(&with(line_key, ""), field, |p| *p == Problem::MissingField);
    }
}

#[test]
fn rejects_missing_match_path() {
    let text = with("match.path", "match.requires = [\"Info.plist\"]");
    assert_rejected(&text, "match.path", |p| *p == Problem::MissingField);
}

#[test]
fn optional_fields_may_be_left_out() {
    let text = with("restore", "");
    let text = text.replace("per_item = true\n", "");
    let (rules, errors) = parse(&text);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(!rules[0].per_item, "per_item defaults to false");
    assert_eq!(rules[0].restore, None);
}

#[test]
fn rejects_wrong_types() {
    let cases = [
        ("id", "id = 7"),
        ("version", "version = \"1\""),
        ("regenerable", "regenerable = \"yes\""),
        ("per_item", "per_item = 1"),
        ("title", "title = [\"Xcode\"]"),
        ("tier", "tier = true"),
    ];
    for (field, line) in cases {
        assert_rejected(&with(field, line), field, |p| {
            matches!(p, Problem::WrongType { .. })
        });
    }
    assert_rejected(
        &with("match.path", "match = \"Library/Caches\""),
        "match",
        |p| matches!(p, Problem::WrongType { .. }),
    );
    let text = with(
        "match.path",
        "match.path = \"Library/x\"\nmatch.requires = \"Info.plist\"",
    );
    assert_rejected(&text, "match.requires", |p| {
        matches!(p, Problem::WrongType { .. })
    });
}

#[test]
fn rejects_values_outside_each_enum() {
    let cases = [
        ("category", "rubbish"),
        ("tier", "maybe"),
        ("confidence", "certain"),
        ("mode", "expert"),
        ("action", "delete"),
    ];
    for (field, value) in cases {
        let line = format!("{field} = \"{value}\"");
        assert_rejected(
            &with(field, &line),
            field,
            |p| matches!(p, Problem::NotAChoice { found, .. } if found == value),
        );
    }
}

#[test]
fn enum_values_are_case_sensitive() {
    assert_rejected(&with("tier", "tier = \"Likely_Safe\""), "tier", |p| {
        matches!(p, Problem::NotAChoice { .. })
    });
}

// ---------------------------------------------------------------------------
// id and version
// ---------------------------------------------------------------------------

#[test]
fn rejects_ids_that_are_not_dotted_lowercase() {
    for id in [
        "xcode",
        "Xcode.derived_data",
        "xcode.derived-data",
        "xcode..x",
        "",
    ] {
        let line = format!("id = \"{id}\"");
        assert_rejected(&with("id", &line), "id", |p| *p == Problem::BadId);
    }
}

#[test]
fn rejects_version_below_one() {
    for v in ["0", "-1"] {
        let line = format!("version = {v}");
        assert_rejected(&with("version", &line), "version", |p| {
            *p == Problem::BadVersion
        });
    }
}

// ---------------------------------------------------------------------------
// match.path and match.requires
// ---------------------------------------------------------------------------

#[test]
fn rejects_match_paths_that_are_not_relative_globs() {
    let cases = [
        ("/Users/sam/Library/Caches", GlobProblem::Absolute),
        ("~/Library/Caches", GlobProblem::Tilde),
        ("Library/Caches/../Keychains", GlobProblem::DotName),
        ("Library/Caches/{a,b}", GlobProblem::Braces),
        ("Library//Caches", GlobProblem::EmptyName),
        ("", GlobProblem::Empty),
    ];
    for (pattern, want) in cases {
        let line = format!("match.path = \"{pattern}\"");
        assert_rejected(&with("match.path", &line), "match.path", |p| {
            *p == Problem::BadPattern(want.clone())
        });
    }
}

#[test]
fn rejects_bad_requires_patterns() {
    let text = with(
        "match.path",
        "match.path = \"Library/Caches/x\"\nmatch.requires = [\"../Info.plist\"]",
    );
    assert_rejected(&text, "match.requires", |p| {
        *p == Problem::BadPattern(GlobProblem::DotName)
    });
}

#[test]
fn rejects_match_paths_that_could_match_hard_blocked_paths() {
    let cases = [
        ("Library/Keychains/*", BlockReason::Keychains),
        ("Library/Mail", BlockReason::Mail),
        ("Library/*", BlockReason::Keychains),
        ("*", BlockReason::MainFolder),
        ("Library", BlockReason::MainFolder),
        ("**", BlockReason::HomeFolder),
        ("**/node_modules", BlockReason::Keychains),
        (".ssh/*", BlockReason::Credentials),
        ("library/mobile documents/x", BlockReason::ICloudDrive),
        ("Library/CloudStorage/*", BlockReason::CloudStorage),
        ("Library/Messages/*", BlockReason::MessagesDatabase),
        ("Pictures/*.photoslibrary", BlockReason::MediaLibrary),
        ("Projects/*.p12", BlockReason::SigningMaterial),
        ("Library/MobileDevice/*", BlockReason::SigningMaterial),
        ("Developer/*/.git", BlockReason::GitRepository),
        (".docker", BlockReason::Credentials),
    ];
    for (pattern, reason) in cases {
        // Even a show-only, keep-tier rule may not point at these.
        let text = with("match.path", &format!("match.path = \"{pattern}\""))
            .replace("tier = \"likely_safe\"", "tier = \"keep\"")
            .replace("action = \"trash\"", "action = \"show_only\"");
        let err = assert_rejected(&text, "match.path", |p| {
            *p == Problem::MatchesHardBlocked(reason)
        });
        assert!(err.to_string().contains("never removes"), "{err}");
    }
}

#[test]
fn likely_safe_rules_may_not_match_review_only_paths() {
    let cases = [
        ("Downloads/*.dmg", ReviewReason::PersonalFile),
        ("Library/Containers/*", ReviewReason::AppLeftovers),
        (
            "Library/Application Support/MobileSync/Backup/*",
            ReviewReason::DeviceBackup,
        ),
        (".android/avd/*", ReviewReason::AndroidEmulator),
        (
            "Library/Developer/Xcode/Archives/*",
            ReviewReason::XcodeArchive,
        ),
        ("Projects/*/.env.local", ReviewReason::EnvFile),
        (".claude/projects/*", ReviewReason::AiToolHistory),
        ("Movies/*.fcpbundle", ReviewReason::CreativeProject),
    ];
    for (pattern, reason) in cases {
        let text = with("match.path", &format!("match.path = \"{pattern}\""));
        assert_rejected(&text, "match.path", |p| {
            *p == Problem::LikelySafeMatchesReviewOnly(reason)
        });
        // The same rule is fine when it asks the user to review first.
        let review = text.replace("tier = \"likely_safe\"", "tier = \"review_first\"");
        let (rules, errors) = parse(&review);
        assert!(errors.is_empty(), "{pattern}: {errors:#?}");
        assert_eq!(rules.len(), 1);
    }
}

// ---------------------------------------------------------------------------
// Text limits
// ---------------------------------------------------------------------------

#[test]
fn rejects_title_over_40_characters() {
    let line = format!("title = \"{}\"", "a".repeat(41));
    assert_rejected(&with("title", &line), "title", |p| {
        *p == Problem::TooLong {
            limit: 40,
            found: 41,
        }
    });
    // Exactly 40 is fine; characters, not bytes, are counted.
    let ok = format!("title = \"{}\"", "é".repeat(40));
    assert!(parse(&with("title", &ok)).1.is_empty());
}

#[test]
fn rejects_explain_over_200_characters() {
    let line = format!("explain = \"{}.\"", "a".repeat(200));
    assert_rejected(&with("explain", &line), "explain", |p| {
        *p == Problem::TooLong {
            limit: 200,
            found: 201,
        }
    });
}

#[test]
fn rejects_explain_with_more_than_two_sentences() {
    let line = "explain = \"Files Xcode makes. They speed up builds. Xcode remakes them.\"";
    assert_rejected(&with("explain", line), "explain", |p| {
        *p == Problem::TooManySentences { found: 3 }
    });
    let ok = "explain = \"Files Xcode makes. They speed up builds.\"";
    assert!(parse(&with("explain", ok)).1.is_empty());
}

#[test]
fn rejects_risk_over_200_characters() {
    let line = format!("risk = \"{}\"", "a".repeat(201));
    assert_rejected(&with("risk", &line), "risk", |p| {
        *p == Problem::TooLong {
            limit: 200,
            found: 201,
        }
    });
}

#[test]
fn rejects_restore_over_200_characters() {
    let line = format!("restore = \"{}\"", "a".repeat(201));
    assert_rejected(&with("restore", &line), "restore", |p| {
        *p == Problem::TooLong {
            limit: 200,
            found: 201,
        }
    });
}

#[test]
fn rejects_empty_text() {
    for field in ["title", "explain", "risk", "restore"] {
        let line = format!("{field} = \"   \"");
        assert_rejected(&with(field, &line), field, |p| *p == Problem::EmptyText);
    }
}

// ---------------------------------------------------------------------------
// action and command
// ---------------------------------------------------------------------------

#[test]
fn command_action_requires_a_command() {
    let text = with("action", "action = \"command\"");
    assert_rejected(&text, "command", |p| *p == Problem::MissingCommand);
}

#[test]
fn command_must_be_on_the_allow_list() {
    let text = with(
        "action",
        "action = \"command\"\ncommand = \"rm_rf_everything\"",
    );
    assert_rejected(&text, "command", |p| {
        *p == Problem::CommandNotAllowed("rm_rf_everything".to_string())
    });
    // Close is not enough: ids match exactly.
    let text = with("action", "action = \"command\"\ncommand = \"Brew_Cleanup\"");
    assert_rejected(&text, "command", |p| {
        matches!(p, Problem::CommandNotAllowed(_))
    });
}

#[test]
fn allowed_command_loads() {
    let text = with("action", "action = \"command\"\ncommand = \"brew_cleanup\"");
    let (rules, errors) = parse(&text);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(rules[0].command.as_deref(), Some("brew_cleanup"));
}

#[test]
fn command_is_refused_unless_action_is_command() {
    for action in ["trash", "show_only"] {
        let text = with(
            "action",
            &format!("action = \"{action}\"\ncommand = \"brew_cleanup\""),
        );
        assert_rejected(&text, "command", |p| {
            *p == Problem::CommandWithoutCommandAction
        });
    }
}

// ---------------------------------------------------------------------------
// Tier and confidence; regenerable
// ---------------------------------------------------------------------------

#[test]
fn low_confidence_can_never_be_likely_safe() {
    let text = with("confidence", "confidence = \"low\"");
    assert_rejected(&text, "confidence", |p| {
        *p == Problem::LowConfidenceLikelySafe
    });
    // Medium is allowed, and low is fine for review_first.
    assert!(parse(&with("confidence", "confidence = \"medium\""))
        .1
        .is_empty());
    let review = text.replace("tier = \"likely_safe\"", "tier = \"review_first\"");
    assert!(parse(&review).1.is_empty());
}

#[test]
fn not_regenerable_forbids_permanent_delete() {
    let (rules, _) = parse(GOOD);
    assert!(rules[0].allows_permanent_delete());
    let (rules, errors) = parse(&with("regenerable", "regenerable = false"));
    assert!(errors.is_empty());
    assert!(!rules[0].allows_permanent_delete());
}

// ---------------------------------------------------------------------------
// Duplicates and isolation
// ---------------------------------------------------------------------------

#[test]
fn duplicate_ids_across_files_are_all_rejected() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(dir.path().join("a.toml"), GOOD).expect("write");
    fs::write(dir.path().join("b.toml"), GOOD).expect("write");
    let (rules, errors) = load_dir(dir.path());
    assert!(rules.is_empty(), "neither copy may load");
    assert_eq!(errors.len(), 2);
    for err in &errors {
        assert!(matches!(err.problem, Problem::DuplicateId { .. }), "{err}");
        assert_eq!(err.field.as_deref(), Some("id"));
    }
    assert!(
        errors[0].to_string().contains("b.toml, rule 1"),
        "{}",
        errors[0]
    );
    assert!(
        errors[1].to_string().contains("a.toml, rule 1"),
        "{}",
        errors[1]
    );
}

#[test]
fn duplicate_ids_in_one_file_are_rejected() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(dir.path().join("a.toml"), format!("{GOOD}\n{GOOD}")).expect("write");
    let (rules, errors) = load_dir(dir.path());
    assert!(rules.is_empty());
    assert_eq!(errors.len(), 2);
}

#[test]
fn a_bad_rule_does_not_stop_good_ones() {
    let other = GOOD.replace("xcode.derived_data", "xcode.other");
    let bad = with("tier", "tier = \"maybe\"").replace("xcode.derived_data", "xcode.bad");
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(dir.path().join("a.toml"), format!("{GOOD}\n{bad}\n{other}")).expect("write");
    fs::write(dir.path().join("notes.txt"), "not a rule file").expect("write");
    let (rules, errors) = load_dir(dir.path());
    let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["xcode.derived_data", "xcode.other"]);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].position, Some(2));
    assert_eq!(errors[0].rule_id.as_deref(), Some("xcode.bad"));
}

#[test]
fn a_missing_rules_folder_is_an_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (rules, errors) = load_dir(&dir.path().join("nope"));
    assert!(rules.is_empty());
    assert_eq!(errors.len(), 1);
    assert!(matches!(errors[0].problem, Problem::FolderUnreadable(_)));
}

// ---------------------------------------------------------------------------
// Messages a reviewer reads
// ---------------------------------------------------------------------------

#[test]
fn error_messages_name_file_rule_and_setting() {
    let err = assert_rejected(&with("tier", "tier = \"maybe\""), "tier", |_| true);
    assert_eq!(
        err.to_string(),
        "test.toml, rule 1 (\"xcode.derived_data\"), setting \"tier\": \
         \"maybe\" is not allowed here; use one of: likely_safe, review_first, keep"
    );
    let err = assert_rejected(
        &with("match.path", "match.path = \"Library/Keychains/*\""),
        "match.path",
        |_| true,
    );
    assert_eq!(
        err.to_string(),
        "test.toml, rule 1 (\"xcode.derived_data\"), setting \"match.path\": \
         this pattern could match your keychains, which Diskwell never removes"
    );
}

#[test]
fn every_problem_in_a_rule_is_reported() {
    let text = with("tier", "tier = \"maybe\"").replace("mode = \"developer\"", "mode = \"x\"");
    let (_, errors) = parse(&text);
    let fields: Vec<_> = errors.iter().filter_map(|e| e.field.as_deref()).collect();
    assert_eq!(fields, ["tier", "mode"]);
}
