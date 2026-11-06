# Build notes

How the Rust core gets into the Xcode app, and every Xcode setting changed by hand.

## Steps from a fresh clone
1. Install Rust with [rustup](https://rustup.rs) and Xcode 15 or later.
2. From the repo root, run `./scripts/build-ffi.sh`. It:
   - adds the `aarch64-apple-darwin` and `x86_64-apple-darwin` Rust targets,
   - builds the `ffi` crate (library name `roomy_ffi`) in release mode for both,
   - joins them with `lipo` into `app/Generated/lib/libroomy_ffi.a`,
   - generates the Swift bindings with UniFFI in library mode into `app/Generated/`.
3. Open `app/Roomy.xcodeproj` and build, or run `xcodebuild -scheme Roomy -destination 'platform=macOS' test` from `app/`.

Run the script again after any change to the Rust core. `app/Generated/` is git-ignored, so Xcode fails with
"no such module 'roomy_coreFFI'" or a missing `roomy_core.swift` until the script has run once.

## Generated files
| File | Used by |
|---|---|
| `app/Generated/roomy_core.swift` | Compiled into the Roomy app target |
| `app/Generated/include/roomy_coreFFI.h` | C header for the FFI layer |
| `app/Generated/include/module.modulemap` | Clang module `roomy_coreFFI`, imported by `roomy_core.swift` |
| `app/Generated/lib/libroomy_ffi.a` | Universal static library linked into the app |

## Xcode settings changed
These were edited directly in `project.pbxproj`; nothing needs setting by hand in Xcode.

| Where | Setting | Value | Why |
|---|---|---|---|
| Project (Debug and Release) | `SWIFT_INCLUDE_PATHS` | `$(PROJECT_DIR)/Generated/include` | Lets both the app and the test target find the `roomy_coreFFI` module |
| Roomy target (Debug and Release) | `LIBRARY_SEARCH_PATHS` | `$(PROJECT_DIR)/Generated/lib` | Where the static library is |
| Roomy target (Debug and Release) | `OTHER_LDFLAGS` | `-lroomy_ffi` | Links the static library |
| Roomy target, Sources | added file | `Generated/roomy_core.swift` (group "Generated") | The Swift bindings |

The test target needs no link settings: it runs hosted in the app (`TEST_HOST` / `BUNDLE_LOADER`), so it uses
the app's copy of the library.

## Notes
- The library is called `roomy_ffi`, not `ffi`, because `libffi.a` would clash with the system libffi.
- Only the `.a` goes into `Generated/lib`. If a `.dylib` were there too, the linker would pick it instead.
- The script sets `MACOSX_DEPLOYMENT_TARGET=13.0` so the Rust objects match the app's deployment target.
- `uniffi-bindgen` is a binary inside the `ffi` crate behind the `bindgen` feature, so the bindings CLI is never
  compiled into the app's library.
- Free space comes from `statfs` (`f_bavail`), which does not count purgeable space. Finder's figure may be
  higher. This is fine for T02; T09 can revisit it.
