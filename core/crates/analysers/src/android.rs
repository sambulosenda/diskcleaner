//! The Android analyser: finds the parts of the Android SDK and Gradle in the
//! home folder, finds the Android projects in the home folder, and says which
//! parts those projects (and the emulators) use.
//!
//! Read only. It reads folder listings, metadata and small text files
//! (`source.properties`, each emulator's `config.ini`, and Gradle build
//! files). It runs no commands and changes nothing.
//!
//! When it is not sure whether something is used, it says so: a part is only
//! called unused when every project it found names its versions plainly and
//! none of them names that part. Saying "used" when a part is not costs the
//! user some space; saying "unused" when it is costs them a download, so the
//! doubt always goes towards "used".

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Where Android Studio puts the SDK, relative to the home folder.
pub const SDK_DIR: &str = "Library/Android/sdk";
/// Where the emulators live, relative to the home folder.
pub const AVD_DIR: &str = ".android/avd";
/// Gradle's own folder, relative to the home folder.
pub const GRADLE_DIR: &str = ".gradle";

/// How deep below the home folder the project search goes.
const MAX_DEPTH: usize = 8;
/// How many folders the project search opens at most, so a huge home folder
/// can't make the screen hang.
const MAX_DIRS: usize = 60_000;
/// How deep below a project's root its build files are looked for.
const PROJECT_DEPTH: usize = 3;
/// How many build files are read per project at most.
const MAX_PROJECT_FILES: usize = 200;
/// Build files bigger than this are not read.
const MAX_FILE_BYTES: u64 = 512 * 1024;

/// Folders the project search never opens, anywhere.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "build",
    "Pods",
    "DerivedData",
    "target",
    "dist",
    "vendor",
    "venv",
    "__pycache__",
];
/// Folders directly in the home folder that the project search skips: they
/// hold apps, media and system data, not projects.
const SKIP_HOME_DIRS: &[&str] = &[
    "Library",
    "Applications",
    "Pictures",
    "Music",
    "Movies",
    "Public",
];
/// Folder name endings for macOS packages, which the search never opens.
const PACKAGE_ENDINGS: &[&str] = &[
    ".app",
    ".photoslibrary",
    ".musiclibrary",
    ".tvlibrary",
    ".fcpbundle",
    ".xcarchive",
    ".framework",
    ".bundle",
    ".xcodeproj",
    ".xcworkspace",
];
/// Folders inside a project that hold no build settings.
const SKIP_PROJECT_DIRS: &[&str] = &["src", "build", "node_modules", "res", "assets", "libs"];

/// What kind of part something is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PartKind {
    /// An emulator system image (`system-images/android-34/google_apis/arm64-v8a`).
    SystemImage,
    /// An emulator (`~/.android/avd/Pixel_7.avd`).
    Emulator,
    /// An SDK platform (`platforms/android-34`), what a project compiles against.
    Platform,
    /// One version of the build tools (`build-tools/34.0.0`).
    BuildTools,
    /// One version of the NDK (`ndk/26.1.10909125`).
    Ndk,
    /// One Gradle version the Gradle wrapper downloaded.
    GradleVersion,
    /// Gradle's download and build cache (`~/.gradle/caches`).
    GradleCache,
}

/// One part of the SDK, an emulator, or a Gradle download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// What it is.
    pub kind: PartKind,
    /// Where it is. Absolute, inside the home folder.
    pub path: PathBuf,
    /// The name its tool knows it by: `android-34/google_apis/arm64-v8a`,
    /// `android-34`, `34.0.0`, `26.1.10909125`, `gradle-8.4-bin`, or the
    /// emulator's folder name without `.avd`.
    pub name: String,
    /// The Android API level, for system images, platforms and emulators
    /// (from the system image they use).
    pub api_level: Option<u32>,
    /// For a system image, its kind (`google_apis`, `google_apis_playstore`).
    pub image_tag: Option<String>,
    /// For a system image, the processor it is for (`arm64-v8a`, `x86_64`).
    pub abi: Option<String>,
    /// For an emulator, the name Android Studio shows.
    pub display_name: Option<String>,
    /// For an emulator, the system image it runs, in the same form as a
    /// system image's `name`. `None` when its settings don't say.
    pub uses_image: Option<String>,
    /// For an emulator, when anything directly inside it last changed,
    /// which is about when it last ran.
    pub last_used: Option<SystemTime>,
}

