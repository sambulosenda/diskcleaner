# T03 — Protected paths and safety tests (tests only)

**This task is reviewed line by line by Sam. No executor code in this task.**

**Special setup:** this is the only task allowed to create `core/crates/safety/src/protected.rs` and `core/crates/safety/tests/**`. Before starting the session, Sam temporarily removes the two matching `deny` lines from `.claude/settings.json`, and restores them as soon as the PR is merged. The PR needs the `sam-approved` label. After this task, those files are Sam-only.

**Goal:** encode `docs/safety/protected-paths.md` as code and tests before any cleanup code exists.

**Do**
- `core/crates/safety/src/protected.rs`: data structures for the hard-blocked and review-only lists, exactly as written in `docs/safety/protected-paths.md`. Every entry has a comment quoting the line it implements.
- `core/crates/safety/src/lib.rs`: `fn validate(home: &Path, scan_root_dev: u64, candidate: &Path, selected_individually: bool) -> Result<ValidatedPath, Refusal>` implementing the 6 plan-time checks in the "Validation order" section. `Refusal` is an enum with one variant per reason.
- `core/crates/safety/tests/protected_paths.rs`: one test per list entry proving it is refused (hard-blocked) or refused unless `selected_individually` (review-only). Use temp directories as the fake home.
- Tests for each validation step: relative path, `..` segments, path outside home, a symlinked parent, a symlink as the final component, a path on another volume (use a mounted disk image in a test marked `#[ignore]` with instructions, plus a unit test with a mocked device id), `.git`-containing folder, and the allowed build-output exception inside a repo.

**Done when**
- Every line in `docs/safety/protected-paths.md` maps to at least one test (list the mapping in the PR).
- `cargo test -p safety` passes.
- There is no code path in `safety` that returns `Ok` for a hard-blocked path under any argument combination (add a property test with `proptest` generating paths under each blocked root).
