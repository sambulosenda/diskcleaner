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

### `start_rescan(mode: Mode) -> ScanHandle`
Like `start_scan`, but may read again only the folders that changed since the last finished
scan in the same mode (F12), and reuse that scan's figures for the rest. The core keeps the last
scan's folder tree in memory, with the macOS file event (FSEvents) id from just before it
started, and asks the event log what changed since. It scans everything instead, exactly like
`start_scan`, whenever it can't be sure: no event log (any system but macOS), no kept tree, a
different mode, a tree over an hour old, dropped events, wrapped event ids, a mount or unmount,
the home folder moved, a change outside home, a hard-linked file in a folder it would read
again, or a folder missing that nothing said changed. Folders the last scan could not read are
always read again. `Overview.quick_rescan` says which it did. The app uses this for every scan.

### `scan_changes(handle: ScanHandle) -> ScanChanges?` throws
What grew or shrank since the previous finished scan in the same mode (F11), or `nil` if there
was none or History could not be used. Each finished scan leaves a short record in History
(item ids, titles, categories and sizes; no paths); the last 5 per mode are kept. Same errors as
`list_items`.

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

### `libraries(handle: ScanHandle) -> [LibrarySize]` throws
How much the Mac's own libraries take, measured from the same scan (PRD E6, E7, C1): Photos,
Music, TV, Mail, Messages attachments and Final Cut Pro, one row per kind, always in that order.
Pure: works on the scan's folder tree and reads nothing more. **Shown only**: Roomy never cleans
inside a library (they are hard-blocked or review-only in `docs/safety/protected-paths.md`), and
no row holds a path. The app names each library and opens the app that manages it. Same errors
as `list_items`.

### `full_disk_access() -> FullDiskAccess`
Whether Roomy can read the folders macOS keeps private, right now. Lists these folders inside
home, never following a symlink, and reads nothing else: `Library/Application Support/com.apple.TCC`,
`Library/Safari`, `Library/Mail`, `Library/Messages`, `.Trash`. A folder that is missing is
skipped. Cheap enough to ask once a second while the app waits for the user to turn Full Disk
Access on. The onboarding screen uses it.

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
| `unreadable_areas` | `[UnreadableArea]` | The same folders, counted by `PrivateArea`. Most folders first, ties in the order `PrivateArea` lists them. The counts add up to `unreadable_count` |
| `quick_rescan` | `bool` | The scan read again only the folders that changed since the previous scan (see `start_rescan`) |

### `ScanChanges` (record)
| Field | Type | Meaning |
|---|---|---|
| `previous_scan_at` | `i64` | When the previous scan finished, seconds since 1970 (UTC) |
| `scanned_bytes_before` | `u64` | Allocated bytes in the whole home folder then |
| `scanned_bytes_now` | `u64` | The same, now |
| `categories` | `[CategoryChange]` | Categories whose total changed by at least 10 MB, biggest change first |
| `items` | `[ItemChange]` | Items that changed by at least 10 MB, biggest change first, at most 20 |

`CategoryChange` holds `category`, `bytes_before` and `bytes_now`. `ItemChange` holds `item_id`
(this scan's id, `nil` when the item is gone), `title`, `category`, `kind` (`ChangeKind`:
`new`, `grew`, `shrank`, `gone`), `bytes_before` (0 if new) and `bytes_now` (0 if gone).

### `PrivateArea` (enum)
A part of home that macOS may keep private: `mail`, `messages`, `safari`, `device_backups`,
`photos`, `contacts_and_calendars`, `trash`, `app_data` (`~/Library/Containers/*` and
`~/Library/Group Containers/*`), `other`. The app turns these into words; no path is sent.

### `UnreadableArea` (record)
| Field | Type | Meaning |
|---|---|---|
| `area` | `PrivateArea` | |
| `folder_count` | `u64` | Folders in that area the scan could not read. Always more than 0 |

