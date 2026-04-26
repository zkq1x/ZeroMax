#!/bin/bash
set -euo pipefail

# Build ZeroMax FFI for macOS and generate Swift bindings.
#
# Usage: ./build-rust.sh [--release]

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FFI_CRATE="$REPO_ROOT/zeromax-ffi"
SWIFT_PKG="$SCRIPT_DIR/ZeroMax"

BUILD_TYPE="debug"
CARGO_FLAGS=""
if [[ "${1:-}" == "--release" ]]; then
    BUILD_TYPE="release"
    CARGO_FLAGS="--release"
fi

echo "==> Building zeromax-ffi ($BUILD_TYPE) for aarch64-apple-darwin..."
cd "$REPO_ROOT"
cargo build -p zeromax-ffi $CARGO_FLAGS --target aarch64-apple-darwin

LIB_PATH="$REPO_ROOT/target/aarch64-apple-darwin/$BUILD_TYPE/libzeromax_ffi.a"
if [ ! -f "$LIB_PATH" ]; then
    echo "ERROR: Library not found at $LIB_PATH"
    exit 1
fi
echo "    Static lib: $LIB_PATH ($(du -h "$LIB_PATH" | cut -f1))"

echo "==> Generating Swift bindings..."
cargo run -p zeromax-ffi --bin uniffi-bindgen $CARGO_FLAGS -- \
    generate "$FFI_CRATE/src/zeromax_ffi.udl" \
    --language swift \
    --out-dir /tmp/zeromax-ffi-bindings

# Copy generated Swift file into the package sources.
cp /tmp/zeromax-ffi-bindings/zeromax_ffi.swift "$SWIFT_PKG/Sources/ZeroMax/zeromax_ffi.swift"

# Copy C header.
mkdir -p "$SWIFT_PKG/Sources/ZeroMaxFFI/include"
cp /tmp/zeromax-ffi-bindings/zeromax_ffiFFI.h "$SWIFT_PKG/Sources/ZeroMaxFFI/include/"

rm -rf /tmp/zeromax-ffi-bindings

echo ""
echo "==> Done!"
echo "    To build the Swift app:"
echo "    cd apps/macos/ZeroMax && swift build"
