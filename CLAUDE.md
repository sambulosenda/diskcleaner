# CLAUDE.md — Mac storage cleaner (codename: Roomy)

Read this file fully at the start of every session. If anything here conflicts with a task spec, this file wins; stop and ask Sam.

## What we are building
A native macOS app that shows any Mac user what is filling their disk, explains every item in plain words, and frees space safely. Simple mode for everyday users, Developer mode for developers (Xcode, Android, JS, Docker and more). Full product spec: `docs/PRD.md`.

## The one rule that matters most
**This app deletes files. A single bug that removes something the user needed ends the product.** When unsure, do less, refuse the action, and ask Sam. Never "make a test pass" by weakening a safety check.

## Architecture
- **Rust core** (`core/`, Cargo workspace) does all scanning, classification, planning and cleanup.
- **SwiftUI app** (`app/`) is the UI only. It calls the core through **UniFFI** typed commands defined in `core/crates/ffi`.
- **Rules** (`rules/*.toml`) are data files describing what each item is and whether it is safe. Schema: `docs/safety/rule-schema.md`.
- **SQLite** stores scan and cleanup history. **Sparkle** handles updates.

Data flow, strictly one direction:
`scanner (read) → classifier (pure) → recommendations (explain only) → planner (preview) → safety gate (validate) → executor (act, record)`

Crates:
| Crate | Role | May touch the filesystem? |
|---|---|---|
| `scanner` | Parallel walk, sizes | Read only |
| `rules` | Load and validate TOML rules | Read rule files only |
| `classifier` | Match scan results to rules | No |
| `analysers` | Per-tool analysis (xcode, android, js, docker, everyday) | Read only; may run read-only CLI queries |
| `recommend` | Build tiered recommendations | No |
| `planner` | Turn selected recommendation IDs into a single-use plan | No |
| `safety` | Path validation, protected paths | Read metadata only |
| `executor` | Move to Trash, permanent delete, allow-listed commands | **Yes — the only crate allowed to modify files** |
| `history` | SQLite log | Writes its own database only |
| `licensing` | Licence token, 5 GB free counter | Keychain only |
| `ffi` | UniFFI surface | No |

## Safety rules (non-negotiable)
1. Only `executor` may move, delete or run modifying commands. No other crate may call `std::fs::remove_*`, `rename` into Trash, or spawn modifying processes. CI greps for this.
2. The UI sends **recommendation IDs**, never paths. The executor only acts on paths from the engine's own plan.
3. Every path is validated by `safety` when the plan is built **and again immediately before acting**: absolute, inside the home folder, not on a protected list, no symlink anywhere in the path or its parents.
4. Plans are single use and expire after 10 minutes.
5. Trash is the default. Permanent delete is allowed only if every item in the plan is `regenerable = true`.
6. No shell string building. Commands run with `std::process::Command` and argument arrays, only from the allow-list in `docs/safety/allowed-commands.md`, implemented in `executor::commands`. Never use `sh -c`.
7. Never follow symlinks. Never cross volumes. Count hard links once.
8. Docker volumes are never removed. Never pass `--force` / `-f` to Docker.
9. Personal content is never `likely_safe` (see `docs/safety/protected-paths.md`).
10. Licence and feature gates are enforced in the Rust core, not only in the UI.

## Files you must NEVER edit
These are owned by Sam (task T03 creates the safety files once, under Sam's supervision). If a task seems to need a change here, stop and explain why in the PR description instead.
- `core/crates/safety/src/protected.rs`
- `core/crates/safety/tests/**`
- `tests/zero_data_loss/**`
- `docs/safety/**`
- `CLAUDE.md`
- `.claude/settings.json`

## How to work
- One task per session, from `docs/tasks/`. Read the whole task file first. Do only that task.
- Work on a branch named `task/TXX-short-name`. Never commit to `main`. Open a pull request when done.
- Write tests first where the task says so. All tests must pass: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, and the Xcode test target when the app is touched.
- Never run cleanup code against the real home folder. Tests use fixture homes in temp directories (`fixtures/`), and every core API takes the home path as a parameter instead of reading `$HOME` directly.
- Do not add dependencies without listing them, with a one-line reason each, in the PR description.
- Keep functions small and documented. Prefer clarity over cleverness.
- PR description must include: what changed, how it was tested, any deviation from the task spec, and anything Sam should look at closely.
- If the task is ambiguous, make the safer choice and note it in the PR.

## Conventions
- Rust: edition 2021, stable toolchain, `thiserror` for errors, no `unwrap()`/`expect()` outside tests, `tracing` for logs.
- Swift: SwiftUI, Swift 5.9+, macOS 13+ deployment target, no third-party UI libraries.
- Sizes are always bytes as `u64` in the core; formatting happens in the UI.
- User-facing text is plain British English, short, no jargon in simple mode, no alarming words like "junk".

## Useful commands
- `cargo test --workspace` (from `core/`)
- `./scripts/make-fixtures.sh <dir>` builds a fixture home (after T04)
- `./scripts/build-ffi.sh` regenerates Swift bindings (after T02)
