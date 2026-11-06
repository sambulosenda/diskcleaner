//! The glob dialect used by `match.path` and `match.requires`, and the
//! "could these two patterns ever match the same path?" test used to reject
//! rules that point at protected places.
//!
//! The dialect is deliberately small:
//! - `/` separates folder names. Paths are relative (to home for
//!   `match.path`, to the matched item for `match.requires`).
//! - `*` matches any run of characters inside one name, `?` matches one
//!   character, `[abc]`, `[a-z]` and `[!abc]` match one character from (or
//!   not from) a set.
//! - `**` on its own between slashes matches zero or more whole names.
//!
//! Anything else that some glob libraries accept (`{a,b}`, backslash
//! escapes, `~`, `.` and `..`, empty names) is refused, so a rule can never
//! mean something different from what a reviewer reads.
//!
//! All comparisons are case-insensitive (macOS volumes usually are), using
//! [`safety::protected::fold`].

use std::collections::HashMap;
use std::fmt;

use safety::protected::fold;

/// One piece of a single name pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CharTok {
    /// A literal character (already folded).
    Lit(char),
    /// `?`: any one character.
    Any,
    /// `[...]` or `[!...]`: one character from (or not from) the ranges.
    Class {
        /// `true` for `[!...]`.
        negated: bool,
        /// Inclusive ranges; a single character `c` is stored as `(c, c)`.
        ranges: Vec<(char, char)>,
    },
    /// `*`: any run of characters, including none.
    Star,
}

/// One piece of a path pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathTok {
    /// A pattern for exactly one name.
    Name(Vec<CharTok>),
    /// `**`: zero or more whole names.
    AnyDepth,
}

impl PathTok {
    /// A name token that matches exactly `name` (folded).
    pub(crate) fn literal(name: &str) -> Self {
        PathTok::Name(fold(name).chars().map(CharTok::Lit).collect())
    }
}

/// A parsed glob. Only [`Glob::parse`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    pub(crate) parts: Vec<PathTok>,
}

/// Why a glob was refused. Each message is written for a reviewer who does
/// not read Rust.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GlobProblem {
    /// The pattern is empty.
    #[error("the pattern is empty")]
    Empty,
    /// The pattern starts with `/`.
    #[error("the pattern must not start with \"/\"; write it relative, without the leading slash")]
    Absolute,
    /// The pattern starts with `~`.
    #[error(
        "the pattern must not start with \"~\"; it is already relative, so leave the \"~/\" out"
    )]
    Tilde,
    /// The pattern has a backslash.
    #[error("the pattern must not contain a backslash (\\)")]
    Backslash,
    /// The pattern has `{` or `}`.
    #[error("the pattern must not contain {{ or }}; write one rule per alternative instead")]
    Braces,
    /// The pattern has a NUL or other control character.
    #[error("the pattern contains an invisible control character")]
    ControlChar,
    /// Two slashes in a row, or a slash at the start or end.
    #[error("the pattern has an empty folder name (two slashes in a row, or a slash at the end)")]
    EmptyName,
    /// A `.` or `..` name.
    #[error("the pattern must not contain \".\" or \"..\" as a folder name")]
    DotName,
    /// `**` mixed with other characters in one name.
    #[error("\"**\" must be a whole folder name on its own, like \"a/**/b\"")]
    DoubleStarInName,
    /// `[` without a closing `]`.
    #[error("a \"[\" has no matching \"]\"")]
    UnclosedBracket,
    /// `[]` or `[!]`.
    #[error("a \"[...]\" set is empty")]
    EmptyBracket,
}

