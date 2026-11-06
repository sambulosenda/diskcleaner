# T05 — Read-only scanner

**Goal:** meet requirement F1.

**Do** in `core/crates/scanner`:
- `fn scan(root: &Path, opts: ScanOptions, progress: impl Fn(ScanProgress)) -> Result<ScanResult, ScanError>`.
- Parallel walk (rayon or `jwalk`), allocated size via `st_blocks * 512`, never follow symlinks (`symlink_metadata`), stay on the root's `st_dev`, count each `(st_dev, st_ino)` hard link once.
- Build a directory tree with aggregated sizes; keep per-folder totals, not every file (memory target). Keep the 500 largest files separately for E2.
- Record folders that could not be read (permission denied) as `unreadable` with their path, not as errors.
- Cancellation via an `AtomicBool`.

**Done when**
- Fixture test: total equals the manifest's expected allocated total; hard-link pair counted once; symlinks not traversed; unreadable folder reported.
- Benchmark (`cargo bench` or an `#[ignore]` test) documented in the PR with numbers from Sam's Mac: target 1 TB / 2M files under 60 s, peak RSS under 500 MB. Within 1% of `du -sk` on the same folder.
- No write, rename or delete calls anywhere in the crate (CI grep from T12).
