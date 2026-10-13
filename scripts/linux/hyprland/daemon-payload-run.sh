#!/usr/bin/env bash
set -euo pipefail
export PATH=/run/current-system/sw/bin
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash /sledge-payload/run.sh --session; fi
export XDG_RUNTIME_DIR=/tmp/hyprland-prerequisite
mkdir -m 700 "$XDG_RUNTIME_DIR" /tmp/sledge-daemon-runtime
export SLEDGE_HYPRLAND_PASSIVE_SOCKET=/tmp/sledge-daemon-runtime/native.sock
export SLEDGE_STOCK_RESEARCH_MODE=replacement-wire
cat > /tmp/hyprland.conf <<'CONF'
monitor = ,preferred,auto,1
misc:disable_hyprland_logo = true
input:kb_layout = us
debug:disable_logs = false
CONF
Hyprland --config /tmp/hyprland.conf > /tmp/compositor-output 2>&1 &
child=$!
daemon_pid='' consumer=''
cleanup() { for pid in "$daemon_pid" "$consumer" "$child"; do [[ -z $pid ]] || kill "$pid" 2>/dev/null || true; done; }
trap cleanup EXIT
for ((i=0;i<60;i++)); do
  signature=$(find "$XDG_RUNTIME_DIR/hypr" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | head -1 || true)
  if [[ -n $signature ]]; then export HYPRLAND_INSTANCE_SIGNATURE=${signature##*/}; if hyprctl -j monitors >/tmp/monitors.json; then break; fi; fi
  sleep 0.2
done
export WAYLAND_DISPLAY=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'wayland-*' -printf '%f\n' | head -1)
[[ -n $WAYLAND_DISPLAY ]]
stdbuf -oL -eL wev >> /tmp/daemon-consumer-events 2>&1 &
consumer=$!
sleep 1
hyprctl -j clients
first_address=$(hyprctl -j clients | python3 -c 'import json,sys;print(next(c["address"] for c in json.load(sys.stdin) if c.get("class")=="wev"))')
stdbuf -oL -eL wev > /tmp/daemon-second-events 2>&1 &
second=$!
sleep 0.5
second_address=$(hyprctl -j clients | python3 -c 'import json,sys;print(next(c["address"] for c in json.load(sys.stdin) if c.get("pid")==int(sys.argv[1])))' "$second")
cp /sledge-payload/daemon-global.toml /tmp/sledge-daemon-runtime/config.toml
api_hash=$(/sledge-payload/sdk-identity)
export RUST_LOG=debug
/sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge \
 --config /tmp/sledge-daemon-runtime/config.toml --socket /tmp/sledge-daemon-runtime/health.sock run --stdout-logs \
 --linux-hyprland-research-socket "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" \
 --linux-hyprland-research-pid "$child" --linux-hyprland-research-api-hash "$api_hash" \
 --linux-hyprland-research-artifact stock-0.52.2:8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca > /tmp/daemon-events 2>&1 &
daemon_pid=$!
for ((i=0;i<60;i++)); do [[ ! -S $SLEDGE_HYPRLAND_PASSIVE_SOCKET ]] || break; sleep 0.1; done
hyprctl plugin load /sledge-payload/plugin.so
stty -F /dev/ttyS1 raw -echo
exec 3<>/dev/ttyS1
step() { printf 'STEP %s\n' "$1" >&3; read -r reply <&3; [[ $reply == OK ]]; }
step capability-hotplug
step capability-remove
hyprctl dispatch focuswindow "address:$second_address"
hyprctl dispatch focuswindow "address:$first_address"
sleep 0.2
step initial-reconcile
for ((i=0;i<2;i++)); do
 echo 'PHASE_BEGIN daemon-a' >> /tmp/daemon-consumer-events
 step replacement-a
 echo 'PHASE_END daemon-a' >> /tmp/daemon-consumer-events
done
sleep 0.2
printf 'invalid = [\n' > /tmp/sledge-daemon-runtime/bad.toml
mv /tmp/sledge-daemon-runtime/bad.toml /tmp/sledge-daemon-runtime/config.toml
kill -HUP "$daemon_pid"
sleep 0.6
echo 'PHASE_BEGIN invalid-reload' >> /tmp/daemon-consumer-events
step replacement-a
sleep 0.2
echo 'PHASE_END invalid-reload' >> /tmp/daemon-consumer-events
sed 's/key = "KeyB"/key = "KeyC"/' /sledge-payload/daemon-global.toml > /tmp/sledge-daemon-runtime/new.toml
mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
kill -HUP "$daemon_pid"
sleep 0.6
echo 'PHASE_BEGIN valid-reload' >> /tmp/daemon-consumer-events
step replacement-a
sleep 0.2
echo 'PHASE_END valid-reload' >> /tmp/daemon-consumer-events
echo 'PHASE_BEGIN alt-held' >> /tmp/daemon-consumer-events
step replacement-alt
sleep 0.2
echo 'PHASE_END alt-held' >> /tmp/daemon-consumer-events
for alias in native_fixture wrong_fixture; do
  { cat /sledge-payload/daemon-global.toml; printf '\nwhen = { app_in = ["%s"] }\n' "$alias"; } > /tmp/sledge-daemon-runtime/new.toml
  mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
  kill -HUP "$daemon_pid"
  sleep 0.6
  echo "PHASE_BEGIN scope-$alias" >> /tmp/daemon-consumer-events
  step replacement-a
  sleep 0.2
  echo "PHASE_END scope-$alias" >> /tmp/daemon-consumer-events