/// What one Android project uses, from its Gradle build files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Uses {
    /// The `compileSdk` values: an API level (`34`) or a preview name.
    pub compile_sdks: BTreeSet<String>,
    /// A `compileSdk` setting Roomy couldn't read.
    pub compile_sdk_unknown: bool,
    /// The `buildToolsVersion` values.
    pub build_tools: BTreeSet<String>,
    /// No `buildToolsVersion` anywhere, so the Android Gradle plugin picks one.
    pub build_tools_default: bool,
    /// A `buildToolsVersion` setting Roomy couldn't read.
    pub build_tools_unknown: bool,
    /// The `ndkVersion` values.
    pub ndks: BTreeSet<String>,
    /// The project builds C or C++ code without naming an NDK version, so
    /// the Android Gradle plugin picks one.
    pub ndk_default: bool,
    /// An `ndkVersion` setting Roomy couldn't read.
    pub ndk_unknown: bool,
    /// The Gradle version the wrapper uses, as the folder Gradle downloads
    /// it into (`gradle-8.4-bin`).
    pub gradle: Option<String>,
    /// The project has a Gradle wrapper whose version Roomy couldn't read.
    pub gradle_unknown: bool,
}

/// One Android project found in the home folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The folder holding its `settings.gradle` (or `build.gradle`).
    pub root: PathBuf,
    /// The project's name: its folder's name, or for a React Native or
    /// Flutter `android` folder, the name of the folder around it.
    pub name: String,
    /// What it uses.
    pub uses: Uses,
    /// When its build files, or anything directly in its folder, last
    /// changed: roughly when it was last worked on.
    pub last_activity: Option<SystemTime>,
}

/// Whether, and by what, a part is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Usage {
    /// Named by these projects (or, for a system image, these emulators).
    Used { by: Vec<String> },
    /// No project names it, but these projects leave the choice to the
    /// Android Gradle plugin, and this is the newest one installed, so it is
    /// probably the one they use.
    ProbablyUsed { by: Vec<String> },
    /// Roomy couldn't read a setting of these projects (or emulators), so
    /// it can't tell.
    NotSure { by: Vec<String> },
    /// No project (or emulator) Roomy found uses it.
    NotUsed,
    /// Not something projects name: an emulator, or Gradle's cache.
    NotApplicable,
}

/// Everything in `home`'s Android SDK, emulators and Gradle folders, in a
/// fixed order (by kind, then by name). Anything that is a symlink, or that
/// can't be read, is left out.
pub fn find_parts(home: &Path) -> Vec<Part> {
    let mut parts = Vec::new();
    if has_sdk(home) {
        let sdk = home.join(SDK_DIR);
        parts.extend(system_images(&sdk));
        parts.extend(simple_parts(&sdk.join("platforms"), PartKind::Platform));
        parts.extend(simple_parts(&sdk.join("build-tools"), PartKind::BuildTools));
        parts.extend(simple_parts(&sdk.join("ndk"), PartKind::Ndk));
    }
    if real_path(home, AVD_DIR) {
        parts.extend(emulators(&home.join(AVD_DIR)));
    }
    if real_path(home, GRADLE_DIR) {
        parts.extend(gradle_parts(&home.join(GRADLE_DIR)));
    }
    parts.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| compare_versions(&b.name, &a.name))
    });
    parts
}

/// Whether `home` has an Android SDK folder.
pub fn has_sdk(home: &Path) -> bool {
    real_path(home, SDK_DIR)
}

/// Whether `home/rel` is a folder reached without passing through a
/// symlink: `rel` and every folder on the way to it are real folders.
fn real_path(home: &Path, rel: &str) -> bool {
    let mut at = home.to_path_buf();
    rel.split('/').all(|part| {
        at.push(part);
        real_dir(&at)
    })
}

/// The Android projects in `home`, by name. Looks at most `MAX_DEPTH`
/// folders deep, never inside `Library`, hidden folders, packages or
/// dependency folders, and never on another volume.
pub fn find_projects(home: &Path) -> Vec<Project> {
    let Ok(home_meta) = std::fs::symlink_metadata(home) else {
        return Vec::new();
    };
    let dev = home_meta.dev();
    let mut projects = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(home.to_path_buf(), 0)];
    let mut opened = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if opened >= MAX_DIRS {
            tracing::info!("android project search stopped at its folder limit");
            break;
        }
        opened += 1;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirs = Vec::new();
        let mut is_root = false;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_file() && is_root_marker(&name) {
                is_root = true;
            } else if kind.is_dir() && depth < MAX_DEPTH && searchable(&name, depth == 0) {
                subdirs.push(entry.path());
            }
        }
        if is_root {
            if let Some(project) = read_project(&dir) {
                projects.push(project);
            }
            // A project's own folders hold modules, not other projects.
            continue;
        }
        for sub in subdirs {
            if std::fs::symlink_metadata(&sub).is_ok_and(|m| m.dev() == dev && m.is_dir()) {
                stack.push((sub, depth + 1));
            }
        }
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.root.cmp(&b.root)));
    projects
}

