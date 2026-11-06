# T08 — Classifier

**Goal:** turn a `ScanResult` plus rules into classified items (requirement F4).

**Do** in `core/crates/classifier`:
- `fn classify(scan: &ScanResult, rules: &[Rule], mode: Mode) -> Vec<ClassifiedItem>`; pure function, no filesystem access (sizes and structure come from the scan; `match.requires` checks use data captured by the scanner — extend the scanner's output minimally if needed and note it).
- Most specific rule wins (longest literal prefix); ties are an error logged at load time.
- Items not matched by any rule are grouped by top-level folder as `unknown` with tier `keep`.
- Each `ClassifiedItem` carries: stable id (hash of rule id + relative path), rule id and version, title, explain, risk, restore, tier, confidence, regenerable, size, path (core-internal only).
- Developer-mode rules are excluded in simple mode.

**Done when**
- Fixture test: every fixture item lands in the expected rule; trap paths never classify as `likely_safe`.
- Ids are stable across two scans of the same fixture.
- Deterministic output order (size descending, then id).
