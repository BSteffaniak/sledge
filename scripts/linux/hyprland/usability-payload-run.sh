#!/usr/bin/env bash
# Disposable guest only: invoke desktop Exec, never start plugin/daemon directly.
set -euo pipefail
export PATH=/run/current-system/sw/bin
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash /sledge-payload/run.sh --session; fi
export XDG_RUNTIME_DIR=/tmp/usability-runtime
export XDG_CONFIG_HOME=/tmp/usability-config
mkdir -m 700 "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME"
mkdir "$XDG_CONFIG_HOME/sledge"
cp /sledge-payload/daemon-tap.toml "$XDG_CONFIG_HOME/sledge/config.toml"
package=$(cat /sledge-payload/session-package-path)
[[ $package == /nix/store/* ]]
python3 - "$package" > /tmp/usability-desktop-exec <<'PY'
import configparser,pathlib,shlex,sys
p=pathlib.Path(sys.argv[1])/'share/wayland-sessions/hyprland-sledge.desktop'
c=configparser.ConfigParser(interpolation=None);c.read(p)
a=shlex.split(c['Desktop Entry']['Exec'])
assert len(a)==1 and a[0].startswith('/nix/store/') and '%' not in a[0]
print(a[0])
PY
exec_path=$(cat /tmp/usability-desktop-exec)
echo "ACTUAL_DESKTOP_EXEC $exec_path"
stty -F /dev/ttyS1 raw -echo
"$exec_path" > /tmp/usability-session-events 2>&1 &
wrapper=$!
cleanup() {
 for log in /tmp/usability-startup-diagnostic /tmp/usability-desktop-exec /tmp/usability-session-events /tmp/usability-first-app-events /tmp/usability-consumer-events; do
  if [[ -f $log ]]; then echo "EVIDENCE_FILE $log"; cat "$log"; fi
 done
 for log in "$XDG_RUNTIME_DIR"/sledge.*/startup.log; do
  if [[ -f $log ]]; then echo "EVIDENCE_FILE $log"; cat "$log"; fi
 done
 # Include compositor's exec-once diagnostics without reading process memory.
 for log in "$XDG_RUNTIME_DIR"/hypr/*/hyprland.log; do
  if [[ -f $log ]]; then echo "EVIDENCE_FILE $log"; tail -c 20000 "$log"; fi
 done
 # No force cleanup: failed lifecycle retains evidence/owners until guest teardown.
}
trap cleanup EXIT
for ((i=0;i<1000;i++)); do
 [[ ! -f /tmp/usability-controller-done ]] || break
 kill -0 "$wrapper"
 sleep 0.1
done
[[ $(cat /tmp/usability-controller-done) == ok ]]
echo NORMAL_LOGOUT_REQUEST
kill -TERM "$wrapper"
for ((i=0;i<150;i++)); do
 if ! kill -0 "$wrapper" 2>/dev/null; then break; fi
 sleep 0.1
done
if kill -0 "$wrapper" 2>/dev/null; then echo NORMAL_LOGOUT_BLOCKED_NO_FORCE; exit 1; fi
set +e
wait "$wrapper"
result=$?
set -e
echo "NORMAL_WRAPPER_SETTLED exit=$result"
[[ $result == 0 ]]
python3 - <<'PY'
import json,pathlib,re,time
logs=list(pathlib.Path('/tmp/usability-runtime').glob('sledge.*/startup.log'))
assert len(logs)==1, 'missing unique private startup log'
log=logs[0].read_text()
records=[json.loads(line) for line in log.splitlines() if line.startswith('{')]
started=[r for r in records if r.get('event')=='daemon_started']
ready=[r for r in records if r.get('operational') is True]
assert len(started)==len(ready)==1 and started[0]['daemon_pid']==ready[0]['daemon_pid'], 'startup identity/readiness mismatch'
pid=started[0]['daemon_pid']
deadline=time.monotonic()+5
while pathlib.Path(f'/proc/{pid}').exists() and time.monotonic()<deadline:
 time.sleep(0.05)
assert not pathlib.Path(f'/proc/{pid}').exists(), 'daemon retained after normal compositor exit'
print(f'NORMAL_DAEMON_EXIT_CONFIRMED pid={pid}')
PY
# Detach/unload confirmation must be emitted by the actual session controller.
echo SLEDGE_NORMAL_DESKTOP_EXEC_COMPLETED_VM_ONLY_NOT_GDM_AUTHENTICATION
exec 3<>/dev/ttyS1
printf 'DONE\n' >&3
read -r reply <&3
[[ $reply == OK ]]
