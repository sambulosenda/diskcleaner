//! Load and validate TOML rules. Reads rule files only.
//!
//! Implements `docs/safety/rule-schema.md`. Each rule file holds `[[rule]]`
//! entries. Every rule is checked on its own: a bad rule is rejected with
//! readable [`RuleError`]s naming the file, the rule and the setting, and
//! never partially loaded; the good rules around it still load.
//!
//! Start with [`load_dir`].

mod check;
pub mod commands;
mod error;
pub mod glob;
mod protection;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use error::{Problem, RuleError};
pub use glob::{Glob, GlobProblem};

/// Generates a plain enum with serde names and a `NAMES` list, so that the
/// readable "use one of" messages always list exactly what serde accepts.
macro_rules! choice_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $text)] $variant),+
        }

        impl $name {
            /// Every allowed value, as written in a rule file.
            pub const NAMES: &'static [&'static str] = &[$($text),+];

            /// The value as written in a rule file.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }
    };
}

choice_enum! {
    /// What kind of item a rule describes.
    Category {
        /// App and system caches.
        Cache => "cache",
        /// Build output.
        Build => "build",
        /// Developer tools data, such as simulators.
        DeveloperTools => "developer_tools",
        /// Docker data.
        Docker => "docker",
        /// Data kept by an app.
        AppData => "app_data",
        /// Log files.
        Logs => "logs",
        /// Downloaded files.
        Downloads => "downloads",
        /// The user's own files.
        UserData => "user_data",
        /// Backups.
        Backups => "backups",
        /// System data.
        System => "system",
        /// Not known.
        Unknown => "unknown",
    }
}

choice_enum! {
    /// How the app presents an item.
    Tier {
        /// Can be preselected for removal.
        LikelySafe => "likely_safe",
        /// Shown, but the user must look first.
        ReviewFirst => "review_first",
        /// Shown for information; not offered for removal.
        Keep => "keep",
    }
}

choice_enum! {
    /// How sure the rule is about what the item is.
    Confidence {
        /// Very sure.
        High => "high",
        /// Fairly sure.
        Medium => "medium",
        /// Not sure. Never `likely_safe`.
        Low => "low",
    }
}

choice_enum! {
    /// Which app mode shows the rule.
    Mode {
        /// Everyday users (and developers).
        Simple => "simple",
        /// Developer mode only.
        Developer => "developer",
    }
}

choice_enum! {
    /// What the app may do with a matched item.
    Action {
        /// Move it to the Trash.
        Trash => "trash",
        /// Run one allow-listed command (see [`commands`]).
        Command => "command",
        /// Show it only; never remove it.
        ShowOnly => "show_only",
    }
}

/// Where a rule's items live.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    /// Glob relative to home, in the dialect described in [`glob`].
    pub path: String,
    /// Globs, relative to the matched item, that must exist inside it.
    #[serde(default)]
    pub requires: Vec<String>,
}

/// One classification rule. Get validated rules from [`load_dir`]; a
/// `Rule` built or deserialised any other way has not been checked.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Stable unique id, dotted lowercase.
    pub id: String,
    /// Bumped on any change; History records which version acted.
    pub version: u32,
    /// Where the item lives.
    #[serde(rename = "match")]
    pub matcher: Match,
    /// What kind of item it is.
    pub category: Category,
    /// How the app presents it.
    pub tier: Tier,
    /// How sure the rule is.
    pub confidence: Confidence,
    /// Whether the item comes back by itself. Only then may permanent
    /// delete be offered.
    pub regenerable: bool,
    /// Which mode shows the rule.
    pub mode: Mode,
    /// Short display name, at most 40 characters.
    pub title: String,
    /// What it is and why it exists: at most 2 sentences and 200 characters.
    pub explain: String,
    /// What could go wrong if it is removed, at most 200 characters.
    pub risk: String,
    /// How it comes back, at most 200 characters.
    #[serde(default)]
    pub restore: Option<String>,
    /// What the app may do with it.
    pub action: Action,
    /// Allow-listed command id, required when `action` is `command`.
    #[serde(default)]
    pub command: Option<String>,
    /// Treat each child of the match as its own item.
    #[serde(default)]
    pub per_item: bool,
}

