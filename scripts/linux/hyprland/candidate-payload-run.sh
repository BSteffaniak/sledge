#!/usr/bin/env bash
set -euo pipefail
export PATH=/run/current-system/sw/bin
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash /sledge-payload/run.sh --session; fi
unset SLEDGE_STOCK_RESEARCH_MODE SLEDGE_STOCK_REPLACEMENT_FAULT
export XDG_RUNTIME_DIR=/tmp/hyprland-prerequisite
mkdir -m 700 "$XDG_RUNTIME_DIR" /tmp/sledge-daemon-runtime
export SLEDGE_HYPRLAND_PASSIVE_SOCKET=/tmp/sledge-daemon-runtime/native.sock
if [[ -f /sledge-payload/causal-case ]]; then echo REQUIRED_CASE causal; fi
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
 # Capture complete bounded status verbatim; causal schema is owned by native/Rust.
 if [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]]; then
  echo EVIDENCE_FILE /tmp/candidate-final-native-status
  timeout 2s hyprctl sledge:status || true
 fi
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
mode=baseline
if [[ -f /sledge-payload/candidate-case ]]; then mode=$(cat /sledge-payload/candidate-case); fi
[[ $mode == baseline || $mode == late-load || $mode == matrix ]]
start_consumer() {
 stdbuf -oL -eL wev >> /tmp/candidate-consumer-events 2>&1 &
 consumer=$!
 for ((i=0;i<50;i++)); do
  hyprctl -j clients > /tmp/candidate-clients.json
  address=$(python3 -c 'import json,sys;print(next((c["address"] for c in json.load(open("/tmp/candidate-clients.json")) if c.get("pid")==int(sys.argv[1]) and c.get("class")=="wev" and not c.get("xwayland")),""))' "$consumer")
  [[ -z $address ]] || break
  sleep 0.1
 done
 [[ -n $address ]]
 hyprctl -j activewindow > /tmp/candidate-focus.json
 python3 -c 'import json,sys;c=json.load(open("/tmp/candidate-focus.json"));assert c["pid"]==int(sys.argv[1]) and c["class"]=="wev" and not c["xwayland"]' "$consumer"
 echo "CANDIDATE_CONSUMER pid=$consumer backend=wayland case=$mode"
 cat /tmp/candidate-focus.json
}
# Late load intentionally misses the existing client's natural enter.
if [[ $mode == late-load ]]; then start_consumer; fi
if [[ -f /sledge-payload/negative-abi-case ]]; then
 start_consumer
 stty -F /dev/ttyS1 raw -echo
 exec 3<>/dev/ttyS1
 echo REQUIRED_CASE negative-abi
 for bad in wrong-sdk wrong-elf; do
  if [[ -f /sledge-payload/prehook-case ]]; then
   python3 /sledge-payload/prehook_bytes.py "$child" > "/tmp/$bad-before-bytes"
  fi
  echo "PHASE_BEGIN $bad" >> /tmp/candidate-consumer-events
  set +e
  hyprctl plugin load "/sledge-payload/$bad.so" > "/tmp/$bad-load" 2>&1
  load_result=$?
  set -e
  echo "NEGATIVE_ABI_LOAD name=$bad exit=$load_result"
  cat "/tmp/$bad-load"
  grep -q 'isolated candidate scope/ELF/SDK mismatch' "/tmp/$bad-load"
  hyprctl plugin list > "/tmp/$bad-plugins"
  cat "/tmp/$bad-plugins"
  if grep -q 'sledge-stock' "/tmp/$bad-plugins"; then exit 1; fi
  [[ ! -S $SLEDGE_HYPRLAND_PASSIVE_SOCKET ]]
  if [[ -f /sledge-payload/prehook-case ]]; then
   python3 /sledge-payload/prehook_bytes.py "$child" > "/tmp/$bad-after-bytes"
   cmp "/tmp/$bad-before-bytes" "/tmp/$bad-after-bytes"
   echo "PREHOOK_BYTES_EQUAL name=$bad"
   cat "/tmp/$bad-after-bytes"
  fi
  printf 'STEP replacement-a\n' >&3
  read -r reply <&3
  [[ $reply == OK ]]
  sleep 0.2
  echo "PHASE_END $bad" >> /tmp/candidate-consumer-events
 done
 echo EVIDENCE_FILE /tmp/candidate-consumer-events
 cat /tmp/candidate-consumer-events
 printf 'DONE\n' >&3
 read -r reply <&3
 [[ $reply == OK ]]
 echo SLEDGE_NEGATIVE_ABI_COMPLETED_NOT_QUALIFIED
 exit 0
