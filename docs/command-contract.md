# Command contract: scan, overview, list items (T09)

This is the typed API the SwiftUI app uses to talk to the Rust core. It is defined in
`core/crates/ffi` and reaches Swift through UniFFI. Everything here is **read only**: no command
moves, deletes or changes a file, except the two in [Cleanup](#cleanup) (`run_cleanup` and
`put_back_cleanup`), which hand the move to the `executor` crate. The cleanup preview (a dry run)
has its own section, [Cleanup preview](#cleanup-preview); it is read only.

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
| `nothing_selected` | `preview_cleanup` was called with no items |
| `cleanup_unavailable { reason }` | The preview could not check anything, for example because the Trash could not be opened. `reason` is plain words with no path |

## Cleanup preview

The dry run. It checks the ticked items exactly the way a cleanup would just before acting, and
says what would move to the Trash. **It moves, deletes and changes nothing.** It is the review
screen the user sees before any cleanup.

Rules:

1. Swift sends item ids, never paths. Every path, size and modification time comes from the
   core's own scan.
2. Only items whose tier is not `keep`, whose rule is known, and whose action is `trash` can
   move. Anything else is listed with a reason and never goes into a plan.
3. Every other check is the executor's own (`executor::preview`): the path checks in
   `docs/safety/` steps 1 to 6, every path inside a folder against the protected lists, no other
   volume inside, and the size (within 1%) and modification time unchanged since the scan
   (step 7). So "will move" means the executor would move it, if nothing changes before then.
   The executor runs every check again when it acts.
4. The core keeps one single-use plan holding only the items that would move. A new preview
   replaces it. It expires ten minutes after the preview.
5. Trash only. Permanent delete is not offered.

### `preview_cleanup(handle: ScanHandle, choices: [CleanupChoice]) -> CleanupPreview` throws
Builds the plan and the preview. A repeated id counts once. Throws `nothing_selected` for an
empty list, `unknown_item` if an id is not in the scan, `cleanup_unavailable` if nothing could be
checked (for example the Trash or the home folder could not be opened), and the same scan errors
as `list_items`.

### `discard_cleanup(plan: PlanHandle)`
Drops the waiting plan if it is `plan`. Safe to call more than once, or with an unknown plan. The
app calls it when the user closes the review without cleaning up.

### `PlanHandle` (record)
| Field | Type | Meaning |
|---|---|---|
| `id` | `u64` | Opaque number. Starts at 1, goes up, never reused while the `RoomyCore` lives |

### `CleanupChoice` (record)
| Field | Type | Meaning |
|---|---|---|
| `item_id` | `ItemId` | An id from `list_items` or `overview` |
| `chosen_alone` | `bool` | Whether the user ticked this item on its own, not as part of a group. Items that are, or hold, review-only things (an iPhone backup, an `.env` file) move only when this is `true`. Ticking a row's checkbox is `true` |

### `CleanupCheck` (enum)
| Value | Meaning |
|---|---|
| `will_move` | Passed every check; would move to the Trash |
| `wont_move { reason: String }` | Would not move. `reason` is a plain-words sentence with no path, for example "This changed since you reviewed it." |

### `CleanupPreviewItem` (record)
| Field | Type | Meaning |
|---|---|---|
| `item_id` | `ItemId` | |
| `title` | `String` | |
| `size_bytes` | `u64` | For `will_move`, measured just now; otherwise the scan's size |
| `check` | `CleanupCheck` | |
| `path` | `String?` | **Developer mode only**, display string starting `~/`, like `ItemDetail.path`. Always `nil` in simple mode |

### `CleanupPreview` (record)
| Field | Type | Meaning |
|---|---|---|
| `plan` | `PlanHandle?` | The plan of the items that would move; `nil` if none would |
| `items` | `[CleanupPreviewItem]` | One per distinct id, in the order sent |
| `bytes_to_move` | `u64` | Sum of `size_bytes` of the `will_move` items |
| `expires_in_seconds` | `u64` | How long the plan stays usable (600) |

## Cleanup

Moving to the Trash, putting it back, and History. Only the `executor` crate moves files.

Rules:

1. The app sends a `PlanHandle` or `CleanupHandle`, never a path.
2. `run_cleanup` takes only the plan the last preview made. A plan is used once, whatever happens.
   The executor checks every item again just before moving it, so an item that changed since the
   review stays where it is and is listed with a reason.
3. Trash only. Nothing is removed for good.
4. `put_back_cleanup` works once, for the most recent cleanup since the core was made. It uses the
   executor's own record of what moved, kept in memory, never anything read from History. An item
   stays in the Trash if what is in the Trash under its name is no longer what Roomy moved there,
   if a folder on the way back changed (for example became a link), or if something new is in its
   old place. A put back never replaces anything.
5. History is a SQLite file at `~/Library/Application Support/Roomy/history.sqlite`. It holds
   titles, sizes, times and outcomes, **no paths**. The core makes the `Roomy` folder the first time
   a cleanup is recorded, and never any other folder. If History can't be written, the cleanup still
   happens and `saved_to_history` is `false`.
6. The licence's free limit is not enforced yet (Sam, 2026-11-06). It will be enforced here in the
   core.

### `run_cleanup(plan: PlanHandle) -> CleanupResult` throws
Moves the plan's items to the Trash. Throws `cleanup_unavailable` if `plan` is not the waiting plan
(already used, discarded, or replaced by a newer preview), has expired, or the Trash or home folder
can't be opened. Nothing moves then.

### `put_back_cleanup(cleanup: CleanupHandle) -> PutBackResult` throws
Moves the last cleanup's items back. Throws `cleanup_unavailable` if `cleanup` is not the most
recent cleanup that moved anything, or was already put back.

### `cleanup_history(limit: u32) -> [HistoryEntry]`
Past cleanups, newest first, at most `limit`. Empty before the first cleanup; reading it never makes
the file.

### `CleanupHandle` (record)
| Field | Type | Meaning |
|---|---|---|
| `id` | `u64` | Opaque number, starts at 1 |

### `CleanupResultItem` (record)
| Field | Type | Meaning |
|---|---|---|
| `item_id` | `ItemId` | |
| `title` | `String` | |
| `size_bytes` | `u64` | Bytes that moved; 0 if it did not |
| `done` | `bool` | Whether it moved (or, in a `PutBackResult`, whether it is back) |
| `reason` | `String?` | Why not, plain words with no path. `nil` when `done` |

### `CleanupResult` (record)
| Field | Type | Meaning |
|---|---|---|
| `cleanup` | `CleanupHandle?` | For putting it back; `nil` if nothing moved |
| `items` | `[CleanupResultItem]` | One per plan item, in order |
| `bytes_moved` | `u64` | Sum of the moved items' sizes. The space comes back when the Trash is emptied |
| `saved_to_history` | `bool` | |

### `PutBackResult` (record)
| Field | Type | Meaning |
|---|---|---|
| `items` | `[CleanupResultItem]` | One per item that had moved |
| `bytes_put_back` | `u64` | |

### `HistoryEntry` (record)
| Field | Type | Meaning |
|---|---|---|
| `id` | `i64` | |
| `finished_at` | `i64` | Seconds since 1970, UTC |
| `put_back_at` | `i64?` | When it was put back |
| `bytes_in_trash` | `u64` | Bytes that went to the Trash and were not put back |
| `items` | `[HistoryItem]` | |

### `HistoryItem` (record)
`title: String`, `size_bytes: u64`, `state: HistoryItemState`.

### `HistoryItemState` (enum)
`moved`, `not_moved { reason }`, `put_back`, `stayed_in_trash { reason }`.

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
