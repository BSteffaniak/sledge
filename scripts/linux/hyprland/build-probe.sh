#!/usr/bin/env bash
# nix shell --no-write-lock-file .#hyprland-sdk --command \
#   sledge-hyprland-sdk bash scripts/linux/hyprland/build-probe.sh OUTPUT_DIRECTORY
# Builds a non-capturing API probe and tests rejection, not compositor parity.
set -euo pipefail
: "${SLEDGE_HYPRLAND_SDK:?run through sledge-hyprland-sdk}"
: "${CXX:?run through sledge-hyprland-sdk}"
if [[ $# != 1 ]]; then
  echo 'usage: build-probe.sh OUTPUT_DIRECTORY' >&2
  exit 2
fi
root=$(cd "$(dirname "$0")/../../.." && pwd)
mkdir -p "$1"
out=$(cd "$1" && pwd)
read -r -a flags <<< "$(pkg-config --cflags hyprland aquamarine hyprutils hyprlang hyprcursor hyprgraphics pixman-1 libdrm libinput xkbcommon wayland-server cairo pango pangocairo)"
"$CXX" -std=c++23 -O2 -Wall -Wextra -Werror -fPIC -shared \
  "${flags[@]}" "$root/native/hyprland/probe.cpp" -o "$out/sledge-hyprland-probe.so"
"$CXX" -std=c++23 -O2 -Wall -Wextra -Werror \
  "$root/native/hyprland/probe_test.cpp" -ldl -o "$out/probe-test"
"$CXX" -std=c++23 -O2 -Wall -Wextra -Werror \
  "$root/native/hyprland/routing_test.cpp" -o "$out/routing-test"
env -i "$out/routing-test"
"$CXX" -std=c++23 -O1 -g -Wall -Wextra -Werror \
  -fsanitize=address,undefined -fno-omit-frame-pointer \
  "$root/native/hyprland/routing_test.cpp" -o "$out/routing-test-sanitized"
env -i ASAN_OPTIONS=halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1 \
  "$out/routing-test-sanitized"
echo 'Deterministic routing tests passed; not compositor qualification.'
sha256sum "$out/sledge-hyprland-probe.so" \
  "$SLEDGE_HYPRLAND_RUNTIME/bin/.Hyprland-wrapped"
if "$out/probe-test" "$out/sledge-hyprland-probe.so"; then
  echo 'Standalone probe test passed; compositor qualification remains unestablished.'
else
  status=$?
  if [[ $status == 77 ]]; then
    echo 'BLOCKED: standalone plugin load unavailable; not a qualified runtime PASS.' >&2
  fi
  exit "$status"
fi
