#!/usr/bin/env bash
# Runs only inside the disposable guest, never against a host Wayland socket.
set -euo pipefail
: "${SLEDGE_STOCK_PLUGIN:?}" "${SLEDGE_LOCK_CLIENT:?}" "${SLEDGE_IME_CLIENT:?}"
export XDG_RUNTIME_DIR=/tmp/hyprland-prerequisite
mkdir -m 700 "$XDG_RUNTIME_DIR"
mkdir -m 700 /tmp/stock-passive
# Must be present in compositor environment before launch, not merely hyprctl's.
export SLEDGE_HYPRLAND_PASSIVE_SOCKET=/tmp/stock-passive/peer.sock
cat > /tmp/hyprland.conf <<'CONF'
monitor = ,preferred,auto,1
misc:disable_hyprland_logo = true
misc:allow_session_lock_restore = true
input:kb_layout = us
debug:disable_logs = false
CONF
if [[ ${SLEDGE_RESEARCH_FIXTURE:-0} == 1 ]]; then
  export SLEDGE_STOCK_RESEARCH_MODE=lock-quarantine
fi
if [[ ${SLEDGE_REPLACEMENT_FIXTURE:-0} == 1 ]]; then
  export SLEDGE_STOCK_RESEARCH_MODE=replacement-wire
