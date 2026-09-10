#!/bin/bash
# One-command update, to be run in Terminal ON the Mac — not over SSH.
#
# codesign needs GUI access to the login keychain, so a remote build can only
# sign ad-hoc. An ad-hoc signature changes with every build, and macOS ties
# Keychain items, Accessibility and Microphone permission to the signature —
# which is why a remotely-deployed build re-asks for all three every time.
# Building here signs with the stable identity, so those grants stick.
set -euo pipefail
cd "$(dirname "$0")/.."
git pull --ff-only
exec bash scripts/build-local.sh
