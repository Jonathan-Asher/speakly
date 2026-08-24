#!/usr/bin/env bash
# Build the ScreenCaptureKit audio sidecar and stage it where the Tauri app
# expects it. Run this before `tauri build` / `tauri dev` when the sidecar
# changed (it is NOT wired into beforeBuildCommand yet — coordinator decision).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG="$ROOT/sidecars/speakly-syscap"
DEST_DIR="$ROOT/src-tauri/binaries"
DEST="$DEST_DIR/speakly-syscap-aarch64-apple-darwin"

swift build --package-path "$PKG" -c release
mkdir -p "$DEST_DIR"
cp "$PKG/.build/release/speakly-syscap" "$DEST"
echo "sidecar staged at $DEST"
