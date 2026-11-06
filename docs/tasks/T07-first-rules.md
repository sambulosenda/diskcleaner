# T07 — First 20 rules

**Goal:** the first rule files, written for real users. Sam signs off every rule's wording and tier.

**Write** in `rules/` (one file per area: `xcode.toml`, `js.toml`, `everyday.toml`):
- Xcode (7): DerivedData (per project), Archives (review_first), iOS DeviceSupport (per version), watchOS DeviceSupport, simulator devices, unavailable simulators (command), CocoaPods cache.
- JS (6): `node_modules` per project (per_item; tier review_first unless the analyser later marks it inactive), npm cache, pnpm store, Yarn cache, Bun cache, Expo cache.
- Everyday (7): iPhone/iPad backups (review_first), installers in Downloads (review_first), browser caches (Safari, Chrome, Firefox — likely_safe), Spotify cache, Slack cache, user Logs, iOS software update files.

**Rules for writing**
- `explain` must make sense to someone who has never opened Terminal.
- Never call anything "junk". Never promise "safe" in `explain`; the tier carries that.
- When unsure of a tier, choose `review_first` and say why in the PR.
- For each rule, the PR lists: the real path on a Mac, evidence it regenerates (link to Apple, Google or vendor docs where possible), and the chosen tier.

**Done when**
- All 20 load with T06's validator.
- Each has a fixture match in T04's fixtures (add fixture paths if missing; that part of T04's script may be edited).
- Sam has approved the wording in the PR.
