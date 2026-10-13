#!/usr/bin/env bash
# Sourced inside the guest after stock compositor/client setup; research ONLY.
# shellcheck disable=SC2154 # Caller supplies child and verified SDK/payload paths.
set -euo pipefail
: "${SLEDGE_DAEMON_PAYLOAD:?explicit verified guest payload}"
: "${SLEDGE_SDK_IDENTITY:?exact stock SDK identity executable}"
mkdir -m 700 /tmp/sledge-daemon-runtime
cp "$SLEDGE_DAEMON_PAYLOAD/daemon-global.toml" /tmp/sledge-daemon-runtime/config.toml
api_hash=$("$SLEDGE_SDK_IDENTITY")
artifact=stock-0.52.2:8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca
export SLEDGE_HYPRLAND_PASSIVE_SOCKET=/tmp/sledge-daemon-runtime/native.sock
"$SLEDGE_DAEMON_PAYLOAD/lib/ld-linux-x86-64.so.2" --library-path "$SLEDGE_DAEMON_PAYLOAD/lib" \
  "$SLEDGE_DAEMON_PAYLOAD/sledge" --config /tmp/sledge-daemon-runtime/config.toml \
  --socket /tmp/sledge-daemon-runtime/health.sock run \
  --linux-hyprland-research-socket "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" \
  --linux-hyprland-research-pid "$child" \
  --linux-hyprland-research-api-hash "$api_hash" \
  --linux-hyprland-research-artifact "$artifact" > /tmp/sledge-daemon-events 2>&1 &
daemon_pid=$!
printf 'DAEMON_RESEARCH_FIXTURE pid=%s capabilities=0 qualified=false\n' "$daemon_pid"
# Caller must wait for listener, load explicit plugin, verify authentic reason8
# and correlated outcomes, then independently verify consumer events.
