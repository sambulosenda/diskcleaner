#!/usr/bin/env bash
# T05 benchmark: times the scanner on <dir>, reports peak memory, and compares
# its total with du on the same folder. Read only: nothing is changed on disk
# except the build output in core/target.
#
#   ./scripts/bench-scanner.sh <dir>        e.g. ./scripts/bench-scanner.sh "$HOME"
#
# Targets (PRD F1): 1 TB / 2M files under 60 s, peak RSS under 500 MB,
# total within 1% of du. For a fair timing, run it twice and use the second
# run (the first warms the file system cache for both tools).
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <dir>" >&2
    exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$(cd "$1" && pwd -P)"   # absolute, no symlinks (the scanner refuses a symlinked root)

cargo build --quiet --release --manifest-path "$REPO_ROOT/core/Cargo.toml" -p scanner --example scan_bench
BIN="$REPO_ROOT/core/target/release/examples/scan_bench"

TIME_LOG="$(mktemp)"
SCAN_OUT="$(mktemp)"
trap 'rm -f "$TIME_LOG" "$SCAN_OUT"' EXIT

echo "== scanner"
RSS_BYTES=""
if [[ ! -x /usr/bin/time ]]; then
    echo "(no /usr/bin/time here, so no peak memory figure)"
    "$BIN" "$TARGET" >"$SCAN_OUT"
elif [[ "$(uname)" == "Darwin" ]]; then
    /usr/bin/time -l "$BIN" "$TARGET" >"$SCAN_OUT" 2>"$TIME_LOG"
    RSS_BYTES="$(awk '/maximum resident set size/ {print $1}' "$TIME_LOG")"
else
    /usr/bin/time -v "$BIN" "$TARGET" >"$SCAN_OUT" 2>"$TIME_LOG"
    RSS_BYTES="$(( $(awk -F: '/Maximum resident set size/ {gsub(/ /,"",$2); print $2}' "$TIME_LOG") * 1024 ))"
fi
cat "$SCAN_OUT"
SCAN_KIB="$(awk '/^total_kib:/ {print $2}' "$SCAN_OUT")"
if [[ -n "$RSS_BYTES" ]]; then
    echo "peak_rss_mb:        $(( RSS_BYTES / 1024 / 1024 ))"
fi

echo "== du -skx (same volume only, like the scanner)"
DU_START="$(date +%s)"
DU_KIB="$(du -skx "$TARGET" 2>/dev/null | awk '{print $1}' || true)"
echo "du_kib:             $DU_KIB  ($(( $(date +%s) - DU_START )) s)"

if [[ -n "$DU_KIB" && "$DU_KIB" -gt 0 ]]; then
    awk -v s="$SCAN_KIB" -v d="$DU_KIB" 'BEGIN {
        diff = (s - d) / d * 100; if (diff < 0) diff = -diff;
        printf "difference:         %.3f%%  (%s)\n", diff, (diff <= 1 ? "within 1%" : "OVER 1%");
    }'
fi
