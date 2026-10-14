#!/usr/bin/env bash
# Source only inside manifested disposable payload. No host execution entrypoint.
# Caller owns guest step(), daemon PID, compositor and cleanup/drain lifecycle.
run_tap_matrix() {
 local events=$1 daemon_log=$2
 [[ -f /sledge-payload/tap-case ]] || return 0
 [[ -f /sledge-payload/tap-run.sh && -f /sledge-payload/daemon-tap.toml ]]
 [[ $(cat /sledge-payload/tap-case) == tap-v1-r1 ]]
 [[ $(cat /sledge-payload/tap-interface) == 'qualification_revision=2 feature_mask=3' ]]
 echo 'TAP_INTERFACE qualification_revision=2 feature_mask=3'
 tap_clock() { python3 -c 'import time;print(time.monotonic_ns())'; }
 tap_gap_proof() {
  local end
  end=$(tap_clock)
  [[ $((end - tap_started)) -lt 520000000 ]] || { echo "TAP_TRANSITION_TOO_SLOW $1"; return 1; }
  echo "TAP_TRANSITION_WITHIN_WINDOW $1 elapsed_ns=$((end - tap_started))"
 }
 tap_phase() {
  echo "PHASE_BEGIN $1" >> "$events"
  step "$2"
  step tap-sentinel
  sleep 0.2
  echo "PHASE_END $1" >> "$events"
 }
 # Existing hotkey config is intentionally still live for ordinary baseline.
 tap_phase tap-baseline tap-baseline
 cp /sledge-payload/daemon-tap.toml /tmp/sledge-daemon-runtime/new.toml
 mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
 kill -HUP "$daemon_pid"
 sleep 0.8
 # Consumer success proves tap rules live; a diagnostic label never does.
 tap_phase tap-positive tap-positive
 sleep 0.8
 tap_phase tap-positive tap-positive
 for case in chord timeout modifier device unknown; do
  sleep 0.8
  tap_phase "tap-$case" "tap-$case"
 done
 # A long standalone hold is allowed to count on release by core semantics.
 # Assert no early action while held, then a balanced ordinary release.
 sleep 0.8
 echo 'PHASE_BEGIN tap-hold-down' >> "$events"
 step tap-hold-down
 sleep 0.2
 echo 'PHASE_END tap-hold-down' >> "$events"
 tap_phase tap-hold-up tap-hold-up
 # Same-rule atomic reload must clear in-flight tap history, not complete it.
 sleep 0.8
 echo 'PHASE_BEGIN tap-reset' >> "$events"
 step tap-prefix
 local tap_started reload_offset
 tap_started=$(tap_clock)
 reload_offset=$(wc -c < "$daemon_log")
 cp /sledge-payload/daemon-tap.toml /tmp/sledge-daemon-runtime/new.toml
 mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
 kill -HUP "$daemon_pid"
 for ((i=0;i<15;i++)); do
  if tail -c +"$((reload_offset + 1))" "$daemon_log" | grep -q 'reload via SIGHUP'; then break; fi
  sleep 0.01
 done
 tail -c +"$((reload_offset + 1))" "$daemon_log" | grep -q 'reload via SIGHUP'
 step tap-suffix
 tap_gap_proof tap-reset
 step tap-sentinel
 sleep 0.2
 echo 'PHASE_END tap-reset' >> "$events"
 # Genuine consumer enter, no manufactured native focus/admission frame.
 sleep 0.8
 echo 'PHASE_BEGIN tap-focus' >> "$events"
 step tap-prefix
 tap_started=$(tap_clock)
 local offset focused
 offset=$(wc -c < "$daemon_log")
 stdbuf -oL -eL wev >> "$events" 2>&1 &
 tap_consumer=$!
 for ((i=0;i<40;i++)); do
  focused=$(hyprctl -j activewindow)
  if python3 -c 'import json,sys;f=json.loads(sys.argv[1]);sys.exit(not(f.get("pid")==int(sys.argv[2]) and f.get("class")=="wev" and not f.get("xwayland")))' "$focused" "$tap_consumer"; then break; fi
  sleep 0.01
 done
 python3 -c 'import json,sys;f=json.loads(sys.argv[1]);assert f["pid"]==int(sys.argv[2]) and f["class"]=="wev" and not f["xwayland"]' "$focused" "$tap_consumer"
 # admission_barrier currently consumes /tmp/candidate-daemon-events; do not
 # silently substitute the qualified log or an unpublished authority protocol.
 if [[ $daemon_log == /tmp/candidate-daemon-events ]]; then
  python3 /sledge-payload/admission_barrier.py "$(command -v hyprctl)" "$offset"
 fi
 step tap-suffix
 tap_gap_proof tap-focus
 step tap-sentinel
 sleep 0.2
 echo 'PHASE_END tap-focus' >> "$events"
 if [[ -f /sledge-payload/tap-ime-case ]]; then
  [[ -x /sledge-payload/ime-grab-client ]]
  sleep 0.8
  mkfifo /tmp/tap-ime-control
  exec 5<>/tmp/tap-ime-control
  /sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/ime-grab-client <&5 >> /tmp/tap-ime-events 2>&1 &
  tap_ime=$!
  for ((i=0;i<100;i++)); do grep -q '^IME_GRAB_READY' /tmp/tap-ime-events && break; sleep 0.05; done
  grep -q '^IME_GRAB_READY' /tmp/tap-ime-events
  echo TAP_IME_AUTHORITY_ACTIVE
  echo 'PHASE_BEGIN tap-ime' >> /tmp/tap-ime-events
  echo 'PHASE_BEGIN tap-ime' >> "$events"
  step tap-ime
  sleep 0.2
  echo 'PHASE_END tap-ime' >> "$events"
  echo 'PHASE_END tap-ime' >> /tmp/tap-ime-events
  printf 'release\n' >&5
  wait "$tap_ime"
  unset tap_ime
  echo EVIDENCE_FILE /tmp/tap-ime-events
  cat /tmp/tap-ime-events
 fi
 if [[ -f /sledge-payload/tap-lock-case ]]; then
  [[ -x /sledge-payload/lock-client ]]
  mkfifo /tmp/tap-lock-control
  exec 4<>/tmp/tap-lock-control
  /sledge-payload/lib/ld-linux-x86-64.so.2 --library-path /sledge-payload/lib /sledge-payload/lock-client <&4 >> /tmp/tap-locked-events 2>&1 &
  tap_locker=$!
  for ((i=0;i<100;i++)); do grep -q '^LOCK_ACK' /tmp/tap-locked-events && break; sleep 0.1; done
  grep -q '^LOCK_ACK' /tmp/tap-locked-events
  echo TAP_LOCK_AUTHORITY_ACK
  echo 'PHASE_BEGIN tap-locked' >> /tmp/tap-locked-events
  echo 'PHASE_BEGIN tap-locked' >> "$events"
  step tap-locked
  sleep 0.2
  echo 'PHASE_END tap-locked' >> "$events"
  echo 'PHASE_END tap-locked' >> /tmp/tap-locked-events
  printf 'unlock\n' >&4
  wait "$tap_locker"
  unset tap_locker
  echo EVIDENCE_FILE /tmp/tap-locked-events
  cat /tmp/tap-locked-events
 fi
 echo SLEDGE_TAP_CONSUMER_MATRIX_COMPLETED_VM_ONLY
 # Re-enter original consumer naturally and restore hotkey config for unchanged
 # later hotkey tests. Keep the extra consumer PID explicit for trap cleanup.
 kill "$tap_consumer"
 wait "$tap_consumer" || true
 unset tap_consumer
 cp /sledge-payload/daemon-global.toml /tmp/sledge-daemon-runtime/new.toml
 mv /tmp/sledge-daemon-runtime/new.toml /tmp/sledge-daemon-runtime/config.toml
 kill -HUP "$daemon_pid"
 sleep 0.8
}
