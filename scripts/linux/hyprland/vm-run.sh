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
driver=()
if [[ -n ${SLEDGE_VM_DRIVER:-} ]]; then
  if [[ $SLEDGE_VM_DRIVER == /nix/store/* && -f $SLEDGE_VM_DRIVER ]]; then
    driver=(python3 "$SLEDGE_VM_DRIVER")
  elif [[ -n ${SLEDGE_VM_PAYLOAD_SCRATCH:-} && $SLEDGE_VM_DRIVER == "$SLEDGE_VM_PAYLOAD_SCRATCH/driver.py" && -f $SLEDGE_VM_DRIVER && ! -L $SLEDGE_VM_DRIVER ]]; then
    sha256sum "$SLEDGE_VM_DRIVER"
    driver=(python3 /scratch/driver.py)
  else exit 77; fi
fi
# Optional prepopulated payload root. Only a fresh dedicated /hdd scratch is bound.
scratch=()
kernel_params=''
disk=/tmp/qualification.qcow2
if [[ -n ${SLEDGE_VM_PAYLOAD_SCRATCH:-} ]]; then
  [[ $SLEDGE_VM_PAYLOAD_SCRATCH == /hdd/* && ! -L $SLEDGE_VM_PAYLOAD_SCRATCH && -d $SLEDGE_VM_PAYLOAD_SCRATCH ]] || exit 77
  [[ $(realpath "$SLEDGE_VM_PAYLOAD_SCRATCH") == "$SLEDGE_VM_PAYLOAD_SCRATCH" ]] || exit 77
  python3 "$(dirname "$0")/payload_disk.py" --verify "$SLEDGE_VM_PAYLOAD_SCRATCH/root/sledge-payload"
  [[ -f $SLEDGE_VM_PAYLOAD_SCRATCH/qualification.qcow2 && ! -L $SLEDGE_VM_PAYLOAD_SCRATCH/qualification.qcow2 ]] || exit 77
  scratch=(--bind "$SLEDGE_VM_PAYLOAD_SCRATCH" /scratch)
  disk=/scratch/qualification.qcow2
  base_init=$(grep -oE 'init=/nix/store/[a-zA-Z0-9.+_-]+/init' "$vm" | head -1)
  [[ -n $base_init ]] || exit 77
  kernel_params="init=/payload-init.sh sledge.base_init=${base_init#init=} systemd.mask=hyprland-prerequisite.service systemd.mask=qualification-report.service"
  if [[ -f $SLEDGE_VM_PAYLOAD_SCRATCH/root/sledge-payload/scope/machine-id ]]; then
    [[ $(cat "$SLEDGE_VM_PAYLOAD_SCRATCH/root/sledge-payload/scope/machine-id") == 736c656467652d63616e646964617465 ]] || exit 77
    kernel_params+=" systemd.machine_id=736c656467652d63616e646964617465"
  fi
fi
# Namespace-local /tmp remains disposable; payload disk/state uses explicit scratch.
mixed_opts=''
if [[ -n ${SLEDGE_VM_PAYLOAD_SCRATCH:-} ]] && { [[ -f $SLEDGE_VM_PAYLOAD_SCRATCH/root/sledge-payload/tap-case ]] || [[ -f $SLEDGE_VM_PAYLOAD_SCRATCH/root/sledge-payload/mixed-case ]] || [[ -f $SLEDGE_VM_PAYLOAD_SCRATCH/root/sledge-payload/different-map-case ]]; }; then
 mixed_opts='-device secondary-vga,id=display-a -device secondary-vga,id=display-b -device usb-kbd,bus=qualification-usb.0,id=mixed-usb,display=display-a -device virtio-keyboard-pci,id=mixed-virtio,display=display-b'
fi
if [[ -f $SLEDGE_VM_PAYLOAD_SCRATCH/root/sledge-payload/tap-case ]]; then
 mixed_opts='-device secondary-vga,id=display-a -device secondary-vga,id=display-b -device usb-kbd,bus=qualification-usb.0,id=keyboard-a,display=display-a -device usb-kbd,bus=qualification-usb.0,id=keyboard-b,display=display-b'
fi
timeout --signal=TERM --kill-after=3s 180s \
  bwrap --unshare-all --die-with-parent --new-session --clearenv \
  --ro-bind /nix/store /nix/store --ro-bind /sys /sys --proc /proc --dev /dev --tmpfs /tmp \
  --dir /dev/dri --dev-bind "$node" "$node" "${extra[@]}" \
  --dir /home --dir /home/probe "${scratch[@]}" --chdir /tmp \
  "$(command -v env)" -i PATH="$PATH" HOME=/home/probe TMPDIR=/tmp \
    NIX_DISK_IMAGE="$disk" \
    QEMU_KERNEL_PARAMS="$kernel_params" \
    XDG_RUNTIME_DIR=/tmp \
    GBM_BACKENDS_PATH="${SLEDGE_VM_MESA:?use hyprland-vm-prepare}/lib/gbm" \
    LIBGL_DRIVERS_PATH="$SLEDGE_VM_MESA/lib/dri" \
    __EGL_VENDOR_LIBRARY_FILENAMES="$SLEDGE_VM_MESA/share/glvnd/egl_vendor.d/50_mesa.json" \
    QEMU_OPTS="-machine accel=$accel -vga none -display egl-headless,rendernode=$node $mixed_opts" \
    "${driver[@]}" "$vm"
