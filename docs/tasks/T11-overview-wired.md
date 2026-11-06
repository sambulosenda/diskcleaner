# T11 — Overview wired to the real core

**Goal:** replace `MockCoreClient` with `RustCoreClient` for Overview and item lists.

**Do**
- Implement `RustCoreClient` over the T09 commands; run scans off the main thread; update progress at most 10 times a second.
- Category screens show `list_items` results; selecting a row shows `item_detail` in a right-hand detail panel (What it is, Why it's here, If you remove it, How it comes back, Reveal in Finder).
- Checkboxes appear but selection does nothing yet; Keep items show no checkbox; Review first items are never preselected.
- Handle permission-denied folders: footer shows "N folders not measured" with a link explaining Full Disk Access.

**Done when**
- Sam runs it on his own Mac and the headline and category totals are plausible against `scripts/mac-storage-survey.sh` output (within ~5% per category).
- UI stays responsive during a full scan.
- No path is visible anywhere in simple mode.
