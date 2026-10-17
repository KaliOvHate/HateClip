#!/usr/bin/env bash
# Build HateClip.app and HateClip.dmg on a Mac.
#
#   scripts/build-dmg.sh            universal build (Apple Silicon and Intel)
#   scripts/build-dmg.sh --native   build for this Mac's architecture only (faster)
#
# Signing: without APPLE_SIGNING_IDENTITY the app is ad-hoc signed, which runs on
# this Mac. Set APPLE_SIGNING_IDENTITY, APPLE_API_KEY, APPLE_API_ISSUER and
# APPLE_API_KEY_PATH to sign with a Developer ID and notarize.
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname)" != "Darwin" ]]; then
  echo "This script must run on macOS." >&2
  exit 1
fi
if ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode Command Line Tools are missing. Run: xcode-select --install" >&2
  exit 1
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust is missing. Install it from https://rustup.rs" >&2
  exit 1
fi

TAURI_CLI_VERSION="2.12.3"
if ! cargo tauri --version 2>/dev/null | grep -q "$TAURI_CLI_VERSION"; then
  echo "Installing tauri-cli $TAURI_CLI_VERSION (one time)..."
  cargo install tauri-cli --version "$TAURI_CLI_VERSION" --locked
fi

if [[ "${1:-}" == "--native" ]]; then
  cargo tauri build --config app/tauri.conf.json
  OUT="target/release/bundle"
else
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo tauri build --target universal-apple-darwin --config app/tauri.conf.json
  OUT="target/universal-apple-darwin/release/bundle"
fi

echo
echo "Built:"
ls -1 "$OUT"/dmg/*.dmg "$OUT"/macos/*.app 2>/dev/null
