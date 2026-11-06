//! Walks the scanned folder tree and turns rule matches into items.
//!
//! The walk starts at the home folder's children and goes down. The first
//! folder (or recorded file) that a rule matches becomes an item, and the
//! walk does not go inside it: everything in it belongs to that item. What
//! is inside is still inspected, so that an item holding something
//! protected, or something another rule describes, gets a more careful tier.

use rules::{FoldedPath, Tier};
use safety::protected::{git_block, hard_block, review_only, BlockReason};
use scanner::{DirId, FileEntry, ScanResult};

use crate::compiled::{verdict, Compiled, Verdict};
use crate::item::{caution, ClassifiedItem, ItemId, ItemKind, Lowered};

/// What the walk needs to see.
pub(crate) struct Ctx<'a, 'r> {
    pub scan: &'a ScanResult,
    pub rules: &'a [Compiled<'r>],
}

/// One folder reached by the walk.
#[derive(Clone)]
struct Frame {
    /// The folder.
    id: DirId,
    /// Its names relative to home.
    names: Vec<String>,
    /// Folder ids from home (index 0) down to this folder (last).
    chain: Vec<DirId>,
}

impl Frame {
    /// The frame for child folder `id` named `name`.
    fn child(&self, id: DirId, name: &str) -> Frame {
        let mut names = self.names.clone();
        names.push(name.to_string());
        let mut chain = self.chain.clone();
        chain.push(id);
        Frame { id, names, chain }
    }
}

/// An item found by a rule, with the top-level folder it sits in.
pub(crate) struct Found {
    /// The home folder's child that holds the item, or `None` for a file
    /// directly in home.
    pub top: Option<DirId>,
    pub item: ClassifiedItem,
}

/// Finds every rule-matched item in the scan.
pub(crate) fn find_items(ctx: &Ctx<'_, '_>) -> Vec<Found> {
    let mut found = Vec::new();
    let root = Frame {
        id: 0,
        names: Vec::new(),
        chain: vec![0],
    };
    let mut stack = Vec::new();
    open_folder(ctx, &root, &mut found, &mut stack);
    while let Some(frame) = stack.pop() {
        let path = FoldedPath::from_names(&frame.names);
        match verdict(ctx.rules, &path, |c| requires_ok(ctx, frame.id, c)) {
            Verdict::Rule(c) => found.extend(folder_item(ctx, &frame, c)),
            Verdict::Ambiguous => {
                tracing::debug!(path = %frame.names.join("/"), "rules disagree; left as unknown");
            }
            Verdict::NoRule => open_folder(ctx, &frame, &mut found, &mut stack),
        }
    }
    found
}

/// Looks at the recorded files in a folder no rule matched, and queues its
/// sub-folders. Names that are not valid UTF-8 are skipped: no rule can be
/// checked against them, so their space stays unknown.
fn open_folder(ctx: &Ctx<'_, '_>, frame: &Frame, found: &mut Vec<Found>, stack: &mut Vec<Frame>) {
    let Some(dir) = ctx.scan.dirs.get(frame.id) else {
        return;
    };
    for file in &dir.files {
        let Some(name) = file.name.to_str() else {
            continue;
        };
        let mut names = frame.names.clone();
        names.push(name.to_string());
        // A file holds nothing, so a rule with `match.requires` never matches it.
        if let Verdict::Rule(c) = verdict(ctx.rules, &FoldedPath::from_names(&names), |_| false) {
            found.extend(file_item(ctx, frame, file, names, c));
        }
    }
    for &child in dir.children.iter().rev() {
        let Some(name) = ctx.scan.dirs.get(child).and_then(|d| d.name.to_str()) else {
            continue;
        };
        stack.push(frame.child(child, name));
    }
}

/// Whether every `match.requires` glob of `c` names something inside
/// folder `dir`.
fn requires_ok(ctx: &Ctx<'_, '_>, dir: DirId, c: &Compiled<'_>) -> bool {
    c.requires
        .iter()
        .all(|glob| exists_inside(ctx.scan, dir, glob))
}