fi
cp /sledge-payload/daemon-global.toml /tmp/sledge-daemon-runtime/config.toml
hyprctl plugin load "$plugin"
# Supported session startup observes the natural first enter after plugin load.
if [[ $mode == baseline ]]; then start_consumer; fi
if [[ $mode == matrix ]]; then start_consumer; fi
hyprctl plugin list
cat "/proc/$child/maps" | grep -E 'candidate|crypto' || true
stat -c 'PLUGIN_IDENTITY dev=%d inode=%i mode=%a uid=%u path=%n' "$plugin"
export RUST_LOG=debug
if [[ -f /sledge-payload/identity-case ]]; then
 echo REQUIRED_CASE identity
 python3 - "$registration" <<'PY'
import json,sys
from pathlib import Path
original=json.load(open(sys.argv[1]))
for field,value in [('plugin_sha256','0'*64),('build_id','sledge-stock-v1:'+'0'*64),('sdk_api_hash','wrong-sdk'),('plugin_path','/tmp/candidate-copy.so')]:
 item=dict(original);item[field]=value
 Path('/tmp/identity-'+field+'.json').write_text(json.dumps(item))
PY
 cp "$plugin" /tmp/candidate-copy.so
 for probe in wrong-pid plugin_sha256 build_id sdk_api_hash plugin_path; do
  probe_pid=$child
  probe_registration=$registration
  if [[ $probe == wrong-pid ]]; then probe_pid=$$; else probe_registration=/tmp/identity-$probe.json; fi
  set +e
  timeout 3s /sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge \
   --config /tmp/sledge-daemon-runtime/config.toml --socket /tmp/sledge-daemon-runtime/probe-health.sock run --stdout-logs \
   --linux-hyprland-registration "$probe_registration" --linux-hyprland-pid "$probe_pid" \
   --linux-hyprland-socket /tmp/sledge-daemon-runtime/probe-native.sock > "/tmp/identity-$probe-events" 2>&1
  probe_result=$?
  set -e
  echo "IDENTITY_REJECT probe=$probe exit=$probe_result"
  cat "/tmp/identity-$probe-events"
  [[ $probe_result != 0 && $probe_result != 124 ]]
  [[ ! -S /tmp/sledge-daemon-runtime/probe-native.sock ]]
 done
fi
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
if [[ $mode == late-load ]]; then
 # Observe the limitation without injecting keys or manufacturing a new enter.
 for ((i=0;i<20;i++)); do
  kill -0 "$daemon_pid"
  hyprctl sledge:status > /tmp/candidate-late-status.json
  python3 -c 'import json;s=json.load(open("/tmp/candidate-late-status.json"));assert s["waiting_for_focus"] is True and s["native_operational"] is False'
  sleep 0.1
 done
 cat /tmp/candidate-late-status.json
 hyprctl dispatch sledge:drain
 python3 /sledge-payload/drain-unload.py --hyprctl "$(command -v hyprctl)" --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin "$plugin"
 hyprctl plugin list > /tmp/candidate-after-unload
 cat /tmp/candidate-after-unload
 if grep -q 'sledge-stock' /tmp/candidate-after-unload; then exit 1; fi
 printf 'DONE\n' >&3
 read -r reply <&3
 [[ $reply == OK ]]
 echo SLEDGE_INSTALLED_LATE_LOAD_WAITING_DRAIN_COMPLETED_NOT_BASELINE
 exit 0
