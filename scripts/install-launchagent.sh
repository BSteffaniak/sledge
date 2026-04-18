#!/usr/bin/env bash
# install-launchagent.sh [BUNDLE_PATH]
#
# Install sledge's LaunchAgent to ~/Library/LaunchAgents/com.braden.sledge.plist
# and load it. BUNDLE_PATH defaults to /Applications/Sledge.app.

set -euo pipefail

BUNDLE="${1:-/Applications/Sledge.app}"

if [[ ! -d "$BUNDLE" ]]; then
  echo "bundle not found: $BUNDLE" >&2
  echo "build one with ./scripts/bundle-macos.sh first" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="$REPO_ROOT/packaging/macos/com.braden.sledge.plist.template"
PLIST="$HOME/Library/LaunchAgents/com.braden.sledge.plist"

mkdir -p "$HOME/Library/LaunchAgents"
mkdir -p "$HOME/Library/Logs/sledge"

echo "==> Writing $PLIST"
sed \
  -e "s|__BUNDLE_PATH__|$BUNDLE|g" \
  -e "s|__HOME__|$HOME|g" \
  "$TEMPLATE" > "$PLIST"

UID_=$(id -u)
echo "==> Unloading any previous agent"
launchctl bootout "gui/$UID_/com.braden.sledge" 2>/dev/null || true

echo "==> Loading agent"
launchctl bootstrap "gui/$UID_" "$PLIST"
launchctl kickstart -k "gui/$UID_/com.braden.sledge"

echo "==> Done. Check status with: launchctl print gui/$UID_/com.braden.sledge | head"
