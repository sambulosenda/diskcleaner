//! Tells apart items that share a title. Pure: no filesystem access.
//!
//! One rule such as `Developer/**/node_modules` finds many items, and they
//! all have the rule's title. The Overview and the lists show a short name
//! beside the title so the rows can be told apart: the project folder's name
//! for a fixed-name folder inside a project, or the matched names themselves
//! for a rule that ends in a wildcard (an emulator, a model, a toolchain).
//!
//! The result is display text built from folder and file names only. It is
//! never a path, and the UI can't act on it: actions still go by item id.

use std::collections::HashMap;
use std::path::Path;

use rules::Rule;

/// Where a rule's items get their distinguishing name from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceFrom {
    /// The item has a fixed name (`node_modules`, `target`, `Render Files`),
    /// so the folder holding it names it.
    Parent,
    /// The rule's last `n` path segments are wildcards, so the item's own
    /// last `n` names name it.
    Own(usize),
}

/// The longest place text shown. Longer names are cut with an ellipsis.
const MAX_CHARS: usize = 60;

/// For every rule that can match more than one item, where those items get
/// their name from. Rules without any wildcard match one item at most and
/// are left out.
pub fn place_sources(rules: &[Rule]) -> HashMap<String, PlaceFrom> {
    rules
        .iter()
        .filter_map(|rule| source_for(&rule.matcher.path).map(|from| (rule.id.clone(), from)))
        .collect()
}

/// Where items matched by `pattern` get their name from, or `None` if the
/// pattern has no wildcard.
fn source_for(pattern: &str) -> Option<PlaceFrom> {
    let segments: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    if !segments.iter().any(|s| is_glob(s)) {
        return None;
    }
    let trailing = segments
        .iter()
        .rev()
        .take_while(|s| is_glob(s) && **s != "**")
        .count();
    Some(if trailing == 0 {
        PlaceFrom::Parent
    } else {
        PlaceFrom::Own(trailing)
    })
}

/// Whether a pattern segment holds glob syntax.
fn is_glob(segment: &str) -> bool {
    segment.contains(['*', '?', '[', '{'])
}

/// The distinguishing name for the item at `path`, matched by a rule whose
/// items get their name `from` there. `home` is used only to tidy names
/// that spell out the home folder. `None` when no useful name exists.
pub fn place(home: &Path, path: &Path, from: PlaceFrom) -> Option<String> {
    let names: Vec<String> = match from {
        PlaceFrom::Parent => vec![path.parent()?.file_name()?.to_string_lossy().into_owned()],
        PlaceFrom::Own(n) => {
            let parts: Vec<String> = path
                .components()
                .rev()
                .take(n)
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            if parts.len() < n {
                return None;
            }
            parts.into_iter().rev().collect()
        }
    };
    let tidied: Vec<String> = names
        .iter()
        .map(|name| tidy(home, name))
        .collect::<Option<Vec<_>>>()?;
    let text = tidied.join(", ");
    if text.is_empty() {
        return None;
    }
    Some(shorten(&text))
}

