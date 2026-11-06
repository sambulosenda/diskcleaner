#!/usr/bin/env bash
# Builds the Rust core as a universal static library and generates the Swift bindings.
#
# Output (all in app/Generated/, which is git-ignored):
#   lib/libroomy_ffi.a        universal (arm64 + x86_64) static library the app links
#   include/roomy_coreFFI.h   C header for the FFI layer
#   include/module.modulemap  Clang module "roomy_coreFFI" that the Swift bindings import
#   roomy_core.swift          Swift bindings, compiled into the Roomy app target
#
# Run from anywhere: ./scripts/build-ffi.sh
# Needs macOS with Xcode command line tools (for lipo) and rustup.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE_DIR="$REPO_ROOT/core"
GEN_DIR="$REPO_ROOT/app/Generated"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
LIB_NAME="libroomy_ffi"

# Match the app's deployment target so the linker does not warn about newer object files.
export MACOSX_DEPLOYMENT_TARGET=13.0

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "build-ffi.sh must run on macOS (it needs lipo and Apple targets)." >&2
    exit 1
fi

echo "==> Adding Rust targets (no-op if already installed)"
rustup target add "${TARGETS[@]}"

cd "$CORE_DIR"
for target in "${TARGETS[@]}"; do
    echo "==> Building ffi for $target"
    cargo build -p ffi --lib --release --target "$target"
done

mkdir -p "$GEN_DIR/lib" "$GEN_DIR/include"

echo "==> Combining into a universal static library"
lipo -create \
    "$CORE_DIR/target/aarch64-apple-darwin/release/$LIB_NAME.a" \
    "$CORE_DIR/target/x86_64-apple-darwin/release/$LIB_NAME.a" \
    -output "$GEN_DIR/lib/$LIB_NAME.a"

echo "==> Generating Swift bindings"
TMP_DIR="$(mktemp -d)"
trap 'rm -r "$TMP_DIR"' EXIT
# Library mode reads the interface the proc macros embedded in the compiled library.
cargo run -q -p ffi --features bindgen --bin uniffi-bindgen -- generate \
    --library "$CORE_DIR/target/aarch64-apple-darwin/release/$LIB_NAME.dylib" \
    --language swift \
    --out-dir "$TMP_DIR"

mv -f "$TMP_DIR/roomy_core.swift" "$GEN_DIR/roomy_core.swift"
mv -f "$TMP_DIR/roomy_coreFFI.h" "$GEN_DIR/include/roomy_coreFFI.h"
# Xcode finds the module through SWIFT_INCLUDE_PATHS, which needs the name module.modulemap.
mv -f "$TMP_DIR/roomy_coreFFI.modulemap" "$GEN_DIR/include/module.modulemap"

echo "==> Done. Open app/Roomy.xcodeproj and build."
