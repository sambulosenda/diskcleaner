#!/usr/bin/env bash
# Builds a fixture home (a fake Mac home folder for tests) in <dir>.
#
#   ./scripts/make-fixtures.sh <dir>
#
# <dir> must be empty or not exist yet. The tool refuses the real home folder.
# The tree is deterministic: building into the same path twice gives the same
# bytes, and <dir>/fixtures-manifest.json lists every path, size and SHA-256.
# The generator lives in core/crates/fixtures; Rust tests use fixtures::home().
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <dir>" >&2
    exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$1"
# Make a relative <dir> relative to where the script was run, not to core/.
if [[ "$TARGET" != /* ]]; then
    TARGET="$PWD/$TARGET"
fi

cargo run --quiet --manifest-path "$REPO_ROOT/core/Cargo.toml" -p fixtures --bin make-fixtures -- "$TARGET"