fi
Hyprland --config /tmp/hyprland.conf &
child=$!
consumer='' second='' locker='' ime='' peer=''
# shellcheck disable=SC2329 # Invoked indirectly by the EXIT trap.
cleanup() {
  for pid in "$peer" "$ime" "$locker" "$second" "$consumer" "$child"; do
    test -z "$pid" || kill "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT
exec 3<> /dev/ttyS1
stty -F /dev/ttyS1 raw -echo
step() {
  printf 'STEP %s\n' "$1" >&3
  local reply
  read -r -t 20 reply <&3
  [[ $reply == OK ]]
  sleep 0.2
}
wait_for() {
  for attempt in $(seq 1 50); do
    if grep -q "$2" "$1"; then return; fi
    sleep 0.1
  done
  echo "FIXTURE_MISSING $1 $2"
  return 1
}
clients_ready() {
  for attempt in $(seq 1 50); do
    hyprctl -j clients > /tmp/stock-clients.json
    if python3 -c 'import json,sys; sys.exit(len([c for c in json.load(open("/tmp/stock-clients.json")) if c.get("class")=="wev"]) < int(sys.argv[1]))' "$1"; then return; fi
    sleep 0.1
  done
  return 1
}
phase() {
  echo "PHASE_BEGIN $1" >> /tmp/stock-consumer-events
  step "$1"
  echo "PHASE_END $1" >> /tmp/stock-consumer-events
}
start_lock() {
  local name=$1
  mkfifo "/tmp/$name-control"
  exec 4<> "/tmp/$name-control"
  "$SLEDGE_LOCK_CLIENT" <&4 > "/tmp/$name-events" 2>&1 &
  locker=$!
  wait_for "/tmp/$name-events" LOCK_ACK
}
for ((attempt = 0; attempt < 40; ++attempt)); do
  for socket in "$XDG_RUNTIME_DIR"/hypr/*/.socket.sock; do
    if test -S "$socket"; then
      HYPRLAND_INSTANCE_SIGNATURE=$(basename "$(dirname "$socket")")
      WAYLAND_DISPLAY=$(basename "$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'wayland-*' | head -1)")
      export HYPRLAND_INSTANCE_SIGNATURE WAYLAND_DISPLAY
      hyprctl -j version
      hyprctl -j monitors
      hyprctl -j devices
      sha256sum "$(readlink -f /proc/$child/exe)" "$SLEDGE_STOCK_PLUGIN" "$SLEDGE_LOCK_CLIENT" "$SLEDGE_IME_CLIENT"
      : > /tmp/stock-consumer-events
      stdbuf -oL -eL wev >> /tmp/stock-consumer-events 2>&1 &
      consumer=$!
      clients_ready 1
      first_address=$(python3 -c 'import json; print(next(c["address"] for c in json.load(open("/tmp/stock-clients.json")) if c.get("class")=="wev"))')
      hyprctl dispatch focuswindow "address:$first_address"
      phase baseline
      if [[ ${SLEDGE_REPLACEMENT_FIXTURE:-0} == 1 ]]; then
        # shellcheck source=scripts/linux/hyprland/stock-replacement-guest.sh
        source "$SLEDGE_REPLACEMENT_SCRIPT"
        printf 'DONE\n' >&3
        exit 0
      fi
      if [[ ${SLEDGE_RESEARCH_FIXTURE:-0} == 1 ]]; then
        # shellcheck source=scripts/linux/hyprland/stock-research-guest.sh
        source "$SLEDGE_RESEARCH_SCRIPT"
        printf 'DONE\n' >&3
        exit 0
      fi
      if [[ ${SLEDGE_STOCK_ROLLBACK_ONLY:-0} == 1 ]]; then
        hyprctl plugin load "$SLEDGE_STOCK_PLUGIN"
        hyprctl -j plugin list
        wait_for /tmp/stock-hook-evidence ROLLBACK_ALL_NINE_BYTES_VERIFIED
        phase rollback
        hyprctl plugin unload "$SLEDGE_STOCK_PLUGIN"
        wait_for /tmp/stock-hook-evidence UNLOAD_RESTORED_BYTES_VERIFIED
        hyprctl -j plugin list
        phase unloaded
        kill -0 "$child"
        echo 'SLEDGE_STOCK_ROLLBACK_FIXTURE_COMPLETED_NOT_CAPTURE'
        printf 'DONE\n' >&3
        exit 0
      fi
      echo 'PHASE_BEGIN virtual-baseline' >> /tmp/stock-consumer-events
      timeout 5 wtype -M shift -k a -k b -m shift
      sleep 0.2
      echo 'PHASE_END virtual-baseline' >> /tmp/stock-consumer-events
      hyprctl plugin load "$SLEDGE_STOCK_PLUGIN"
      hyprctl -j plugin list
      phase loaded
      echo 'PHASE_BEGIN virtual-loaded' >> /tmp/stock-consumer-events
      timeout 5 wtype -k a -k b
      sleep 0.2
      echo 'PHASE_END virtual-loaded' >> /tmp/stock-consumer-events
      mkfifo /tmp/stock-ime-control
      exec 5<> /tmp/stock-ime-control
      "$SLEDGE_IME_CLIENT" <&5 > /tmp/stock-ime-events 2>&1 &
      ime=$!
      wait_for /tmp/stock-ime-events IME_GRAB_READY
      phase ime
      printf 'release\n' >&5
      wait "$ime"
      ime=
      exec 5>&-
      phase recovered
      phase second-keyboard
      hyprctl -j devices
      phase remove-second
      echo 'PHASE_BEGIN focus-hold' >> /tmp/stock-consumer-events
      step hold
      stdbuf -oL -eL wev > /tmp/stock-second-consumer-events 2>&1 &
      second=$!
      clients_ready 2
      second_address=$(python3 -c 'import json,sys; print(next(c["address"] for c in json.load(open("/tmp/stock-clients.json")) if c.get("class")=="wev" and c["address"] != sys.argv[1]))' "$first_address")
      hyprctl dispatch focuswindow "address:$second_address"
      step release
      echo 'PHASE_END focus-hold' >> /tmp/stock-consumer-events
      hyprctl dispatch focuswindow "address:$first_address"
      # Hold Shift+f across the actual lock transition and locker delivery.
      echo 'PHASE_BEGIN lock-held' >> /tmp/stock-consumer-events
      step hold
      start_lock stock-lock
      step release
      phase locked
      printf 'unlock\n' >&4
      wait "$locker"
      locker=
      exec 4>&-
      hyprctl dispatch focuswindow "address:$first_address"
      phase recovered
      # Crashed locker must remain locked; recovery uses stock's explicit relock option.
      start_lock stock-crash
      printf 'crash\n' >&4
      set +e
      wait "$locker"
      crash_status=$?
      set -e
      test "$crash_status" = 42
      locker=
      exec 4>&-
      phase crashed
      start_lock stock-relock
      phase locked
      printf 'unlock\n' >&4
      wait "$locker"
      locker=
      exec 4>&-
      hyprctl dispatch focuswindow "address:$first_address"
      phase recovered
      # Unload while held: observe restoration and releases, not just plugin-list removal.
      echo 'PHASE_BEGIN unload-held' >> /tmp/stock-consumer-events
      step hold
      hyprctl plugin unload "$SLEDGE_STOCK_PLUGIN"
      hyprctl -j plugin list
      step release
      echo 'PHASE_END unload-held' >> /tmp/stock-consumer-events
      phase unloaded
      api_hash=$("$SLEDGE_SDK_IDENTITY")
      artifact=stock-0.52.2:8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca
      sha256sum "$SLEDGE_PASSIVE_PLUGIN" "$SLEDGE_SDK_IDENTITY" "$SLEDGE_PASSIVE_PEER"
      printf 'PASSIVE_EXPECTED pid=%s uid=%s api=%s artifact=%s\n' "$child" "$(id -u)" "$api_hash" "$artifact"
      "$SLEDGE_PASSIVE_PEER" "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" "$child" "$api_hash" "$artifact" 5000 > /tmp/stock-passive-peer-events 2>&1 &
      peer=$!
      wait_for /tmp/stock-passive-peer-events 'probe listening capabilities=0 capture=false'
      test -S "$SLEDGE_HYPRLAND_PASSIVE_SOCKET"
      hyprctl plugin load "$SLEDGE_PASSIVE_PLUGIN"
      hyprctl -j plugin list
      wait "$peer"
      peer=''
      wait_for /tmp/stock-passive-peer-events 'handshakes=1 events=0 capabilities=0 capture=false'
      phase passive
      hyprctl plugin unload "$SLEDGE_PASSIVE_PLUGIN"
      hyprctl -j plugin list
      kill -0 "$child"
      echo 'PASSIVE_UNLOAD_COMPOSITOR_ALIVE_NOT_BYTE_PROOF'
      phase recovered
      echo 'SLEDGE_STOCK_EXPANDED_FIXTURE_COMPLETED_NOT_PARITY'
      printf 'DONE\n' >&3
      exit 0
    fi
  done
  kill -0 "$child" || exit 77
  sleep 1
done
exit 77