fi
# Startup reconciliation follows the captured natural enter at a released boundary,
# without hotplug, a sacrificial key or external focus choreography.
operational=false
for ((i=0;i<100;i++)); do
 kill -0 "$daemon_pid"
 hyprctl sledge:status > /tmp/candidate-startup-status.json
 if python3 -c 'import json,sys;s=json.load(open("/tmp/candidate-startup-status.json"));sys.exit(s.get("native_operational") is not True)'; then
  operational=true
  break
 fi
 sleep 0.1
done
cat /tmp/candidate-startup-status.json
[[ $operational == true ]]
for ((i=0;i<2;i++)); do
 echo 'PHASE_BEGIN candidate-a' >> /tmp/candidate-consumer-events
 step replacement-a
 sleep 0.2
 echo 'PHASE_END candidate-a' >> /tmp/candidate-consumer-events
done
hyprctl sledge:status
if [[ $mode == matrix ]]; then
 phase() {
  echo "PHASE_BEGIN $1" >> /tmp/candidate-consumer-events
  echo "NATIVE_PHASE_BEFORE $1"
  hyprctl sledge:status
  step "$2"
  echo "NATIVE_PHASE_AFTER $1"
  hyprctl sledge:status
  if [[ -f /sledge-payload/causal-case ]]; then
   echo "EVIDENCE_FILE /tmp/candidate-causal-$1"
   python3 /sledge-payload/collect_causal.py "$(command -v hyprctl)" || true
  fi
  sleep 0.2
  echo "PHASE_END $1" >> /tmp/candidate-consumer-events
 }
 phase alt-held replacement-alt
 if [[ -f /sledge-payload/different-map-case ]]; then
  echo REQUIRED_CASE different-map
  hyprctl -j devices > /tmp/different-map-devices-before
  python3 -c 'import json;k=json.load(open("/tmp/different-map-devices-before"))["keyboards"];assert sum(d["name"]=="qemu-virtio-keyboard-1" for d in k)==1;assert sum(d["name"]=="qemu-qemu-usb-keyboard" for d in k)==1'
  phase map-held-alt candidate-mixed-hold
  hyprctl keyword 'device[qemu-virtio-keyboard-1]:kb_layout' de
  hyprctl -j devices > /tmp/different-map-devices-after
  cat /tmp/different-map-devices-after
  python3 -c 'import json;k=json.load(open("/tmp/different-map-devices-after"))["keyboards"];assert next(d for d in k if d["name"]=="qemu-virtio-keyboard-1")["layout"]=="de";assert next(d for d in k if d["name"]=="qemu-qemu-usb-keyboard")["layout"]=="us"'
  echo EVIDENCE_FILE /tmp/maps-different
  python3 /sledge-payload/collect_maps.py "$(command -v hyprctl)"
  phase map-held-original candidate-mixed-a
  phase map-held-layout-probe candidate-mixed-y
  map_reset_before=$(grep -c 'control=Reset(8)' /tmp/candidate-daemon-events || true)
  echo "MAP_RESET8_BEFORE_UP $map_reset_before"
  phase map-real-up candidate-mixed-up
  hyprctl keyword 'device[qemu-virtio-keyboard-1]:kb_layout' us
  echo EVIDENCE_FILE /tmp/maps-restored
  python3 /sledge-payload/collect_maps.py "$(command -v hyprctl)"
  kill "$consumer"
  wait "$consumer" || true
  start_consumer
  for ((i=0;i<100;i++)); do
   hyprctl sledge:status > /tmp/map-released-status
   if python3 -c 'import json,sys;s=json.load(open("/tmp/map-released-status"));sys.exit(s["native_operational"] is not True)'; then break; fi
   sleep 0.1
  done
  cat /tmp/map-released-status
  map_reset_after=$(grep -c 'control=Reset(8)' /tmp/candidate-daemon-events || true)
  echo "MAP_RESET8_BEFORE_B $map_reset_after"
  [[ $map_reset_after -gt $map_reset_before ]]
  echo EVIDENCE_FILE /tmp/maps-entered
  python3 /sledge-payload/collect_maps.py "$(command -v hyprctl)"
  phase map-released-replacement candidate-mixed-a
 fi
 if [[ -f /sledge-payload/mixed-case ]]; then
  echo REQUIRED_CASE mixed-source
  hyprctl -j devices
  phase mixed-alt-down candidate-mixed-hold
  phase mixed-replacement candidate-mixed-a
  phase mixed-alt-up candidate-mixed-up
 fi
 if [[ -f /sledge-payload/pipelined-case ]]; then
  echo REQUIRED_CASE pipelined
  phase pipelined-100 candidate-pipelined
 fi
 if [[ -f /sledge-payload/deadline-case ]]; then
  echo REQUIRED_CASE deadline
  phase stress-100 candidate-stress
  kill -STOP "$daemon_pid"
  phase deadline-expired replacement-a
  kill -CONT "$daemon_pid"
  # Reconnect is admitted only after an actual native proof, never a reset flag.
  for ((i=0;i<100;i++)); do
   hyprctl sledge:status > /tmp/candidate-deadline-status.json
   if python3 -c 'import json,sys;s=json.load(open("/tmp/candidate-deadline-status.json"));sys.exit(s["native_operational"] is not True)'; then break; fi
   sleep 0.1
  done
  cat /tmp/candidate-deadline-status.json
  phase deadline-recovered replacement-a
  echo 'PHASE_BEGIN pending-stop' >> /tmp/candidate-consumer-events
  kill -STOP "$daemon_pid"
  step daemon-a-down
  hyprctl dispatch sledge:drain
  hyprctl sledge:status
  step daemon-a-up
  if [[ -f /sledge-payload/death-case ]]; then
   # Record health while the process still exists; subsequent death is real SIGKILL.
   kill -CONT "$daemon_pid"
   /sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge --socket /tmp/sledge-daemon-runtime/health.sock status
   kill -KILL "$daemon_pid"
   wait "$daemon_pid" || true
   daemon_pid=''
   echo DAEMON_DEATH_CONFIRMED
  else
   kill -CONT "$daemon_pid"
  fi
  sleep 0.2
  echo 'PHASE_END pending-stop' >> /tmp/candidate-consumer-events
  for ((i=0;i<100;i++)); do
   hyprctl sledge:status > /tmp/candidate-pending-status.json
   if python3 -c 'import json,sys;s=json.load(open("/tmp/candidate-pending-status.json"));sys.exit(s["detached"] is not True)'; then break; fi
   sleep 0.1
  done
  cat /tmp/candidate-pending-status.json
  python3 -c 'import json;s=json.load(open("/tmp/candidate-pending-status.json"));assert s["detached"]'
  python3 /sledge-payload/drain-unload.py --hyprctl "$(command -v hyprctl)" --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin "$plugin"
  echo EVIDENCE_FILE /tmp/candidate-daemon-events
  cat /tmp/candidate-daemon-events
  echo EVIDENCE_FILE /tmp/candidate-consumer-events
  cat /tmp/candidate-consumer-events
  if [[ ! -f /sledge-payload/death-case ]]; then
   /sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge --socket /tmp/sledge-daemon-runtime/health.sock status
  fi
  printf 'DONE\n' >&3
  read -r reply <&3
  [[ $reply == OK ]]
  echo SLEDGE_INSTALLED_CANDIDATE_COMPLETED_QUALIFIED_FALSE
  exit 0
 fi
 if [[ -f /sledge-payload/extended-case ]]; then
  phase held-repeat candidate-repeat
  hyprctl keyword input:kb_layout de
  sleep 0.3
  phase layout-de replacement-a
  hyprctl keyword input:kb_layout us
  sleep 0.3
  phase layout-us replacement-a
 fi
 if [[ -f /sledge-payload/hotplug-case ]]; then
  step capability-hotplug
  hyprctl -j devices
  for ((i=0;i<100;i++)); do
   hyprctl sledge:status > /tmp/candidate-hotplug-status.json
   if python3 -c 'import json,sys;s=json.load(open("/tmp/candidate-hotplug-status.json"));sys.exit(s["native_operational"] is not True)'; then break; fi
   sleep 0.1
  done
  cat /tmp/candidate-hotplug-status.json
  phase two-keyboards replacement-a
  step capability-remove
  hyprctl -j devices
  phase keyboard-removed replacement-a
 fi
 phase caps-on candidate-caps
 phase caps-replacement replacement-a
 phase caps-off candidate-caps
 if [[ -f /sledge-payload/lock-case ]]; then
  mkfifo /tmp/candidate-lock-control
  exec 4<>/tmp/candidate-lock-control
  /sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/lock-client <&4 > /tmp/candidate-lock-events 2>&1 &
  locker=$!
  for ((i=0;i<100;i++)); do grep -q LOCK_ACK /tmp/candidate-lock-events && break; sleep 0.1; done
  grep -q LOCK_ACK /tmp/candidate-lock-events
  echo 'PHASE_BEGIN locked-a' >> /tmp/candidate-lock-events
  step replacement-a
  sleep 0.2
  echo 'PHASE_END locked-a' >> /tmp/candidate-lock-events
  hyprctl sledge:status
  printf 'unlock\n' >&4
  wait "$locker"
  echo EVIDENCE_FILE /tmp/candidate-lock-events
  cat /tmp/candidate-lock-events
  for ((i=0;i<100;i++)); do
   hyprctl sledge:status > /tmp/candidate-unlock-status.json
   if python3 -c 'import json,sys;s=json.load(open("/tmp/candidate-unlock-status.json"));sys.exit(s["native_operational"] is not True)'; then break; fi
   sleep 0.1
  done
  cat /tmp/candidate-unlock-status.json
  phase after-unlock replacement-a
 fi
 export DISPLAY=:0
 stdbuf -oL -eL xev -event keyboard -name sledge-xev >> /tmp/candidate-x-events 2>&1 &
 xconsumer=$!
 for ((i=0;i<50;i++)); do grep -q 'Outer window is' /tmp/candidate-x-events && break; sleep 0.1; done
 xwindow=$(python3 -c 'import re;print(re.search(r"Outer window is (0x[0-9a-fA-F]+)",open("/tmp/candidate-x-events").read())[1])')
 export SLEDGE_FIXTURE_X11_LIBRARY=/nix/store/r398kwsyxnw58gcwy2h1g6d430hmlmxq-libx11-1.8.12/lib/libX11.so.6
 python3 /sledge-payload/set_fixture_class.py "$xwindow"
 xprop -id "$xwindow" WM_CLASS
 for alias in x_fixture wrong_fixture; do
  { cat /sledge-payload/daemon-global.toml; printf '\nwhen = { app_in = ["%s"] }\n' "$alias"; } > /tmp/sledge-daemon-runtime/new.toml
  mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
  kill -HUP "$daemon_pid"
  sleep 0.6
  hyprctl -j activewindow
  for ((i=0;i<100;i++)); do
   hyprctl sledge:status > /tmp/candidate-x-status.json
   if python3 -c 'import json,sys;s=json.load(open("/tmp/candidate-x-status.json"));sys.exit(s["native_operational"] is not True)'; then break; fi
   sleep 0.1
  done
  cat /tmp/candidate-x-status.json
  echo "PHASE_BEGIN x-$alias" >> /tmp/candidate-x-events
  step replacement-a
  sleep 0.2
  echo "PHASE_END x-$alias" >> /tmp/candidate-x-events
 done
 kill "$xconsumer"
 wait "$xconsumer" || true
 sleep 0.2
 echo EVIDENCE_FILE /tmp/candidate-x-events
 cat /tmp/candidate-x-events
 for alias in native_fixture wrong_fixture; do
  { cat /sledge-payload/daemon-global.toml; printf '\nwhen = { app_in = ["%s"] }\n' "$alias"; } > /tmp/sledge-daemon-runtime/new.toml
  mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
  kill -HUP "$daemon_pid"
  sleep 0.6
  phase "scope-$alias" replacement-a
 done
 sed 's/key = "KeyB"/key = "KeyC"/' /sledge-payload/daemon-global.toml > /tmp/sledge-daemon-runtime/new.toml
 mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
 kill -HUP "$daemon_pid"
 sleep 0.6
 phase valid-reload replacement-a
 printf 'invalid = [\n' > /tmp/sledge-daemon-runtime/config.toml
 kill -HUP "$daemon_pid"
 sleep 0.6
 phase invalid-reload replacement-a
 if [[ -f /sledge-payload/restart-case ]]; then
  cp /sledge-payload/daemon-global.toml /tmp/sledge-daemon-runtime/config.toml
  kill "$daemon_pid"
  wait "$daemon_pid" || true
  /sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge \
   --config /tmp/sledge-daemon-runtime/config.toml --socket /tmp/sledge-daemon-runtime/health.sock run --stdout-logs \
   --linux-hyprland-registration "$registration" --linux-hyprland-pid "$child" \
   --linux-hyprland-socket "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" >> /tmp/candidate-daemon-events 2>&1 &
  daemon_pid=$!
  sleep 1
  # A new genuine client supplies a natural enter, not a synthetic reset grant.
  kill "$consumer"
  wait "$consumer" || true
  start_consumer
  for ((i=0;i<100;i++)); do
   hyprctl sledge:status > /tmp/candidate-restart-status.json
   if python3 -c 'import json,sys;s=json.load(open("/tmp/candidate-restart-status.json"));sys.exit(s["native_operational"] is not True)'; then break; fi
   sleep 0.1
  done
  cat /tmp/candidate-restart-status.json
  phase daemon-restart replacement-a
 fi
 echo 'PHASE_BEGIN held-drain' >> /tmp/candidate-consumer-events
 step daemon-a-down
 hyprctl dispatch sledge:drain
 hyprctl sledge:status > /tmp/candidate-held-status.json
 cat /tmp/candidate-held-status.json
 python3 -c 'import json;s=json.load(open("/tmp/candidate-held-status.json"));assert s["draining"] and not s["detached"]'
 step daemon-a-up
 sleep 0.2
 echo 'PHASE_END held-drain' >> /tmp/candidate-consumer-events