done
export DISPLAY=:0
stdbuf -oL -eL xev -event keyboard -name sledge-xev >> /tmp/daemon-x-events 2>&1 &
xconsumer=$!
for ((i=0;i<50;i++)); do grep -q 'Outer window is' /tmp/daemon-x-events && break; sleep 0.1; done
xwindow=$(python3 -c 'import re;print(re.search(r"Outer window is (0x[0-9a-fA-F]+)",open("/tmp/daemon-x-events").read())[1])')
export SLEDGE_FIXTURE_X11_LIBRARY=/nix/store/r398kwsyxnw58gcwy2h1g6d430hmlmxq-libx11-1.8.12/lib/libX11.so.6
python3 /sledge-payload/set_fixture_class.py "$xwindow"
xprop -id "$xwindow" WM_CLASS
sleep 0.3
xaddress=$(hyprctl -j clients | python3 -c 'import json,sys;print(next(c["address"] for c in json.load(sys.stdin) if c.get("xwayland") and c.get("class")=="SledgeFixtureX11"))')
hyprctl dispatch focuswindow "address:$xaddress"
for alias in x_fixture wrong_fixture; do
  { cat /sledge-payload/daemon-global.toml; printf '\nwhen = { app_in = ["%s"] }\n' "$alias"; } > /tmp/sledge-daemon-runtime/new.toml
  mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
  kill -HUP "$daemon_pid"
  sleep 0.6
  echo "PHASE_BEGIN x-$alias" >> /tmp/daemon-x-events
  step replacement-a
  sleep 0.2
  echo "PHASE_END x-$alias" >> /tmp/daemon-x-events
done
hyprctl dispatch focuswindow "address:$first_address"
# Restore current C mapping before held drain.
sed 's/key = "KeyB"/key = "KeyC"/' /sledge-payload/daemon-global.toml > /tmp/sledge-daemon-runtime/new.toml
mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
kill -HUP "$daemon_pid"
sleep 0.6
echo EVIDENCE_FILE /tmp/daemon-events
cat /tmp/daemon-events
echo EVIDENCE_FILE /tmp/daemon-consumer-events
cat /tmp/daemon-consumer-events
# Hold the consumed original while requesting drain; release must still settle.
echo 'PHASE_BEGIN held-drain' >> /tmp/daemon-consumer-events
step daemon-a-down
hyprctl dispatch sledge:drain
hyprctl sledge:status > /tmp/held-drain-status.json
cat /tmp/held-drain-status.json
python3 -c 'import json,sys;s=json.load(open("/tmp/held-drain-status.json"));sys.exit(not(s["draining"] and not s["detached"]))'
step daemon-a-up
sleep 0.2
echo 'PHASE_END held-drain' >> /tmp/daemon-consumer-events
for ((i=0;i<50;i++)); do
  hyprctl sledge:status > /tmp/drain-status.json
  if python3 -c 'import json,sys;s=json.load(open("/tmp/drain-status.json"));sys.exit(not(s["detached"] and s["quiescent"]))'; then break; fi
  sleep 0.1
done
cat /tmp/drain-status.json
python3 -c 'import json,sys;s=json.load(open("/tmp/drain-status.json"));sys.exit(not(s["detached"] and s["quiescent"]))'
# Detached hook lease persists: new physical Down goes through stock before unload.
echo 'PHASE_BEGIN detached-unload' >> /tmp/daemon-consumer-events
step daemon-a-down
python3 /sledge-payload/drain-unload.py --hyprctl "$(command -v hyprctl)" --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin /sledge-payload/plugin.so
step daemon-a-up
sleep 0.2
echo 'PHASE_END detached-unload' >> /tmp/daemon-consumer-events
echo EVIDENCE_FILE /tmp/daemon-x-events
cat /tmp/daemon-x-events
echo EVIDENCE_FILE /tmp/daemon-consumer-events-final
cat /tmp/daemon-consumer-events
printf 'DONE\n' >&3
echo SLEDGE_ACTUAL_DAEMON_RESEARCH_COMPLETED_QUALIFIED_FALSE