impl Glob {
    /// Parses a pattern in the dialect described at the top of this module.
    pub fn parse(pattern: &str) -> Result<Self, GlobProblem> {
        if pattern.is_empty() {
            return Err(GlobProblem::Empty);
        }
        if pattern.starts_with('/') {
            return Err(GlobProblem::Absolute);
        }
        if pattern.starts_with('~') {
            return Err(GlobProblem::Tilde);
        }
        if pattern.contains('\\') {
            return Err(GlobProblem::Backslash);
        }
        if pattern.contains('{') || pattern.contains('}') {
            return Err(GlobProblem::Braces);
        }
        if pattern.chars().any(char::is_control) {
            return Err(GlobProblem::ControlChar);
        }
        let parts = pattern
            .split('/')
            .map(parse_name)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Glob { parts })
    }

    /// Whether some path could match both this glob and `target`.
    ///
    /// This errs towards "yes": a character set is assumed to overlap any
    /// other set unless both are plain lists that clearly do not.
    pub(crate) fn could_overlap(&self, target: &[PathTok]) -> bool {
        overlap(&self.parts, target, PathTok::is_any_depth, path_toks_share)
    }

    /// Whether the relative path `rel` (folder names separated by `/`, with
    /// no leading slash) matches this glob, ignoring case.
    ///
    /// Only compares text: it never looks at the filesystem.
    pub fn matches(&self, rel: &str) -> bool {
        if rel.is_empty() {
            return false;
        }
        let names: Vec<PathTok> = rel.split('/').map(PathTok::literal).collect();
        self.could_overlap(&names)
    }

    /// Like [`Glob::matches`], for a path already split and folded with
    /// [`FoldedPath::new`]. Use it when one path is checked against many
    /// globs, so the path is folded once.
    pub fn matches_folded(&self, path: &FoldedPath) -> bool {
        if path.names.is_empty() {
            return false;
        }
        // Fast reject: without `**` a glob matches paths of one depth only.
        if !self.parts.iter().any(PathTok::is_any_depth) && self.parts.len() != path.names.len() {
            return false;
        }
        self.could_overlap(&path.names)
    }

    /// Whether some path could match both this glob and `other`.
    ///
    /// Errs towards "yes", like the protected-path check.
    pub fn could_overlap_glob(&self, other: &Glob) -> bool {
        self.could_overlap(&other.parts)
    }

    /// Number of characters before the first wildcard (`*`, `?`, `[` or
    /// `**`), counting the `/` between names. A longer literal prefix means
    /// a more specific glob.
    pub fn literal_prefix_len(&self) -> usize {
        let mut len = 0;
        for (i, part) in self.parts.iter().enumerate() {
            if i > 0 {
                len += 1;
            }
            let PathTok::Name(toks) = part else {
                return len;
            };
            for tok in toks {
                match tok {
                    CharTok::Lit(_) => len += 1,
                    _ => return len,
                }
            }
        }
        len
    }

    /// Whether the glob has a `**` part, so it can match paths of any depth.
    pub fn has_any_depth(&self) -> bool {
        self.parts.iter().any(PathTok::is_any_depth)
    }

    /// Number of folder names in the glob, counting `**` as one.
    pub fn depth(&self) -> usize {
        self.parts.len()
    }

    /// One simple path that this glob matches, as folder names: `*` becomes
    /// nothing, `?` and `[...]` become one fitting character, and `**`
    /// becomes no folders at all. Used to ask the safety crate about the
    /// plainest reading of the rule.
    pub(crate) fn witness(&self) -> Vec<String> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                PathTok::AnyDepth => None,
                PathTok::Name(toks) => Some(name_witness(toks)),
            })
            .collect()
    }
}

/// A relative path split into folded names, ready for
/// [`Glob::matches_folded`]. Only compares text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldedPath {
    names: Vec<PathTok>,
}

impl FoldedPath {
    /// Splits `rel` (names separated by `/`, no leading slash) and folds
    /// each name. An empty `rel` gives an empty path, which no glob matches.
    pub fn new(rel: &str) -> Self {
        if rel.is_empty() {
            return Self { names: Vec::new() };
        }
        Self {
            names: rel.split('/').map(PathTok::literal).collect(),
        }
    }

    /// Builds a path from names that are each one folder or file name.
    pub fn from_names<S: AsRef<str>>(names: &[S]) -> Self {
        Self {
            names: names.iter().map(|n| PathTok::literal(n.as_ref())).collect(),
        }
    }
}

impl PathTok {
    fn is_any_depth(&self) -> bool {
        matches!(self, PathTok::AnyDepth)
    }
}

