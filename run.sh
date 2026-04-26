#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"

# Kill previous instance.
pkill -f "\.build/arm64-apple-macosx/debug/ZeroMax" 2>/dev/null || true

echo "==> Building Rust FFI..."
source "$HOME/.cargo/env"
cargo build -p zeromax-ffi --target aarch64-apple-darwin 2>&1 | tail -2

echo "==> Building Swift app..."
cd apps/macos/ZeroMax
swift build 2>&1 | tail -2

echo "==> Launching ZeroMax..."
rm -f /tmp/zeromax-stderr.txt
.build/arm64-apple-macosx/debug/ZeroMax 2>/tmp/zeromax-stderr.txt &
echo "PID: $!"
echo ""
echo "Logs: tail -f /tmp/zeromax-stderr.txt"
