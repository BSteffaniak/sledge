#!/usr/bin/env bash
# Sourced only in the isolated guest; no synthetic capability override.
# shellcheck disable=SC2034,SC2154 # Shared caller functions/state and explicit Nix paths.
export DISPLAY=:0
: > /tmp/replacement-xev-events
stdbuf -oL -eL xev -event keyboard -name sledge-xev >> /tmp/replacement-xev-events 2>&1 &
second=$!
for ((x_ready_attempt = 0; x_ready_attempt < 50; ++x_ready_attempt)); do
  hyprctl -j clients > /tmp/replacement-clients.json
  if python3 -c 'import json,sys;sys.exit(not any(c.get("xwayland") and c.get("title")=="sledge-xev" for c in json.load(open("/tmp/replacement-clients.json"))))'; then break; fi
  sleep 0.1
done
x_address=$(python3 -c 'import json;print(next(c["address"] for c in json.load(open("/tmp/replacement-clients.json")) if c.get("xwayland") and c.get("title")=="sledge-xev"))')
cat /tmp/replacement-clients.json
hyprctl dispatch focuswindow "address:$x_address"
echo 'PHASE_BEGIN x-baseline' >> /tmp/replacement-xev-events
step baseline
echo 'PHASE_END x-baseline' >> /tmp/replacement-xev-events
hyprctl dispatch focuswindow "address:$first_address"
api_hash=$("$SLEDGE_SDK_IDENTITY")
artifact=stock-0.52.2:8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca
sha256sum "$SLEDGE_RESEARCH_PLUGIN" "$SLEDGE_RESEARCH_PEER"
"$SLEDGE_RESEARCH_PEER" "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" "$child" "$api_hash" "$artifact" 64 30000 > /tmp/replacement-peer-events 2>&1 &
peer=$!
wait_for /tmp/replacement-peer-events 'research listening capabilities=0'
hyprctl plugin load "$SLEDGE_RESEARCH_PLUGIN"
hyprctl -j plugin list > /tmp/replacement-plugins.json
cat /tmp/replacement-plugins.json
python3 -c 'import json,sys;sys.exit(not any(p["name"]=="sledge-stock-passive" for p in json.load(open("/tmp/replacement-plugins.json"))))'
# A real libinput keyboard arrival updates stock seat capabilities after load.
step capability-hotplug
hyprctl -j devices
step capability-remove
hyprctl -j devices
# Both focus transitions are real post-load enters, required by reservation.
hyprctl dispatch focuswindow "address:$x_address"
hyprctl dispatch focuswindow "address:$first_address"
for name in replacement-a replacement-a replacement-extra replacement-target-held; do
  phase "$name"
done
hyprctl dispatch focuswindow "address:$x_address"
for name in replacement-a replacement-a replacement-extra replacement-target-held; do
  echo "PHASE_BEGIN x-$name" >> /tmp/replacement-xev-events
  step "$name"
  echo "PHASE_END x-$name" >> /tmp/replacement-xev-events
done
hyprctl plugin unload "$SLEDGE_RESEARCH_PLUGIN"
hyprctl -j plugin list
# Listener intentionally bounded; disconnect after all observed consumer phases.
kill "$peer" 2>/dev/null || true
wait "$peer" || true
peer=''
kill -0 "$child"
echo 'SLEDGE_REPLACEMENT_OBSERVATION_COMPLETED_NOT_QUALIFICATION'