impl fmt::Display for Glob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self
            .parts
            .iter()
            .map(|p| match p {
                PathTok::AnyDepth => "**".to_string(),
                PathTok::Name(toks) => toks.iter().map(tok_text).collect(),
            })
            .collect();
        f.write_str(&names.join("/"))
    }
}

/// Text form of one character token, for [`Glob`]'s `Display`.
fn tok_text(tok: &CharTok) -> String {
    match tok {
        CharTok::Lit(c) => c.to_string(),
        CharTok::Any => "?".to_string(),
        CharTok::Star => "*".to_string(),
        CharTok::Class { negated, ranges } => {
            let mut s = String::from(if *negated { "[!" } else { "[" });
            for (lo, hi) in ranges {
                s.push(*lo);
                if lo != hi {
                    s.push('-');
                    s.push(*hi);
                }
            }
            s.push(']');
            s
        }
    }
}

/// Parses one folder name of a pattern.
fn parse_name(name: &str) -> Result<PathTok, GlobProblem> {
    if name.is_empty() {
        return Err(GlobProblem::EmptyName);
    }
    if name == "." || name == ".." {
        return Err(GlobProblem::DotName);
    }
    if name == "**" {
        return Ok(PathTok::AnyDepth);
    }
    if name.contains("**") {
        return Err(GlobProblem::DoubleStarInName);
    }
    let folded = fold(name);
    let mut chars = folded.chars().peekable();
    let mut toks = Vec::new();
    while let Some(c) = chars.next() {
        match c {
            '*' => toks.push(CharTok::Star),
            '?' => toks.push(CharTok::Any),
            '[' => toks.push(parse_class(&mut chars)?),
            other => toks.push(CharTok::Lit(other)),
        }
    }
    Ok(PathTok::Name(toks))
}

/// Parses the inside of a `[...]` set, after the `[`.
fn parse_class(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<CharTok, GlobProblem> {
    let negated = matches!(chars.peek(), Some('!') | Some('^'));
    if negated {
        chars.next();
    }
    let mut ranges = Vec::new();
    loop {
        let Some(c) = chars.next() else {
            return Err(GlobProblem::UnclosedBracket);
        };
        if c == ']' {
            break;
        }
        let mut hi = c;
        if chars.peek() == Some(&'-') {
            let mut ahead = chars.clone();
            ahead.next();
            if let Some(&end) = ahead.peek() {
                if end != ']' {
                    chars.next();
                    chars.next();
                    hi = end;
                }
            }
        }
        ranges.push((c.min(hi), c.max(hi)));
    }
    if ranges.is_empty() {
        return Err(GlobProblem::EmptyBracket);
    }
    Ok(CharTok::Class { negated, ranges })
}

/// One simple name that the name pattern matches.
fn name_witness(toks: &[CharTok]) -> String {
    let mut s = String::new();
    for tok in toks {
        match tok {
            CharTok::Lit(c) => s.push(*c),
            CharTok::Any => s.push('a'),
            CharTok::Star => {}
            CharTok::Class { negated, ranges } => s.push(class_witness(*negated, ranges)),
        }
    }
    if s.is_empty() {
        s.push('a');
    }
    s
}

/// One character that the set matches.
fn class_witness(negated: bool, ranges: &[(char, char)]) -> char {
    if !negated {
        return ranges.first().map(|r| r.0).unwrap_or('a');
    }
    ('a'..='z')
        .chain('0'..='9')
        .find(|c| !in_ranges(*c, ranges))
        .unwrap_or('_')
}

fn in_ranges(c: char, ranges: &[(char, char)]) -> bool {
    ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c))
}

/// Whether two name patterns could match the same name.
fn path_toks_share(a: &PathTok, b: &PathTok) -> bool {
    match (a, b) {
        (PathTok::Name(x), PathTok::Name(y)) => {
            overlap(x, y, |t| matches!(t, CharTok::Star), char_toks_share)
        }
        // `**` is handled as a star by `overlap` and never reaches here.
        _ => true,
    }
}