impl Rule {
    /// Whether permanent delete may ever be offered for this rule's items.
    /// `regenerable = false` forbids it regardless of what the user chooses.
    pub fn allows_permanent_delete(&self) -> bool {
        self.regenerable
    }
}

/// Loads every `*.toml` file directly inside `dir` (not in sub-folders), in
/// file-name order.
///
/// Returns the rules that passed every check, and one [`RuleError`] per
/// problem found. A rule with any problem is left out entirely. Rules that
/// share an id are all left out, wherever they are, because there is no
/// safe way to tell which one is meant.
pub fn load_dir(dir: &Path) -> (Vec<Rule>, Vec<RuleError>) {
    let files = match rule_files(dir) {
        Ok(files) => files,
        Err(e) => {
            let err = RuleError {
                file: dir.to_path_buf(),
                position: None,
                rule_id: None,
                field: None,
                problem: Problem::FolderUnreadable(e.to_string()),
            };
            tracing::warn!(error = %err, "rules folder rejected");
            return (Vec::new(), vec![err]);
        }
    };
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for file in files {
        match fs::read_to_string(&file) {
            Ok(text) => {
                let (rules, errs) = parse_file(&file, &text);
                loaded.extend(rules.into_iter().map(|(pos, r)| (file.clone(), pos, r)));
                errors.extend(errs);
            }
            Err(e) => errors.push(RuleError {
                file,
                position: None,
                rule_id: None,
                field: None,
                problem: Problem::FileUnreadable(e.to_string()),
            }),
        }
    }
    let rules = drop_duplicate_ids(loaded, &mut errors);
    for err in &errors {
        tracing::warn!(error = %err, "rule rejected");
    }
    (rules, errors)
}

/// Parses and checks the text of one rule file. `file` is used only in
/// error messages. Returns each good rule with its position (from 1).
///
/// Does not check for duplicate ids across files; [`load_dir`] does.
pub fn parse_file(file: &Path, text: &str) -> (Vec<(usize, Rule)>, Vec<RuleError>) {
    check::parse_file(file, text)
}

/// The `*.toml` files directly inside `dir`, sorted by name.
fn rule_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let is_file = entry.file_type()?.is_file();
        if is_file && path.extension().is_some_and(|e| e == "toml") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Removes every rule whose id is used more than once, adding one error
/// per copy.
fn drop_duplicate_ids(
    loaded: Vec<(PathBuf, usize, Rule)>,
    errors: &mut Vec<RuleError>,
) -> Vec<Rule> {
    let mut by_id: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, (_, _, rule)) in loaded.iter().enumerate() {
        by_id.entry(rule.id.as_str()).or_default().push(i);
    }
    let mut duplicate = vec![false; loaded.len()];
    for indexes in by_id.values().filter(|v| v.len() > 1) {
        for &i in indexes {
            duplicate[i] = true;
            let (file, position, rule) = &loaded[i];
            let others = indexes
                .iter()
                .filter(|&&j| j != i)
                .map(|&j| {
                    let (f, p, _) = &loaded[j];
                    let name = f.file_name().map(|n| n.to_string_lossy().into_owned());
                    format!("{}, rule {p}", name.unwrap_or_default())
                })
                .collect();
            errors.push(RuleError {
                file: file.clone(),
                position: Some(*position),
                rule_id: Some(rule.id.clone()),
                field: Some("id".to_string()),
                problem: Problem::DuplicateId { others },
            });
        }
    }
    loaded
        .into_iter()
        .zip(duplicate)
        .filter(|(_, dup)| !dup)
        .map(|((_, _, rule), _)| rule)
        .collect()
}
