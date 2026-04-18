#!/usr/bin/env bash
# bundle-macos.sh BINARY OUTDIR
#
# Wrap a sledge binary into a code-signed .app bundle at OUTDIR/Sledge.app.
# The bundle uses a stable CFBundleIdentifier (com.braden.sledge) and is
# ad-hoc codesigned with a stable identifier so TCC preserves Accessibility
# and Input Monitoring grants across rebuilds.
#
# Usage:
#   ./scripts/bundle-macos.sh target/release/sledge ./dist

set -euo pipefail

BINARY="${1:?usage: bundle-macos.sh BINARY OUTDIR}"
OUTDIR="${2:?usage: bundle-macos.sh BINARY OUTDIR}"

if [[ ! -x "$BINARY" ]]; then
  echo "binary not found or not executable: $BINARY" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="$REPO_ROOT/packaging/macos/Info.plist.template"
VERSION="$(awk -F'"' '/^version = / { print $2; exit }' "$REPO_ROOT/Cargo.toml")"

APP="$OUTDIR/Sledge.app"
MACOS="$APP/Contents/MacOS"
RESOURCES="$APP/Contents/Resources"

echo "==> Creating bundle at $APP (version $VERSION)"
rm -rf "$APP"
mkdir -p "$MACOS" "$RESOURCES"

cp "$BINARY" "$MACOS/sledge"
chmod +x "$MACOS/sledge"

# Substitute version into Info.plist.
sed "s/__VERSION__/$VERSION/g" "$TEMPLATE" > "$APP/Contents/Info.plist"

echo "==> Ad-hoc codesigning with stable identifier"
# Remove any extended attributes that would invalidate the signature.
xattr -cr "$APP" || true
# NOTE: We deliberately do NOT pass `--options runtime`. Hardened runtime
# enables library validation, which refuses to load dylibs whose Team ID
# differs from the main binary. Our main binary is ad-hoc signed (no Team
# ID) but depends on nix-store dylibs (e.g. libiconv) that carry nix's
# signature \u2014 under hardened runtime dyld aborts the process. Ad-hoc
# signature without hardened runtime is sufficient for TCC (Input
# Monitoring / Accessibility) stability across rebuilds because those
# grants are keyed off the stable `--identifier` + bundle path.
codesign \
  --sign - \
  --identifier com.braden.sledge \
  --force \
  --deep \
  --timestamp=none \
  "$APP"

echo "==> Verifying signature"
codesign --verify --verbose=2 "$APP"

echo "==> Bundle ready: $APP"
echo
echo "First-run checklist:"
echo "  1. open '$APP'  (macOS will prompt for Accessibility + Input Monitoring)"
echo "  2. Grant both permissions under System Settings > Privacy & Security"
echo "  3. Copy config.example.toml to ~/.config/sledge/config.toml"
echo "  4. launchctl bootstrap gui/\$(id -u) ~/Library/LaunchAgents/com.braden.sledge.plist"