/// Whether two single-character tokens could match the same character.
fn char_toks_share(a: &CharTok, b: &CharTok) -> bool {
    use CharTok::*;
    match (a, b) {
        (Lit(x), Lit(y)) => x == y,
        (Lit(c), Class { negated, ranges }) | (Class { negated, ranges }, Lit(c)) => {
            in_ranges(*c, ranges) != *negated
        }
        (
            Class {
                negated: false,
                ranges: r1,
            },
            Class {
                negated: false,
                ranges: r2,
            },
        ) => r1
            .iter()
            .any(|(a_lo, a_hi)| r2.iter().any(|(b_lo, b_hi)| a_lo <= b_hi && b_lo <= a_hi)),
        // `?`, negated sets: assume they overlap (errs towards refusing).
        _ => true,
    }
}

/// Whether some sequence could match both `a` and `b`, where tokens for
/// which `is_star` is true match any run (including none) and other tokens
/// match exactly one item when `share` says they can.
///
/// Used twice: on names inside a folder name, and on folder names inside a
/// path (where `**` is the star).
fn overlap<T>(
    a: &[T],
    b: &[T],
    is_star: impl Fn(&T) -> bool + Copy,
    share: impl Fn(&T, &T) -> bool + Copy,
) -> bool {
    let mut memo = HashMap::new();
    overlap_from(a, b, 0, 0, is_star, share, &mut memo)
}

