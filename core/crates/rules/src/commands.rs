//! The command ids a rule may name in `command`.
//!
//! These mirror the table in `docs/safety/allowed-commands.md` (owned by
//! Sam). A test reads that file and fails if the two ever differ, so adding
//! a command needs a change to the document first. Naming an id here does
//! not let anything run: only `executor::commands` runs commands.

/// Every command id in `docs/safety/allowed-commands.md`.
pub const ALLOWED_COMMAND_IDS: &[&str] = &[
    "xcrun_simctl_delete_unavailable",
    "docker_image_prune",
    "docker_builder_prune",
    "brew_cleanup",
    "sdkmanager_uninstall",
];

/// Whether `id` is on the allow-list. Exact match only.
pub fn is_allowed(id: &str) -> bool {
    ALLOWED_COMMAND_IDS.contains(&id)
}