/// How `part` is used by `projects`, and (for system images) by the
/// emulators in `parts`.
pub fn usage(part: &Part, parts: &[Part], projects: &[Project]) -> Usage {
    match part.kind {
        PartKind::SystemImage => image_usage(part, parts),
        PartKind::Emulator | PartKind::GradleCache => Usage::NotApplicable,
        PartKind::Platform => named_usage(projects, |u| {
            let used = u.compile_sdks.iter().any(|v| platform_matches(part, v));
            (used, false, u.compile_sdk_unknown)
        }),
        PartKind::BuildTools => {
            let newest = is_newest(part, parts);
            named_usage(projects, |u| {
                let used = u.build_tools.iter().any(|v| version_matches(&part.name, v));
                (used, u.build_tools_default && newest, u.build_tools_unknown)
            })
        }
        PartKind::Ndk => {
            let newest = is_newest(part, parts);
            named_usage(projects, |u| {
                let used = u.ndks.iter().any(|v| version_matches(&part.name, v));
                (used, u.ndk_default && newest, u.ndk_unknown)
            })
        }
        PartKind::GradleVersion => named_usage(projects, |u| {
            let used = u.gradle.as_deref() == Some(part.name.as_str());
            (used, false, u.gradle_unknown)
        }),
    }
}

/// The usage of a part that projects name. `check` says, for one project,
/// whether it names the part, whether it probably uses it by default, and
/// whether one of its settings couldn't be read.
fn named_usage(projects: &[Project], check: impl Fn(&Uses) -> (bool, bool, bool)) -> Usage {
    let mut used = Vec::new();
    let mut probably = Vec::new();
    let mut unsure = Vec::new();
    for p in projects {
        let (named, by_default, unknown) = check(&p.uses);
        if named {
            used.push(p.name.clone());
        } else if by_default {
            probably.push(p.name.clone());
        } else if unknown {
            unsure.push(p.name.clone());
        }
    }
    if !used.is_empty() {
        Usage::Used { by: dedup(used) }
    } else if !probably.is_empty() {
        Usage::ProbablyUsed {
            by: dedup(probably),
        }
    } else if !unsure.is_empty() {
        Usage::NotSure { by: dedup(unsure) }
    } else {
        Usage::NotUsed
    }
}

/// Which emulators run a system image.
fn image_usage(image: &Part, parts: &[Part]) -> Usage {
    let emulators = parts.iter().filter(|p| p.kind == PartKind::Emulator);
    let mut used = Vec::new();
    let mut unsure = Vec::new();
    for e in emulators {
        let name = e.display_name.clone().unwrap_or_else(|| e.name.clone());
        match &e.uses_image {
            Some(i) if i.eq_ignore_ascii_case(&image.name) => used.push(name),
            Some(_) => {}
            None => unsure.push(name),
        }
    }
    if !used.is_empty() {
        Usage::Used { by: dedup(used) }
    } else if !unsure.is_empty() {
        Usage::NotSure { by: dedup(unsure) }
    } else {
        Usage::NotUsed
    }
}

/// `names` sorted, each once.
fn dedup(names: Vec<String>) -> Vec<String> {
    names
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Whether `part` is the newest installed part of its kind.
fn is_newest(part: &Part, parts: &[Part]) -> bool {
    !parts
        .iter()
        .filter(|p| p.kind == part.kind)
        .any(|p| compare_versions(&p.name, &part.name) == std::cmp::Ordering::Greater)
}

/// Whether the platform `part` is the one a `compileSdk` value names. An
/// API level matches every platform folder with that level (such as
/// `android-34` and `android-34-ext8`); a preview name matches its folder.
fn platform_matches(part: &Part, value: &str) -> bool {
    if let Ok(level) = value.parse::<u32>() {
        return part.api_level == Some(level);
    }
    let folder = part.name.strip_prefix("android-").unwrap_or(&part.name);
    folder.eq_ignore_ascii_case(value)
}

/// Whether the installed version `installed` is the one `named` names:
/// the same, or `named` is a shorter form of it (`34` for `34.0.0`).
fn version_matches(installed: &str, named: &str) -> bool {
    installed == named
        || installed
            .strip_prefix(named)
            .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('-'))
}

/// Compares two version names part by part, numbers as numbers.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let split =
        |s: &str| -> Vec<String> { s.split(['.', '-', '/', '_']).map(str::to_string).collect() };
    let (a, b) = (split(a), split(b));
    for (x, y) in a.iter().zip(b.iter()) {
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if order != std::cmp::Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

/// Whether `path` is a folder and not a symlink.
fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// The real folders directly in `dir`, by name.
fn subdirs(dir: &Path) -> Vec<(String, PathBuf)> {
    if !real_dir(dir) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.file_name().to_str()?.to_string(), e.path())))
        .collect();
    out.sort();
    out
}

