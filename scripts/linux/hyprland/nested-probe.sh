#!/usr/bin/env bash
# nix shell --no-write-lock-file .#hyprland-sdk --command \
#   sledge-hyprland-sdk bash scripts/linux/hyprland/nested-probe.sh PROBE_SO
# Device-free private Weston parent. Never uses a host display or session bus.
set -euo pipefail
: "${SLEDGE_HYPRLAND_RUNTIME:?run through sledge-hyprland-sdk}"
if [[ $# != 1 || ! -f $1 ]]; then
  echo 'usage: nested-probe.sh PROBE_SO' >&2
  exit 2
fi
probe=$(realpath "$1")
runner=$(realpath "$(dirname "$0")/nested-probe-inner.sh")
sha256sum "$probe" "$SLEDGE_HYPRLAND_RUNTIME/bin/.Hyprland-wrapped"
timeout --signal=TERM --kill-after=3s 40s \
  bwrap --unshare-all --die-with-parent --new-session --clearenv \
  --ro-bind /nix/store /nix/store --proc /proc --dev /dev --tmpfs /tmp \
  --dir /home --dir /home/probe --dir /run --dir /run/user \
  --ro-bind "$probe" /probe.so --ro-bind "$runner" /runner.sh \
  --setenv PATH "$PATH" --setenv HOME /home/probe \
  --setenv XDG_RUNTIME_DIR /tmp/runtime \
  --setenv SLEDGE_HYPRLAND_RUNTIME "$SLEDGE_HYPRLAND_RUNTIME" \
  --setenv LIBGL_ALWAYS_SOFTWARE 1 --setenv GALLIUM_DRIVER llvmpipe \
  --setenv EGL_PLATFORM wayland \
  env -i PATH="$PATH" HOME=/home/probe XDG_RUNTIME_DIR=/tmp/runtime \
    SLEDGE_HYPRLAND_RUNTIME="$SLEDGE_HYPRLAND_RUNTIME" SLEDGE_PRIVATE_NESTED=1 \
    LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe EGL_PLATFORM=wayland \
    bash /runner.sh
