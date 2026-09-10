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
  # Refuse rather than fall back. An ad-hoc build silently resets
  # Accessibility, Microphone and Keychain grants, and re-granting them by hand
  # after every update is exactly what this keychain exists to prevent.
  echo "error: $SIGNING_KEYCHAIN is missing, so this build could only be signed" >&2
  echo "       ad-hoc — which would reset Accessibility, Microphone and Keychain" >&2
  echo "       permissions again. Restore the signing identity before building." >&2
  exit 1
fi

# createUpdaterArtifacts insists on an updater key, so give it the local one.
# Note the .tar.gz it produces is a throwaway: this key is not the one the app
# trusts (that private half lives only in the TAURI_SIGNING_PRIVATE_KEY repo
# secret), so tauri prints a key-mismatch warning here. Harmless — local builds
# install straight into /Applications and never go through the updater. Real
# updates come from the release workflow, which signs with the trusted key.
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/speakly-updater.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

APPLE_SIGNING_IDENTITY="$IDENTITY" pnpm tauri build --bundles app

pkill -f "Speakly.app/Contents/MacOS/speakly" 2>/dev/null || true
ditto src-tauri/target/release/bundle/macos/Speakly.app /Applications/Speakly.app
open /Applications/Speakly.app
echo "installed + launched (signed as: ${IDENTITY:-ad-hoc})"