### `FullDiskAccess` (enum)
| Value | Meaning |
|---|---|
| `granted` | Every private folder checked that exists could be listed |
| `not_granted` | At least one could not be listed: Full Disk Access is off |
| `unknown` | None of the folders checked exist (a test home), so the core can't tell |

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
| `device_backup` | `DeviceBackup?` | iPhone or iPad backups (`ios.device_backup`) only: which device it is. Always set for a backup, even when its details couldn't be read, so the app always shows the warning. `nil` for everything else |

### `DeviceBackup` (record)
Read from the backup's `Info.plist` by `analysers::ios_backup`, after `safety::validate` and without following a symlink. Shown in both modes; it holds no path.

| Field | Type | Meaning |
|---|---|---|
| `device_name` | `String?` | The device's name, with control and text-direction characters removed, at most 60 characters |
| `last_backup_unix_seconds` | `i64?` | When the backup last finished, in seconds since 1970. The app formats it |

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

### `LibraryKind` (enum)
`photos` (every `*.photoslibrary` in home), `music` (every `*.musiclibrary`, plus
`~/Music/Music/Media`, `~/Music/Music/Media.localized` and `~/Music/iTunes`), `tv` (every
`*.tvlibrary`, plus `~/Movies/TV/Media` and `Media.localized`), `mail` (`~/Library/Mail` and
`~/Library/Containers/com.apple.mail`), `messages_attachments` (`~/Library/Messages/Attachments`
only, never the chat database), `final_cut` (every `*.fcpbundle`). A library inside another is
counted once.

### `LibrarySize` (record)
| Field | Type | Meaning |
|---|---|---|
| `kind` | `LibraryKind` | Which library. |
| `bytes` | `u64` | Allocated bytes, counted once. |
| `library_count` | `u32` | How many separate library bundles were found. 0 for `mail` and `messages_attachments`. |
| `found` | `bool` | Whether anything of this kind was found. |
| `needs_access` | `bool` | Part of it couldn't be read (usually Full Disk Access is off), so it may be larger, or exist even though `found` is false. |

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
| `nothing_to_put_back` | `put_back_cleanup` was given a cleanup the core no longer holds. Trying again won't help |
| `docker_unavailable { reason }` | A Docker clean-up could not be previewed, or was refused before it ran (Docker missing or stopped, review expired, or something new appeared). Nothing was removed. Plain words, no path |
| `tool_unavailable { reason }` | A tool clean-up (Homebrew, simulators, Android SDK) could not be previewed, or was refused before it ran (tool missing, Java missing for `sdkmanager`, review expired, package not in the review, or something changed). Nothing was removed. Plain words, no path |
| `unknown_app` | `review_app` or `preview_uninstall` was given an app id that is not from the latest `list_apps` (or not the app last reviewed) |

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
| `item_id` | `ItemId` | An id from `list_items` or `overview`, or a row id from `large_files` |
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
| `device_backup` | `DeviceBackup?` | As `ItemDetail.device_backup` |

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
Moves the last cleanup's items back. Throws `nothing_to_put_back` if `cleanup` is not the most
recent cleanup that moved anything, or was already put back; trying again won't help. Throws
`cleanup_unavailable` if the Trash or home folder can't be opened; nothing moved, and the same
cleanup can be put back later.

### `cleanup_history(limit: u32) -> [HistoryEntry]`
Past cleanups, newest first, at most `limit`. Empty before the first cleanup; reading it never makes
the file.

### `set_protected_folders(folders: [String]) -> [String]` throws
The folders the user asked Roomy to keep. Nothing inside one of them, and nothing that holds one, is
ever cleaned: the preview shows such an item as won't move, and the cleanup refuses it again just
before acting. Replaces the earlier list and returns the list as the core will use it (a folder that
exists is stored as its real location, symlinks resolved). Throws `invalid_path` if a folder is not a
plain full path inside the home folder; the earlier list then stays. The app stores the list in its
settings and sends it before every preview and cleanup. This is the one command where the app sends
paths, and they can only stop things being cleaned.

