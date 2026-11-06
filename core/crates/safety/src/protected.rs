//! Protected paths, encoded from `docs/safety/protected-paths.md`.
//!
//! Owned by Sam. Agents must not edit this file. Every entry quotes the line
//! of `protected-paths.md` it implements.
//!
//! Everything here is pure: it works on the components of a candidate path
//! relative to the home folder and never touches the filesystem. The checks
//! that need the filesystem (the `.git` rule and same-file identity) take a
//! probe from `lib.rs`, which only reads metadata.
//!
//! Matching rules used throughout:
//! - Names are compared after [`fold`], because macOS volumes are usually
//!   case-insensitive (`~/library` is the same folder as `~/Library`).
//! - Exceptions (the build-output folders inside a repository) are compared
//!   exactly, so case tricks can only make a path *more* protected.

use std::fmt;

/// How much of the tree a fixed entry covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Only the path itself. What is inside it is not covered by this entry.
    Itself,
    /// The path itself and everything below it.
    Subtree,
    /// The path itself and its direct children, but nothing deeper.
    ItselfAndChildren,
}

/// One fixed path under the home folder, with the reason it is protected.
#[derive(Debug, Clone, Copy)]
pub struct Entry<R: 'static> {
    /// Path components below home. Empty means the home folder itself.
    pub components: &'static [&'static str],
    /// How much of the tree the entry covers.
    pub scope: Scope,
    /// Why the entry is protected.
    pub reason: R,
}

/// Why a path is hard-blocked. There is no override for any of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockReason {
    /// The home folder itself.
    HomeFolder,
    /// One of the main folders in home, such as Desktop or Library.
    MainFolder,
    /// Keychains.
    Keychains,
    /// Mail.
    Mail,
    /// The Messages folder itself.
    MessagesFolder,
    /// A Messages database file.
    MessagesDatabase,
    /// iCloud Drive.
    ICloudDrive,
    /// Files from cloud storage apps.
    CloudStorage,
    /// A Photos, Music or TV library.
    MediaLibrary,
    /// Keys, passwords and sign-in details.
    Credentials,
    /// A Git repository, or something inside one.
    GitRepository,
    /// App signing keys, certificates and profiles.
    SigningMaterial,
    /// The path contains a hard-blocked folder or file.
    ContainsProtected,
    /// A Docker volume.
    DockerVolume,
}

impl fmt::Display for BlockReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::HomeFolder => "your home folder",
            Self::MainFolder => "one of the main folders in your home folder",
            Self::Keychains => "your keychains",
            Self::Mail => "your Mail data",
            Self::MessagesFolder => "your Messages folder",
            Self::MessagesDatabase => "your Messages history",
            Self::ICloudDrive => "iCloud Drive",
            Self::CloudStorage => "files from a cloud storage app",
            Self::MediaLibrary => "a Photos, Music or TV library",
            Self::Credentials => "keys or sign-in details",
            Self::GitRepository => "a code project under version control",
            Self::SigningMaterial => "app signing keys or certificates",
            Self::ContainsProtected => "a folder that holds protected items",
            Self::DockerVolume => "a Docker volume",
        };
        f.write_str(text)
    }
}

/// Why a path is review-only: never `likely_safe`, never preselected, and
/// the user must select each item individually.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewReason {
    /// An iPhone or iPad backup.
    DeviceBackup,
    /// A Messages attachment. Not deletable at all in v1.
    MessagesAttachment,
    /// Something in Downloads, Desktop or Documents.
    PersonalFile,
    /// Data left behind by an app.
    AppLeftovers,
    /// An Android emulator or snapshot.
    AndroidEmulator,
    /// An Xcode archive.
    XcodeArchive,
    /// A `.env` settings file.
    EnvFile,
    /// Chat history from an AI coding tool.
    AiToolHistory,
    /// A Final Cut, Logic, GarageBand or Adobe project library.
    CreativeProject,
    /// The path contains a review-only item.
    ContainsReviewOnly,
}

