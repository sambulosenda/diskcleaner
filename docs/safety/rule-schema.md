# Classification rule schema (owned by Sam)

Rules are TOML files in `rules/`. The `rules` crate validates every file at build time (a test) and at load time; any invalid rule is rejected and logged, never partially loaded.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | string, dotted lowercase | Yes | Stable unique id, e.g. `xcode.derived_data` |
| `version` | integer ≥ 1 | Yes | Bumped on any change; History records which version acted |
| `match.path` | glob relative to home | Yes | Where the item lives, e.g. `Library/Developer/Xcode/DerivedData/*` |
| `match.requires` | list of globs, relative to the matched item | No | Files that must exist inside the match, e.g. `Info.plist` |
| `category` | enum | Yes | `cache`, `build`, `developer_tools`, `docker`, `app_data`, `logs`, `downloads`, `user_data`, `backups`, `system`, `unknown` |
| `tier` | enum | Yes | `likely_safe`, `review_first`, `keep` |
| `confidence` | enum | Yes | `high`, `medium`, `low`. `low` can never be `likely_safe` |
| `regenerable` | bool | Yes | Whether permanent delete may be offered |
| `mode` | enum | Yes | `simple` or `developer` |
| `title` | string ≤ 40 chars | Yes | Short display name |
| `explain` | string, ≤ 2 sentences, ≤ 200 chars | Yes | What it is and why it exists, plain words |
| `risk` | string ≤ 200 chars | Yes | What could go wrong if removed |
| `restore` | string ≤ 200 chars | No | How it comes back |
| `action` | enum | Yes | `trash`, `command`, `show_only` |
| `command` | allow-list id | If `action = "command"` | One id from `allowed-commands.md` |
| `per_item` | bool | No (default false) | Treat each child as its own item |

Validation rules beyond types:
- `tier = "likely_safe"` requires `confidence = "high"` or `"medium"`.
- `regenerable = false` forbids permanent delete regardless of user choice.
- A rule whose `match.path` could match anything on the hard-blocked list is rejected.
- Unknown fields are an error (`deny_unknown_fields`).
- Any new field added later must default to the safer behaviour.

## Examples

```toml
[[rule]]
id = "xcode.derived_data"
version = 1
match.path = "Library/Developer/Xcode/DerivedData/*"
category = "build"
tier = "likely_safe"
confidence = "high"
regenerable = true
mode = "developer"
per_item = true
title = "Xcode build data"
explain = "Temporary files Xcode creates while building a project."
risk = "The next build of this project will take longer."
restore = "Rebuilds automatically on the next Xcode build."
action = "trash"

[[rule]]
id = "ios.device_backup"
version = 1
match.path = "Library/Application Support/MobileSync/Backup/*"
match.requires = ["Info.plist"]
category = "backups"
tier = "review_first"
confidence = "high"
regenerable = false
mode = "simple"
per_item = true
title = "iPhone or iPad backup"
explain = "A full local backup of one of your devices."
risk = "If this device has no iCloud backup, its data could be lost for good."
action = "trash"

[[rule]]
id = "ios.simulators_unavailable"
version = 1
match.path = "Library/Developer/CoreSimulator/Devices/*"
category = "developer_tools"
tier = "likely_safe"
confidence = "high"
regenerable = true
mode = "developer"
title = "Unavailable iOS simulators"
explain = "Simulators for iOS versions no longer installed in Xcode."
risk = "None; they cannot run without their iOS runtime."
action = "command"
command = "xcrun_simctl_delete_unavailable"
```