### `protected_folders() -> [String]`
The kept folders as the core uses them.

### Extra checks in the preview and the cleanup
Besides the protected-path rules, the executor skips an item, with a plain reason and no path, when:
- it is inside or holds a kept folder;
- the app that uses it is open (for example Xcode for DerivedData), or the running apps can't be listed;
- it is inside a git project that doesn't ignore it, git tracks files inside it, the project is
  partway through a merge or rebase, or the project's git data can't be read;
- it, or anything in it, changed in the last 10 minutes.

All of these only read. They run in `preview_cleanup` and again in `run_cleanup` for every item.

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

## Large files and old Downloads (E2)

What the Large files screen lists. Read once, when the scan finishes; nothing here changes a file.
Rows are not rule items: they are not in `list_items`, the Overview or its totals. Their ids go to
`preview_cleanup` like any item id, and are then cleaned, put back and kept in History the same way.

Rules:

1. Nothing is ever ticked for the user, and every row is Review first. Rows in Downloads, Desktop
   and Documents move only when `chosen_alone` is `true` (they are review only in `docs/safety/`).
2. Trash only, never regenerable: a row never allows permanent delete.
3. Large files are the 500 largest plain files in home, leaving out `~/Library`, hidden folders,
   `~/Applications`, anything inside a package (`.app`, `.photoslibrary`, `.fcpbundle` and the
   others in `analysers::files`), the Music and TV apps' media folders and `node_modules`. Anything
   a rule explains, or inside such an item, is on its own screen and left out here. So are files
   with more than one hard link (removing one name frees nothing), anything that changed since the
   scan, and anything the safety rules would refuse.
4. Old Downloads are the files and folders directly in `~/Downloads`, with the same exclusions, not
   opened for 90 days (or, with no record of being opened, not changed for 90 days). A folder that
   could not be read in full is left out.
5. "Last opened" is macOS's own record (`com.apple.lastuseddate#PS`, what Finder shows). A file
   opened only from Terminal may have none; that is `null`, not "never".

### `large_files(handle: ScanHandle) -> LargeFiles` throws
The rows for a finished scan, each list largest first. Throws the same scan errors as `list_items`.

### `LargeFiles` (record)
| Field | Type | Meaning |
|---|---|---|
| `large_files` | `[FileRow]` | Up to 500 of the largest files, largest first |
| `old_downloads` | `[FileRow]` | Old things in Downloads, largest first. A file can also be in `large_files`, with the same id |
| `old_after_days` | `u32` | How many days count as old (90) |

### `FileRow` (record)
| Field | Type | Meaning |
|---|---|---|
| `item_id` | `String` | Send back in a `CleanupChoice` |
| `name` | `String` | The file's or folder's own name |
| `place` | `String` | The folder in home it is in, such as `Movies`, or `Home folder` |
| `size_bytes` | `u64` | Space on disk |
| `is_folder` | `bool` | `true` for a folder in Downloads |
| `last_opened_unix_seconds` | `i64?` | When macOS last recorded it being opened; `null` when there is no record |
| `modified_unix_seconds` | `i64` | When it last changed |
| `path` | `String?` | `~/...` in Developer mode only; `null` in simple mode |

## Low-space alerts

Free functions, not methods on `RoomyCore`. Pure arithmetic: they read nothing and change nothing.
The app reads free space with `disk_summary(path)` and passes the number in. Nothing here can start
a scan or a cleanup; an alert only tells the user.

### `check_low_space(free_bytes: u64, threshold_bytes: u64, warned: bool) -> LowSpaceCheck`
Decides whether free space is low and whether to show a notification now. `warned` is the value
the previous call returned (false the first time). The threshold is kept between 1 GB and 200 GB.
An alert fires once when free space drops below the threshold, and cannot fire again until free
space climbs back above the threshold by a margin (a tenth of the threshold, at least 1 GB).

