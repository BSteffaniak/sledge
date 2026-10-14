#!/usr/bin/env bash
# Authorized disposable-VM qualification of the actual qualified startup path.
# Never invokes host sockets/devices or certifies production release by itself.
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
monitor = ,preferred,auto,1
misc:disable_hyprland_logo = true
input:kb_layout = us
debug:disable_logs = false
CONF
Hyprland --config /tmp/hyprland.conf > /tmp/compositor-output 2>&1 &
child=$!
cleanup() {
 [[ -z ${tap_locker:-} ]] || kill "$tap_locker" 2>/dev/null || true
 [[ -z ${tap_consumer:-} ]] || kill "$tap_consumer" 2>/dev/null || true
 for log in /tmp/qualified-startup /tmp/qualified-daemon-events /tmp/qualified-consumer-events; do
  if [[ -f $log ]]; then echo "EVIDENCE_FILE $log"; cat "$log"; fi
 done
 kill "$child" 2>/dev/null || true
 wait "$child" || true
}
trap cleanup EXIT
for ((i=0;i<60;i++)); do
 signature=$(find "$XDG_RUNTIME_DIR/hypr" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | head -1 || true)
 if [[ -n $signature ]]; then
  export HYPRLAND_INSTANCE_SIGNATURE=${signature##*/}
  if hyprctl -j monitors > /tmp/monitors.json; then break; fi
 fi
 sleep 0.2
done
export WAYLAND_DISPLAY
WAYLAND_DISPLAY=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'wayland-*' -printf '%f\n' | head -1)
[[ -n $WAYLAND_DISPLAY ]]
cat > /tmp/qualified-daemon <<'DAEMON'
#!/run/current-system/sw/bin/bash
exec /sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge "$@" --stdout-logs >> /tmp/qualified-daemon-events 2>&1
DAEMON
cat > /tmp/qualified-consumer <<'CONSUMER'
#!/run/current-system/sw/bin/bash
exec stdbuf -oL -eL wev >> /tmp/qualified-consumer-events 2>&1
CONSUMER
chmod 700 /tmp/qualified-daemon /tmp/qualified-consumer
python3 /sledge-payload/startup.py --daemon /tmp/qualified-daemon --registration "$registration" \
 --plugin "$plugin" --hyprctl "$(command -v hyprctl)" --instance "$HYPRLAND_INSTANCE_SIGNATURE" \
 --pid "$child" --config /tmp/sledge-daemon-runtime/config.toml \
 --socket "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" --ipc-socket /tmp/sledge-daemon-runtime/health.sock \
 --first-app /tmp/qualified-consumer > /tmp/qualified-startup
cat /tmp/qualified-startup
hyprctl -j activewindow > /tmp/qualified-focus
python3 -c 'import json;f=json.load(open("/tmp/qualified-focus"));assert f["class"]=="wev" and not f["xwayland"]'
stty -F /dev/ttyS1 raw -echo
exec 3<>/dev/ttyS1
step() { printf 'STEP %s\n' "$1" >&3; read -r reply <&3; [[ $reply == OK ]]; }
for phase in candidate-a candidate-a; do
 echo "PHASE_BEGIN $phase" >> /tmp/qualified-consumer-events
 step replacement-a
 sleep 0.2
 echo "PHASE_END $phase" >> /tmp/qualified-consumer-events
done
if [[ -f /sledge-payload/tap-case ]]; then
 daemon_pid=$(python3 -c 'import json;print(json.load(open("/tmp/qualified-startup"))["daemon_pid"])')
 source /sledge-payload/tap-run.sh
 run_tap_matrix /tmp/qualified-consumer-events /tmp/qualified-daemon-events
fi
python3 /sledge-payload/drain-unload.py --hyprctl "$(command -v hyprctl)" \
 --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin "$plugin"
hyprctl plugin list > /tmp/qualified-after-unload
if grep -q sledge-stock /tmp/qualified-after-unload; then exit 1; fi
echo 'PHASE_BEGIN detached' >> /tmp/qualified-consumer-events
step replacement-a
sleep 0.2
echo 'PHASE_END detached' >> /tmp/qualified-consumer-events
printf 'DONE\n' >&3
read -r reply <&3
[[ $reply == OK ]]
echo SLEDGE_QUALIFIED_STARTUP_REPLACEMENT_DRAIN_COMPLETED_VM_ONLY
