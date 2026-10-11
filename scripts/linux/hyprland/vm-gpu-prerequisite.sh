#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 1 || ! $1 =~ ^/dev/dri/renderD[0-9]+$ || ! -c $1 || -L $1 ]]; then
  echo 'usage: vm-gpu-prerequisite.sh /dev/dri/renderDNUMBER' >&2
  exit 2
fi
node=$1
: "${SLEDGE_VM_QEMU:?pinned QEMU required}"
: "${SLEDGE_VM_QMP:?pinned QMP script required}"
stat -c '%n %F mode=%a uid=%u gid=%g major=%t minor=%T' "$node"
timeout --signal=TERM --kill-after=2s 10s \
  bwrap --unshare-all --die-with-parent --new-session --clearenv \
  --ro-bind /nix/store /nix/store --ro-bind /sys /sys --proc /proc --dev /dev --tmpfs /tmp \
  --dir /dev/dri --dev-bind "$node" "$node" --dir /home --dir /home/probe \
  "$(command -v env)" -i PATH="$PATH" HOME=/home/probe XDG_RUNTIME_DIR=/tmp \
    GBM_BACKENDS_PATH="${SLEDGE_VM_MESA:?}/lib/gbm" \
    LIBGL_DRIVERS_PATH="$SLEDGE_VM_MESA/lib/dri" \
    __EGL_VENDOR_LIBRARY_FILENAMES="$SLEDGE_VM_MESA/share/glvnd/egl_vendor.d/50_mesa.json" \
    SLEDGE_VM_QEMU="$SLEDGE_VM_QEMU" \
    python3 "$SLEDGE_VM_QMP" "$node"