impl ReviewReason {
    /// Whether the app may remove this kind of item in v1 at all, even when
    /// the user selects it individually.
    pub fn deletable_in_v1(self) -> bool {
        // "shown and opened in Finder only; the app does not delete them in v1."
        !matches!(self, Self::MessagesAttachment)
    }
}

impl fmt::Display for ReviewReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::DeviceBackup => "an iPhone or iPad backup",
            Self::MessagesAttachment => "a Messages attachment",
            Self::PersonalFile => "a file in Downloads, Desktop or Documents",
            Self::AppLeftovers => "data left by an app",
            Self::AndroidEmulator => "an Android emulator",
            Self::XcodeArchive => "an Xcode archive",
            Self::EnvFile => "a settings file that may hold passwords",
            Self::AiToolHistory => "chat history from an AI tool",
            Self::CreativeProject => "a video, music or photo project",
            Self::ContainsReviewOnly => "a folder that holds items to review one by one",
        };
        f.write_str(text)
    }
}

// ---------------------------------------------------------------------------
// Hard-blocked — the executor refuses, no override
// ---------------------------------------------------------------------------

/// "Anything outside the home folder, including `/System`, `/Library`,
/// `/usr`, `/bin`, `/sbin`, `/private`, `/Applications`, `/Volumes`."
///
/// Enforced by validation step 2 (path starts with home), not by this list.
/// The list is kept so tests can prove each named root is refused.
pub const OUTSIDE_HOME_EXAMPLES: &[&str] = &[
    "/System",
    "/Library",
    "/usr",
    "/bin",
    "/sbin",
    "/private",
    "/Applications",
    "/Volumes",
];

