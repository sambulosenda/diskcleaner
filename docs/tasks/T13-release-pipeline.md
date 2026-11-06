# T13 — Signing, notarisation and Sparkle pipeline

**Sam handles all credentials. The agent writes scripts and docs only; it never sees certificate passwords or API keys as plain text.**

**Do**
- `scripts/release.sh`: build universal app → codesign with Developer ID (hardened runtime, entitlements file committed) → `xcrun notarytool submit --keychain-profile <profile> --wait` → `xcrun stapler staple` → build a signed `.dmg` → generate the Sparkle appcast entry (EdDSA signature via Sparkle's `sign_update`).
- Integrate Sparkle 2 into the app with an "Check for Updates…" menu item; feed URL from `Info.plist`.
- `docs/release.md`: one-time setup Sam must do (create the notarytool keychain profile, generate Sparkle keys, store the private key outside the repo), then the per-release steps.
- Entitlements: no App Sandbox; only what is needed. List and justify each in the PR.

**Done when**
- Sam runs `scripts/release.sh` and gets a notarised, stapled `.dmg` that opens on a second Mac with no Gatekeeper warning.
- Sparkle detects a test update from a local appcast.
- No secrets committed (CI secret scan or `git grep` check in the PR).
