# Command contract: scan, overview, list items (T09)

This is the typed API the SwiftUI app uses to talk to the Rust core. It is defined in
`core/crates/ffi` and reaches Swift through UniFFI. Everything here is **read only**: no command
moves, deletes or changes a file. Cleanup commands come in a later task and will get their own
section.

Sam approves this document in the T09 pull request before the implementation is merged.

## Ground rules

1. **No path goes to Swift in simple mode.** Items are named by an opaque `ItemId`. In Developer
   mode, `ItemDetail.path` holds a display string such as `~/Library/Developer/Xcode/DerivedData/App-abc`.
   It is for showing only.
2. **Swift never sends a path to the core.** Swift sends scan handles, item ids and enum values.
   (The one exception is the test-only constructor described under [Testing](#testing), which is
   not in the app build.)
3. **The headline figure is exact.** `Overview.likely_safe_bytes` is the sum of `size_bytes` of
   every item whose tier is `likely_safe`, not only the items in `top_items` (requirement F2).
4. **The home folder comes from the core.** In the app it is `dirs::home_dir()`. Tests pass a
   fixture home through the test-only constructor.
5. **Sizes are bytes as `u64`.** Formatting happens in the UI.
6. **Rules are built in.** The rule files in `rules/*.toml` are compiled into the core, so the app
   never reads rules from a path Swift gives it. A test checks every file in `rules/` is included.
7. **Errors are thrown, never crashes.** Every failure is a `CoreError`. Error text is plain words
   and never holds a path.

## Threading and cancelling

All commands return quickly and are safe to call from any thread, including at the same time.

- `start_scan` returns a handle at once. The scan runs on its own thread inside the core.
- The app polls `scan_progress` (for example every 100 ms) until the phase is `finished`,
  `cancelled` or `failed`.
- `cancel_scan` asks the scan to stop and returns at once. The phase then becomes `cancelled`.
- `overview`, `list_items` and `item_detail` read the finished scan, which is held in memory. They
  do not touch the disk, except `overview`, which reads the volume's total and free space with
  `statfs` (metadata only).
- A finished scan stays in memory until `close_scan` is called or the `RoomyCore` object is freed.

## The object

```text
object RoomyCore
    constructor new() throws CoreError
    // only with the `testing` feature, see Testing
    constructor with_home_for_testing(home: String) throws CoreError
```

`new()` finds the home folder with `dirs::home_dir()` and loads the built-in rules. It throws
`home_not_found` if there is no home folder, and `rules_invalid` if any built-in rule fails
validation (a bug that tests catch before a release). Two rules that tie are logged but not
refused, because the classifier already shows whatever they match as unknown with tier `keep`.

The app makes one `RoomyCore` and keeps it for its whole life.

## Commands

All are methods on `RoomyCore`. Names below are the Rust names; Swift gets them in camelCase
(`startScan(mode:)`, `scanProgress(handle:)` and so on).

### `start_scan(mode: Mode) -> ScanHandle`
Starts a read-only scan of the home folder in the given mode and returns at once. Each call
starts a new, independent scan with a new handle. The mode decides which rules apply (simple
mode leaves out developer rules, so what they describe counts as unknown) and whether
`item_detail` fills in `path`.

Licence gates for Developer mode are not part of T09. They belong to the licensing task and will
be enforced here in the core, not only in the UI.

### `scan_progress(handle: ScanHandle) -> ScanProgress` throws
How far the scan has got. Throws `unknown_scan` for a handle the core does not know (never
issued, or closed).

### `cancel_scan(handle: ScanHandle)`
Asks the scan to stop. Safe to call more than once, after the scan has finished, or with an
unknown handle (it then does nothing). A scan that has already finished stays finished.

### `close_scan(handle: ScanHandle)`
Cancels the scan if it is still running and frees its results. The handle is unknown afterwards.
Safe to call more than once. (Not in the task sketch; added so memory does not grow with every
rescan.)

### `overview(handle: ScanHandle) -> Overview` throws
What the Overview screen shows. Throws `scan_not_finished` while the scan runs,
`scan_cancelled` or `scan_failed` if it ended that way, and `unknown_scan` for an unknown handle.
Throws `io` if the volume's space cannot be read.

### `list_items(handle: ScanHandle, category: Category?, tier: Tier?, sort: Sort) -> [ItemSummary]` throws
Every item of the scan, optionally only one category and/or one tier, in the given order.
`nil` means "any". Same errors as `overview` (except `io`).

### `item_detail(handle: ScanHandle, item_id: ItemId) -> ItemDetail` throws
Everything the detail panel shows for one item. Throws `unknown_item` if the scan has no item
with that id, plus the same errors as `list_items`.

## Types

### `ScanHandle` (record)
| Field | Type | Meaning |
|---|---|---|
| `id` | `u64` | Opaque number. Starts at 1, goes up, never reused while the `RoomyCore` lives. |

### `ItemId`
A `String` of 32 lowercase hex characters: the first 16 bytes of
`SHA-256(rule id, 0x00, path relative to home)`. The same rule matching the same place gives the
same id on every scan, so the UI can keep a selection across a rescan. It reveals nothing about
the path.

### `Mode` (enum)
`simple`, `developer`.

### `Tier` (enum)
`likely_safe`, `review_first`, `keep`. The tier after the classifier's safety adjustments, which
only ever make it more careful than the rule's.

### `Category` (enum)
The rule categories from `docs/safety/rule-schema.md`: `cache`, `build`, `developer_tools`,
`docker`, `app_data`, `logs`, `downloads`, `user_data`, `backups`, `system`, `unknown`.
The sidebar and disk strip may group these (for example `cache` and `app_data` as "Apps and
caches"); that grouping is UI work in T11.

### `Confidence` (enum)
`high`, `medium`, `low`.

### `Action` (enum)
What the app may later offer for the item: `trash`, `command`, `show_only`. Shown for
information in T09; nothing acts on it yet.

### `Sort` (enum)
| Value | Order |
|---|---|
| `largest_first` | Size, largest first |
| `smallest_first` | Size, smallest first |
| `title` | Title A to Z, ignoring case |

Ties are always broken by `ItemId`, so the order is the same every time.

### `ScanPhase` (enum)
| Value | Meaning |
|---|---|
| `scanning` | Walking the home folder and adding up sizes |
| `classifying` | Matching what was found to rules |
| `finished` | Done; `overview`, `list_items` and `item_detail` are ready |
| `cancelled` | Stopped by `cancel_scan` |
| `failed { reason: String }` | Could not run. `reason` is plain words with no path, for example "Roomy could not read your home folder." |

### `ScanProgress` (record)
| Field | Type | Meaning |
|---|---|---|
| `bytes_seen` | `u64` | Allocated bytes counted so far |
| `items_seen` | `u64` | Files counted so far (hard links once) |
| `phase` | `ScanPhase` | See above |

After `finished`, `bytes_seen` equals the scan's total.

### `Overview` (record)
| Field | Type | Meaning |
|---|---|---|
| `likely_safe_bytes` | `u64` | Exact sum of `size_bytes` of every `likely_safe` item (F2) |
| `scanned_bytes` | `u64` | Allocated bytes in the whole home folder |
| `disk_total` | `u64` | Size of the volume holding the home folder |
| `disk_free` | `u64` | Space available to the user on that volume (`statfs` `f_bavail`; does not count purgeable space) |
| `categories` | `[CategoryTotal]` | One entry per category with any items, largest first (ties in the order `Category` lists them) |
| `top_items` | `[ItemSummary]` | Up to 8 of the largest `likely_safe` and `review_first` items, largest first. `keep` items are never here |
| `unreadable_count` | `u64` | Folders the scan could not read (for example because of privacy protection) |

`scanned_bytes` and `categories` are additions to the task sketch: the disk strip needs a total
and the item count per category.

### `CategoryTotal` (record)
| Field | Type | Meaning |
|---|---|---|
| `category` | `Category` | |
| `bytes` | `u64` | Sum of `size_bytes` of every item in the category |
| `likely_safe_bytes` | `u64` | The part of `bytes` that is `likely_safe` |
| `item_count` | `u64` | Number of items in the category |

The `likely_safe_bytes` of all categories add up to `Overview.likely_safe_bytes`.

### `ItemSummary` (record)
One row in a list. No path, in any mode.

| Field | Type | Meaning |
|---|---|---|
| `id` | `ItemId` | |
| `title` | `String` | Short display name |
| `explain` | `String` | What it is, in plain words |
| `size_bytes` | `u64` | Allocated bytes on disk |
| `tier` | `Tier` | |
| `category` | `Category` | |

### `ItemDetail` (record)
| Field | Type | Meaning |
|---|---|---|
| `id` | `ItemId` | |
| `title` | `String` | |
| `explain` | `String` | What it is and why it exists |
| `risk` | `String` | What could go wrong if it is removed |
| `restore` | `String?` | How it comes back, if it does |
| `rule_id` | `String` | The rule that matched, or `unknown` |
| `rule_version` | `u32` | That rule's version; 0 for `unknown` |
| `confidence` | `Confidence` | How sure the rule is |
| `tier` | `Tier` | |
| `category` | `Category` | |
| `size_bytes` | `u64` | |
| `regenerable` | `bool` | Whether it comes back by itself. Always `false` for `keep` |
| `action` | `Action` | Always `show_only` for `keep` |
| `command` | `String?` | Allow-listed command id when `action` is `command` |
| `cautions` | `[Caution]` | Why the tier is more careful than the rule's, if it is |
| `path` | `String?` | **Developer mode only**: the place as a display string starting `~/`. Always `nil` in simple mode |

### `Caution` (enum)
Why the core made an item's tier more careful. Carries no path.

| Value | Meaning |
|---|---|
| `protected` | The item is, or holds, a protected place, so it is `keep` |
| `review_only` | The item is, or holds, something the user must look at first |
| `nested_rule { rule_id: String }` | Another rule matches something inside it |
| `rules_disagree` | Two rules equally describe something inside it |
| `low_confidence` | The rule is not sure, which is never `likely_safe` |
| `unreadable` | Part of it could not be read during the scan |
| `unusual_name` | A name inside it could not be checked |

### `CoreError` (error)
| Value | When |
|---|---|
| `invalid_path { reason }` | A path argument was empty, relative or held a NUL byte (T02 `disk_summary`, and the test constructor) |
| `io { reason }` | Reading volume information failed |
| `home_not_found` | The core could not find the home folder |
| `rules_invalid { reason }` | A built-in rule failed validation |
| `unknown_scan` | The handle was never issued or has been closed |
| `scan_not_finished` | The scan is still running |
| `scan_cancelled` | The scan was cancelled |
| `scan_failed { reason }` | The scan could not run; same plain-words reason as `ScanPhase.failed` |
| `unknown_item` | The scan has no item with that id |

## Testing

The `ffi` crate has a `testing` Cargo feature. With it, and only with it, the core also exports:

- `RoomyCore.with_home_for_testing(home: String)`: like `new()`, but scans `home` instead of the
  real home folder. `home` must be an absolute path to a folder.
- `make_fixture_home(dir: String) -> String`: builds the T04 fixture home in `dir` (which must be
  empty or not exist yet) and returns its canonical path, ready for `with_home_for_testing`.

`./scripts/build-ffi.sh` builds with `testing` by default so the Xcode test target can call these.
`./scripts/build-ffi.sh --release-app` builds without it; the release pipeline (T13) must use
that, so a shipped app can only ever scan the real home folder. This matters more once cleanup
commands exist, because the executor checks paths are inside the home folder it was given.

The T02 commands `core_version()` and `disk_summary(path)` are unchanged.
