#!/usr/bin/env bash
# Internal entry point, only within nested-probe.sh's private namespaces.
set -euo pipefail
[[ ${SLEDGE_PRIVATE_NESTED:-} == 1 && -f /probe.so ]] || {
  echo 'run only through nested-probe.sh' >&2
  exit 2
}
ulimit -c 0
mkdir -m 700 /tmp/runtime
parent=''
child=''
cleanup() {
  status=$?
  trap - EXIT
  for pid in "$child" "$parent"; do
    if [[ -n $pid ]]; then kill "$pid" 2>/dev/null || true; fi
  done
  sleep 0.2
  for pid in "$child" "$parent"; do
    if [[ -n $pid ]]; then kill -KILL "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
  done
  printf '\n=== Weston backend evidence ===\n'
  grep -E 'weston 14|Command line|headless|pixman|Output|fatal|error' /tmp/weston.log 2>/dev/null || true
  printf '\n=== Private Wayland registry and startup evidence ===\n'
  grep -E 'wl_registry.*global|CBackend|terminate|what\(' /tmp/hyprland.log 2>/dev/null || true
  for log in /tmp/runtime/hypr/*/hyprland.log; do
    [[ -f $log ]] && grep -E 'Wayland backend|Connected to a wayland|Missing protocols|allocator|Failed|failed|plugin|sledge qualification' "$log" || true
  done
  exit "$status"
}
trap cleanup EXIT
weston --backend=headless --renderer=pixman --shell=kiosk-shell.so \
  --socket=private-parent --idle-time=0 --no-config \
  --log=/tmp/weston.log > /tmp/weston.stdout 2>&1 &
parent=$!
for ((i=0; i<50; i++)); do
  [[ -S /tmp/runtime/private-parent ]] && break
  kill -0 "$parent" 2>/dev/null || { cat /tmp/weston.stdout; exit 77; }
  sleep 0.1
done
[[ -S /tmp/runtime/private-parent ]] || { echo 'BLOCKED: private parent socket unavailable'; exit 77; }
echo 'PASS: private headless/pixman Weston parent ready'
cat > /tmp/hyprland.conf <<'CONFIG'
monitor = , 640x480, auto, 1
debug {
  disable_logs = false
}
misc {
  disable_hyprland_logo = true
  disable_splash_rendering = true
}
CONFIG
# The only display is the private parent's socket in this namespace.
WAYLAND_DEBUG=1 WAYLAND_DISPLAY=private-parent "$SLEDGE_HYPRLAND_RUNTIME/bin/Hyprland" \
  --config /tmp/hyprland.conf > /tmp/hyprland.log 2>&1 &
child=$!
shopt -s nullglob
sockets=()
for ((i=0; i<100; i++)); do
  sockets=(/tmp/runtime/hypr/*/.socket.sock)
  [[ ${#sockets[@]} == 1 ]] && break
  kill -0 "$child" 2>/dev/null || { echo 'BLOCKED: nested Hyprland exited before IPC readiness'; exit 77; }
  sleep 0.1
done
[[ ${#sockets[@]} == 1 ]] || { echo 'BLOCKED: nested IPC socket unavailable'; exit 77; }
export HYPRLAND_INSTANCE_SIGNATURE
HYPRLAND_INSTANCE_SIGNATURE=$(basename "$(dirname "${sockets[0]}")")
ctl="$SLEDGE_HYPRLAND_RUNTIME/bin/hyprctl"
"$ctl" -j plugin list > /tmp/plugins-before.json
cat /tmp/plugins-before.json
python3 -c 'import json; assert json.load(open("/tmp/plugins-before.json")) == []'
set +e
"$ctl" plugin load /probe.so
load_status=$?
set -e
echo "plugin load command status: $load_status"
"$ctl" -j plugin list > /tmp/plugins-after-load.json
cat /tmp/plugins-after-load.json
python3 -c 'import json; assert json.load(open("/tmp/plugins-after-load.json")) == []'
set +e
"$ctl" plugin unload /probe.so
unload_status=$?
set -e
echo "plugin unload command status: $unload_status"
"$ctl" -j plugin list > /tmp/plugins-after-unload.json
cat /tmp/plugins-after-unload.json
python3 -c 'import json; assert json.load(open("/tmp/plugins-after-unload.json")) == []'
# Evidence must establish the actual refusal reason; command status alone isn't proof.
if ! grep -q 'sledge qualification probe: unknown SDK commit; no hooks installed' /tmp/runtime/hypr/*/hyprland.log; then
  echo 'BLOCKED: expected probe refusal not established in compositor log'
  exit 77
fi
echo 'PASS: compositor executed unknown-SDK refusal (not capture qualification)'
