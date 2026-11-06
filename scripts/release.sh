#!/usr/bin/env bash
# Builds a signed, notarised and stapled Roomy release, and its Sparkle appcast (task T13).
#
# Usage (from the repo root, on the build Mac):
#   ROOMY_TEAM_ID=<your 10-character team id> ./scripts/release.sh <version> <build-number>
#   e.g. ROOMY_TEAM_ID=AB12CD34EF ./scripts/release.sh 0.1.0 1
#
# The build number must go up by at least one for every release; Sparkle compares it to decide
# whether an update is newer.
#
# Settings (environment variables):
#   ROOMY_TEAM_ID              required. Apple Developer team id (not a secret).
#   ROOMY_NOTARY_PROFILE       notarytool keychain profile name. Default: roomy-notary
#   ROOMY_DOWNLOAD_URL_PREFIX  where the .dmg will be downloaded from, ending in "/".
#                              Default: https://github.com/sambulosenda/diskcleaner/releases/download/v<version>/
#
# The script never sees a password or private key. notarytool reads the App Store Connect
# credentials from the keychain profile, codesign uses the Developer ID certificate in the
# keychain, and Sparkle's sign_update reads the EdDSA private key from the keychain.
# One-time setup for all three is in docs/release.md.
#
# Output, in build/release/<version>-<build>/:
#   Roomy-<version>.dmg   signed, notarised and stapled disk image
#   appcast.xml           Sparkle feed with this release as its only item
#   Roomy.xcarchive       the archive (keep it; it holds the dSYMs for crash reports)
#   logs/                 notarisation results
#
# The script only creates files under build/. It does not upload or publish anything.
set -euo pipefail

# ---------- helpers ----------

fail() {
    echo "release.sh: $*" >&2
    exit 1
}

step() {
    echo
    echo "==> $*"
}

# Reads one key from a JSON or plist file. Prints nothing if the key is missing.
plist_value() {
    plutil -extract "$2" raw -o - "$1" 2>/dev/null || true
}

# Submits a file to Apple's notary service, waits, and fails unless it is accepted.
# $1 = file to submit, $2 = name for the log files.
notarise() {
    local file="$1" name="$2"
    local result="$LOG_DIR/$name-submit.json"
    xcrun notarytool submit "$file" \
        --keychain-profile "$NOTARY_PROFILE" \
        --wait \
        --output-format json >"$result" || true
    local status id
    status="$(plist_value "$result" status)"
    id="$(plist_value "$result" id)"
    if [[ -n "$id" ]]; then
        xcrun notarytool log "$id" --keychain-profile "$NOTARY_PROFILE" "$LOG_DIR/$name-log.json" >/dev/null || true
    fi
    if [[ "$status" != "Accepted" ]]; then
        echo "Notary result for $name:" >&2
        cat "$result" >&2 || true
        fail "Apple did not accept $name (status: ${status:-unknown}). Details: $LOG_DIR/$name-log.json"
    fi
    echo "Accepted by Apple (submission $id)."
}

# ---------- arguments and settings ----------

