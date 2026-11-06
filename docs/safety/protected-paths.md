# Protected paths (owned by Sam — agents must not edit)

Enforced in code by `core/crates/safety/src/protected.rs`, never in rule files, so no rule update can loosen them. `~` means the home path passed to the core.

## Hard-blocked — the executor refuses, no override
- Anything outside the home folder, including `/System`, `/Library`, `/usr`, `/bin`, `/sbin`, `/private`, `/Applications`, `/Volumes`.
- The home folder itself, and these folders themselves (their contents may be targeted, the folders never): `~/Desktop`, `~/Documents`, `~/Downloads`, `~/Pictures`, `~/Movies`, `~/Music`, `~/Library`, `~/Applications`, `~/Public`.
- `~/Library/Keychains/**`, `~/Library/Mail/**`, `~/Library/Messages/*.db*` and `~/Library/Messages/` itself, `~/Library/Mobile Documents/**` (iCloud Drive), `~/Library/CloudStorage/**`.
- Any `*.photoslibrary`, `*.musiclibrary`, `*.tvlibrary` bundle and everything inside it.
- `~/.ssh/**`, `~/.gnupg/**`, `~/.aws/**`, `~/.netrc`, `~/.docker/config.json`, `~/.kube/**`, `~/.config/gh/**`.
- Any `.git` directory, and any directory that contains a `.git` directory, **except** these named build-output children inside a repository: `node_modules`, `build`, `target`, `DerivedData`, `.gradle`, `Pods`, `.next`, `dist`, `.venv`, `venv`, `__pycache__`.
- Signing material: `~/Library/MobileDevice/Provisioning Profiles/**`, and any file matching `*.keystore`, `*.jks`, `*.p12`, `*.p8`, `*.mobileprovision`, `*.pem`, `*.key`.
- Docker volumes, by identifier, in every Docker action.

## Review-only — never `likely_safe`, never preselected, user must select each item individually
- iPhone and iPad backups (`~/Library/Application Support/MobileSync/Backup/*`).
- Messages attachments (`~/Library/Messages/Attachments/**`) — shown and opened in Finder only; the app does not delete them in v1.
- Any file directly in Downloads, Desktop or Documents, including large files and duplicates.
- App leftovers in `~/Library/Containers/*` and `~/Library/Application Support/*` for apps not currently installed.
- Android AVDs (`~/.android/avd/**`) and emulator snapshots; Xcode archives (`~/Library/Developer/Xcode/Archives/**`).
- Any `.env` / `.env.*` file.
- AI-tool session history: `~/.claude/projects/**`, `~/.codex/sessions/**`, Cursor chat databases.
- Final Cut (`*.fcpbundle`), Logic, GarageBand and Adobe project libraries (caches inside them may be `likely_safe` only via a specific rule).

## Validation order for every path (plan time and execute time)
1. Path is absolute and canonical (no `..`, no trailing tricks).
2. Path starts with the home path.
3. No component of the path, or any parent up to home, is a symlink (`symlink_metadata`, never `metadata`).
4. Path is on the same volume (`st_dev`) as the scan root.
5. Path is not hard-blocked.
6. If path is review-only, the plan item must have been individually selected by the user.
7. At execute time only: the item's size and modification time still match the plan (within 1%); otherwise refuse with "changed since review".

Changes to this file need Sam's sign-off. Removing anything also needs a passing test proving the change is safe.
