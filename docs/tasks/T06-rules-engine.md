# T06 — Rule schema, loader and validator

**Goal:** implement `docs/safety/rule-schema.md` exactly.

**Do** in `core/crates/rules`:
- Serde types with `deny_unknown_fields`; enums for category, tier, confidence, mode, action.
- `fn load_dir(dir: &Path) -> (Vec<Rule>, Vec<RuleError>)`: invalid rules are rejected individually with a clear error naming file, rule id and field.
- All validation rules from the schema doc, including rejecting any `match.path` that could match a hard-blocked path (call into `safety`).
- A test that loads every file in `rules/` and fails on any error, so a bad rule blocks the build.
- Duplicate `id` across files is an error.

**Done when**
- The three example rules from the schema doc load.
- One test per validation rule proves an invalid rule is rejected.
- Error messages are readable by a non-Rust reviewer.
