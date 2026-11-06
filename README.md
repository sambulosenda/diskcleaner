# Mac storage cleaner

A native macOS app that shows what is filling your disk, explains every item in plain words, and frees space safely. Read `CLAUDE.md` before changing anything, and see `docs/PRD.md` for the product spec.

## Layout
- `core/`: Rust Cargo workspace. All scanning, classification, planning and cleanup live here.
- `app/`: SwiftUI app (UI only), which talks to the core through UniFFI.
- `rules/`: TOML rule files.
- `fixtures/`: fixture home folders used by tests.
- `tests/zero_data_loss/`: end-to-end safety tests.
- `scripts/`, `docs/`: helper scripts and specs.

## Requirements
- macOS 13 or later, with Xcode 15 or later
- Rust stable, installed with [rustup](https://rustup.rs) (`rust-toolchain.toml` selects the channel)

## Build and test the core
```sh
cd core
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Build and test the app
```sh
cd app
xcodebuild -scheme CODENAME -destination 'platform=macOS' test
```
