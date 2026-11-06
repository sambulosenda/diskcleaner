#!/usr/bin/env bash
# Checks that no signing or notarisation secret is committed (task T13).
#
# Looks at every file git tracks for:
#   - certificate, key and profile files (.p12, .p8, .cer, .mobileprovision, ...),
#   - PEM private keys,
#   - Apple app-specific passwords (xxxx-xxxx-xxxx-xxxx),
#   - long base64 blobs, such as an exported Sparkle EdDSA private key.
#
# Exit code 0 means nothing was found. Run from anywhere: ./scripts/check-no-secrets.sh
# It only reads files; it changes nothing.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

found=0

report() {
    echo "check-no-secrets: $1" >&2
    found=1
}

echo "==> Looking for key, certificate and profile files"
bad_files="$(git ls-files | grep -i -E '\.(p12|pfx|p8|cer|der|pem|key|keychain|keychain-db|mobileprovision|provisionprofile)$|(^|/)AuthKey_[A-Z0-9]+\.p8$|sparkle.*private|private.*key' || true)"
if [[ -n "$bad_files" ]]; then
    report "these tracked files look like keys or certificates:"
    echo "$bad_files" >&2
fi

echo "==> Looking for private keys, app-specific passwords and long base64 blobs"
PATTERNS=(
    -e '-----BEGIN ([A-Z]+ )?PRIVATE KEY-----'
    -e '\b[a-z]{4}-[a-z]{4}-[a-z]{4}-[a-z]{4}\b'
    -e '[A-Za-z0-9+/]{80,}={0,2}'
)
if matches="$(git grep -I -n -E "${PATTERNS[@]}" -- . ':(exclude)scripts/check-no-secrets.sh')"; then
    report "these lines look like secrets:"
    echo "$matches" | cut -c1-200 >&2
fi

if [[ "$found" -ne 0 ]]; then
    echo "check-no-secrets: FAILED. Remove the secret, rotate it, and never commit it again." >&2
    exit 1
fi
echo "check-no-secrets: nothing found."
