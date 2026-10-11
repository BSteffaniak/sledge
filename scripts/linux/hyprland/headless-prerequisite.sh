#!/usr/bin/env bash
# Run through: nix shell --no-write-lock-file .#hyprland-sdk --command \
#   sledge-hyprland-sdk bash scripts/linux/hyprland/headless-prerequisite.sh
# No desktop sockets, host devices, services, home, or network are visible.
set -euo pipefail
: "${SLEDGE_HYPRLAND_RUNTIME:?run through sledge-hyprland-sdk}"
# The single-quoted child script expands variables inside the private namespace.
# shellcheck disable=SC2016
bwrap --unshare-all --die-with-parent --new-session --clearenv \
  --ro-bind /nix/store /nix/store --proc /proc --dev /dev --tmpfs /tmp \
  --dir /home --dir /home/probe --dir /run --dir /run/user \
  --setenv PATH "$PATH" --setenv HOME /home/probe \
  --setenv XDG_RUNTIME_DIR /tmp/runtime \
  --setenv SLEDGE_HYPRLAND_RUNTIME "$SLEDGE_HYPRLAND_RUNTIME" \
  --setenv LIBGL_ALWAYS_SOFTWARE 1 --setenv WLR_RENDERER_ALLOW_SOFTWARE 1 \
  bash -c '
    set -eu
    ulimit -c 0
    mkdir -m 700 /tmp/runtime
    printf "%s\n" "monitor = , 640x480, auto, 1" "misc {" \
      "disable_hyprland_logo = true" "disable_splash_rendering = true" "}" \
      > /tmp/hyprland.conf
    timeout --signal=TERM --kill-after=2s 10s \
      "$SLEDGE_HYPRLAND_RUNTIME/bin/Hyprland" --config /tmp/hyprland.conf
  '
