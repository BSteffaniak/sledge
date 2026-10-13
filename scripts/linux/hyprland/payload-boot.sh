#!/usr/bin/env bash
# Root-only inside the disposable guest, selected by systemd.run kernel parameter.
set -euo pipefail
export PATH=/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin
exec > /dev/ttyS0 2>&1
echo SLEDGE_PAYLOAD_BOOT_ENTERED
cat /proc/cmdline
cat /sledge-payload/pre-init-scope.log || true
cd /sledge-payload
sha256sum --check manifest.sha256
if [[ -d scope ]]; then
  echo SLEDGE_SCOPE_POST_INIT
  cat /etc/machine-id
  stat -c '%u:%g:%a' /etc/machine-id
  findmnt /etc/machine-id || true
  cmp scope/machine-id /etc/machine-id
  stat -c '%u:%g:%a' /etc/sledge-qualification-candidate
  cat /etc/sledge-qualification-candidate
  cmp scope/boot-marker /etc/sledge-qualification-candidate
  [[ $(stat -c '%u:%g:%a' /etc/machine-id) == 0:0:444 ]]
  [[ $(stat -c '%u:%g:%a' /etc/sledge-qualification-candidate) == 0:0:444 ]]
  echo SLEDGE_CANDIDATE_SCOPE_PRESERVED
  mkdir -p /nix/payload-upper /nix/payload-work
  mount -t overlay overlay -o lowerdir=/nix/.ro-store,upperdir=/nix/payload-upper,workdir=/nix/payload-work /nix/store
  for entry in store/*; do
    target=/nix/store/${entry##*/}
    [[ ! -e $target ]]
    cp -R "$entry" "$target"
    chown -R 0:0 "$target"
    find "$target" -type f -exec chmod 444 {} +
    find "$target" -type d -exec chmod 555 {} +
  done
  # Exact copied regular files must match manifested bytes before admission.
  while read -r digest name; do
    if [[ $name == store/* ]]; then
      target=/nix/$name
      # Overlay stat and mmap devices differ; bind the verified backing regular
      # file at its exact store path so descriptor identity remains exact.
      chown 0:0 "$name"
      chmod 444 "$name"
      mount --bind "/sledge-payload/$name" "$target"
      mount -o remount,bind,ro "$target"
      printf '%s  %s\n' "$digest" "$target" | sha256sum --check
    fi
  done < manifest.sha256
  echo SLEDGE_CANDIDATE_STORE_TRANSFER_VERIFIED
fi
cat > /run/systemd/system/sledge-payload.service <<'UNIT'
[Unit]
After=systemd-user-sessions.service dbus.service systemd-logind.service
Requires=dbus.service systemd-logind.service
[Service]
Type=oneshot
User=probe
PAMName=login
TTYPath=/dev/tty7
StandardInput=tty
StandardOutput=append:/tmp/sledge-payload-output
StandardError=append:/tmp/sledge-payload-output
TimeoutStartSec=120
ExecStart=/bin/sh /sledge-payload/run.sh
UNIT
chmod -R a+rX /sledge-payload
reader_pid=''
if [[ -f /sledge-payload/prehook-case ]]; then
 python3 /sledge-payload/prehook_bytes.py --serve > /tmp/prehook-root-reader.log 2>&1 &
 reader_pid=$!
 for ((i=0;i<50;i++)); do [[ ! -S /run/sledge-prehook-reader.sock ]] || break; sleep 0.1; done
 [[ -S /run/sledge-prehook-reader.sock ]]
fi
chvt 7
systemctl daemon-reload
status=0
systemctl start sledge-payload.service || status=$?
echo SLEDGE_PAYLOAD_REPORT_BEGIN
cat /tmp/sledge-payload-output
if [[ -n $reader_pid ]]; then
 kill "$reader_pid" 2>/dev/null || true
 wait "$reader_pid" || true
 cat /tmp/prehook-root-reader.log
fi
if [[ -f /tmp/compositor-output ]]; then cat /tmp/compositor-output; fi
journalctl -b -u sledge-payload.service --no-pager
for log in /tmp/hyprland-prerequisite/hypr/*/hyprland.log; do
  test ! -f "$log" || grep -E 'SLEDGE_RESEARCH_[A-Z_]+|SLEDGE_WORKER|SLEDGE_ROUTER|SLEDGE_PRODUCTION_[A-Z_]+' "$log" || true
done
echo "SLEDGE_PAYLOAD_REPORT_END status=$status qualified=false"
exit "$status"
