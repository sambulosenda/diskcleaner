# Fixtures

Fixture homes are fake Mac home folders that tests run against, so no test ever touches a real home.

They are not checked in. They are built on demand by the generator in `core/crates/fixtures`:

- In Rust tests: `let home = fixtures::home();` gives a fresh fixture home in a temp folder (removed when dropped). Use `fixtures::canonical_root(&home)` for the path, and the names in `fixtures::paths` to find things in it.
- From the shell: `./scripts/make-fixtures.sh <dir>` builds one in an empty `<dir>`.

Each fixture home has a `fixtures-manifest.json` at its root listing every path, its kind, size and SHA-256. Contents are deterministic; the few files that hold the home path itself (Xcode `WorkspacePath`, Android AVD `.ini`) are marked `contains_home_path`.