/// Whether a folder or recorded file inside `dir` matches `glob`, relative
/// to `dir`. Only goes as deep as the glob can reach.
fn exists_inside(scan: &ScanResult, dir: DirId, glob: &rules::Glob) -> bool {
    let max_depth = if glob.has_any_depth() {
        usize::MAX
    } else {
        glob.depth()
    };
    let mut stack: Vec<(DirId, Vec<String>)> = vec![(dir, Vec::new())];
    while let Some((id, names)) = stack.pop() {
        let Some(d) = scan.dirs.get(id) else {
            continue;
        };
        if names.len() < max_depth {
            for file in &d.files {
                if let Some(name) = file.name.to_str() {
                    let mut file_names = names.clone();
                    file_names.push(name.to_string());
                    if glob.matches_folded(&FoldedPath::from_names(&file_names)) {
                        return true;
                    }
                }
            }
            for &child in &d.children {
                let Some(name) = scan.dirs.get(child).and_then(|c| c.name.to_str()) else {
                    continue;
                };
                let mut child_names = names.clone();
                child_names.push(name.to_string());
                if glob.matches_folded(&FoldedPath::from_names(&child_names)) {
                    return true;
                }
                stack.push((child, child_names));
            }
        }
    }
    false
}

/// Collects the reasons an item gets a more careful tier than its rule's.
struct Caution {
    tier: Tier,
    lowered: Vec<Lowered>,
}

impl Caution {
    fn new(tier: Tier) -> Self {
        Self {
            tier,
            lowered: Vec::new(),
        }
    }

    /// Caps the tier at `max` for `reason`. The reason is recorded only if
    /// it makes the tier more careful than the rule's own.
    fn cap(&mut self, rule_tier: Tier, reason: Lowered, max: Tier) {
        if caution(max) <= caution(rule_tier) {
            return;
        }
        if caution(max) > caution(self.tier) {
            self.tier = max;
        }
        if !self.lowered.contains(&reason) {
            self.lowered.push(reason);
        }
    }

    fn is_keep(&self) -> bool {
        self.tier == Tier::Keep
    }
}

/// Applies the safety crate's name-based checks to one path relative to
/// home. `contains_git(i)` says whether the folder made of the first `i`
/// names holds `.git`.
fn check_path(
    names: &[String],
    contains_git: impl FnMut(usize) -> Result<bool, ()>,
    rule_tier: Tier,
    caution: &mut Caution,
) {
    let rel: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Some(reason) = hard_block(&rel) {
        caution.cap(rule_tier, Lowered::Protected(reason), Tier::Keep);
    } else if let Ok(Some(reason)) = git_block(&rel, contains_git) {
        caution.cap(rule_tier, Lowered::Protected(reason), Tier::Keep);
    }
    if let Some(reason) = review_only(&rel) {
        let max = if reason.deletable_in_v1() {
            Tier::ReviewFirst
        } else {
            Tier::Keep
        };
        caution.cap(rule_tier, Lowered::ReviewOnly(reason), max);
    }
}

/// Checks what another rule says about a path inside an item.
fn check_nested(
    ctx: &Ctx<'_, '_>,
    names: &[String],
    dir: Option<DirId>,
    own: &Compiled<'_>,
    caution: &mut Caution,
) {
    let path = FoldedPath::from_names(names);
    let v = verdict(ctx.rules, &path, |c| match dir {
        Some(d) => requires_ok(ctx, d, c),
        None => false,
    });
    let rule_tier = own.rule.tier;
    match v {
        Verdict::NoRule => {}
        Verdict::Rule(other) if other.rule.id == own.rule.id => {}
        Verdict::Rule(other) => caution.cap(
            rule_tier,
            Lowered::NestedRule {
                rule_id: other.rule.id.clone(),
            },
            other.rule.tier,
        ),
        Verdict::Ambiguous => caution.cap(rule_tier, Lowered::RulesDisagree, Tier::Keep),
    }
}

