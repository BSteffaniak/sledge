#!/usr/bin/env bash
# Sourced in replacement guest AFTER real capability/focus setup, no host use.
# shellcheck disable=SC2034,SC2154 # Explicit caller state/functions supplied by Nix guest.
case "$SLEDGE_FAILURE_FIXTURE" in preflight|mods|down) ;; *) exit 2 ;; esac
echo "FAILURE_CASE $SLEDGE_FAILURE_FIXTURE qualified=false"
# Trigger with Shift held so isolated and restored masks are independently visible.
phase replacement-extra
# Physical release has happened; never permit delayed A replay into the next phase.
phase failure-recovery
hyprctl dispatch focuswindow "address:$x_address"
echo 'PHASE_BEGIN x-failure-recovery' >> /tmp/replacement-xev-events
step failure-recovery
echo 'PHASE_END x-failure-recovery' >> /tmp/replacement-xev-events
echo "FAILURE_OBSERVATION_COMPLETED $SLEDGE_FAILURE_FIXTURE"
