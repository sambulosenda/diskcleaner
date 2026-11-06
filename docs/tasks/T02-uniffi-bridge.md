# T02 — UniFFI bridge, one command end to end

**Goal:** prove the Rust ↔ Swift bridge with one harmless command before anything real depends on it.

**Do**
- In `core/crates/ffi`, expose with UniFFI (proc-macro style): `fn core_version() -> String` and `fn disk_summary(path: String) -> Result<DiskSummary, CoreError>` where `DiskSummary { total_bytes: u64, free_bytes: u64 }` comes from `statfs` on the given path. Read-only.
- `scripts/build-ffi.sh`: builds the Rust static library for `aarch64-apple-darwin` and `x86_64-apple-darwin`, combines with `lipo` (or an XCFramework), and generates Swift bindings into `app/Generated/`.
- Wire into the Xcode project so the app links the library. Show `core_version()` and the disk summary for the home folder on a placeholder screen.
- Document every manual Xcode setting you changed in `docs/build-notes.md`.

**Done when**
- Fresh clone → `./scripts/build-ffi.sh` → build in Xcode works with no manual steps beyond those in `docs/build-notes.md`.
- App launches and shows the version and free space.
- A Swift unit test calls `core_version()` and asserts it is non-empty.
- Errors from Rust surface as Swift `throws`, not crashes.
