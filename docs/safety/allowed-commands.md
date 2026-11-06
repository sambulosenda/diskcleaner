# Allow-listed tool commands for v1 (owned by Sam)

The executor may run only these, via `std::process::Command` with argument arrays, never through a shell. Each one must first be **previewed**: the core runs the listed read-only preview, shows the affected items, and runs the command only after the user confirms. Full stdout and stderr are stored in History.

| Id | Preview (read-only) | Command | Notes |
|---|---|---|---|
| `xcrun_simctl_delete_unavailable` | `xcrun simctl list devices unavailable -j` | `xcrun simctl delete unavailable` | Skip if Xcode is not installed |
| `docker_image_prune` | `docker image ls --filter dangling=true --format json` | `docker image prune` with stdin answering the prompt, **no `-f`, no `-a`** in v1 | Docker must be running; never touches volumes |
| `docker_builder_prune` | `docker builder du` | `docker builder prune` with stdin answering the prompt, no `-f`, no `--all` | Same |
| `brew_cleanup` | `brew cleanup --dry-run` | `brew cleanup` | Skip if Homebrew not installed |
| `sdkmanager_uninstall` | `sdkmanager --list_installed` | `sdkmanager --uninstall <package>` where `<package>` exactly matches one installed package id from the preview | One package per invocation; validate the id against the preview output |

Any other tool command is shown to the user as text to copy, never run. Adding a command needs Sam's sign-off and fixture tests.
