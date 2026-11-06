# T04 — Fixture generator

**Goal:** reproducible fake home folders for every test, so no test ever touches a real home.

**Do** `scripts/make-fixtures.sh <dir>` (or a Rust binary in `fixtures/`) that builds, inside `<dir>`:
- Everyday: `Library/Application Support/MobileSync/Backup/<2 device ids>/Info.plist` (valid plist with Device Name, Product Version, Last Backup Date); `Downloads/` with a `.dmg`, `.pkg`, `.zip` and a normal file; `Library/Caches/<3 app ids>/`; a `Pictures/Test.photoslibrary/` bundle; `Library/Messages/Attachments/`.
- Xcode: `Library/Developer/Xcode/DerivedData/` for two projects whose `info.plist` WorkspacePath exists in the fixture and one whose path does not; `iOS DeviceSupport/` for three versions; `CoreSimulator/Devices/` with three devices, one marked unavailable in its `device.plist`; one archive.
- Android: `Library/Android/sdk/system-images/android-{33,34,35}/`, `build-tools/` two versions, `ndk/` one version; `.android/avd/` two AVDs with `.ini` files; two projects with `build.gradle` referencing API 34 only; `.gradle/wrapper/dists/` three versions.
- JS: `projects/{a,b,c}/package.json` + `node_modules/`, with `c/package.json` mtime set 120 days ago; `.npm/`, `Library/pnpm/`, `Library/Caches/Yarn/`.
- Traps: a symlink to `/etc`, a symlink from a cache folder into `Documents/`, a hard link pair, `projects/a/.git/` plus `projects/a/node_modules/`, `projects/a/.env`, `release.keystore`.
- Files filled with deterministic bytes (seeded), small sizes (KBs), with a `fixtures-manifest.json` listing every created path, size and checksum.

**Done when**
- Running twice produces byte-identical trees (same checksums).
- A Rust test helper `fixtures::home() -> TempDir` builds one per test.
- Total fixture size under 20 MB; runs in under 5 s.
