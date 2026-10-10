#!/usr/bin/env bash
# Builds Echo and installs it for the current user (binary, launcher entry, icon).
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release
install -Dm755 target/release/echo-desktop "$HOME/.local/bin/echo-desktop"
install -Dm644 assets/echo.desktop "$HOME/.local/share/applications/echo.desktop"
install -Dm644 assets/logo.svg "$HOME/.local/share/icons/hicolor/scalable/apps/echo.svg"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Installed. Launch Echo from the application menu or run: echo-desktop"