/// Reads a small text file, or `None` if it is missing, a symlink, too big
/// or unreadable.
fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The value of `key` in a `key=value` properties file.
fn property(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// The API level in a folder name such as `android-34` or `android-34-ext8`.
fn api_from_name(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("android-")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The API level a `source.properties` file states.
fn api_from_properties(text: &str) -> Option<u32> {
    property(text, "AndroidVersion.ApiLevel")?.parse().ok()
}

/// Every system image: `system-images/<api>/<tag>/<abi>` folders that hold
/// a `source.properties`.
fn system_images(sdk: &Path) -> Vec<Part> {
    let mut out = Vec::new();
    for (api_name, api_dir) in subdirs(&sdk.join("system-images")) {
        for (tag, tag_dir) in subdirs(&api_dir) {
            for (abi, abi_dir) in subdirs(&tag_dir) {
                let Some(props) = read_small(&abi_dir.join("source.properties")) else {
                    continue;
                };
                out.push(Part {
                    kind: PartKind::SystemImage,
                    path: abi_dir,
                    name: format!("{api_name}/{tag}/{abi}"),
                    api_level: api_from_name(&api_name).or_else(|| api_from_properties(&props)),
                    image_tag: Some(tag.clone()),
                    abi: Some(abi),
                    display_name: None,
                    uses_image: None,
                    last_used: None,
                });
            }
        }
    }
    out
}

/// Platforms, build tools and NDKs: folders directly in `dir` that hold a
/// `source.properties`.
fn simple_parts(dir: &Path, kind: PartKind) -> Vec<Part> {
    subdirs(dir)
        .into_iter()
        .filter_map(|(name, path)| {
            let props = read_small(&path.join("source.properties"))?;
            let api_level = match kind {
                PartKind::Platform => api_from_name(&name).or_else(|| api_from_properties(&props)),
                _ => None,
            };
            Some(Part {
                kind,
                path,
                name,
                api_level,
                image_tag: None,
                abi: None,
                display_name: None,
                uses_image: None,
                last_used: None,
            })
        })
        .collect()
}

/// Every emulator: `*.avd` folders in `avd_dir` that hold a `config.ini`.
fn emulators(avd_dir: &Path) -> Vec<Part> {
    subdirs(avd_dir)
        .into_iter()
        .filter_map(|(folder, path)| {
            let name = folder.strip_suffix(".avd")?.to_string();
            let config = read_small(&path.join("config.ini"))?;
            let uses_image = property(&config, "image.sysdir.1").map(|v| image_name(&v));
            let api_level = uses_image
                .as_deref()
                .and_then(|i| i.split('/').next())
                .and_then(api_from_name);
            Some(Part {
                kind: PartKind::Emulator,
                last_used: newest_inside(&path),
                path,
                display_name: property(&config, "avd.ini.displayname").filter(|n| !n.is_empty()),
                name,
                api_level,
                image_tag: None,
                abi: None,
                uses_image,
            })
        })
        .collect()
}

/// A `config.ini` image path (`system-images/android-34/google_apis/arm64-v8a/`)
/// in the form of a system image's name (`android-34/google_apis/arm64-v8a`).
fn image_name(value: &str) -> String {
    let value = value.replace('\\', "/");
    let value = value.trim().trim_matches('/');
    value
        .strip_prefix("system-images/")
        .unwrap_or(value)
        .trim_matches('/')
        .to_string()
}

/// The newest modification time of anything directly inside `dir`.
fn newest_inside(dir: &Path) -> Option<SystemTime> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .filter_map(|e| std::fs::symlink_metadata(e.path()).ok()?.modified().ok())
        .max()
}

/// The Gradle versions the wrapper downloaded, and Gradle's cache.
fn gradle_parts(gradle: &Path) -> Vec<Part> {
    let mut out: Vec<Part> = subdirs(&gradle.join("wrapper/dists"))
        .into_iter()
        .filter(|(name, _)| name.starts_with("gradle-"))
        .map(|(name, path)| Part {
            kind: PartKind::GradleVersion,
            path,
            name,
            api_level: None,
            image_tag: None,
            abi: None,
            display_name: None,
            uses_image: None,
            last_used: None,
        })
        .collect();
    let caches = gradle.join("caches");
    if real_dir(&caches) {
        out.push(Part {
            kind: PartKind::GradleCache,
            path: caches,
            name: "caches".to_string(),
            api_level: None,
            image_tag: None,
            abi: None,
            display_name: None,
            uses_image: None,
            last_used: None,
        });
    }
    out
}

/// Whether a file with this name marks the root of a Gradle project.
fn is_root_marker(name: &str) -> bool {
    matches!(
        name,
        "settings.gradle" | "settings.gradle.kts" | "build.gradle" | "build.gradle.kts"
    )
}

/// Whether the project search opens a folder with this name.
fn searchable(name: &str, in_home: bool) -> bool {
    if name.starts_with('.') || SKIP_DIRS.contains(&name) {
        return false;
    }
    if in_home && SKIP_HOME_DIRS.contains(&name) {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    !PACKAGE_ENDINGS.iter().any(|e| lower.ends_with(e))
}

/// Reads the project whose root is `root`, or `None` if it is not an
/// Android project.
fn read_project(root: &Path) -> Option<Project> {
    let files = project_files(root);
    let texts: Vec<(PathBuf, String)> = files
        .iter()
        .filter_map(|f| Some((f.clone(), read_small(f)?)))
        .collect();
    let is_android = texts.iter().any(|(_, t)| {
        t.contains("com.android") || t.contains("android {") || t.contains("android{")
    });
    if !is_android {
        return None;
    }
    let folder = root.file_name()?.to_string_lossy().into_owned();
    let around = root
        .parent()
        .filter(|_| folder.eq_ignore_ascii_case("android"));
    let name = around
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(folder);
    let mut activity: Vec<SystemTime> = files
        .iter()
        .filter_map(|f| std::fs::symlink_metadata(f).ok()?.modified().ok())
        .collect();
    activity.extend(newest_inside(root));
    if let Some(around) = around {
        activity.extend(newest_inside(around));
    }
    Some(Project {
        root: root.to_path_buf(),
        name,
        uses: read_uses(&texts),
        last_activity: activity.into_iter().max(),
    })
}

/// The build files of the project at `root`: Gradle scripts, properties and
/// version catalogs, up to `PROJECT_DEPTH` folders deep.
fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if kind.is_file() && is_build_file(&name) {
                out.push(entry.path());
            } else if kind.is_dir()
                && depth < PROJECT_DEPTH
                && !name.starts_with('.')
                && !SKIP_PROJECT_DIRS.contains(&name.as_ref())
                && !SKIP_DIRS.contains(&name.as_ref())
            {
                stack.push((entry.path(), depth + 1));
            }
            if out.len() >= MAX_PROJECT_FILES {
                out.sort();
                return out;
            }
        }
    }
    out.sort();
    out
}

