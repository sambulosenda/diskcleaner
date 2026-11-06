//! Errors from loading rules, written so that someone who does not read
//! Rust can see which file, which rule and which setting is wrong, and why.

use std::fmt;
use std::path::PathBuf;

use safety::protected::{BlockReason, ReviewReason};

use crate::glob::GlobProblem;

/// One problem with one rule (or with a whole file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleError {
    /// The rule file.
    pub file: PathBuf,
    /// Position of the rule in its file, counting from 1. `None` when the
    /// problem is with the whole file.
    pub position: Option<usize>,
    /// The rule's `id`, when it has a readable one.
    pub rule_id: Option<String>,
    /// The setting at fault, as written in the file (for example
    /// `match.path`). `None` when the problem is with the whole rule.
    pub field: Option<String>,
    /// What is wrong.
    pub problem: Problem,
}

/// What is wrong with a rule.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Problem {
    /// The rules folder could not be listed.
    #[error("the rules folder could not be read ({0})")]
    FolderUnreadable(String),
    /// The file could not be read.
    #[error("the file could not be read ({0})")]
    FileUnreadable(String),
    /// The file is not valid TOML.
    #[error("the file is not valid TOML: {0}")]
    NotToml(String),
    /// The file has something other than `[[rule]]` entries.
    #[error(
        "the file has a section the app does not know; rule files may only hold [[rule]] entries"
    )]
    UnknownSection,
    /// `rule` is not a list of tables.
    #[error("\"rule\" must be written as [[rule]] entries")]
    RuleListShape,
    /// A setting the schema does not have.
    #[error("this setting is not part of the rule format (check the spelling)")]
    UnknownField,
    /// A required setting is missing.
    #[error("this setting is required but missing")]
    MissingField,
    /// The value has the wrong type.
    #[error("this setting must be {expected}")]
    WrongType {
        /// What the value should be, in plain words.
        expected: &'static str,
    },
    /// The value is not one of the allowed choices.
    #[error("\"{found}\" is not allowed here; use one of: {}", .allowed.join(", "))]
    NotAChoice {
        /// The value in the file.
        found: String,
        /// The allowed values.
        allowed: &'static [&'static str],
    },
    /// The id is not dotted lowercase.
    #[error("the id must be lowercase words joined by dots, such as \"xcode.derived_data\" (letters, digits and _ only, each word starting with a letter)")]
    BadId,
    /// The version is below 1.
    #[error("the version must be a whole number of 1 or more")]
    BadVersion,
    /// The pattern is not in the supported glob dialect.
    #[error("the pattern is not valid: {0}")]
    BadPattern(GlobProblem),
    /// The text is empty.
    #[error("this text must not be empty")]
    EmptyText,
    /// The text is too long.
    #[error("this text is {found} characters long; the limit is {limit}")]
    TooLong {
        /// The limit, in characters.
        limit: usize,
        /// The length found, in characters.
        found: usize,
    },
    /// `explain` has more than two sentences.
    #[error("this text has {found} sentences; the limit is 2")]
    TooManySentences {
        /// The number of sentences found.
        found: usize,
    },
    /// `action = "command"` without `command`.
    #[error(
        "action is \"command\", so a \"command\" setting naming an allowed command is required"
    )]
    MissingCommand,
    /// `command` is not on the allow-list.
    #[error("\"{0}\" is not an allowed command; it must be one of the ids in docs/safety/allowed-commands.md")]
    CommandNotAllowed(String),
    /// `command` given when `action` is not `command`.
    #[error("a command is only allowed when action is \"command\"")]
    CommandWithoutCommandAction,
    /// `tier = "likely_safe"` with `confidence = "low"`.
    #[error("a rule with low confidence can never be \"likely_safe\"; use \"review_first\" or raise the confidence")]
    LowConfidenceLikelySafe,
    /// `match.path` could match something hard-blocked.
    #[error("this pattern could match {0}, which the app never removes")]
    MatchesHardBlocked(BlockReason),
    /// A `likely_safe` rule whose `match.path` could match a review-only item.
    #[error("this pattern could match {0}, which must always be reviewed one by one, so the rule cannot be \"likely_safe\"")]
    LikelySafeMatchesReviewOnly(ReviewReason),
    /// The same id is used by more than one rule.
    #[error("the id is used by more than one rule (also in: {}); every copy is rejected until only one is left", .others.join(", "))]
    DuplicateId {
        /// Where the other rules with this id are, as "file, rule N".
        others: Vec<String>,
    },
    /// Serde refused the rule for a reason the earlier checks did not
    /// catch. Kept as a safety net; the earlier checks should make this
    /// unreachable.
    #[error("the rule could not be read: {0}")]
    Unreadable(String),
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = self
            .file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.file.display().to_string());
        write!(f, "{file}")?;
        match (&self.position, &self.rule_id) {
            (Some(n), Some(id)) => write!(f, ", rule {n} (\"{id}\")")?,
            (Some(n), None) => write!(f, ", rule {n}")?,
            (None, Some(id)) => write!(f, ", rule \"{id}\"")?,
            (None, None) => {}
        }
        if let Some(field) = &self.field {
            write!(f, ", setting \"{field}\"")?;
        }
        write!(f, ": {}", self.problem)
    }
}

impl std::error::Error for RuleError {}
