#!/usr/bin/env bash
# Only invoke after vm-gpu-prerequisite.sh passes:
# nix shell --no-write-lock-file path:.#hyprland-vm PIN#bubblewrap PIN#coreutils \
#   --command bash scripts/linux/hyprland/vm-run.sh
# Only the selected render node and explicitly opted-in KVM can be host devices.
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 || ( $# == 2 && $2 != --kvm ) ]]; then
  echo 'usage: vm-run.sh /dev/dri/renderDNUMBER [--kvm]' >&2
  exit 2
fi
bash "${SLEDGE_VM_GATE:-$(dirname "$0")/vm-gpu-prerequisite.sh}" "$1"
node=$1
accel=tcg
extra=()
if [[ ${2:-} == --kvm ]]; then
  stat -c '%n %F mode=%a uid=%u gid=%g' /dev/kvm
  [[ -c /dev/kvm ]] || exit 77
  extra=(--dev-bind /dev/kvm /dev/kvm)
  accel=kvm
fi
vm=${SLEDGE_VM_EXECUTABLE:?immutable Nix-generated executable required}
python3 "${SLEDGE_VM_IDENTITY:?runner admission validator required}" "$vm" "$SLEDGE_VM_EXECUTABLE"
: "${SLEDGE_VM_QEMU:?pinned QEMU required}"
sha256sum "$SLEDGE_VM_QEMU" "$vm"
# Namespace-local /tmp is the explicit disposable disk/state root.
timeout --signal=TERM --kill-after=3s 180s \
  bwrap --unshare-all --die-with-parent --new-session --clearenv \
  --ro-bind /nix/store /nix/store --ro-bind /sys /sys --proc /proc --dev /dev --tmpfs /tmp \
  --dir /dev/dri --dev-bind "$node" "$node" "${extra[@]}" \
  --dir /home --dir /home/probe --chdir /tmp \
  "$(command -v env)" -i PATH="$PATH" HOME=/home/probe TMPDIR=/tmp \
    NIX_DISK_IMAGE=/tmp/qualification.qcow2 \
    XDG_RUNTIME_DIR=/tmp \
    GBM_BACKENDS_PATH="${SLEDGE_VM_MESA:?use hyprland-vm-prepare}/lib/gbm" \
    LIBGL_DRIVERS_PATH="$SLEDGE_VM_MESA/lib/dri" \
    __EGL_VENDOR_LIBRARY_FILENAMES="$SLEDGE_VM_MESA/share/glvnd/egl_vendor.d/50_mesa.json" \
    QEMU_OPTS="-machine accel=$accel -vga none -display egl-headless,rendernode=$node" \
    "$vm"