/// Whether a file with this name holds build settings.
fn is_build_file(name: &str) -> bool {
    name.ends_with(".gradle")
        || name.ends_with(".gradle.kts")
        || name.ends_with(".versions.toml")
        || name == "gradle.properties"
        || name == "gradle-wrapper.properties"
}

/// The setting names that hold the SDK platform a project compiles with.
const COMPILE_SDK_KEYS: &[&str] = &["compileSdk", "compileSdkVersion", "compileSdkPreview"];
/// The setting names that hold the build tools version.
const BUILD_TOOLS_KEYS: &[&str] = &["buildToolsVersion"];
/// The setting names that hold the NDK version.
const NDK_KEYS: &[&str] = &["ndkVersion"];
/// Words in a build file that mean the project builds C or C++ code.
const NATIVE_MARKERS: &[&str] = &["externalNativeBuild", "ndkBuild"];

/// What a project uses, from the text of its build files.
fn read_uses(texts: &[(PathBuf, String)]) -> Uses {
    let lines: Vec<String> = texts
        .iter()
        .filter(|(f, _)| !f.ends_with("gradle-wrapper.properties"))
        .flat_map(|(_, t)| t.lines().map(strip_comment).collect::<Vec<_>>())
        .collect();
    let mut uses = Uses::default();
    let (sdks, unknown) = values_of(&lines, COMPILE_SDK_KEYS);
    uses.compile_sdks = sdks
        .into_iter()
        .map(|v| v.strip_prefix("android-").map(str::to_string).unwrap_or(v))
        .collect();
    uses.compile_sdk_unknown = unknown;
    let (tools, unknown) = values_of(&lines, BUILD_TOOLS_KEYS);
    uses.build_tools_default = tools.is_empty() && !unknown;
    uses.build_tools = tools;
    uses.build_tools_unknown = unknown;
    let (ndks, unknown) = values_of(&lines, NDK_KEYS);
    let native = lines
        .iter()
        .any(|l| NATIVE_MARKERS.iter().any(|m| l.contains(m)));
    uses.ndk_default = native && ndks.is_empty() && !unknown;
    uses.ndks = ndks;
    uses.ndk_unknown = unknown;
    let wrappers: Vec<&String> = texts
        .iter()
        .filter(|(f, _)| f.ends_with("gradle-wrapper.properties"))
        .map(|(_, t)| t)
        .collect();
    for text in wrappers {
        match property(text, "distributionUrl").and_then(|u| gradle_dist(&u)) {
            Some(dist) if uses.gradle.is_none() => uses.gradle = Some(dist),
            Some(_) => {}
            None => uses.gradle_unknown = true,
        }
    }
    uses
}

/// The folder Gradle downloads a `distributionUrl` into:
/// `https\://services.gradle.org/distributions/gradle-8.4-bin.zip` gives
/// `gradle-8.4-bin`.
fn gradle_dist(url: &str) -> Option<String> {
    let file = url.rsplit('/').next()?;
    let dist = file.strip_suffix(".zip")?;
    dist.starts_with("gradle-").then(|| dist.to_string())
}