fi
/sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/sledge --socket /tmp/sledge-daemon-runtime/health.sock status
hyprctl dispatch sledge:drain
if [[ -f /sledge-payload/causal-case ]]; then
 echo EVIDENCE_FILE /tmp/candidate-causal-final
 python3 /sledge-payload/collect_causal.py "$(command -v hyprctl)" || true
fi
if [[ $mode == matrix ]]; then
 for ((i=0;i<100;i++)); do
  hyprctl sledge:status > /tmp/candidate-detached-status.json
  if python3 -c 'import json,sys;s=json.load(open("/tmp/candidate-detached-status.json"));sys.exit(s["detached"] is not True)'; then break; fi
  sleep 0.1
 done
 cat /tmp/candidate-detached-status.json
 python3 -c 'import json;s=json.load(open("/tmp/candidate-detached-status.json"));assert s["detached"]'
 echo 'PHASE_BEGIN detached-unload' >> /tmp/candidate-consumer-events
 step daemon-a-down
fi
python3 /sledge-payload/drain-unload.py --hyprctl "$(command -v hyprctl)" --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin "$plugin"
if [[ $mode == matrix ]]; then
 step daemon-a-up
 sleep 0.2
 echo 'PHASE_END detached-unload' >> /tmp/candidate-consumer-events
fi
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
