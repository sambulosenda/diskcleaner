# Releasing Roomy

How to build a signed, notarised Roomy `.dmg` and publish it as a Sparkle update (task T13).

Releases are built on Sam's Mac with `scripts/release.sh`. Every credential stays in Sam's login keychain:
the script, the repo and CI never see a password or private key. Nothing is stored as a GitHub secret.

- [One-time setup](#one-time-setup)
- [Each release](#each-release)
- [Test an update from a local appcast](#test-an-update-from-a-local-appcast)
- [How it fits together](#how-it-fits-together)
- [Entitlements](#entitlements)
- [If something goes wrong](#if-something-goes-wrong)

## One-time setup

Do these once on the build Mac, in Terminal, from `~/Developer/diskcleaner`.

### 1. Find your team id

Your team id is the 10-character code on <https://developer.apple.com/account> under **Membership details**.
It is not a secret. Add it to your shell so every release picks it up:

```sh
echo 'export ROOMY_TEAM_ID=YOURTEAMID' >> ~/.zshrc && source ~/.zshrc
```

### 2. Install a Developer ID Application certificate

In Xcode: **Settings → Accounts →** select your Apple ID **→ Manage Certificates… → + → Developer ID Application**.
Xcode creates the certificate and puts it, with its private key, in your login keychain.

Check it is there (it should print exactly one line ending in `(YOURTEAMID)`):

```sh
security find-identity -v -p codesigning | grep "Developer ID Application"
```

Back up the certificate with its private key: in Keychain Access, right-click it → **Export…** → save the `.p12`
with a strong password into your password manager. Never put the `.p12` in the repo (`.gitignore` blocks `*.p12`).

### 3. Store notarisation credentials in the keychain

1. Go to <https://account.apple.com> → **Sign-In and Security → App-Specific Passwords** and create one called
   "Roomy notarytool". Keep the page open; you will paste the password into Terminal, never into chat.
2. Run this. It asks for the app-specific password and saves everything in the keychain as a profile called
   `roomy-notary`:

   ```sh
   xcrun notarytool store-credentials roomy-notary --apple-id YOUR_APPLE_ID_EMAIL --team-id "$ROOMY_TEAM_ID"
   ```

3. Check it works (prints your notarisation history, empty at first):

   ```sh
   xcrun notarytool history --keychain-profile roomy-notary
   ```

An App Store Connect API key works too, if you prefer one:
`xcrun notarytool store-credentials roomy-notary --key /path/to/AuthKey_XXXX.p8 --key-id XXXX --issuer <issuer-uuid>`.
Keep the `.p8` outside the repo (`.gitignore` blocks `*.p8`).

### 4. Create the Sparkle update key

Sparkle checks every update against an EdDSA key. The private key lives in your keychain; the public key is
built into each release of the app by `release.sh`.

1. Fetch Sparkle's tools (the version pinned in the project):

   ```sh
   xcodebuild -resolvePackageDependencies -project app/Roomy.xcodeproj -clonedSourcePackagesDirPath build/SourcePackages
   ```

2. Create the key. It is saved in the login keychain, and the public key is printed:

   ```sh
   build/SourcePackages/artifacts/sparkle/Sparkle/bin/generate_keys
   ```

3. **Back up the private key.** If it is lost, people with Roomy installed can never be sent an update again.
   This writes it to a file on your Desktop; move the file into your password manager, then delete it from the
   Desktop and empty the Bin:

   ```sh
   build/SourcePackages/artifacts/sparkle/Sparkle/bin/generate_keys -x ~/Desktop/roomy-sparkle-private-key.txt
   ```

   Never commit this file, never paste it anywhere, and never share it in chat.

Run `generate_keys` only once. Running it again on another Mac makes a different key that existing apps reject.
To build on a new Mac, import the backup instead: `generate_keys -f <backup file>`.

## Each release

1. Pick the version (`1.2.3`) and a build number one higher than the last release. Sparkle uses the build number
   to tell which version is newer, so it must always go up.
2. Start from a clean, up-to-date `main` (the script refuses to run with uncommitted changes):

   ```sh
   git switch main && git pull
   ```

3. Build, sign, notarise and staple. This takes about 10 to 20 minutes, most of it waiting for Apple:

   ```sh
   ./scripts/release.sh 0.1.0 1
   ```

   The output goes to `build/release/0.1.0-1/`: `Roomy-0.1.0.dmg`, `appcast.xml`, `Roomy.xcarchive` and `logs/`.
   Keep the `.xcarchive`: it has the debug symbols for reading crash reports.

4. Test the `.dmg` on a second Mac (see the next section for what "passes" means). Send it by AirDrop or
   download it from a draft release, so it arrives quarantined the way a real download does.
5. Publish it as a GitHub release, uploading the `.dmg` and the `appcast.xml` together:

   ```sh
   gh release create v0.1.0 build/release/0.1.0-1/Roomy-0.1.0.dmg build/release/0.1.0-1/appcast.xml \
       --title "Roomy 0.1.0" --notes "What changed, in plain words."
   ```

   The app's feed URL is `https://github.com/sambulosenda/diskcleaner/releases/latest/download/appcast.xml`,
   so the newest published (not draft, not pre-release) release is the update everyone sees.
6. Put the development build of the core back before running Xcode tests again:

   ```sh
   ./scripts/build-ffi.sh
   ```

### What the second-Mac check should show

- Double-clicking the `.dmg` opens it with no warning.
- After dragging Roomy to Applications and opening it, macOS shows at most the usual
  "Roomy is an app downloaded from the Internet. Are you sure you want to open it?" with an **Open** button.
  That is normal for every notarised app. A message saying Apple **cannot check it for malicious software**,
  or that it **cannot be opened**, means notarisation failed. Do not publish.
- In Terminal on that Mac, `spctl --assess --verbose=2 /Applications/Roomy.app` prints
  `accepted` and `source=Notarized Developer ID`.

## Test an update from a local appcast

This shows Sparkle finding and installing an update, without publishing anything.

1. Build and install an older version. Open the `.dmg` from a release (for example `0.1.0`, build `1`), drag
   Roomy to Applications, open it once, then quit it. Sparkle cannot update an app running from the `.dmg`.
2. Build a newer version whose download link points at this Mac:

   ```sh
   ROOMY_DOWNLOAD_URL_PREFIX=http://127.0.0.1:8000/ ./scripts/release.sh 0.1.1 2
   ```

3. Serve that folder on this Mac only (leave it running):

   ```sh
   cd build/release/0.1.1-2 && python3 -m http.server 8000 --bind 127.0.0.1
   ```

4. In a second Terminal window, open the installed app pointed at the local feed:

   ```sh
   open /Applications/Roomy.app --args -RoomyTestFeedURL http://127.0.0.1:8000/appcast.xml
   ```

5. Choose **Roomy → Check for Updates…**. Sparkle should offer version 0.1.1. Install it; Roomy relaunches,
   and **Roomy → About Roomy** shows 0.1.1 (2).

`-RoomyTestFeedURL` only accepts addresses on this Mac (`127.0.0.1`, `::1`, `localhost`). The update must still
be signed with your Sparkle key and your Developer ID, so the switch cannot be used to install anything else.

## How it fits together

| Piece | Where | Notes |
|---|---|---|
| Sparkle 2.9.6 | Swift package in `app/Roomy.xcodeproj`, pinned exactly in `Package.resolved` | The only new dependency |
| Updater | `app/Roomy/UpdaterController.swift` | Starts Sparkle only if the build has a feed URL and public key |
| Menu item | `app/Roomy/RoomyApp.swift` | **Roomy → Check for Updates…**, disabled when updates are off |
| Feed URL and public key | `app/Roomy/Info.plist`, from the build settings `ROOMY_SPARKLE_FEED_URL` and `ROOMY_SPARKLE_PUBLIC_KEY` | The key is empty in the project, so development and CI builds have updates off |
| Entitlements | `app/Roomy/Roomy.entitlements` | Empty; see below |
| Release build | `scripts/release.sh` | Passes the public key from the keychain into the build |
| Secret check | `scripts/check-no-secrets.sh` | Run before every PR that touches release files |

What `release.sh` does, in order, stopping at the first failure:

1. Checks the Mac, the arguments, a clean git tree, the Developer ID certificate and the notarytool profile.
2. Fetches Sparkle at the pinned version and reads the public key from the keychain.
3. Builds the Rust core with `build-ffi.sh --release-app` and refuses to go on if the test-only commands
   (`withHomeForTesting`, `makeFixtureHome`) are still in the bindings.
4. Archives a universal (Apple silicon and Intel) Release build with the hardened runtime.
5. Exports it with Developer ID signing. Xcode signs the app and Sparkle's helper programs inside it.
6. Checks the result: valid signature, your team, Developer ID, hardened runtime, secure timestamp, Sparkle
   signed by your team, no forbidden entitlements, both architectures, the right version and build, and a
   public key that matches the keychain.
7. Notarises the app, staples the ticket, and checks it with Gatekeeper (`spctl`).
8. Builds the `.dmg` (Roomy plus an Applications shortcut), signs it, notarises it, staples it, checks it.
9. Signs the `.dmg` with the Sparkle key, verifies that signature, and writes `appcast.xml`.

## Entitlements

Roomy uses the hardened runtime with **no** App Sandbox and **no** entitlements. Each one we might be asked to add,
and why it is not there:

| Entitlement | Present? | Why |
|---|---|---|
| `com.apple.security.app-sandbox` | No | The PRD needs Roomy to read the whole home folder, which the sandbox prevents. Not distributing through the Mac App Store. |
| `com.apple.security.cs.allow-jit`, `allow-unsigned-executable-memory` | No | Roomy does not generate code at run time. The Rust core is compiled ahead of time and linked statically. |
| `com.apple.security.cs.disable-library-validation` | No | The only embedded framework, Sparkle, is re-signed with your Developer ID, so it passes library validation. |
| `com.apple.security.cs.allow-dyld-environment-variables` | No | Not needed, and allowing it would let other programs inject code. |
| `com.apple.security.get-task-allow` | No | Debugging only. Xcode adds it to Debug builds; `release.sh` fails if a release has it. |
| Full Disk Access | Not an entitlement | The user grants it in System Settings → Privacy & Security. |
| Network | Not an entitlement | Outside the sandbox, Sparkle can reach the feed without one. |

Adding an entitlement needs a reason in the PR and Sam's review.

## If something goes wrong

- **"expected exactly one 'Developer ID Application' certificate"**: none or several are installed for your team.
  Check with the `security find-identity` command in step 2; delete expired duplicates in Keychain Access.
- **"notarytool cannot use the keychain profile"**: redo one-time setup step 3. App-specific passwords stop working
  if you change your Apple ID password.
- **"Apple did not accept app"**: open `build/release/<version>-<build>/logs/app-log.json`. It lists each file Apple
  rejected and why.
- **"no Sparkle key in the keychain"**: redo one-time setup step 4, or import your backup with `generate_keys -f`.
- **Xcode tests fail with missing `withHomeForTesting`** after a release: run `./scripts/build-ffi.sh`.
- **`build/release/<version>-<build>` already exists**: the script never overwrites a release. Move the folder
  aside or use the next build number.
