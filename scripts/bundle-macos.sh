#!/usr/bin/env bash
# bundle-macos.sh [--no-sign] BINARY OUTDIR
#
# Wrap a sledge binary into a code-signed .app bundle at OUTDIR/Sledge.app.
# The bundle uses a stable CFBundleIdentifier (com.braden.sledge) and is
# signed with a self-signed "Sledge Local Signing" identity from the login
# keychain. That identity is created on demand by
# scripts/setup-signing-identity.sh, which this script invokes.
#
# Signing with a stable self-signed identity (rather than ad-hoc) means
# TCC grants survive rebuilds: the bundle's designated requirement resolves
# to `identifier "com.braden.sledge" and certificate leaf H"<cert hash>"`,
# and the certificate hash is stable across rebuilds.
#
# Usage:
#   ./scripts/bundle-macos.sh target/release/sledge ./dist
#   ./scripts/bundle-macos.sh --no-sign target/release/sledge ./dist
#
# The `--no-sign` flag produces the bundle tree WITHOUT running the
# signing-identity setup or codesign steps. This is intended for build
# systems (e.g. the nix build sandbox) that cannot access the login
# keychain; signing is deferred to a later stage.

set -euo pipefail

# Ensure a deterministic PATH. This script may be invoked from
# constrained environments (e.g. a nix build sandbox or a minimal
# PATH set by a calling script) where /usr/bin is not on PATH. The
# external tools we use (awk, sed, mktemp, xattr, codesign, etc.)
# all live under /usr/bin or /bin on macOS, so pinning the PATH to
# those locations is both sufficient and the most defensive choice.
export PATH="/usr/bin:/bin"

SIGN=true
if [[ "${1:-}" == "--no-sign" ]]; then
  SIGN=false
  shift
fi

BINARY="${1:?usage: bundle-macos.sh [--no-sign] BINARY OUTDIR}"
OUTDIR="${2:?usage: bundle-macos.sh [--no-sign] BINARY OUTDIR}"

if [[ ! -x "$BINARY" ]]; then
  echo "binary not found or not executable: $BINARY" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="$REPO_ROOT/packaging/macos/Info.plist.template"

# Extract the workspace version from the [workspace.package] section of
# Cargo.toml. The key may be indented or aligned with extra whitespace.
VERSION="$(awk '
  /^\[workspace\.package\]/ { p = 1; next }
  /^\[/ { p = 0 }
  p && /^version[[:space:]]*=/ {
    match($0, /"[^"]+"/)
    print substr($0, RSTART + 1, RLENGTH - 2)
    exit
  }
' "$REPO_ROOT/Cargo.toml")"

if [[ -z "$VERSION" ]]; then
  echo "failed to extract version from Cargo.toml" >&2
  exit 1
fi

APP="$OUTDIR/Sledge.app"
MACOS="$APP/Contents/MacOS"
RESOURCES="$APP/Contents/Resources"

IDENTITY_NAME="Sledge Local Signing"
IDENTITY_HASH=""

if [[ "$SIGN" == "true" ]]; then
  # Ensure the self-signed signing identity exists in the login keychain
  # and capture its SHA-1 hash. Idempotent: a no-op if the identity is
  # already present and trusted. Otherwise creates the identity and
  # trusts it for code signing (may prompt for keychain password on
  # first run to authorise the trust-settings change).
  SETUP_OUTPUT="$("$REPO_ROOT/scripts/setup-signing-identity.sh")"
  printf '%s\n' "$SETUP_OUTPUT"

  IDENTITY_HASH="$(printf '%s\n' "$SETUP_OUTPUT" \
    | awk '/^==> SHA-1:/ { print $NF; exit }')"

  if [[ -z "$IDENTITY_HASH" ]]; then
    echo "failed: setup-signing-identity.sh did not report a SHA-1" >&2
    exit 1
  fi
fi

echo "==> Creating bundle at $APP (version $VERSION)"
rm -rf "$APP"
mkdir -p "$MACOS" "$RESOURCES"

cp "$BINARY" "$MACOS/sledge"
chmod +x "$MACOS/sledge"

# Substitute version into Info.plist.
sed "s/__VERSION__/$VERSION/g" "$TEMPLATE" > "$APP/Contents/Info.plist"

# Remove any extended attributes that would invalidate a signature.
# Always safe; harmless when --no-sign is set.
xattr -cr "$APP" || true

if [[ "$SIGN" == "false" ]]; then
  echo "==> Skipping codesign (--no-sign)"
  echo "==> Bundle ready (unsigned): $APP"
  exit 0
fi

echo "==> Codesigning with \"$IDENTITY_NAME\" ($IDENTITY_HASH)"
# We sign by the identity's SHA-1 hash rather than its CN so the result
# is unambiguous even if multiple certs with the same CN exist in the
# keychain (e.g. leftover entries from prior experiments).
#
# We deliberately do NOT pass `--options runtime`. Hardened runtime
# enables library validation, which refuses to load dylibs whose Team ID
# differs from the main binary. Our main binary's certificate has no
# Apple Team ID but depends on nix-store dylibs (e.g. libiconv) that
# carry nix's signature — under hardened runtime dyld aborts the process.
# Using a stable self-signed cert without hardened runtime is sufficient
# for TCC (Input Monitoring / Accessibility) stability across rebuilds:
# the designated requirement includes the certificate leaf hash, which
# stays the same as long as we re-use the same identity in the login
# keychain.
/usr/bin/codesign \
  --sign "$IDENTITY_HASH" \
  --identifier com.braden.sledge \
  --force \
  --deep \
  --timestamp=none \
  "$APP"

echo "==> Verifying signature"
/usr/bin/codesign --verify --verbose=2 "$APP"

echo "==> Bundle ready: $APP"
echo
echo "First-run checklist:"
echo "  1. open '$APP'  (macOS will prompt for Accessibility + Input Monitoring)"
echo "  2. Grant both permissions under System Settings > Privacy & Security"
echo "  3. Copy config.example.toml to ~/.config/sledge/config.toml"
echo "  4. launchctl bootstrap gui/\$(id -u) ~/Library/LaunchAgents/com.braden.sledge.plist"