### `low_space_threshold_choices() -> [u64]`
The thresholds Settings offers, in bytes, smallest first: 5, 10, 20 and 50 GB.

### `low_space_default_threshold() -> u64`
The threshold used until the user picks one: 10 GB.

### `LowSpaceCheck` (record)
`is_low: bool`, `should_alert: bool`, `warned: bool` (pass back next time), `threshold_bytes: u64`
(after clamping).

## App uninstaller

Removes an app and what it left in the home folder. It reuses the cleanup machinery: the dry run
makes the same single-use plan, `run_cleanup` carries it out, and `put_back_cleanup` and History
work as for any cleanup. Trash only.

Rules:

1. Swift sends app ids and part ids, never paths. The core finds apps in `/Applications` and
   `~/Applications` (top level only), reads each `Info.plist`, and never lists Apple's apps
   (`com.apple.*`) or Roomy itself.
2. Leftovers are matched by bundle id in `~/Library` (Application Support, Caches, Containers,
   Group Containers, Preferences and `ByHost`, Saved Application State, HTTPStorages, WebKit,
   Cookies, Logs, LaunchAgents, Application Scripts). A name that starts with the bundle id and a
   dot, or a folder with the app's name (Application Support, Caches, Logs), is offered as "check
   this" (`sure = false`). Symlinks, and anything the safety rules would refuse, are not offered.
3. `preselected` is never true for a `sure = false` part, for Application Support, Containers or
   Group Containers, or for any review-only path. The user ticks those themselves.
4. The app itself is offered only when it is in `~/Applications`. `/Applications` is outside the
   home folder, so the safety rules refuse it; the app shows it in Finder instead.
5. `preview_uninstall` refuses (`cleanup_unavailable`) while anything inside the app is running,
   or if the running programs can't be listed. The executor then skips each part, just before
   acting, if anything inside the app or inside that part is running. All the other cleanup
   checks apply unchanged, including the recent-change window and folders you keep.
6. If another copy of the app (same bundle id) is installed, no leftovers are offered, since both
   copies use them.
7. `bundle_path` is given in both modes: an app in an Applications folder is not personal. Part
   paths are given in Developer mode only.

### `list_apps() -> [AppSummary]`
Every app, sorted by name, with its size. New ids on each call; forgets the earlier review.

### `review_app(app_id: String, mode: Mode) -> AppReview` throws
What the app left behind, measured just now: the app itself first (if offered), then largest
first. Replaces the earlier review. Throws `unknown_app`.

### `preview_uninstall(app_id: String, part_ids: [String]) -> CleanupPreview` throws
The dry run for the ticked parts of the latest review. Like `preview_cleanup`; History names each
part "<app>: <part>". Throws `unknown_app`, `unknown_item`, `nothing_selected` or
`cleanup_unavailable`.

### `AppSummary` (record)
`app_id`, `name`, `version: String?`, `bundle_id`, `size_bytes: u64`, `location: AppLocation`,
`bundle_path: String`, `is_open: Bool`.

### `AppLocation` (enum)
`applications` (`/Applications`), `home_applications` (`~/Applications`).

### `AppReview` (record)
`app: AppSummary`, `parts: [AppPart]`, `note: String?`.

### `AppPart` (record)
`item_id`, `title`, `explain`, `size_bytes: u64`, `preselected: Bool`, `sure: Bool`,
`path: String?` (Developer mode only).

## JavaScript screen

The Developer mode JavaScript screen (PRD D2). Read only: rows come from the finished scan (the
`js.*` rules); for each `node_modules` the core reads only the modified times of the project's
`package.json`, lock files (`package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml`,
`yarn.lock`, `bun.lock`, `bun.lockb`) and the nearest git repository's `.git/index` and
`.git/logs/HEAD` (never above or at the home folder). It never opens a file and never follows a
symlink, and it reads a project only after `safety::validate` passes for its `node_modules`.
Cleaning uses `preview_cleanup` with the rows' item ids, so plans, extra checks, the Trash, put
back and History are the same as everywhere else.

