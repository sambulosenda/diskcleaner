//! Rules made ready for matching: parsed globs, specificity and ties.

use rules::{FoldedPath, Glob, Mode, Rule};

/// One rule with its globs parsed.
#[derive(Debug)]
pub(crate) struct Compiled<'r> {
    /// The rule itself.
    pub rule: &'r Rule,
    /// `match.path`.
    pub glob: Glob,
    /// `match.requires`.
    pub requires: Vec<Glob>,
    /// Length of the literal prefix of `match.path`. Longer wins.
    pub specificity: usize,
    /// The rule ties with another one, so it never classifies anything.
    pub tied: bool,
}

/// Two rules that are equally specific and could match the same path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleTie {
    /// The first rule's id.
    pub first: String,
    /// The second rule's id.
    pub second: String,
}

/// What the rules say about one path.
#[derive(Debug)]
pub(crate) enum Verdict<'c, 'r> {
    /// No rule matches.
    NoRule,
    /// Exactly one most specific rule matches.
    Rule(&'c Compiled<'r>),
    /// The most specific match is a tied rule, or more than one rule is
    /// equally specific. Nobody can say what the path is.
    Ambiguous,
}

/// Whether `rule` is shown in `mode`. Developer mode shows every rule.
fn in_mode(rule: &Rule, mode: Mode) -> bool {
    match mode {
        Mode::Developer => true,
        Mode::Simple => rule.mode == Mode::Simple,
    }
}

/// Parses the globs of the rules shown in `mode` and marks ties.
///
/// A rule whose globs do not parse is left out with a warning. Rules from
/// [`rules::load_dir`] always parse, so this only happens to rules built
/// some other way.
pub(crate) fn compile(rules: &[Rule], mode: Mode) -> Vec<Compiled<'_>> {
    let mut compiled: Vec<Compiled<'_>> = rules
        .iter()
        .filter(|r| in_mode(r, mode))
        .filter_map(compile_one)
        .collect();
    for (i, j) in tie_pairs(&compiled) {
        compiled[i].tied = true;
        compiled[j].tied = true;
    }
    compiled
}

fn compile_one(rule: &Rule) -> Option<Compiled<'_>> {
    let glob = match Glob::parse(&rule.matcher.path) {
        Ok(glob) => glob,
        Err(problem) => {
            tracing::warn!(rule = %rule.id, %problem, "rule left out: bad match.path");
            return None;
        }
    };
    let mut requires = Vec::with_capacity(rule.matcher.requires.len());
    for pattern in &rule.matcher.requires {
        match Glob::parse(pattern) {
            Ok(g) => requires.push(g),
            Err(problem) => {
                tracing::warn!(rule = %rule.id, %problem, "rule left out: bad match.requires");
                return None;
            }
        }
    }
    Some(Compiled {
        rule,
        specificity: glob.literal_prefix_len(),
        glob,
        requires,
        tied: false,
    })
}

/// Index pairs of rules that are equally specific and could match the same
/// path. `match.requires` is ignored: one folder can hold both files.
fn tie_pairs(compiled: &[Compiled<'_>]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for i in 0..compiled.len() {
        for j in i + 1..compiled.len() {
            let (a, b) = (&compiled[i], &compiled[j]);
            if a.specificity == b.specificity && a.glob.could_overlap_glob(&b.glob) {
                pairs.push((i, j));
            }
        }
    }
    pairs
}

/// Finds rules that tie: equally specific (same literal prefix length) and
/// able to match the same path. Each tie is logged as an error.
///
/// Call this once, right after loading the rules. Tied rules are never used
/// to classify anything: [`crate::classify`] treats whatever they match as
/// unknown and `keep`, whether or not this was called.
pub fn check_ties(rules: &[Rule], mode: Mode) -> Vec<RuleTie> {
    let compiled = compile(rules, mode);
    tie_pairs(&compiled)
        .into_iter()
        .map(|(i, j)| {
            let tie = RuleTie {
                first: compiled[i].rule.id.clone(),
                second: compiled[j].rule.id.clone(),
            };
            tracing::error!(
                first = %tie.first,
                second = %tie.second,
                "two rules are equally specific and can match the same path; neither will be used"
            );
            tie
        })
        .collect()
}

/// The verdict for one path. `requires_ok` says whether a rule's
/// `match.requires` are all present inside the path; it is only asked for
/// rules whose `match.path` matches.
pub(crate) fn verdict<'c, 'r>(
    compiled: &'c [Compiled<'r>],
    path: &FoldedPath,
    mut requires_ok: impl FnMut(&Compiled<'r>) -> bool,
) -> Verdict<'c, 'r> {
    let mut best: Vec<&'c Compiled<'r>> = Vec::new();
    for c in compiled {
        if !c.glob.matches_folded(path) {
            continue;
        }
        if let Some(top) = best.first() {
            if c.specificity < top.specificity {
                continue;
            }
        }
        if !c.requires.is_empty() && !requires_ok(c) {
            continue;
        }
        if best
            .first()
            .is_some_and(|top| c.specificity > top.specificity)
        {
            best.clear();
        }
        best.push(c);
    }
    match best.as_slice() {
        [] => Verdict::NoRule,
        [one] if !one.tied => Verdict::Rule(one),
        _ => Verdict::Ambiguous,
    }
}