/// Inspects everything inside a matched folder: protected and review-only
/// paths, git repositories, unreadable folders, odd names and other rules.
fn inspect_inside(ctx: &Ctx<'_, '_>, item: &Frame, own: &Compiled<'_>, caution: &mut Caution) {
    let rule_tier = own.rule.tier;
    let mut stack = vec![item.clone()];
    while let Some(frame) = stack.pop() {
        if caution.is_keep() {
            return;
        }
        let Some(dir) = ctx.scan.dirs.get(frame.id) else {
            continue;
        };
        if frame.id != item.id {
            if dir.has_git {
                let reason = Lowered::Protected(BlockReason::GitRepository);
                caution.cap(rule_tier, reason, Tier::Keep);
            }
            check_path(&frame.names, |_| Ok(false), rule_tier, caution);
            check_nested(ctx, &frame.names, Some(frame.id), own, caution);
        }
        if dir.unreadable {
            caution.cap(rule_tier, Lowered::Unreadable, Tier::ReviewFirst);
        }
        for file in &dir.files {
            let Some(name) = file.name.to_str() else {
                caution.cap(rule_tier, Lowered::UnusualName, Tier::ReviewFirst);
                continue;
            };
            let mut names = frame.names.clone();
            names.push(name.to_string());
            check_path(&names, |_| Ok(false), rule_tier, caution);
            check_nested(ctx, &names, None, own, caution);
        }
        for &child in &dir.children {
            match ctx.scan.dirs.get(child).and_then(|d| d.name.to_str()) {
                Some(name) => stack.push(frame.child(child, name)),
                None => caution.cap(rule_tier, Lowered::UnusualName, Tier::ReviewFirst),
            }
        }
    }
}

/// Builds the item for a folder that rule `c` matched.
fn folder_item(ctx: &Ctx<'_, '_>, frame: &Frame, c: &Compiled<'_>) -> Option<Found> {
    let dir = ctx.scan.dirs.get(frame.id)?;
    let path = ctx.scan.path_of(frame.id)?;
    let rule_tier = c.rule.tier;
    let mut caution = Caution::new(rule_tier);
    // The `.git` rule looks at the folders from home down to the item itself.
    let has_git = |i: usize| -> Result<bool, ()> {
        Ok(frame
            .chain
            .get(i)
            .and_then(|&id| ctx.scan.dirs.get(id))
            .is_some_and(|d| d.has_git))
    };
    check_path(&frame.names, has_git, rule_tier, &mut caution);
    inspect_inside(ctx, frame, c, &mut caution);
    Some(Found {
        top: frame.chain.get(1).copied(),
        item: build(
            c,
            &frame.names,
            ItemKind::Folder,
            dir.total_bytes,
            dir.modified,
            path,
            caution,
        ),
    })
}

/// Builds the item for a recorded file that rule `c` matched. `parent` is
/// the folder the file is in and `names` the file's own names from home.
fn file_item(
    ctx: &Ctx<'_, '_>,
    parent: &Frame,
    file: &FileEntry,
    names: Vec<String>,
    c: &Compiled<'_>,
) -> Option<Found> {
    let path = ctx.scan.path_of(parent.id)?.join(&file.name);
    let rule_tier = c.rule.tier;
    let mut caution = Caution::new(rule_tier);
    // The folders from home down to the file's own folder; a file holds no `.git`.
    let has_git = |i: usize| -> Result<bool, ()> {
        Ok(i < parent.chain.len()
            && parent
                .chain
                .get(i)
                .and_then(|&id| ctx.scan.dirs.get(id))
                .is_some_and(|d| d.has_git))
    };
    check_path(&names, has_git, rule_tier, &mut caution);
    Some(Found {
        top: parent.chain.get(1).copied(),
        item: build(
            c,
            &names,
            ItemKind::File,
            file.allocated_bytes,
            file.modified,
            path,
            caution,
        ),
    })
}

/// Fills in a [`ClassifiedItem`] from the rule and the cautions found.
fn build(
    c: &Compiled<'_>,
    names: &[String],
    kind: ItemKind,
    size_bytes: u64,
    modified: Option<std::time::SystemTime>,
    path: std::path::PathBuf,
    mut caution: Caution,
) -> ClassifiedItem {
    let rule = c.rule;
    if rule.confidence == rules::Confidence::Low {
        caution.cap(rule.tier, Lowered::LowConfidence, Tier::ReviewFirst);
    }
    let rel = names.join("/");
    let keep = caution.is_keep();
    ClassifiedItem {
        id: ItemId::new(&rule.id, rel.as_bytes()),
        rule_id: rule.id.clone(),
        rule_version: rule.version,
        title: rule.title.clone(),
        explain: rule.explain.clone(),
        risk: rule.risk.clone(),
        restore: rule.restore.clone(),
        category: rule.category,
        tier: caution.tier,
        confidence: rule.confidence,
        regenerable: rule.regenerable && !keep,
        action: if keep {
            rules::Action::ShowOnly
        } else {
            rule.action
        },
        command: if keep { None } else { rule.command.clone() },
        kind,
        size_bytes,
        modified,
        path,
        lowered: caution.lowered,
    }
}