[[ $# -eq 2 ]] || fail "usage: ROOMY_TEAM_ID=<team id> $0 <version, e.g. 0.1.0> <build number, e.g. 1>"
VERSION="$1"
BUILD="$2"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "version must look like 1.2.3, got '$VERSION'"
[[ "$BUILD" =~ ^[1-9][0-9]*$ ]] || fail "build number must be a whole number above 0, got '$BUILD'"

TEAM_ID="${ROOMY_TEAM_ID:-}"
[[ "$TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] || fail "set ROOMY_TEAM_ID to your 10-character Apple team id (see docs/release.md)"
NOTARY_PROFILE="${ROOMY_NOTARY_PROFILE:-roomy-notary}"
URL_PREFIX="${ROOMY_DOWNLOAD_URL_PREFIX:-https://github.com/sambulosenda/diskcleaner/releases/download/v$VERSION/}"
[[ "$URL_PREFIX" == */ ]] || fail "ROOMY_DOWNLOAD_URL_PREFIX must end with /"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="$REPO_ROOT/app/Roomy.xcodeproj"
BUILD_DIR="$REPO_ROOT/build"
PACKAGES_DIR="$BUILD_DIR/SourcePackages"
OUT_DIR="$BUILD_DIR/release/$VERSION-$BUILD"
LOG_DIR="$OUT_DIR/logs"
ARCHIVE="$OUT_DIR/Roomy.xcarchive"
EXPORT_DIR="$OUT_DIR/export"
APP="$EXPORT_DIR/Roomy.app"
DMG_NAME="Roomy-$VERSION.dmg"
DMG="$OUT_DIR/$DMG_NAME"

# ---------- preflight ----------

step "Checking the build Mac"
[[ "$(uname -s)" == "Darwin" ]] || fail "release.sh must run on macOS"
for tool in xcodebuild codesign hdiutil ditto plutil security lipo spctl xmllint git; do
    command -v "$tool" >/dev/null || fail "missing tool: $tool"
done
xcrun --find notarytool >/dev/null || fail "notarytool not found; install Xcode 14 or later"
xcrun --find stapler >/dev/null || fail "stapler not found; install Xcode"

cd "$REPO_ROOT"
if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
    fail "the working tree has uncommitted changes. Release only from a clean, committed checkout."
fi
COMMIT="$(git rev-parse --short HEAD)"
echo "Releasing Roomy $VERSION (build $BUILD) from commit $COMMIT"

[[ ! -e "$OUT_DIR" ]] || fail "$OUT_DIR already exists. Move it aside or use a new build number."
mkdir -p "$LOG_DIR"

step "Finding the Developer ID Application certificate for team $TEAM_ID"
IDENTITIES="$(security find-identity -v -p codesigning | grep "Developer ID Application" | grep "($TEAM_ID)" || true)"
IDENTITY_COUNT="$(printf '%s' "$IDENTITIES" | grep -c . || true)"
[[ "$IDENTITY_COUNT" == "1" ]] || fail "expected exactly one 'Developer ID Application' certificate for team $TEAM_ID in the keychain, found $IDENTITY_COUNT"
IDENTITY_HASH="$(printf '%s' "$IDENTITIES" | awk '{print $2}')"
echo "$IDENTITIES"

step "Checking the notarytool keychain profile '$NOTARY_PROFILE'"
xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null \
    || fail "notarytool cannot use the keychain profile '$NOTARY_PROFILE'. See docs/release.md, one-time setup step 3."

# ---------- Sparkle tools and key ----------

step "Fetching Sparkle at the version pinned in Package.resolved"
xcodebuild -resolvePackageDependencies \
    -project "$PROJECT" \
    -clonedSourcePackagesDirPath "$PACKAGES_DIR" \
    -disableAutomaticPackageResolution >"$LOG_DIR/resolve-packages.log"
SPARKLE_BIN="$PACKAGES_DIR/artifacts/sparkle/Sparkle/bin"
for tool in sign_update generate_keys; do
    [[ -x "$SPARKLE_BIN/$tool" ]] || fail "Sparkle tool not found: $SPARKLE_BIN/$tool"
done

step "Reading the Sparkle public key from the keychain"
SPARKLE_PUBLIC_KEY="$("$SPARKLE_BIN/generate_keys" -p)" \
    || fail "no Sparkle key in the keychain. See docs/release.md, one-time setup step 4."
[[ "$SPARKLE_PUBLIC_KEY" =~ ^[A-Za-z0-9+/]{43}=$ ]] || fail "unexpected Sparkle public key format: '$SPARKLE_PUBLIC_KEY'"
echo "Public key: $SPARKLE_PUBLIC_KEY"

# ---------- build ----------

step "Building the Rust core for release (no test-only commands)"
"$REPO_ROOT/scripts/build-ffi.sh" --release-app
BINDINGS="$REPO_ROOT/app/Generated/roomy_core.swift"
[[ -f "$BINDINGS" ]] || fail "missing $BINDINGS"
if grep -q -E "withHomeForTesting|makeFixtureHome" "$BINDINGS"; then
    fail "the generated bindings still contain test-only commands; refusing to ship them"
fi

step "Archiving the universal app (arm64 + x86_64)"
xcodebuild archive \
    -project "$PROJECT" \
    -scheme Roomy \
    -configuration Release \
    -destination "generic/platform=macOS" \
    -archivePath "$ARCHIVE" \
    -derivedDataPath "$BUILD_DIR/DerivedData-release" \
    -clonedSourcePackagesDirPath "$PACKAGES_DIR" \
    -disableAutomaticPackageResolution \
    ARCHS="arm64 x86_64" \
    ONLY_ACTIVE_ARCH=NO \
    MARKETING_VERSION="$VERSION" \
    CURRENT_PROJECT_VERSION="$BUILD" \
    DEVELOPMENT_TEAM="$TEAM_ID" \
    CODE_SIGN_STYLE=Manual \
    CODE_SIGN_IDENTITY="Developer ID Application" \
    ENABLE_HARDENED_RUNTIME=YES \
    OTHER_CODE_SIGN_FLAGS="--timestamp" \
    ROOMY_SPARKLE_PUBLIC_KEY="$SPARKLE_PUBLIC_KEY" \
    >"$LOG_DIR/archive.log" \
    || fail "xcodebuild archive failed; see $LOG_DIR/archive.log"

step "Signing with Developer ID (Xcode export re-signs the app and Sparkle's helpers)"
EXPORT_OPTIONS="$OUT_DIR/ExportOptions.plist"
cat >"$EXPORT_OPTIONS" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>developer-id</string>
    <key>signingStyle</key>
    <string>manual</string>
    <key>signingCertificate</key>
    <string>Developer ID Application</string>
    <key>teamID</key>
    <string>$TEAM_ID</string>
</dict>
</plist>
EOF
xcodebuild -exportArchive \
    -archivePath "$ARCHIVE" \
    -exportPath "$EXPORT_DIR" \
    -exportOptionsPlist "$EXPORT_OPTIONS" \
    >"$LOG_DIR/export.log" \
    || fail "xcodebuild -exportArchive failed; see $LOG_DIR/export.log"
[[ -d "$APP" ]] || fail "export did not produce $APP"

# ---------- check the app before sending it to Apple ----------

step "Checking the signed app"
codesign --verify --deep --strict --verbose=2 "$APP"

SIGN_INFO="$(codesign --display --verbose=4 "$APP" 2>&1)"
grep -q "^TeamIdentifier=$TEAM_ID$" <<<"$SIGN_INFO" || fail "app is not signed by team $TEAM_ID"
grep -q "^Authority=Developer ID Application" <<<"$SIGN_INFO" || fail "app is not signed with a Developer ID Application certificate"
grep -q -E "flags=0x[0-9a-f]+\(.*runtime.*\)" <<<"$SIGN_INFO" || fail "app is not signed with the hardened runtime"
grep -q "^Timestamp=" <<<"$SIGN_INFO" || fail "app signature has no secure timestamp"

SPARKLE_FRAMEWORK="$APP/Contents/Frameworks/Sparkle.framework"
[[ -d "$SPARKLE_FRAMEWORK" ]] || fail "Sparkle.framework is not embedded in the app"
grep -q "^TeamIdentifier=$TEAM_ID$" <<<"$(codesign --display --verbose=4 "$SPARKLE_FRAMEWORK" 2>&1)" \
    || fail "Sparkle.framework is not signed by team $TEAM_ID"

ENTITLEMENTS="$(codesign --display --entitlements - --xml "$APP" 2>/dev/null || true)"
for forbidden in get-task-allow com.apple.security.app-sandbox com.apple.security.cs.; do
    if grep -q "$forbidden" <<<"$ENTITLEMENTS"; then
        fail "app has an entitlement it must not ship with: $forbidden"
    fi
done

ARCHS_FOUND="$(lipo -archs "$APP/Contents/MacOS/Roomy")"
[[ "$ARCHS_FOUND" == *arm64* && "$ARCHS_FOUND" == *x86_64* ]] || fail "app is not universal (found: $ARCHS_FOUND)"

INFO="$APP/Contents/Info.plist"
[[ "$(plist_value "$INFO" CFBundleShortVersionString)" == "$VERSION" ]] || fail "app version is not $VERSION"
[[ "$(plist_value "$INFO" CFBundleVersion)" == "$BUILD" ]] || fail "app build number is not $BUILD"
[[ "$(plist_value "$INFO" SUPublicEDKey)" == "$SPARKLE_PUBLIC_KEY" ]] || fail "app's SUPublicEDKey does not match the key in the keychain"
FEED_URL="$(plist_value "$INFO" SUFeedURL)"
[[ "$FEED_URL" == https://* ]] || fail "app's SUFeedURL is not https: '$FEED_URL'"
echo "Signed by team $TEAM_ID, hardened runtime, universal ($ARCHS_FOUND), feed $FEED_URL"

# ---------- notarise and staple the app ----------

step "Notarising the app (this usually takes a few minutes)"
APP_ZIP="$OUT_DIR/Roomy-notarise.zip"
ditto -c -k --keepParent "$APP" "$APP_ZIP"
notarise "$APP_ZIP" app

step "Stapling the notarisation ticket to the app"
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=2 "$APP"

# ---------- disk image ----------

step "Building $DMG_NAME"
STAGING="$OUT_DIR/dmg-staging"
mkdir -p "$STAGING"
ditto "$APP" "$STAGING/Roomy.app"
ln -s /Applications "$STAGING/Applications"
hdiutil create \
    -volname "Roomy" \
    -srcfolder "$STAGING" \
    -format UDZO \
    -imagekey zlib-level=9 \
    "$DMG" >"$LOG_DIR/hdiutil.log"

step "Signing the disk image"
codesign --sign "$IDENTITY_HASH" --timestamp "$DMG"
codesign --verify --strict --verbose=2 "$DMG"

step "Notarising the disk image"
notarise "$DMG" dmg

step "Stapling the notarisation ticket to the disk image"
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"

# ---------- Sparkle appcast ----------

step "Signing the disk image for Sparkle (EdDSA)"
# Prints: sparkle:edSignature="..." length="..."
SPARKLE_ATTRS="$("$SPARKLE_BIN/sign_update" "$DMG")"
[[ "$SPARKLE_ATTRS" =~ ^sparkle:edSignature=\"([A-Za-z0-9+/=]+)\"\ length=\"([0-9]+)\"$ ]] \
    || fail "unexpected sign_update output: $SPARKLE_ATTRS"
ED_SIGNATURE="${BASH_REMATCH[1]}"
LENGTH="${BASH_REMATCH[2]}"
"$SPARKLE_BIN/sign_update" --verify "$DMG" "$ED_SIGNATURE" \
    || fail "the EdDSA signature does not verify"

step "Writing appcast.xml"
PUB_DATE="$(LC_ALL=C date -u "+%a, %d %b %Y %H:%M:%S +0000")"
cat >"$OUT_DIR/appcast.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
    <channel>
        <title>Roomy</title>
        <link>https://github.com/sambulosenda/diskcleaner/releases</link>
        <description>New versions of Roomy.</description>
        <language>en</language>
        <item>
            <title>Version $VERSION</title>
            <pubDate>$PUB_DATE</pubDate>
            <sparkle:version>$BUILD</sparkle:version>
            <sparkle:shortVersionString>$VERSION</sparkle:shortVersionString>
            <sparkle:minimumSystemVersion>13.0</sparkle:minimumSystemVersion>
            <enclosure url="$URL_PREFIX$DMG_NAME" length="$LENGTH" type="application/octet-stream" sparkle:edSignature="$ED_SIGNATURE" />
        </item>
    </channel>
</rss>
EOF
xmllint --noout "$OUT_DIR/appcast.xml" || fail "appcast.xml is not valid XML"

# ---------- done ----------

step "Done"
echo "Disk image: $DMG"
echo "Appcast:    $OUT_DIR/appcast.xml"
echo "Archive:    $ARCHIVE (keep it for crash reports)"
echo
echo "Next: test the .dmg on a second Mac, then publish it (docs/release.md, 'Each release')."
echo "Note: app/Generated now holds the release build of the core. Run ./scripts/build-ffi.sh"
echo "before running the Xcode tests again, so the test-only commands are back."
