#!/usr/bin/env bash
# Builds Rune and installs it for the current user (binary, launcher entry, icon).
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release
install -Dm755 target/release/rune-desktop "$HOME/.local/bin/rune-desktop"
install -Dm644 assets/rune.desktop "$HOME/.local/share/applications/rune.desktop"
install -Dm644 assets/logo.svg "$HOME/.local/share/icons/hicolor/scalable/apps/rune.svg"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Installed. Launch Rune from the application menu or run: rune-desktop"
