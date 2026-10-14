#!/usr/bin/env bash
# Fresh qualification-only guest path; deliberately no legacy instrumentation.
set -euo pipefail
export PATH=/run/current-system/sw/bin
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash /sledge-payload/run.sh --session; fi
export XDG_RUNTIME_DIR=/tmp/hyprland-prerequisite
mkdir -m 700 "$XDG_RUNTIME_DIR" /tmp/sledge-daemon-runtime
export SLEDGE_HYPRLAND_PASSIVE_SOCKET=/tmp/sledge-daemon-runtime/native.sock
registration=$(cat /sledge-payload/candidate-registration-path)
plugin=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["plugin_path"])' "$registration")
[[ $plugin == /nix/store/*/lib/qualified.so ]]
cp /sledge-payload/daemon-global.toml /tmp/sledge-daemon-runtime/config.toml
cat > /tmp/hyprland.conf <<'CONF'
monitor = ,640x480,auto,1
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
 for pid in "${tap_ime:-}" "${tap_locker:-}" "${tap_consumer:-}" "$consumer" "$daemon_pid" "$child"; do [[ -z $pid ]] || kill "$pid" 2>/dev/null || true; done
}
trap cleanup EXIT
for ((i=0;i<100;i++)); do
 signature=$(find "$XDG_RUNTIME_DIR/hypr" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | head -1 || true)
 if [[ -n $signature ]]; then
  export HYPRLAND_INSTANCE_SIGNATURE=${signature##*/}
  if hyprctl -j monitors >/tmp/monitors.json; then break; fi
 fi
 sleep 0.1
done
export WAYLAND_DISPLAY
WAYLAND_DISPLAY=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'wayland-*' -printf '%f\n' | head -1)
[[ -n $WAYLAND_DISPLAY ]]
hyprctl plugin load "$plugin"
# Natural enter after plugin load establishes stock authority.
stdbuf -oL -eL wev >> /tmp/candidate-consumer-events 2>&1 &
consumer=$!
/sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge \
 --config /tmp/sledge-daemon-runtime/config.toml --socket /tmp/sledge-daemon-runtime/health.sock run --stdout-logs \
 --linux-hyprland-registration "$registration" --linux-hyprland-pid "$child" \
 --linux-hyprland-socket "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" > /tmp/candidate-daemon-events 2>&1 &
daemon_pid=$!
python3 /sledge-payload/admission_barrier.py "$(command -v hyprctl)" 0
stty -F /dev/ttyS1 raw -echo
exec 3<>/dev/ttyS1
step() { printf 'STEP %s\n' "$1" >&3; read -r reply <&3; [[ $reply == OK ]]; }
source /sledge-payload/tap-run.sh
run_tap_matrix /tmp/candidate-consumer-events /tmp/candidate-daemon-events
python3 /sledge-payload/drain-unload.py --hyprctl "$(command -v hyprctl)" --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin "$plugin"
printf 'DONE\n' >&3
read -r reply <&3
[[ $reply == OK ]]
echo SLEDGE_TAP_CANDIDATE_ADMISSION_COMPLETED_NOT_PRODUCTION_APPROVAL
