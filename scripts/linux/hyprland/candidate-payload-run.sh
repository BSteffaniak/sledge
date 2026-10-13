#!/usr/bin/env bash
set -euo pipefail
export PATH=/run/current-system/sw/bin
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash /sledge-payload/run.sh --session; fi
unset SLEDGE_STOCK_RESEARCH_MODE SLEDGE_STOCK_REPLACEMENT_FAULT
export XDG_RUNTIME_DIR=/tmp/hyprland-prerequisite
mkdir -m 700 "$XDG_RUNTIME_DIR" /tmp/sledge-daemon-runtime
export SLEDGE_HYPRLAND_PASSIVE_SOCKET=/tmp/sledge-daemon-runtime/native.sock
registration=$(cat /sledge-payload/candidate-registration-path)
[[ $registration == /nix/store/*/registration.json && -f $registration && ! -L $registration ]]
plugin=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["plugin_path"])' "$registration")
[[ $plugin == /nix/store/*/lib/candidate.so && -f $plugin && ! -L $plugin ]]
cat > /tmp/hyprland.conf <<'CONF'
monitor = ,preferred,auto,1
misc:disable_hyprland_logo = true
input:kb_layout = us
debug:disable_logs = false
CONF
Hyprland --config /tmp/hyprland.conf > /tmp/compositor-output 2>&1 &
child=$!
daemon_pid='' consumer=''
cleanup() {
 for log in /tmp/candidate-daemon-events /tmp/candidate-consumer-events; do
  if [[ -f $log ]]; then echo "EVIDENCE_FILE $log"; cat "$log"; fi
 done
 for pid in "$daemon_pid" "$consumer" "$child"; do [[ -z $pid ]] || kill "$pid" 2>/dev/null || true; done
}
trap cleanup EXIT
for ((i=0;i<60;i++)); do
 signature=$(find "$XDG_RUNTIME_DIR/hypr" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | head -1 || true)
 if [[ -n $signature ]]; then export HYPRLAND_INSTANCE_SIGNATURE=${signature##*/}; if hyprctl -j monitors >/tmp/monitors.json; then break; fi; fi
 sleep 0.2
done
WAYLAND_DISPLAY=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'wayland-*' -printf '%f\n' | head -1)
export WAYLAND_DISPLAY
[[ -n $WAYLAND_DISPLAY ]]
stdbuf -oL -eL wev >> /tmp/candidate-consumer-events 2>&1 &
consumer=$!
for ((i=0;i<50;i++)); do
 address=$(hyprctl -j clients | python3 -c 'import json,sys;print(next((c["address"] for c in json.load(sys.stdin) if c.get("class")=="wev"),""))')
 [[ -z $address ]] || break
 sleep 0.1
done
[[ -n $address ]]
hyprctl dispatch focuswindow "address:$address"
cp /sledge-payload/daemon-global.toml /tmp/sledge-daemon-runtime/config.toml
hyprctl plugin load "$plugin"
hyprctl plugin list
cat "/proc/$child/maps" | grep -E 'candidate|crypto' || true
stat -c 'PLUGIN_IDENTITY dev=%d inode=%i mode=%a uid=%u path=%n' "$plugin"
export RUST_LOG=debug
/sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge \
 --config /tmp/sledge-daemon-runtime/config.toml --socket /tmp/sledge-daemon-runtime/health.sock run --stdout-logs \
 --linux-hyprland-registration "$registration" --linux-hyprland-pid "$child" \
 --linux-hyprland-socket "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" > /tmp/candidate-daemon-events 2>&1 &
daemon_pid=$!
for ((i=0;i<100;i++)); do
 kill -0 "$daemon_pid"
 [[ ! -S $SLEDGE_HYPRLAND_PASSIVE_SOCKET ]] || break
 sleep 0.1
done
[[ -S $SLEDGE_HYPRLAND_PASSIVE_SOCKET ]]
stty -F /dev/ttyS1 raw -echo
exec 3<>/dev/ttyS1
step() { printf 'STEP %s\n' "$1" >&3; read -r reply <&3; [[ $reply == OK ]]; }
step capability-hotplug
step capability-remove
step initial-reconcile
for ((i=0;i<100;i++)); do
 grep -q 'accepted.*true' /tmp/candidate-daemon-events && break
 sleep 0.1
done
for ((i=0;i<2;i++)); do
 echo 'PHASE_BEGIN candidate-a' >> /tmp/candidate-consumer-events
 step replacement-a
 sleep 0.2
 echo 'PHASE_END candidate-a' >> /tmp/candidate-consumer-events
done
hyprctl sledge:status
/sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge --socket /tmp/sledge-daemon-runtime/health.sock status
hyprctl dispatch sledge:drain
python3 /sledge-payload/drain-unload.py --hyprctl "$(command -v hyprctl)" --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin "$plugin"
echo EVIDENCE_FILE /tmp/candidate-daemon-events
cat /tmp/candidate-daemon-events
echo EVIDENCE_FILE /tmp/candidate-consumer-events
cat /tmp/candidate-consumer-events
echo EVIDENCE_FILE /tmp/compositor-output
cat /tmp/compositor-output
printf 'DONE\n' >&3
read -r reply <&3
[[ $reply == OK ]]
echo SLEDGE_INSTALLED_CANDIDATE_COMPLETED_QUALIFIED_FALSE
