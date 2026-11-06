//! Reading one rule file and checking each rule in it.
//!
//! Each rule goes through three stages, and stops at the first stage that
//! finds problems (reporting every problem found in that stage):
//!
//! 1. **Shape**: every setting is known, every required setting is there,
//!    and every value has the right type or is one of the allowed choices.
//!    Done by hand so that messages can name the setting.
//! 2. **Serde**: the rule is read into [`Rule`], whose types use
//!    `deny_unknown_fields`. After stage 1 this should always succeed; it
//!    is the final authority on the format.
//! 3. **Meaning**: the checks from `docs/safety/rule-schema.md` that go
//!    beyond types, plus protected-path checks through the `safety` crate.

use std::path::Path;

use toml::{Table, Value};

use crate::commands;
use crate::error::{Problem, RuleError};
use crate::glob::Glob;
use crate::protection;
use crate::{Action, Category, Confidence, Mode, Rule, Tier};

/// Longest `title`, in characters.
pub(crate) const TITLE_LIMIT: usize = 40;
/// Longest `explain`, `risk` and `restore`, in characters.
pub(crate) const TEXT_LIMIT: usize = 200;
/// Most sentences in `explain`.
pub(crate) const SENTENCE_LIMIT: usize = 2;

/// The type a setting must have.
#[derive(Clone, Copy)]
enum Kind {
    Text,
    Version,
    Bool,
    Choice(&'static [&'static str]),
    TextList,
    MatchTable,
}

/// Every setting of a rule: name, type, and whether it is always required.
/// `command` is required only when `action = "command"`; that is checked in
/// stage 3.
const RULE_FIELDS: &[(&str, Kind, bool)] = &[
    ("id", Kind::Text, true),
    ("version", Kind::Version, true),
    ("match", Kind::MatchTable, true),
    ("category", Kind::Choice(Category::NAMES), true),
    ("tier", Kind::Choice(Tier::NAMES), true),
    ("confidence", Kind::Choice(Confidence::NAMES), true),
    ("regenerable", Kind::Bool, true),
    ("mode", Kind::Choice(Mode::NAMES), true),
    ("title", Kind::Text, true),
    ("explain", Kind::Text, true),
    ("risk", Kind::Text, true),
    ("restore", Kind::Text, false),
    ("action", Kind::Choice(Action::NAMES), true),
    ("command", Kind::Text, false),
    ("per_item", Kind::Bool, false),
];

/// Every setting inside `match`.
const MATCH_FIELDS: &[(&str, Kind, bool)] = &[
    ("path", Kind::Text, true),
    ("requires", Kind::TextList, false),
];

/// Builds errors for one rule.
struct Reporter<'a> {
    file: &'a Path,
    position: usize,
    rule_id: Option<String>,
    errors: Vec<RuleError>,
}

impl Reporter<'_> {
    fn add(&mut self, field: &str, problem: Problem) {
        self.errors.push(RuleError {
            file: self.file.to_path_buf(),
            position: Some(self.position),
            rule_id: self.rule_id.clone(),
            field: Some(field.to_string()),
            problem,
        });
    }
}

/// See [`crate::parse_file`].
pub(crate) fn parse_file(file: &Path, text: &str) -> (Vec<(usize, Rule)>, Vec<RuleError>) {
    let file_error = |field: Option<&str>, position: Option<usize>, problem| RuleError {
        file: file.to_path_buf(),
        position,
        rule_id: None,
        field: field.map(str::to_string),
        problem,
    };
    let top: Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(e) => {
            let msg = e.message().to_string();
            return (
                Vec::new(),
                vec![file_error(None, None, Problem::NotToml(msg))],
            );
        }
    };
    let mut errors = Vec::new();
    for key in top.keys().filter(|k| *k != "rule") {
        errors.push(file_error(Some(key), None, Problem::UnknownSection));
    }
    let entries = match top.get("rule") {
        None => return (Vec::new(), errors),
        Some(Value::Array(entries)) => entries,
        Some(_) => {
            errors.push(file_error(Some("rule"), None, Problem::RuleListShape));
            return (Vec::new(), errors);
        }
    };
    let mut rules = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let position = i + 1;
        match entry {
            Value::Table(table) => match check_rule(file, position, table) {
                Ok(rule) => rules.push((position, rule)),
                Err(errs) => errors.extend(errs),
            },
            _ => errors.push(file_error(
                Some("rule"),
                Some(position),
                Problem::RuleListShape,
            )),
        }
    }
    (rules, errors)
}

