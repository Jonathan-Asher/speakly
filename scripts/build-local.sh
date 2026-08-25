#!/bin/bash
# Local signed build + install. Signing with a stable identity keeps macOS
# permission grants (mic, accessibility, screen recording) across rebuilds —
# ad-hoc signatures reset TCC every build. CI signs with its own identity.
set -euo pipefail
cd "$(dirname "$0")/.."

IDENTITY="${SPEAKLY_SIGN_IDENTITY:-MacRec Dev}"

# createUpdaterArtifacts signs the update archive with the updater key.
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/speakly-updater.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

APPLE_SIGNING_IDENTITY="$IDENTITY" pnpm tauri build --bundles app

pkill -f "Speakly.app/Contents/MacOS/speakly" 2>/dev/null || true
ditto src-tauri/target/release/bundle/macos/Speakly.app /Applications/Speakly.app
open /Applications/Speakly.app
echo "installed + launched (signed as: $IDENTITY)"