### `javascript_projects(handle: ScanHandle) -> JavaScriptOverview` throws

Same errors as `list_items`. In simple mode the list is empty (the rules are Developer only), and
no row would carry a folder anyway.

### `JavaScriptOverview` (record)

`projects: [JsProject]` (largest first, then id), `caches: [ItemSummary]` (the other `js.*` items,
same order), `total_bytes: u64` (every row added up).

### `JsProject` (record)

`item: ItemSummary` (the `node_modules` item; its id goes to `preview_cleanup`), `name: String`
(the project folder's name in Developer mode, the item title in simple mode), `folder: String?`
(Developer mode only, starts `~`), `last_activity_unix_seconds: i64?` (newest of the files above),
`activity_source: ActivitySource?`, `package_manager: PackageManager?` (from the newest lock file),
`install_command: String?` (for example `pnpm install`), `has_manifest: bool`.

### `ActivitySource` (enum)

`git`, `lock_file`, `manifest`.

### `PackageManager` (enum)

`npm`, `pnpm`, `yarn`, `bun`.

## Testing

The `ffi` crate has a `testing` Cargo feature. With it, and only with it, the core also exports:

- `RoomyCore.with_home_for_testing(home: String)`: like `new()`, but scans `home` instead of the
  real home folder. `home` must be an absolute path to a folder.
- `RoomyCore.use_home_applications_only_for_testing()`: the uninstaller looks for apps only in
  the test home's `Applications` folder, never the test machine's `/Applications`.
- `make_fixture_home(dir: String) -> String`: builds the T04 fixture home in `dir` (which must be
  empty or not exist yet) and returns its canonical path, ready for `with_home_for_testing`.

`./scripts/build-ffi.sh` builds with `testing` by default so the Xcode test target can call these.
`./scripts/build-ffi.sh --release-app` builds without it; the release pipeline (T13) must use
that, so a shipped app can only ever scan the real home folder. This matters more once cleanup
commands exist, because the executor checks paths are inside the home folder it was given.

The T02 commands `core_version()` and `disk_summary(path)` are unchanged.

## Docker

The Docker screen (developer mode). The app sends a `DockerCleanupKind`, never a command line or a
Docker id. Only the two Docker commands on the allow-list in `docs/safety/allowed-commands.md` can
run, each after its read-only preview. Volumes are shown and never offered for removal. No command
ever gets `-f`, `--force`, `-a` or `--all`.

The core looks for `docker` in `/usr/local/bin`, `/opt/homebrew/bin`, `~/.docker/bin`, `~/.orbstack/bin`,
`/Applications/Docker.app/Contents/Resources/bin`, `/usr/bin` and `/bin`, because a Mac app's own
`PATH` holds none of them. Every call blocks until Docker answers, so the app calls them off the main
thread.

### `docker_overview() -> DockerOverview`
Runs the read-only `docker system df --format '{{json .}}'` (at most 20 seconds) and reads the size
of Docker Desktop's disk image (`~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw`,
allocated bytes, never following a symlink). Never throws: a missing, stopped or unreadable Docker
is a `DockerStatus`.

### `preview_docker_cleanup(kind: DockerCleanupKind) -> DockerCleanupPreview` throws
Runs the kind's read-only preview (`docker image ls --filter dangling=true --format json` or
`docker builder du`) and keeps what it listed as a single-use plan, replacing any earlier Docker
plan. For build cache only records Docker marks reclaimable are listed. `plan` is `nil` when there is
nothing to remove. Throws `docker_unavailable`.

### `discard_docker_cleanup(plan: DockerPlanHandle)`
Drops the waiting Docker plan if it is `plan`.

