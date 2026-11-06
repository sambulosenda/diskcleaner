# T12 — CI on a macOS runner

**Goal:** every PR is checked automatically.

**Do** (GitHub Actions or the repo host's equivalent), on `macos-latest`:
- `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`.
- `xcodebuild test` for the app.
- **Safety grep:** fail if any crate other than `executor` contains `remove_file`, `remove_dir`, `remove_dir_all`, `fs::rename`, `trashItem`, `Command::new` (except `analysers` read-only queries listed in an allow-file `ci/allowed-commands-readonly.txt`), or `sh -c` anywhere.
- **Protected files check:** fail if a PR changes any path listed under "Files you must NEVER edit" in CLAUDE.md, unless the PR has the label `sam-approved` (Sam adds it by hand).
- Rule validation test runs as part of `cargo test`.
- Cache cargo and DerivedData between runs.

**Done when**
- A deliberately bad PR (e.g. `std::fs::remove_file` in `scanner`) fails CI — show the failing run in the PR.
- A normal PR passes in under 15 minutes.
