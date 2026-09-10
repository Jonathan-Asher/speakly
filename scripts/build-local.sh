#!/bin/bash
# Local signed build + install.
#
# macOS ties Keychain items, Accessibility and Microphone permission to the
# app's code signature, and an ad-hoc signature changes with every build — so
# an unsigned rebuild is treated as a brand new app and re-asks for all of
# them. Signing with the stable Developer ID identity is what makes those
# grants stick.
#
# The identity lives in a dedicated keychain rather than the login keychain, so
# this works over SSH too: the login keychain refuses non-GUI sessions, which
# is why remote builds used to fall back to ad-hoc signing.
set -euo pipefail
cd "$(dirname "$0")/.."

SIGNING_KEYCHAIN="$HOME/.speakly-signing/speakly.keychain-db"
IDENTITY="${SPEAKLY_SIGN_IDENTITY:-Developer ID Application: Jonathan Ashurov (3L92BZK46V)}"

if [ -f "$SIGNING_KEYCHAIN" ]; then
  security unlock-keychain -p speakly "$SIGNING_KEYCHAIN"
  # Keep it on the search list so codesign can find the identity.
  security list-keychains -d user | tr -d '"' | grep -qF "$SIGNING_KEYCHAIN" ||
    security list-keychains -d user -s "$SIGNING_KEYCHAIN" $(security list-keychains -d user | tr -d '"')
else
  echo "warning: $SIGNING_KEYCHAIN missing — falling back to ad-hoc signing," >&2
  echo "         which resets Accessibility/Microphone/Keychain on every build." >&2
  IDENTITY=""
fi

# createUpdaterArtifacts signs the update archive with the updater key.
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/speakly-updater.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

APPLE_SIGNING_IDENTITY="$IDENTITY" pnpm tauri build --bundles app

pkill -f "Speakly.app/Contents/MacOS/speakly" 2>/dev/null || true
ditto src-tauri/target/release/bundle/macos/Speakly.app /Applications/Speakly.app
open /Applications/Speakly.app
echo "installed + launched (signed as: ${IDENTITY:-ad-hoc})"
