#!/usr/bin/env bash
# setup-signing-identity.sh
#
# Create a self-signed code-signing identity "Sledge Local Signing" in the
# user's login keychain, if one does not already exist. This identity is
# used by scripts/bundle-macos.sh to produce reproducibly-signed bundles
# whose TCC (Accessibility / Input Monitoring) grants survive rebuilds.
#
# Why a self-signed identity instead of ad-hoc?
#   Ad-hoc signatures have no certificate chain. macOS's TCC system keys
#   grants on a "designated requirement" derived from the signature; for
#   ad-hoc this reduces to bundle identifier + CDHash. Every rebuild
#   rotates the CDHash, so TCC silently revokes grants across rebuilds.
#
#   A self-signed certificate that is re-used across rebuilds produces a
#   designated requirement of the form:
#       identifier "com.braden.sledge" and certificate leaf H"<stable hash>"
#   The certificate hash is stable across rebuilds (same cert each time),
#   so TCC grants persist.
#
# This script is idempotent. Re-running after the identity exists is a
# zero-cost no-op.
#
# No manual keychain UI interaction is required: we use the `security`
# CLI to import a PKCS#12 blob generated non-interactively via `openssl`.
# The login keychain is expected to be unlocked (it is during a normal
# user session after login).

set -euo pipefail

# Ensure a deterministic PATH. This script may be invoked from
# constrained environments (e.g. nix home-manager activation) where
# PATH does not include /usr/bin. Every external tool we use (awk,
# mktemp, security, openssl, codesign) lives under /usr/bin or /bin
# on macOS, so pinning the PATH to those locations is both sufficient
# and the most defensive choice.
export PATH="/usr/bin:/bin"

IDENTITY_NAME="Sledge Local Signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if [[ ! -f "$KEYCHAIN" ]]; then
  echo "login keychain not found at $KEYCHAIN" >&2
  exit 1
fi

# If an identity with this name is already present AND trusted for code
# signing, we can skip the entire setup flow. Print its SHA-1 so callers
# that need the hash (e.g. bundle-macos.sh) can read it.
EXISTING="$(/usr/bin/security find-identity -v -p codesigning "$KEYCHAIN" 2>/dev/null \
  | awk -v n="\"$IDENTITY_NAME\"" '
      index($0, n) {
        for (i = 1; i <= NF; i++) {
          if ($i ~ /^[0-9A-F]{40}$/) { print $i; exit }
        }
      }
  ')"
if [[ -n "$EXISTING" ]]; then
  echo "==> Signing identity \"$IDENTITY_NAME\" already present and trusted"
  echo "==> SHA-1: $EXISTING"
  exit 0
fi

# Purge ALL certificates with this CN, handling the common case of
# multiple orphan entries left by earlier failed imports. We iterate by
# SHA-1 hash (which is always unique per cert) because `delete-certificate
# -c` removes only the first match.
while true; do
  HASHES="$(/usr/bin/security find-certificate -c "$IDENTITY_NAME" -Z -a "$KEYCHAIN" 2>/dev/null \
    | awk '/^SHA-1 hash:/ { print $NF }')"
  if [[ -z "$HASHES" ]]; then
    break
  fi
  PROGRESS=0
  while IFS= read -r h; do
    if /usr/bin/security delete-certificate -Z "$h" "$KEYCHAIN" >/dev/null 2>&1; then
      PROGRESS=1
    fi
  done <<< "$HASHES"
  if [[ "$PROGRESS" -eq 0 ]]; then
    # No cert was successfully deleted on this pass; bail to avoid
    # spinning forever if delete-certificate fails for every hash.
    break
  fi
done

echo "==> Creating self-signed code-signing identity \"$IDENTITY_NAME\""

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

KEY_FILE="$TMPDIR/key.pem"
CERT_FILE="$TMPDIR/cert.pem"
P12_FILE="$TMPDIR/id.p12"

# Random transport password. macOS `security import` on recent versions
# rejects empty passwords for PKCS#12 imports, so we always supply one.
# The password only protects the P12 file in transit from openssl to
# security; once imported, the key is protected by the keychain's ACL.
P12_PASSWORD="$(/usr/bin/openssl rand -hex 16)"

OPENSSL=/usr/bin/openssl

# 10-year self-signed cert with the Code Signing extended key usage.
"$OPENSSL" req -x509 -newkey rsa:4096 \
  -keyout "$KEY_FILE" -out "$CERT_FILE" \
  -days 3650 -nodes \
  -subj "/CN=$IDENTITY_NAME" \
  -addext "basicConstraints=CA:FALSE" \
  -addext "keyUsage=digitalSignature" \
  -addext "extendedKeyUsage=codeSigning" \
  >/dev/null 2>&1

# Bundle the cert + key into a PKCS#12 for import. `security import`
# pairs them into an identity automatically when given a P12.
"$OPENSSL" pkcs12 -export \
  -out "$P12_FILE" \
  -inkey "$KEY_FILE" \
  -in "$CERT_FILE" \
  -name "$IDENTITY_NAME" \
  -passout "pass:$P12_PASSWORD" \
  >/dev/null 2>&1

# Import the P12.
#
# `-A` grants ANY application access to the key. This avoids macOS's
# keychain partition-list mechanism, which would otherwise force codesign
# to show an interactive "allow access?" prompt on each use.
# `set-key-partition-list` (the proper way to authorise specific tools)
# requires the keychain unlock password, which we can't supply non-
# interactively. `-A` is a well-understood trade-off on a personal
# developer machine: any process running as this user could already
# read the key via other means, and the TCC grants that the cert
# protects are themselves scoped to the specific bundle path.
/usr/bin/security import "$P12_FILE" \
  -k "$KEYCHAIN" \
  -P "$P12_PASSWORD" \
  -A \
  >/dev/null

# Trust the cert for code signing as a user-level trust setting. Without
# this, `find-identity -p codesigning` reports the identity as
# `CSSMERR_TP_NOT_TRUSTED` and codesign refuses to use it.
#
# `-r trustRoot` marks the cert as a trusted root for our user only
# (omitting `-d` keeps it user-level, not system-level, so no sudo).
# `-p codeSign` scopes the trust to code signing only.
/usr/bin/security add-trusted-cert \
  -r trustRoot \
  -p codeSign \
  -k "$KEYCHAIN" \
  "$CERT_FILE" \
  >/dev/null 2>&1

# Print the SHA-1 hash so bundle-macos.sh (or a caller) can unambiguously
# reference this specific identity when there are other certs with the
# same CN in the keychain.
HASH="$(/usr/bin/security find-identity -v -p codesigning "$KEYCHAIN" \
  | awk -v n="\"$IDENTITY_NAME\"" '
      index($0, n) {
        for (i = 1; i <= NF; i++) {
          if ($i ~ /^[0-9A-F]{40}$/) { print $i; exit }
        }
      }
  ')"

if [[ -z "$HASH" ]]; then
  echo "failed: identity was imported but is not listed as a valid code-signing identity" >&2
  exit 1
fi

echo "==> Created identity"
echo "==> SHA-1: $HASH"