/// Runs all three stages on one rule.
fn check_rule(file: &Path, position: usize, table: &Table) -> Result<Rule, Vec<RuleError>> {
    let mut report = Reporter {
        file,
        position,
        rule_id: table.get("id").and_then(Value::as_str).map(str::to_string),
        errors: Vec::new(),
    };
    check_shape(&mut report, "", table, RULE_FIELDS);
    if !report.errors.is_empty() {
        return Err(report.errors);
    }
    let rule: Rule = match Value::Table(table.clone()).try_into() {
        Ok(rule) => rule,
        Err(e) => {
            report.add("rule", Problem::Unreadable(e.message().to_string()));
            return Err(report.errors);
        }
    };
    check_meaning(&mut report, &rule);
    if report.errors.is_empty() {
        Ok(rule)
    } else {
        Err(report.errors)
    }
}

/// Stage 1: known settings, required settings, and value types.
fn check_shape(
    report: &mut Reporter<'_>,
    prefix: &str,
    table: &Table,
    fields: &[(&str, Kind, bool)],
) {
    for key in table.keys() {
        if !fields.iter().any(|(name, _, _)| name == key) {
            report.add(&format!("{prefix}{key}"), Problem::UnknownField);
        }
    }
    for &(name, kind, required) in fields {
        let label = format!("{prefix}{name}");
        match table.get(name) {
            None if required => report.add(&label, Problem::MissingField),
            None => {}
            Some(value) => check_kind(report, &label, kind, value),
        }
    }
}

/// Stage 1 for one value.
fn check_kind(report: &mut Reporter<'_>, label: &str, kind: Kind, value: &Value) {
    let wrong = |expected| Problem::WrongType { expected };
    match kind {
        Kind::Text if !value.is_str() => report.add(label, wrong("text in quotes")),
        Kind::Bool if !value.is_bool() => report.add(label, wrong("true or false")),
        Kind::Version => match value.as_integer() {
            None => report.add(label, wrong("a whole number")),
            Some(n) if n < 1 || n > i64::from(u32::MAX) => report.add(label, Problem::BadVersion),
            Some(_) => {}
        },
        Kind::Choice(allowed) => match value.as_str() {
            None => report.add(label, wrong("text in quotes")),
            Some(s) if !allowed.contains(&s) => report.add(
                label,
                Problem::NotAChoice {
                    found: s.to_string(),
                    allowed,
                },
            ),
            Some(_) => {}
        },
        Kind::TextList => {
            let ok = value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_str));
            if !ok {
                report.add(
                    label,
                    wrong("a list of text values, such as [\"Info.plist\"]"),
                );
            }
        }
        Kind::MatchTable => match value.as_table() {
            None => report.add(
                label,
                wrong("a group of settings, such as match.path = \"...\""),
            ),
            Some(table) => check_shape(report, &format!("{label}."), table, MATCH_FIELDS),
        },
        Kind::Text | Kind::Bool => {}
    }
}

/// Stage 3: the schema's rules beyond types.
fn check_meaning(report: &mut Reporter<'_>, rule: &Rule) {
    if !is_dotted_lowercase(&rule.id) {
        report.add("id", Problem::BadId);
    }
    check_match_path(report, rule);
    for pattern in &rule.matcher.requires {
        if let Err(problem) = Glob::parse(pattern) {
            report.add("match.requires", Problem::BadPattern(problem));
        }
    }
    check_text(report, "title", &rule.title, TITLE_LIMIT);
    check_text(report, "explain", &rule.explain, TEXT_LIMIT);
    let sentences = count_sentences(&rule.explain);
    if sentences > SENTENCE_LIMIT {
        report.add("explain", Problem::TooManySentences { found: sentences });
    }
    check_text(report, "risk", &rule.risk, TEXT_LIMIT);
    if let Some(restore) = &rule.restore {
        check_text(report, "restore", restore, TEXT_LIMIT);
    }
    check_command(report, rule);
    if rule.tier == Tier::LikelySafe && rule.confidence == Confidence::Low {
        report.add("confidence", Problem::LowConfidenceLikelySafe);
    }
}

