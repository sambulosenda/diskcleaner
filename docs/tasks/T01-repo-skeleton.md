# T01 — Repo skeleton

**Goal:** an empty but building monorepo with the agreed layout.

**Create**
```
core/                      Cargo workspace
  Cargo.toml               workspace members below
  crates/scanner rules classifier analysers recommend planner safety executor history licensing ffi
rules/                     (empty, .gitkeep)
app/                       Xcode project "Roomy" (SwiftUI, macOS 13+), with a unit test target
fixtures/                  (empty, .gitkeep)
tests/zero_data_loss/      (empty, .gitkeep)
scripts/
docs/
```
Each crate: `lib.rs` with a one-line doc comment describing its role (copy from the CLAUDE.md table) and one trivial passing test.

Add `executor` crate-level doc: "The only crate allowed to modify the filesystem."

Add `rust-toolchain.toml` (stable), `.gitignore` for Rust, Xcode and macOS, and a short `README.md` with build steps.

**Out of scope:** any real logic.

**Done when**
- `cargo build --workspace` and `cargo test --workspace` pass from `core/`.
- `cargo clippy --workspace -- -D warnings` passes.
- The Xcode project builds and its empty test target passes (`xcodebuild test`).
- Layout matches the tree above exactly.