### `run_docker_cleanup(plan: DockerPlanHandle) -> DockerCleanupResult` throws
Throws `cleanup_unavailable` if `plan` is not the waiting Docker plan. Otherwise the executor
refuses it with `docker_unavailable`, running nothing, if it is ten minutes old or more, or if the
preview, run again just now, lists anything the user did not see. Then it runs `docker image prune`
or `docker builder prune`, answering Docker's own prompt with `y` on stdin, waits up to 30 minutes,
and saves the run to History (`command_runs`: command, exit status, bytes Docker says it freed, and
Docker's full stdout and stderr, cut at 64 KB each). A run that started is recorded even if Docker
reported a problem; `success` says which.

### `DockerCleanupKind` (enum)
`unused_images` (`docker_image_prune`), `build_cache` (`docker_builder_prune`).

### `DockerStatus` (enum)
`not_installed`, `not_running`, `unavailable { reason }`, `ready`.

### `DockerUsageRow` (record)
`count`, `active`, `size_bytes`, `reclaimable_bytes`, all `u64`. Docker rounds sizes for people, so
these are estimates.

### `DockerOverview` (record)
`status`, `images`, `containers`, `volumes`, `build_cache` (all zero unless `ready`),
`disk_image_bytes: u64?`.

### `DockerCleanupPreview` (record)
`plan: DockerPlanHandle?`, `kind`, `command_line` (such as `docker image prune`), `items` (each
`id`, `size_bytes`, `age` in Docker's words), `total_bytes`, `expires_in_seconds`.

### `DockerCleanupResult` (record)
`kind`, `success`, `reclaimed_bytes: u64?`, `saved_to_history`.

## System Data

What macOS's "System Data" bar holds (PRD F9). Read only: nothing here can be cleaned. What Roomy can
clean in these places (your own caches, logs and iPhone updates) is already in the scan's items and
goes through the usual Trash-only cleanup.

### `system_data(mode: Mode) -> SystemData`
Measures, just now: caches (`~/Library/Caches`, `/Library/Caches`), logs (`~/Library/Logs`,
`/Library/Logs`, `/private/var/log`, `/private/var/db/diagnostics`), software updates
(`~/Library/iTunes/iPhone Software Updates`, `~/Library/iTunes/iPad Software Updates`,
`/Library/Updates`), the user's temporary folder under `/private/var/folders` (from
`_CS_DARWIN_USER_CACHE_DIR`), swap files (`/private/var/vm/swapfile*`) and the sleep image
(`/private/var/vm/sleepimage`). Folders go through the read-only scanner (allocated size, no
symlink followed, one volume, hard links once); files are read with `lstat`. Also counts Time
Machine local snapshots with the read-only `tmutil listlocalsnapshots /` (15 second limit). Never
throws. Can take several seconds; call it off the main thread. Paths only in Developer mode.

### `SystemData` (record)
`snapshots: LocalSnapshots`, `parts: [SystemDataPart]` (in the order caches, logs, software
updates, temporary files, swap, sleep image), `total_bytes` (sum of the parts; snapshots have no
size).

### `SystemDataPart` (record)
`kind: SystemDataKind`, `size_bytes`, `partly_unreadable` (a place or a folder inside one could not
be read, so the real size is larger), `places: [SystemDataPlace]`.

### `SystemDataPlace` (record)
`id` (stable, such as `user_caches`, `shared_caches`, `sleep_image`), `owner: SystemDataOwner`
(`you` inside the home folder, `system` outside it), `state: SystemDataPlaceState` (`missing`,
`unreadable`, `measured`), `size_bytes`, `file_count`, `partly_unreadable`, `path: String?`
(`~/…` or `/…`, Developer mode only).

### `LocalSnapshots` (record)
`status: SnapshotStatus` (`listed` or `unavailable { reason }`), `count`, `oldest_unix_seconds:
i64?`, `newest_unix_seconds: i64?`. macOS names snapshots in local time; the core reads the name as
if it were UTC, so show the date in UTC.

## Tool clean-ups

The Tool clean-ups screen (developer mode): the other three commands on the allow-list in
`docs/safety/allowed-commands.md`. The app sends a `ToolCleanupKind`, never a command line or a path.
For the Android SDK it also sends the id of one package that the preview listed.

| Kind | Preview (read only) | Command |
|---|---|---|
| `homebrew` (`brew_cleanup`) | `brew cleanup --dry-run` | `brew cleanup` |
| `unavailable_simulators` (`xcrun_simctl_delete_unavailable`) | `xcrun simctl list devices unavailable -j` | `xcrun simctl delete unavailable` |
| `android_sdk_package` (`sdkmanager_uninstall`) | `sdkmanager --list_installed` | `sdkmanager --uninstall <package>` |

Where the core looks, because a Mac app's own `PATH` holds none of these:

- `brew`: `/opt/homebrew/bin/brew`, then `/usr/local/bin/brew`. Runs with `HOMEBREW_NO_AUTO_UPDATE`,
  `HOMEBREW_NO_ANALYTICS`, `HOMEBREW_NO_ENV_HINTS` and `HOMEBREW_NO_COLOR` set.
- `xcrun`: `/usr/bin/xcrun`, only when an Xcode with `simctl` is in `/Applications/Xcode.app`,
  `/Applications/Xcode-beta.app` or `~/Applications/Xcode.app`. `DEVELOPER_DIR` is set to that Xcode,
  so a Mac without Xcode never sees the "install developer tools" prompt.
- `sdkmanager`: inside `~/Library/Android/sdk` only: `cmdline-tools/latest/bin`, else the newest
  other `cmdline-tools/<version>/bin`, else `tools/bin`. Must be a regular file, not a symlink.
  `JAVA_HOME` is set to Android Studio's own Java (`/Applications/Android Studio.app/Contents/jbr`)
  when it is there.

Every program runs with an argument array and a fixed `PATH`, never through a shell. Sizes are
measured from metadata without following symlinks or leaving the volume: the simulator's folder in
`~/Library/Developer/CoreSimulator/Devices`, the package's `Location` folder inside the SDK, and
Homebrew's own figures for `brew`.

### `preview_tool_cleanup(kind: ToolCleanupKind) -> ToolCleanupPreview` throws
Runs the kind's read-only preview (at most two minutes) and keeps what it listed as a single-use
plan, replacing any earlier tool plan. For the Android SDK the `cmdline-tools` packages are never
listed, as removing them would remove `sdkmanager` itself. `plan` is `nil` when there is nothing to
remove. Throws `tool_unavailable`.

### `discard_tool_cleanup(plan: ToolPlanHandle)`
Drops the waiting tool plan if it is `plan`.

### `run_tool_cleanup(plan: ToolPlanHandle, package: String?) -> ToolCleanupResult` throws
Throws `cleanup_unavailable` if `plan` is not the waiting tool plan. The plan is used up either way.
The executor refuses with `tool_unavailable`, running nothing, if the plan is ten minutes old or
more; if `package` is missing or not one the preview listed (Android SDK), or given at all (the
others); or if the preview, run again just now, lists anything the user did not see (for the
Android SDK: if the package is no longer installed). Then it runs the command, waits up to 30
minutes, and saves the run to History (`command_runs`, as for Docker). `reclaimed_bytes` is
Homebrew's "freed approximately" figure, or for the others the size the preview measured when the
tool succeeded.

### `ToolCleanupKind` (enum)
`homebrew`, `unavailable_simulators`, `android_sdk_package`.

### `ToolCleanupPreview` (record)
`plan: ToolPlanHandle?`, `kind`, `command_line` (for the Android SDK, ending in `<package>`),
`items` (each `id`, `title`, `detail`, `size_bytes`), `total_bytes`, `expires_in_seconds`.

### `ToolCleanupResult` (record)
`kind`, `command_line`, `success`, `reclaimed_bytes: u64?`, `saved_to_history`.
