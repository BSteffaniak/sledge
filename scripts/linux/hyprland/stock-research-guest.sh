#!/usr/bin/env bash
# Sourced by the private guest fixture after physical baseline; no host use.
# shellcheck disable=SC2034 # Shared caller state tracks child processes for cleanup.
# shellcheck disable=SC2154 # Functions and paths are supplied by the guest caller.
sha256sum "$SLEDGE_RESEARCH_PLUGIN"
hyprctl plugin load "$SLEDGE_RESEARCH_PLUGIN"
hyprctl -j plugin list
# Successful init requires all twelve exact hooks; verify the named plugin exists.
hyprctl -j plugin list > /tmp/research-plugins.json
python3 -c 'import json,sys; sys.exit(not any(p["name"] == "sledge-stock-passive" for p in json.load(open("/tmp/research-plugins.json"))))'
echo 'RESEARCH_PLUGIN_LOADED_TWELVE_HOOK_INIT_NOT_DELIVERY_PROOF'
echo 'PHASE_BEGIN research-old-held' >> /tmp/stock-consumer-events
step hold
start_lock research-lock
echo 'PHASE_END research-old-held' >> /tmp/stock-consumer-events
# Pre-lock up is quarantined; should not reach locker.
step release
phase locked
# Hold NEW locker keys through unlock: they must not enter or release to app.
step locker-hold
echo 'PHASE_BEGIN research-unlock-held' >> /tmp/stock-consumer-events
printf 'unlock\n' >&4
wait "$locker"
locker=''
exec 4>&-
hyprctl dispatch focuswindow "address:$first_address"
sleep 0.2
step locker-release
echo 'PHASE_END research-unlock-held' >> /tmp/stock-consumer-events
phase recovered
# The independent IME owns the pre-lock Shift+f route in this case.
mkfifo /tmp/research-ime-control
exec 5<> /tmp/research-ime-control
"$SLEDGE_IME_CLIENT" <&5 > /tmp/research-ime-events 2>&1 &
ime=$!
wait_for /tmp/research-ime-events IME_GRAB_READY
phase ime
step hold
start_lock research-ime-lock
step release
phase locked
printf 'unlock\n' >&4
wait "$locker"
locker=''
exec 4>&-
printf 'release\n' >&5
wait "$ime"
ime=''
exec 5>&-
hyprctl dispatch focuswindow "address:$first_address"
phase recovered
hyprctl plugin unload "$SLEDGE_RESEARCH_PLUGIN"
hyprctl -j plugin list
kill -0 "$child"
phase unloaded
echo 'SLEDGE_STOCK_RESEARCH_FIXTURE_COMPLETED_NOT_CAPTURE'