fn overlap_from<T>(
    a: &[T],
    b: &[T],
    i: usize,
    j: usize,
    is_star: impl Fn(&T) -> bool + Copy,
    share: impl Fn(&T, &T) -> bool + Copy,
    memo: &mut HashMap<(usize, usize), bool>,
) -> bool {
    if let Some(&known) = memo.get(&(i, j)) {
        return known;
    }
    let star_a = a.get(i).is_some_and(is_star);
    let star_b = b.get(j).is_some_and(is_star);
    let result = if i == a.len() && j == b.len() {
        true
    } else if star_a && overlap_from(a, b, i + 1, j, is_star, share, memo) {
        // a's star matches nothing.
        true
    } else if star_b && overlap_from(a, b, i, j + 1, is_star, share, memo) {
        // b's star matches nothing.
        true
    } else if i < a.len() && j < b.len() {
        if star_a {
            // a's star swallows what b[j] matches.
            overlap_from(a, b, i, j + 1, is_star, share, memo)
        } else if star_b {
            overlap_from(a, b, i + 1, j, is_star, share, memo)
        } else {
            share(&a[i], &b[j]) && overlap_from(a, b, i + 1, j + 1, is_star, share, memo)
        }
    } else {
        false
    };
    memo.insert((i, j), result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(names: &[&str]) -> Vec<PathTok> {
        names
            .iter()
            .map(|n| {
                if *n == "**" {
                    PathTok::AnyDepth
                } else {
                    PathTok::literal(n)
                }
            })
            .collect()
    }

    fn overlaps(glob: &str, t: &[&str]) -> bool {
        Glob::parse(glob).unwrap().could_overlap(&target(t))
    }

    #[test]
    fn matches_plain_paths() {
        let g = Glob::parse("Library/Caches/*-cache").unwrap();
        assert!(g.matches("Library/Caches/versions-cache"));
        assert!(g.matches("library/caches/VERSIONS-CACHE"));
        assert!(!g.matches("Library/Caches/state.json"));
        assert!(!g.matches("Library/Caches/versions-cache/entry"));
        assert!(!g.matches("Library/Caches"));
        assert!(!g.matches(""));
        let deep = Glob::parse("a/**/b").unwrap();
        assert!(deep.matches("a/b"));
        assert!(deep.matches("a/x/y/b"));
        assert!(!deep.matches("a/x/c"));
        // A `*` in the path is a plain character, not a wildcard.
        assert!(!Glob::parse("a/b").unwrap().matches("a/*"));
    }

    #[test]
    fn folded_matching_and_specificity() {
        let g = Glob::parse("Library/Caches/*-cache").unwrap();
        assert!(g.matches_folded(&FoldedPath::new("library/caches/A-CACHE")));
        assert!(!g.matches_folded(&FoldedPath::new("Library/Caches")));
        assert!(!g.matches_folded(&FoldedPath::new("")));
        assert!(g.matches_folded(&FoldedPath::from_names(&["Library", "Caches", "x-cache"])));
        assert!(Glob::parse("a/**/b")
            .unwrap()
            .matches_folded(&FoldedPath::new("a/x/y/b")));
        assert_eq!(g.literal_prefix_len(), "library/caches/".len());
        assert_eq!(
            Glob::parse("Library/Logs").unwrap().literal_prefix_len(),
            12
        );
        assert_eq!(Glob::parse("a/**/b").unwrap().literal_prefix_len(), 2);
        assert_eq!(Glob::parse("*").unwrap().literal_prefix_len(), 0);
        let a = Glob::parse("Downloads/*.dmg").unwrap();
        assert!(!a.could_overlap_glob(&Glob::parse("Downloads/*.pkg").unwrap()));
        assert!(a.could_overlap_glob(&Glob::parse("Downloads/*").unwrap()));
    }

    #[test]
    fn literal_paths() {
        assert!(overlaps("Library/Keychains", &["Library", "Keychains"]));
        assert!(overlaps("library/KEYCHAINS", &["Library", "Keychains"]));
        assert!(!overlaps("Library/Caches", &["Library", "Keychains"]));
    }

    #[test]
    fn stars_and_double_stars() {
        assert!(overlaps("Library/*", &["Library", "Keychains"]));
        assert!(overlaps("*/Keychains/x", &["Library", "Keychains", "**"]));
        assert!(overlaps("**/x", &[".ssh", "**"]));
        assert!(overlaps("**", &[]));
        assert!(!overlaps("Library/*", &["Library"]));
        assert!(!overlaps(
            "Library/Caches/*",
            &["Library", "Keychains", "**"]
        ));
        assert!(overlaps("Library/K*s", &["Library", "Keychains"]));
        assert!(!overlaps("Library/K*z", &["Library", "Keychains"]));
    }

    #[test]
    fn sets_and_question_marks() {
        assert!(overlaps("Library/[jk]eychains", &["Library", "Keychains"]));
        assert!(!overlaps("Library/[!k]eychains", &["Library", "Keychains"]));
        assert!(overlaps("Library/?eychains", &["Library", "Keychains"]));
        assert!(!overlaps("Library/?", &["Library", "Keychains"]));
    }

    #[test]
    fn name_patterns_against_name_patterns() {
        let t = vec![
            PathTok::AnyDepth,
            PathTok::Name(vec![CharTok::Star, CharTok::Lit('.'), CharTok::Lit('p')]),
        ];
        assert!(Glob::parse("a/*.p").unwrap().could_overlap(&t));
        assert!(Glob::parse("a/*").unwrap().could_overlap(&t));
        assert!(!Glob::parse("a/*.q").unwrap().could_overlap(&t));
    }

    #[test]
    fn witness_is_plain() {
        let g = Glob::parse("Pictures/**/*.photoslibrary/[!x]?").unwrap();
        assert_eq!(g.witness(), vec!["pictures", ".photoslibrary", "aa"]);
    }

    #[test]
    fn refuses_unusual_syntax() {
        use GlobProblem::*;
        let cases = [
            ("", Empty),
            ("/Library", Absolute),
            ("~/Library", Tilde),
            ("Library\\x", Backslash),
            ("Library/{a,b}", Braces),
            ("Library/a\u{0}", ControlChar),
            ("Library//Caches", EmptyName),
            ("Library/Caches/", EmptyName),
            ("Library/../x", DotName),
            ("./Library", DotName),
            ("Library/a**", DoubleStarInName),
            ("Library/[ab", UnclosedBracket),
            ("Library/[]", EmptyBracket),
        ];
        for (pattern, want) in cases {
            assert_eq!(Glob::parse(pattern), Err(want), "pattern {pattern:?}");
        }
    }

    #[test]
    fn display_round_trips_simple_patterns() {
        let g = Glob::parse("Library/Developer/Xcode/DerivedData/*").unwrap();
        assert_eq!(g.to_string(), "library/developer/xcode/deriveddata/*");
    }
}