/// A line without a `//` comment. A `//` right after a `:` (as in a URL) is
/// kept.
fn strip_comment(line: &str) -> String {
    let mut prev = ' ';
    for (i, c) in line.char_indices() {
        if c == '/' && line[i + 1..].starts_with('/') && prev != ':' {
            return line[..i].to_string();
        }
        if c == '#' && line[..i].trim().is_empty() {
            return String::new();
        }
        prev = c;
    }
    line.to_string()
}

/// A setting's value, as written after its name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    /// A plain number or quoted text.
    Literal(String),
    /// The name of another setting (the last part of `rootProject.ext.x`
    /// or `libs.versions.x.get()`).
    Reference(String),
    /// Something else, such as a calculation.
    Unknown,
    /// Nothing usable follows the name (it is only mentioned).
    Nothing,
}

/// Every value given to any of `keys` in `lines`, with references to other
/// settings followed. The flag is true if any value couldn't be read.
fn values_of(lines: &[String], keys: &[&str]) -> (BTreeSet<String>, bool) {
    let mut found = BTreeSet::new();
    let mut unknown = false;
    for key in keys {
        for value in raw_values(lines, key) {
            match value {
                Value::Literal(v) => {
                    found.insert(v);
                }
                Value::Reference(name) => {
                    let resolved = resolve(lines, &name);
                    if resolved.is_empty() {
                        unknown = true;
                    }
                    found.extend(resolved);
                }
                Value::Unknown => unknown = true,
                Value::Nothing => {}
            }
        }
    }
    (found, unknown)
}

/// The plain values given to the setting `name` anywhere in `lines`. A
/// reference is followed one step only, so a loop can't hang.
fn resolve(lines: &[String], name: &str) -> BTreeSet<String> {
    raw_values(lines, name)
        .into_iter()
        .filter_map(|v| match v {
            Value::Literal(v) => Some(v),
            _ => None,
        })
        .collect()
}

/// Each value written after the word `key` in `lines`.
fn raw_values(lines: &[String], key: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for line in lines {
        let mut from = 0;
        while let Some(at) = line[from..].find(key) {
            let start = from + at;
            let end = start + key.len();
            from = end;
            let before = line[..start].chars().next_back();
            let after = line[end..].chars().next();
            if before.is_some_and(is_ident) || after.is_some_and(is_ident) {
                continue;
            }
            out.push(parse_value(&line[end..]));
        }
    }
    out
}

