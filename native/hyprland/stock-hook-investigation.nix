let
  pkgs = import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/b6018f87da91d19d0ab4cf979885689b469cdd41.tar.gz";
    sha256 = "sha256-twXPFqFsrrY5r28Zh7Homgcp2gUMBgQ6WDS98Q/3xFI=";
  }) { };
  plugin = pkgs.stdenv.mkDerivation {
    pname = "sledge-stock-hook-investigation";
    version = "0";
    src = ./stock_hook_probe.cpp;
    dontUnpack = true;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.hyprland.dev ] ++ pkgs.hyprland.buildInputs;
    buildPhase = ''
      export PKG_CONFIG_PATH=${pkgs.hyprland.dev}/share/pkgconfig:$PKG_CONFIG_PATH
      c++ -std=c++23 -O2 -Wall -Wextra -Werror -fPIC -shared \
        $(pkg-config --cflags hyprland) -x c++ "$src" -o probe.so
    '';
    installPhase = "mkdir -p $out; cp probe.so $out/probe.so";
  };
  lockClient = pkgs.stdenv.mkDerivation {
    pname = "sledge-stock-lock-client";
    version = "0";
    src = ./stock_lock_client.c;
    dontUnpack = true;
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.wayland-scanner
    ];
    buildInputs = [ pkgs.wayland ];
    buildPhase = ''
      xml=${pkgs.wayland-protocols}/share/wayland-protocols/staging/ext-session-lock/ext-session-lock-v1.xml
      wayland-scanner client-header "$xml" session-lock-client.h
      wayland-scanner private-code "$xml" session-lock-code.c
      cc -std=c11 -O2 -Wall -Wextra -Werror -I. $(pkg-config --cflags wayland-client) \
        -x c "$src" session-lock-code.c $(pkg-config --libs wayland-client) -o lock-client
    '';
    installPhase = "mkdir -p $out/bin; cp lock-client $out/bin/lock-client";
  };
  vm =
    (import "${pkgs.path}/nixos" {
      system = "x86_64-linux";
      configuration = { lib, ... }: {
        imports = [ ../../scripts/linux/hyprland/vm.nix ];
        virtualisation.qemu.package = pkgs.qemu;
        systemd.services.hyprland-prerequisite.script = lib.mkForce ''
          export XDG_RUNTIME_DIR=/tmp/hyprland-prerequisite
          mkdir -m 700 "$XDG_RUNTIME_DIR"
          printf 'monitor = ,preferred,auto,1\nmisc:disable_hyprland_logo = true\n' > /tmp/hyprland.conf
          ${pkgs.hyprland}/bin/Hyprland --config /tmp/hyprland.conf &
          child=$!
          trap 'kill "$child" 2>/dev/null || true; wait "$child" 2>/dev/null || true' EXIT
          for attempt in $(seq 1 40); do
            for socket in "$XDG_RUNTIME_DIR"/hypr/*/.socket.sock; do
              if test -S "$socket"; then
                export HYPRLAND_INSTANCE_SIGNATURE=$(basename "$(dirname "$socket")")
                {
                  ${pkgs.hyprland}/bin/hyprctl -j version
                  sha256sum ${pkgs.hyprland}/bin/.Hyprland-wrapped ${plugin}/probe.so
                  ${pkgs.hyprland}/bin/hyprctl plugin load ${plugin}/probe.so
                  ${pkgs.hyprland}/bin/hyprctl -j plugin list
                  export WAYLAND_DISPLAY=$(basename "$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'wayland-*' | head -1)")
                  ${pkgs.coreutils}/bin/stdbuf -oL -eL ${pkgs.wev}/bin/wev > /tmp/stock-consumer-events 2>&1 &
                  consumer=$!
                  for ready in $(seq 1 20); do
                    ${pkgs.hyprland}/bin/hyprctl -j clients > /tmp/stock-clients.json
                    if ${pkgs.python3}/bin/python3 -c 'import json; import sys; sys.exit(not any(c.get("class") == "wev" for c in json.load(open("/tmp/stock-clients.json"))))'; then break; fi
                    sleep 0.1
                  done
                  ${pkgs.hyprland}/bin/hyprctl dispatch focuswindow class:wev
                  timeout 5 ${pkgs.wtype}/bin/wtype -M shift -k a -k b -m shift
                  sleep 0.2
                  timeout 5 ${lockClient}/bin/lock-client
                  kill "$consumer"
                  wait "$consumer" || true
                  cat /tmp/stock-consumer-events
                  ${pkgs.hyprland}/bin/hyprctl plugin unload ${plugin}/probe.so
                  ${pkgs.hyprland}/bin/hyprctl -j plugin list
                } > /tmp/hyprland-prerequisite-result 2>&1
                exit 0
              fi
            done
            kill -0 "$child" || exit 77
            sleep 1
          done
          exit 77
        '';
        systemd.services.qualification-report.script = lib.mkForce ''
          echo SLEDGE_STOCK_HOOK_REPORT_BEGIN
          test ! -f /tmp/hyprland-prerequisite-result || cat /tmp/hyprland-prerequisite-result
          test ! -f /tmp/stock-hook-evidence || cat /tmp/stock-hook-evidence
          test ! -f /tmp/stock-hook-events || cat /tmp/stock-hook-events
          journalctl -b -u hyprland-prerequisite --no-pager | grep -E 'SLEDGE_|hook|Hook|plugin|Plugin|error' || true
          for log in /tmp/hyprland-prerequisite/hypr/*/hyprland.log; do
            test ! -f "$log" || grep -E 'SLEDGE_|hook|Hook|plugin|Plugin' "$log" || true
          done
          echo SLEDGE_STOCK_HOOK_REPORT_END_NOT_INPUT_QUALIFICATION
          systemctl poweroff
        '';
      };
    }).vm;
in
{
  inherit plugin lockClient vm;
}