/// Fixed hard-blocked paths under home.
pub const HARD_BLOCKED: &[Entry<BlockReason>] = &[
    // "The home folder itself"
    Entry {
        components: &[],
        scope: Scope::Itself,
        reason: BlockReason::HomeFolder,
    },
    // "these folders themselves (their contents may be targeted, the folders never): `~/Desktop`"
    Entry {
        components: &["Desktop"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Documents`"
    Entry {
        components: &["Documents"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Downloads`"
    Entry {
        components: &["Downloads"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Pictures`"
    Entry {
        components: &["Pictures"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Movies`"
    Entry {
        components: &["Movies"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Music`"
    Entry {
        components: &["Music"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Library`"
    Entry {
        components: &["Library"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Applications`"
    Entry {
        components: &["Applications"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Public`"
    Entry {
        components: &["Public"],
        scope: Scope::Itself,
        reason: BlockReason::MainFolder,
    },
    // "`~/Library/Keychains/**`"
    Entry {
        components: &["Library", "Keychains"],
        scope: Scope::Subtree,
        reason: BlockReason::Keychains,
    },
    // "`~/Library/Mail/**`"
    Entry {
        components: &["Library", "Mail"],
        scope: Scope::Subtree,
        reason: BlockReason::Mail,
    },
    // "`~/Library/Messages/` itself"
    Entry {
        components: &["Library", "Messages"],
        scope: Scope::Itself,
        reason: BlockReason::MessagesFolder,
    },
    // "`~/Library/Mobile Documents/**` (iCloud Drive)"
    Entry {
        components: &["Library", "Mobile Documents"],
        scope: Scope::Subtree,
        reason: BlockReason::ICloudDrive,
    },
    // "`~/Library/CloudStorage/**`"
    Entry {
        components: &["Library", "CloudStorage"],
        scope: Scope::Subtree,
        reason: BlockReason::CloudStorage,
    },
    // "`~/.ssh/**`"
    Entry {
        components: &[".ssh"],
        scope: Scope::Subtree,
        reason: BlockReason::Credentials,
    },
    // "`~/.gnupg/**`"
    Entry {
        components: &[".gnupg"],
        scope: Scope::Subtree,
        reason: BlockReason::Credentials,
    },
    // "`~/.aws/**`"
    Entry {
        components: &[".aws"],
        scope: Scope::Subtree,
        reason: BlockReason::Credentials,
    },
    // "`~/.netrc`" (Subtree rather than Itself, in case it is ever a folder: safer)
    Entry {
        components: &[".netrc"],
        scope: Scope::Subtree,
        reason: BlockReason::Credentials,
    },
    // "`~/.docker/config.json`"
    Entry {
        components: &[".docker", "config.json"],
        scope: Scope::Subtree,
        reason: BlockReason::Credentials,
    },
    // "`~/.kube/**`"
    Entry {
        components: &[".kube"],
        scope: Scope::Subtree,
        reason: BlockReason::Credentials,
    },
    // "`~/.config/gh/**`"
    Entry {
        components: &[".config", "gh"],
        scope: Scope::Subtree,
        reason: BlockReason::Credentials,
    },
    // "Signing material: `~/Library/MobileDevice/Provisioning Profiles/**`"
    Entry {
        components: &["Library", "MobileDevice", "Provisioning Profiles"],
        scope: Scope::Subtree,
        reason: BlockReason::SigningMaterial,
    },
];

/// "`~/Library/Messages/*.db*`": the folder under which database names are blocked.
///
/// Safer than written: a name containing `.db` is blocked at any depth under
/// Messages, not only directly inside it.
pub const MESSAGES_FOLDER: &[&str] = &["Library", "Messages"];

/// "`~/Library/Messages/*.db*`": the text a blocked name contains.
pub const MESSAGES_DB_MARKER: &str = ".db";

/// "Any `*.photoslibrary`, `*.musiclibrary`, `*.tvlibrary` bundle and
/// everything inside it."
pub const MEDIA_LIBRARY_EXTENSIONS: &[&str] = &["photoslibrary", "musiclibrary", "tvlibrary"];

/// "any file matching `*.keystore`, `*.jks`, `*.p12`, `*.p8`,
/// `*.mobileprovision`, `*.pem`, `*.key`."
///
/// Safer than written: a folder with one of these names, and anything inside
/// it, is blocked too.
pub const SIGNING_EXTENSIONS: &[&str] = &[
    "keystore",
    "jks",
    "p12",
    "p8",
    "mobileprovision",
    "pem",
    "key",
];

/// "Any `.git` directory, and any directory that contains a `.git` directory"
pub const GIT_DIR_NAME: &str = ".git";

/// "**except** these named build-output children inside a repository:
/// `node_modules`, `build`, `target`, `DerivedData`, `.gradle`, `Pods`,
/// `.next`, `dist`, `.venv`, `venv`, `__pycache__`."
///
/// Matched exactly (case-sensitive), and only as a direct child of the
/// folder that holds `.git`.
pub const BUILD_OUTPUT_EXCEPTIONS: &[&str] = &[
    "node_modules",
    "build",
    "target",
    "DerivedData",
    ".gradle",
    "Pods",
    ".next",
    "dist",
    ".venv",
    "venv",
    "__pycache__",
];

/// "Docker volumes, by identifier, in every Docker action."
///
/// Docker volumes are not paths, so this is a separate check that every
/// Docker action must call. It refuses every volume, whatever its id.
pub fn docker_volume_block(_volume_id: &str) -> BlockReason {
    BlockReason::DockerVolume
}

// ---------------------------------------------------------------------------
// Review-only — never `likely_safe`, never preselected, user must select
// each item individually
// ---------------------------------------------------------------------------

/// Fixed review-only paths under home.
pub const REVIEW_ONLY: &[Entry<ReviewReason>] = &[
    // "iPhone and iPad backups (`~/Library/Application Support/MobileSync/Backup/*`)."
    // Subtree, so the Backup folder and anything inside a backup is covered too.
    Entry {
        components: &["Library", "Application Support", "MobileSync", "Backup"],
        scope: Scope::Subtree,
        reason: ReviewReason::DeviceBackup,
    },
    // "Messages attachments (`~/Library/Messages/Attachments/**`) — shown and
    // opened in Finder only; the app does not delete them in v1."
    Entry {
        components: &["Library", "Messages", "Attachments"],
        scope: Scope::Subtree,
        reason: ReviewReason::MessagesAttachment,
    },
    // "Any file directly in Downloads, Desktop or Documents, including large
    // files and duplicates." Safer than written: anything at any depth.
    Entry {
        components: &["Downloads"],
        scope: Scope::Subtree,
        reason: ReviewReason::PersonalFile,
    },
    Entry {
        components: &["Desktop"],
        scope: Scope::Subtree,
        reason: ReviewReason::PersonalFile,
    },
    Entry {
        components: &["Documents"],
        scope: Scope::Subtree,
        reason: ReviewReason::PersonalFile,
    },
    // "App leftovers in `~/Library/Containers/*` ... for apps not currently installed."
    // Whether the app is installed is not known here, so every app folder is review-only.
    Entry {
        components: &["Library", "Containers"],
        scope: Scope::ItselfAndChildren,
        reason: ReviewReason::AppLeftovers,
    },
    // "... and `~/Library/Application Support/*` for apps not currently installed."
    Entry {
        components: &["Library", "Application Support"],
        scope: Scope::ItselfAndChildren,
        reason: ReviewReason::AppLeftovers,
    },
    // "Android AVDs (`~/.android/avd/**`) and emulator snapshots"
    // (snapshots live inside each AVD, so the subtree covers them).
    Entry {
        components: &[".android", "avd"],
        scope: Scope::Subtree,
        reason: ReviewReason::AndroidEmulator,
    },
    // "Xcode archives (`~/Library/Developer/Xcode/Archives/**`)."
    Entry {
        components: &["Library", "Developer", "Xcode", "Archives"],
        scope: Scope::Subtree,
        reason: ReviewReason::XcodeArchive,
    },
    // "AI-tool session history: `~/.claude/projects/**`"
    Entry {
        components: &[".claude", "projects"],
        scope: Scope::Subtree,
        reason: ReviewReason::AiToolHistory,
    },
    // "`~/.codex/sessions/**`"
    Entry {
        components: &[".codex", "sessions"],
        scope: Scope::Subtree,
        reason: ReviewReason::AiToolHistory,
    },
    // "Cursor chat databases" (Cursor keeps them in these two folders).
    Entry {
        components: &[
            "Library",
            "Application Support",
            "Cursor",
            "User",
            "workspaceStorage",
        ],
        scope: Scope::Subtree,
        reason: ReviewReason::AiToolHistory,
    },
    Entry {
        components: &[
            "Library",
            "Application Support",
            "Cursor",
            "User",
            "globalStorage",
        ],
        scope: Scope::Subtree,
        reason: ReviewReason::AiToolHistory,
    },
];

/// "Any `.env` / `.env.*` file." The exact name.
pub const ENV_FILE_NAME: &str = ".env";

/// "Any `.env` / `.env.*` file." The prefix of `.env.*` names.
pub const ENV_FILE_PREFIX: &str = ".env.";

/// "Final Cut (`*.fcpbundle`), Logic, GarageBand and Adobe project libraries
/// (caches inside them may be `likely_safe` only via a specific rule)."
///
/// Logic is `.logicx`, GarageBand is `.band`, Adobe is Premiere `.prproj`,
/// After Effects `.aep` and Lightroom `.lrcat` / `.lrlibrary`.
/// The bundle and everything inside it are review-only here; a later rule may
/// only lower the tier of caches inside, never this gate.
pub const CREATIVE_PROJECT_EXTENSIONS: &[&str] = &[
    "fcpbundle",
    "logicx",
    "band",
    "prproj",
    "aep",
    "lrcat",
    "lrlibrary",
];

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// Folds a name for case-insensitive comparison, the way a case-insensitive
/// macOS volume treats it closely enough for protection. `ſ` (long s) is
/// folded to `s` because Unicode case folding does so and `to_lowercase`
/// does not.
pub fn fold(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ſ' { 's' } else { c })
        .collect()
}

/// Folds every component of a relative path.
pub fn fold_all(rel: &[&str]) -> Vec<String> {
    rel.iter().map(|c| fold(c)).collect()
}

/// Whether the folded path `rel` starts with the entry's components.
fn starts_with_entry(rel: &[String], entry: &[&str]) -> bool {
    rel.len() >= entry.len() && rel.iter().zip(entry).all(|(a, b)| *a == fold(b))
}

/// Whether the entry covers the folded path `rel`.
pub fn entry_covers<R>(entry: &Entry<R>, rel: &[String]) -> bool {
    if !starts_with_entry(rel, entry.components) {
        return false;
    }
    let extra = rel.len() - entry.components.len();
    match entry.scope {
        Scope::Itself => extra == 0,
        Scope::Subtree => true,
        Scope::ItselfAndChildren => extra <= 1,
    }
}

/// Whether the folded path `rel` is a strict ancestor of the entry, so
/// removing it would remove the entry too.
pub fn contains_entry<R>(entry: &Entry<R>, rel: &[String]) -> bool {
    entry.components.len() > rel.len()
        && rel.iter().zip(entry.components).all(|(a, b)| *a == fold(b))
}

/// Whether a folded name ends with `.<ext>` for any of the extensions.
fn has_extension(folded_name: &str, extensions: &[&str]) -> bool {
    extensions
        .iter()
        .any(|ext| folded_name.ends_with(&format!(".{}", fold(ext))))
}

/// The hard-block reason for a path relative to home, from the name-based
/// rules alone. The `.git` rule needs the filesystem: see [`git_block`].
pub fn hard_block(rel: &[&str]) -> Option<BlockReason> {
    let folded = fold_all(rel);
    if let Some(entry) = HARD_BLOCKED.iter().find(|e| entry_covers(e, &folded)) {
        return Some(entry.reason);
    }
    if HARD_BLOCKED.iter().any(|e| contains_entry(e, &folded)) {
        return Some(BlockReason::ContainsProtected);
    }
    let messages_len = MESSAGES_FOLDER.len();
    if starts_with_entry(&folded, MESSAGES_FOLDER)
        && folded[messages_len..]
            .iter()
            .any(|n| n.contains(MESSAGES_DB_MARKER))
    {
        return Some(BlockReason::MessagesDatabase);
    }
    if folded
        .iter()
        .any(|n| has_extension(n, MEDIA_LIBRARY_EXTENSIONS))
    {
        return Some(BlockReason::MediaLibrary);
    }
    if folded.iter().any(|n| has_extension(n, SIGNING_EXTENSIONS)) {
        return Some(BlockReason::SigningMaterial);
    }
    if folded.iter().any(|n| n == GIT_DIR_NAME) {
        return Some(BlockReason::GitRepository);
    }
    None
}

/// The `.git` rule. `contains_git(i)` must say whether the folder made of
/// home plus the first `i` components holds an entry named `.git` (of any
/// type). It is asked for `i = 0` (home) up to `i = rel.len()` (the
/// candidate), stopping at the first block.
///
/// A path is blocked if it is a folder that holds `.git`, or if it is inside
/// such a folder and does not pass through one of the
/// [`BUILD_OUTPUT_EXCEPTIONS`] as the direct child of that folder.
pub fn git_block<E>(
    rel: &[&str],
    mut contains_git: impl FnMut(usize) -> Result<bool, E>,
) -> Result<Option<BlockReason>, E> {
    for i in 0..=rel.len() {
        if !contains_git(i)? {
            continue;
        }
        match rel.get(i) {
            Some(child) if BUILD_OUTPUT_EXCEPTIONS.contains(child) => continue,
            _ => return Ok(Some(BlockReason::GitRepository)),
        }
    }
    Ok(None)
}

/// The review-only reason for a path relative to home, if any. Call only
/// after the path has passed every hard-block check.
pub fn review_only(rel: &[&str]) -> Option<ReviewReason> {
    let folded = fold_all(rel);
    if let Some(entry) = REVIEW_ONLY.iter().find(|e| entry_covers(e, &folded)) {
        return Some(entry.reason);
    }
    if REVIEW_ONLY.iter().any(|e| contains_entry(e, &folded)) {
        return Some(ReviewReason::ContainsReviewOnly);
    }
    if folded
        .iter()
        .any(|n| n == ENV_FILE_NAME || n.starts_with(ENV_FILE_PREFIX))
    {
        return Some(ReviewReason::EnvFile);
    }
    if folded
        .iter()
        .any(|n| has_extension(n, CREATIVE_PROJECT_EXTENSIONS))
    {
        return Some(ReviewReason::CreativeProject);
    }
    None
}