/// Whether `c` can be part of a setting name.
fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Reads the value that follows a setting's name.
fn parse_value(rest: &str) -> Value {
    // The closing quote of a name given as text, as in `set("name", 34)`.
    let rest = rest
        .strip_prefix('"')
        .or_else(|| rest.strip_prefix('\''))
        .unwrap_or(rest);
    let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || "=(,:".contains(c));
    let Some(first) = rest.chars().next() else {
        return Value::Nothing;
    };
    if first == '"' || first == '\'' {
        let inner = &rest[1..];
        return match inner.find(first) {
            Some(end) if !inner[..end].contains('$') && !inner[..end].trim().is_empty() => {
                Value::Literal(inner[..end].trim().to_string())
            }
            _ => Value::Unknown,
        };
    }
    if first.is_ascii_digit() {
        let literal: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        let next = rest[literal.len()..].trim_start().chars().next();
        // `34 + 1` or `34 ?: x` is a calculation, not a plain number.
        return match next {
            Some(c) if "+-*/?:".contains(c) => Value::Unknown,
            _ => Value::Literal(literal),
        };
    }
    // A member access (`x.compileSdk.get()`) or an index (`ext["x"]`) only
    // mentions the name.
    if ")}];.".contains(first) {
        return Value::Nothing;
    }
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Value::Unknown;
    }
    let expr: String = rest
        .chars()
        .take_while(|c| is_ident(*c) || *c == '.')
        .collect();
    let next = rest[expr.len()..].chars().next();
    // `ext["name"]` names the setting in quotes.
    if next == Some('[') {
        let inner = &rest[expr.len() + 1..];
        let inner = inner.trim_start_matches(['"', '\'']);
        let name: String = inner.chars().take_while(|c| is_ident(*c)).collect();
        return if name.is_empty() {
            Value::Unknown
        } else {
            Value::Reference(name)
        };
    }
    const CALLS: &[&str] = &["get", "toInt", "toInteger", "toString", "asInt", "orNull"];
    let mut segments: Vec<&str> = expr.split('.').filter(|s| !s.is_empty()).collect();
    while segments.last().is_some_and(|s| CALLS.contains(s)) {
        segments.pop();
    }
    match segments.last() {
        // `property("x")` and the like can't be followed.
        Some(_) if next == Some('(') && segments.len() == expr.split('.').count() => Value::Unknown,
        Some(last) => Value::Reference((*last).to_string()),
        None => Value::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(strip_comment).collect()
    }

    #[test]
    fn reads_plain_values_in_every_style() {
        let text = r#"
            android {
                compileSdk 34
                compileSdkVersion = 33
                buildToolsVersion "34.0.0"
                ndkVersion = '26.1.10909125'
                set("compileSdk", 32)
                compileSdkVersion "android-31"
            }
        "#;
        let l = lines(text);
        let (sdks, unknown) = values_of(&l, COMPILE_SDK_KEYS);
        assert!(!unknown);
        assert_eq!(
            sdks.into_iter().collect::<Vec<_>>(),
            ["32", "33", "34", "android-31"]
        );
        assert_eq!(values_of(&l, BUILD_TOOLS_KEYS).0.len(), 1);
        assert!(values_of(&l, NDK_KEYS).0.contains("26.1.10909125"));
    }

    #[test]
    fn follows_react_native_and_catalog_references() {
        let text = r#"
            buildscript { ext { compileSdkVersion = 35; ndkVersion = "27.1.12297006" } }
            android {
                compileSdkVersion rootProject.ext.compileSdkVersion
                ndkVersion rootProject.ext["ndkVersion"]
                compileSdk = libs.versions.compileSdk.get().toInt()
            }
            [versions]
            compileSdk = "34"
        "#;
        let l = lines(text);
        let (sdks, unknown) = values_of(&l, COMPILE_SDK_KEYS);
        assert!(!unknown, "{sdks:?}");
        assert!(sdks.contains("35") && sdks.contains("34"), "{sdks:?}");
        assert!(values_of(&l, NDK_KEYS).0.contains("27.1.12297006"));
    }

    #[test]
    fn anything_it_cannot_read_is_unknown_never_unused() {
        for text in [
            "compileSdk flutter.compileSdkVersion",
            "compileSdk = property(\"sdk\").toInt()",
            "compileSdk = (findProperty('x') ?: 34)",
            "compileSdk = 33 + 1",
            "compileSdk = \"${sdk}\"",
        ] {
            let (_, unknown) = values_of(&lines(text), COMPILE_SDK_KEYS);
            assert!(unknown, "{text}");
        }
    }

    #[test]
    fn comments_are_ignored_but_urls_kept() {
        assert_eq!(
            strip_comment("compileSdk 34 // was 33").trim(),
            "compileSdk 34"
        );
        assert_eq!(strip_comment("# compileSdk=30"), "");
        assert!(strip_comment("url 'https://x.org/a'").contains("https://x.org"));
    }

    #[test]
    fn gradle_dist_folder_from_wrapper_url() {
        assert_eq!(
            gradle_dist("https\\://services.gradle.org/distributions/gradle-8.4-bin.zip")
                .as_deref(),
            Some("gradle-8.4-bin")
        );
        assert_eq!(gradle_dist("https://x/other.zip"), None);
    }

    #[test]
    fn versions_compare_as_numbers() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("34.0.0", "9.0.0"), Greater);
        assert_eq!(compare_versions("30.0.3", "30.0.3"), Equal);
        assert!(version_matches("34.0.0", "34"));
        assert!(!version_matches("340.0.0", "34"));
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn project(name: &str, uses: Uses) -> Project {
        Project {
            root: PathBuf::from("/x").join(name),
            name: name.into(),
            uses,
            last_activity: None,
        }
    }

    #[test]
    fn finds_parts_and_projects_in_a_home() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let sdk = home.join(SDK_DIR);
        write(
            &sdk.join("system-images/android-34/google_apis/arm64-v8a/source.properties"),
            "AndroidVersion.ApiLevel=34\n",
        );
        write(
            &sdk.join("system-images/android-30/google_apis/x86_64/source.properties"),
            "AndroidVersion.ApiLevel=30\n",
        );
        write(&sdk.join("platforms/android-34/source.properties"), "x=1\n");
        write(&sdk.join("platforms/android-28/source.properties"), "x=1\n");
        write(&sdk.join("build-tools/34.0.0/source.properties"), "x=1\n");
        write(&sdk.join("build-tools/30.0.3/source.properties"), "x=1\n");
        write(&sdk.join("ndk/26.1.10909125/source.properties"), "x=1\n");
        // No source.properties: not offered.
        fs::create_dir_all(sdk.join("build-tools/29.0.0")).unwrap();
        write(
            &home.join(".android/avd/Pixel_7.avd/config.ini"),
            "avd.ini.displayname=Pixel 7\nimage.sysdir.1=system-images/android-34/google_apis/arm64-v8a/\n",
        );
        write(
            &home.join(".gradle/wrapper/dists/gradle-8.4-bin/abc/x"),
            "x",
        );
        write(
            &home.join(".gradle/wrapper/dists/gradle-7.5-all/abc/x"),
            "x",
        );
        write(&home.join(".gradle/caches/modules-2/x"), "x");

        let app = home.join("Developer/Shop/android");
        write(&app.join("settings.gradle"), "include ':app'\n");
        write(
            &app.join("build.gradle"),
            "buildscript { ext { compileSdkVersion = 34\n ndkVersion = \"26.1.10909125\" } }\n",
        );
        write(
            &app.join("app/build.gradle"),
            "apply plugin: \"com.android.application\"\nandroid {\n compileSdkVersion rootProject.ext.compileSdkVersion\n ndkVersion rootProject.ext.ndkVersion\n}\n",
        );
        write(
            &app.join("gradle/wrapper/gradle-wrapper.properties"),
            "distributionUrl=https\\://services.gradle.org/distributions/gradle-8.4-bin.zip\n",
        );
        // Dependencies, Library and hidden folders are never searched.
        write(
            &home.join("Developer/Shop/node_modules/lib/android/build.gradle"),
            "android { compileSdk 28 }",
        );
        write(
            &home.join("Library/Old/build.gradle"),
            "android { compileSdk 28 }",
        );
        write(
            &home.join(".hidden/build.gradle"),
            "android { compileSdk 28 }",
        );
        // A Gradle project that is not an Android one.
        write(
            &home.join("Developer/server/build.gradle"),
            "plugins { id 'java' }",
        );

        let parts = find_parts(home);
        let projects = find_projects(home);
        assert_eq!(projects.len(), 1, "{projects:?}");
        assert_eq!(projects[0].name, "Shop");
        assert!(projects[0].uses.build_tools_default);
        assert_eq!(projects[0].uses.gradle.as_deref(), Some("gradle-8.4-bin"));

        let usage_of = |kind: PartKind, name: &str| {
            let part = parts
                .iter()
                .find(|p| p.kind == kind && p.name == name)
                .unwrap();
            usage(part, &parts, &projects)
        };
        let shop = vec!["Shop".to_string()];
        assert_eq!(
            usage_of(PartKind::Platform, "android-34"),
            Usage::Used { by: shop.clone() }
        );
        assert_eq!(usage_of(PartKind::Platform, "android-28"), Usage::NotUsed);
        assert_eq!(
            usage_of(PartKind::BuildTools, "34.0.0"),
            Usage::ProbablyUsed { by: shop.clone() }
        );
        assert_eq!(usage_of(PartKind::BuildTools, "30.0.3"), Usage::NotUsed);
        assert_eq!(
            usage_of(PartKind::Ndk, "26.1.10909125"),
            Usage::Used { by: shop.clone() }
        );
        assert_eq!(
            usage_of(PartKind::GradleVersion, "gradle-8.4-bin"),
            Usage::Used { by: shop }
        );
        assert_eq!(
            usage_of(PartKind::GradleVersion, "gradle-7.5-all"),
            Usage::NotUsed
        );
        assert_eq!(
            usage_of(PartKind::SystemImage, "android-34/google_apis/arm64-v8a"),
            Usage::Used {
                by: vec!["Pixel 7".into()]
            }
        );
        assert_eq!(
            usage_of(PartKind::SystemImage, "android-30/google_apis/x86_64"),
            Usage::NotUsed
        );
        assert_eq!(
            usage_of(PartKind::Emulator, "Pixel_7"),
            Usage::NotApplicable
        );
        assert!(!parts.iter().any(|p| p.name == "29.0.0"));
        assert!(parts.iter().any(|p| p.kind == PartKind::GradleCache));
    }

    #[test]
    fn an_unreadable_setting_makes_unnamed_parts_not_sure() {
        let platform = Part {
            kind: PartKind::Platform,
            path: PathBuf::from("/x"),
            name: "android-28".into(),
            api_level: Some(28),
            image_tag: None,
            abi: None,
            display_name: None,
            uses_image: None,
            last_used: None,
        };
        let uses = Uses {
            compile_sdk_unknown: true,
            ..Uses::default()
        };
        assert_eq!(
            usage(&platform, &[], &[project("Flutterish", uses)]),
            Usage::NotSure {
                by: vec!["Flutterish".into()]
            }
        );
    }

    #[test]
    fn an_emulator_with_no_image_setting_makes_images_not_sure() {
        let image = Part {
            kind: PartKind::SystemImage,
            path: PathBuf::from("/x"),
            name: "android-30/default/x86".into(),
            api_level: Some(30),
            image_tag: None,
            abi: None,
            display_name: None,
            uses_image: None,
            last_used: None,
        };
        let emulator = Part {
            kind: PartKind::Emulator,
            name: "Old".into(),
            ..image.clone()
        };
        assert_eq!(
            usage(&image, &[image.clone(), emulator], &[]),
            Usage::NotSure {
                by: vec!["Old".into()]
            }
        );
    }

    #[test]
    fn a_symlinked_sdk_folder_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("elsewhere");
        write(&real.join("platforms/android-34/source.properties"), "x=1");
        let home = dir.path().join("home");
        fs::create_dir_all(home.join("Library/Android")).unwrap();
        std::os::unix::fs::symlink(&real, home.join(SDK_DIR)).unwrap();
        assert!(find_parts(&home).is_empty());
        assert!(!has_sdk(&home));
    }
}
