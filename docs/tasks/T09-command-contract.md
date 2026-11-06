# T09 — Command contract: scan, overview, list items

**Goal:** the typed API the SwiftUI app uses, read-only for now.

**Expose via UniFFI** (`core/crates/ffi`), all async-safe and cancellable:
- `start_scan(mode: Mode) -> ScanHandle`; `scan_progress(handle) -> ScanProgress { bytes_seen, items_seen, phase }`; `cancel_scan(handle)`.
- `overview(handle) -> Overview { likely_safe_bytes, disk_total, disk_free, categories: [CategoryTotal], top_items: [ItemSummary], unreadable_count }`.
- `list_items(handle, category: Option<Category>, tier: Option<Tier>, sort: Sort) -> [ItemSummary]`.
- `item_detail(handle, item_id) -> ItemDetail` (explain, risk, restore, rule id, confidence; `path` only populated in Developer mode).
- Home path comes from the core (`dirs::home_dir`) in production but is injectable for tests.

**Rules**
- No path ever crosses to Swift in simple mode. In Developer mode, paths are display strings only; Swift never sends a path back.
- `likely_safe_bytes` equals the exact sum of `likely_safe` item sizes (F2).

**Done when**
- Write the contract in `docs/command-contract.md` first (types and each function's meaning); Sam approves it in the PR before implementation is merged.
- Swift tests call each command against a fixture home.