/// Makes one name easier to read, or `None` if it would mean nothing to a
/// person (a random identifier).
fn tidy(home: &Path, name: &str) -> Option<String> {
    if is_uuid(name) {
        return None;
    }
    // Claude Code names a project's folder after its path, with `/` and `.`
    // turned into `-`: `-Users-sam-Developer-roomy`. Drop the home part.
    let encoded_home: String = home
        .to_string_lossy()
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    let name = name
        .strip_prefix(&format!("{encoded_home}-"))
        .unwrap_or(name);
    // Xcode's DerivedData folders end in `-` and 28 lowercase letters.
    let name = strip_derived_data_hash(name);
    // Android emulators are `<name>.avd`.
    let name = name.strip_suffix(".avd").unwrap_or(name);
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// `Roomy-abcdefghijklmnopqrstuvwxyzab` becomes `Roomy`.
fn strip_derived_data_hash(name: &str) -> &str {
    match name.rsplit_once('-') {
        Some((stem, hash))
            if !stem.is_empty()
                && hash.len() == 28
                && hash.chars().all(|c| c.is_ascii_lowercase()) =>
        {
            stem
        }
        _ => name,
    }
}

/// Whether `name` is a UUID such as a simulator device's folder name.
fn is_uuid(name: &str) -> bool {
    let groups: Vec<&str> = name.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, len)| g.len() == len && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Cuts text longer than [`MAX_CHARS`] and ends it with an ellipsis.
fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_CHARS - 1).collect();
    format!("{cut}\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/Users/sam";

    fn place_for(pattern: &str, rel: &str) -> Option<String> {
        let from = source_for(pattern)?;
        place(Path::new(HOME), &Path::new(HOME).join(rel), from)
    }

    #[test]
    fn a_fixed_name_inside_a_project_is_named_by_the_project() {
        assert_eq!(
            place_for(
                "Developer/**/node_modules",
                "Developer/apps/storefront/node_modules"
            )
            .as_deref(),
            Some("storefront")
        );
        assert_eq!(
            place_for("[!.]*/target", "roomy/target").as_deref(),
            Some("roomy")
        );
        assert_eq!(
            place_for(
                "Movies/*.fcpbundle/*/Render Files",
                "Movies/Trip.fcpbundle/Day 1/Render Files"
            )
            .as_deref(),
            Some("Day 1")
        );
    }

    #[test]
    fn a_wildcard_name_is_named_by_itself() {
        assert_eq!(
            place_for(".android/avd/*.avd", ".android/avd/Pixel_8_API_35.avd").as_deref(),
            Some("Pixel_8_API_35")
        );
        assert_eq!(
            place_for(
                "Library/Android/sdk/system-images/*/*/*",
                "Library/Android/sdk/system-images/android-35/google_apis/arm64-v8a"
            )
            .as_deref(),
            Some("android-35, google_apis, arm64-v8a")
        );
    }

    #[test]
    fn rules_without_a_wildcard_have_no_place() {
        assert_eq!(source_for("Library/Caches/Homebrew"), None);
        assert_eq!(source_for(".cargo/registry/cache"), None);
    }

    #[test]
    fn claude_code_folders_drop_the_home_folder() {
        assert_eq!(
            place_for(
                ".claude/projects/*",
                ".claude/projects/-Users-sam-Developer-roomy"
            )
            .as_deref(),
            Some("Developer-roomy")
        );
    }

    #[test]
    fn derived_data_drops_the_hash() {
        assert_eq!(
            place_for(
                "Library/Developer/Xcode/DerivedData/*",
                "Library/Developer/Xcode/DerivedData/Roomy-abcdefghijklmnopqrstuvwxyzab"
            )
            .as_deref(),
            Some("Roomy")
        );
        // A name that only looks a little like it is kept as it is.
        assert_eq!(strip_derived_data_hash("my-app"), "my-app");
    }

    #[test]
    fn random_identifiers_give_no_place() {
        assert_eq!(
            place_for(
                "Library/Developer/CoreSimulator/Devices/*",
                "Library/Developer/CoreSimulator/Devices/0A1B2C3D-4E5F-6A7B-8C9D-0E1F2A3B4C5D"
            ),
            None
        );
    }

    #[test]
    fn long_names_are_cut() {
        let long = "a".repeat(100);
        let got = place_for(
            "Developer/**/node_modules",
            &format!("Developer/{long}/node_modules"),
        )
        .unwrap_or_default();
        assert_eq!(got.chars().count(), MAX_CHARS);
        assert!(got.ends_with('\u{2026}'));
    }

    #[test]
    fn a_place_is_never_a_path() {
        let got = place_for("Developer/**/node_modules", "Developer/a/b/c/node_modules");
        assert_eq!(got.as_deref(), Some("c"));
    }
}