/// `match.path`: valid pattern, never near a hard-blocked path, and never
/// near a review-only path when the rule is `likely_safe`.
fn check_match_path(report: &mut Reporter<'_>, rule: &Rule) {
    let glob = match Glob::parse(&rule.matcher.path) {
        Ok(glob) => glob,
        Err(problem) => {
            report.add("match.path", Problem::BadPattern(problem));
            return;
        }
    };
    if let Some(reason) = protection::hard_block(&glob) {
        report.add("match.path", Problem::MatchesHardBlocked(reason));
        return;
    }
    if rule.tier == Tier::LikelySafe {
        if let Some(reason) = protection::review_only(&glob) {
            report.add("match.path", Problem::LikelySafeMatchesReviewOnly(reason));
        }
    }
}

/// `action` and `command` must agree, and the command must be allowed.
fn check_command(report: &mut Reporter<'_>, rule: &Rule) {
    match (rule.action, &rule.command) {
        (Action::Command, None) => report.add("command", Problem::MissingCommand),
        (Action::Command, Some(id)) if !commands::is_allowed(id) => {
            report.add("command", Problem::CommandNotAllowed(id.clone()));
        }
        (Action::Trash | Action::ShowOnly, Some(_)) => {
            report.add("command", Problem::CommandWithoutCommandAction);
        }
        _ => {}
    }
}

/// Text must not be blank and must fit the limit (in characters).
fn check_text(report: &mut Reporter<'_>, field: &str, text: &str, limit: usize) {
    if text.trim().is_empty() {
        report.add(field, Problem::EmptyText);
        return;
    }
    let found = text.chars().count();
    if found > limit {
        report.add(field, Problem::TooLong { limit, found });
    }
}

/// `xcode.derived_data`: two or more words joined by dots, each word made of
/// lowercase ASCII letters, digits and `_`, starting with a letter.
pub(crate) fn is_dotted_lowercase(id: &str) -> bool {
    let words: Vec<&str> = id.split('.').collect();
    words.len() >= 2
        && words.iter().all(|w| {
            w.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

/// Counts sentences: each `.`, `!` or `?` followed by a space or the end
/// ends one, and trailing words with no full stop count as one more.
///
/// Errs towards counting more ("e.g. this" counts as two), which can only
/// make a rule be rejected, never let a long text through.
pub(crate) fn count_sentences(text: &str) -> usize {
    let chars: Vec<char> = text.trim().chars().collect();
    let mut count = 0;
    let mut open = false;
    for (i, &c) in chars.iter().enumerate() {
        let ends = matches!(c, '.' | '!' | '?')
            && chars.get(i + 1).is_none_or(|next| next.is_whitespace());
        if ends {
            count += 1;
            open = false;
        } else if !c.is_whitespace() {
            open = true;
        }
    }
    count + usize::from(open)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert!(is_dotted_lowercase("xcode.derived_data"));
        assert!(is_dotted_lowercase("ios.simulators_unavailable"));
        assert!(is_dotted_lowercase("a.b2.c_d"));
        assert!(!is_dotted_lowercase("xcode"));
        assert!(!is_dotted_lowercase("Xcode.derived_data"));
        assert!(!is_dotted_lowercase("xcode..derived"));
        assert!(!is_dotted_lowercase(".xcode"));
        assert!(!is_dotted_lowercase("xcode.derived-data"));
        assert!(!is_dotted_lowercase("xcode.2data"));
        assert!(!is_dotted_lowercase("xcode.derived data"));
    }

    #[test]
    fn sentences() {
        assert_eq!(count_sentences("One."), 1);
        assert_eq!(count_sentences("One. Two."), 2);
        assert_eq!(count_sentences("One. Two. Three."), 3);
        assert_eq!(count_sentences("One! Two? Three"), 3);
        assert_eq!(count_sentences("No full stop"), 1);
        assert_eq!(count_sentences("Version 1.2 is here."), 1);
        assert_eq!(count_sentences("Wait... then go."), 2);
        assert_eq!(count_sentences(""), 0);
    }
}
