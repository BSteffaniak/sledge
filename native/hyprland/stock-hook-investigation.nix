{
  peer,
  system ? builtins.currentSystem,
}:
let
  pkgs = import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/b6018f87da91d19d0ab4cf979885689b469cdd41.tar.gz";
    sha256 = "sha256-twXPFqFsrrY5r28Zh7Homgcp2gUMBgQ6WDS98Q/3xFI=";
  }) { inherit system; };
  plugin = pkgs.stdenv.mkDerivation {
    pname = "sledge-stock-hook-investigation";
    version = "0";
    src = ./.;
    dontUnpack = true;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.hyprland.dev ] ++ pkgs.hyprland.buildInputs;
    buildPhase = ''
      export PKG_CONFIG_PATH=${pkgs.hyprland.dev}/share/pkgconfig:$PKG_CONFIG_PATH
      c++ -std=c++23 -O2 -Wall -Wextra -Werror -fPIC -shared \
        $(pkg-config --cflags hyprland) "$src/stock_hook_probe.cpp" -o probe.so
    '';
    installPhase = "mkdir -p $out; cp probe.so $out/probe.so";
  };
  # Explicit coordinator-built Rust closure: no guest Cargo/network realization.
  passivePlugin = pkgs.stdenv.mkDerivation {
    pname = "sledge-stock-passive-adapter";
    version = "1";
    src = ./.;
    dontUnpack = true;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.hyprland.dev ] ++ pkgs.hyprland.buildInputs;
    buildPhase = ''
      export PKG_CONFIG_PATH=${pkgs.hyprland.dev}/share/pkgconfig:$PKG_CONFIG_PATH
      c++ -std=c++23 -O2 -Wall -Wextra -Werror -pthread -fPIC -shared \
        $(pkg-config --cflags hyprland) "$src/stock_adapter.cpp" -o passive.so
      c++ -std=c++23 -O2 -Wall -Wextra -Werror \
        $(pkg-config --cflags hyprland) "$src/stock_sdk_identity.cpp" -o stock-sdk-identity
    '';
    installPhase = "mkdir -p $out/bin; cp passive.so $out/passive.so; cp stock-sdk-identity $out/bin/stock-sdk-identity";
  };
  researchPlugin = passivePlugin.overrideAttrs (_: {
    pname = "sledge-stock-lock-quarantine-research";
    buildPhase = ''
      export PKG_CONFIG_PATH=${pkgs.hyprland.dev}/share/pkgconfig:$PKG_CONFIG_PATH
      c++ -std=c++23 -O2 -Wall -Wextra -Werror -pthread -fPIC -shared -DSLEDGE_STOCK_RESEARCH \
        $(pkg-config --cflags hyprland) "$src/stock_adapter.cpp" -o passive.so
      c++ -std=c++23 -O2 -Wall -Wextra -Werror \
        $(pkg-config --cflags hyprland) "$src/stock_sdk_identity.cpp" -o stock-sdk-identity
    '';
  });
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
  imeClient = pkgs.stdenv.mkDerivation {
    pname = "sledge-stock-ime-grab-client";
    version = "0";
    src = ../../scripts/linux/hyprland/ime-grab-client.c;
    dontUnpack = true;
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.wayland-scanner
    ];
    buildInputs = [ pkgs.wayland ];
    buildPhase = ''
      xml=${pkgs.hyprland.src}/protocols/input-method-unstable-v2.xml
      wayland-scanner client-header "$xml" input-method-client.h
      wayland-scanner private-code "$xml" input-method-code.c
      cc -std=c11 -O2 -Wall -Wextra -Werror -I. $(pkg-config --cflags wayland-client) \
        -x c "$src" input-method-code.c $(pkg-config --libs wayland-client) -o ime-grab-client
    '';
    installPhase = "mkdir -p $out/bin; cp ime-grab-client $out/bin/ime-grab-client";
  };
  makeVm =
    rollbackOnly: research: replacement:
    (import "${pkgs.path}/nixos" {
      system = "x86_64-linux";
      configuration = { lib, ... }: {
        imports = [ ../../scripts/linux/hyprland/vm.nix ];
        virtualisation.qemu.package = pkgs.qemu;
        virtualisation.qemu.options = [ "-device qemu-xhci,id=qualification-usb" ];
        users.users.probe.extraGroups = lib.mkAfter [ "dialout" ];
        environment.systemPackages = [
          pkgs.xorg.xev
          pkgs.wev
          pkgs.wtype
          pkgs.python3
        ];
        systemd.services.hyprland-prerequisite.script = lib.mkForce ''
          export SLEDGE_REPLACEMENT_FIXTURE=${if replacement then "1" else "0"}
          export SLEDGE_REPLACEMENT_SCRIPT=${../../scripts/linux/hyprland/stock-replacement-guest.sh}
          export SLEDGE_RESEARCH_PEER=${peer}/bin/hyprland_research_peer
          export SLEDGE_RESEARCH_FIXTURE=${if research then "1" else "0"}
          export SLEDGE_RESEARCH_PLUGIN=${researchPlugin}/passive.so
          export SLEDGE_RESEARCH_SCRIPT=${../../scripts/linux/hyprland/stock-research-guest.sh}
          export SLEDGE_STOCK_ROLLBACK_ONLY=${if rollbackOnly then "1" else "0"}
          export SLEDGE_STOCK_PLUGIN=${plugin}/probe.so
          export SLEDGE_PASSIVE_PLUGIN=${passivePlugin}/passive.so
          export SLEDGE_SDK_IDENTITY=${passivePlugin}/bin/stock-sdk-identity
          export SLEDGE_PASSIVE_PEER=${peer}/bin/hyprland_probe_peer
          export SLEDGE_IME_CLIENT=${imeClient}/bin/ime-grab-client
          export SLEDGE_LOCK_CLIENT=${lockClient}/bin/lock-client
          ${pkgs.bash}/bin/bash ${../../scripts/linux/hyprland/stock-qualification-guest.sh} > /tmp/hyprland-prerequisite-result 2>&1
        '';
        systemd.services.qualification-report.script = lib.mkForce ''
          echo SLEDGE_STOCK_HOOK_REPORT_BEGIN
          for file in /tmp/hyprland-prerequisite-result /tmp/stock-hook-evidence /tmp/stock-hook-events \
            /tmp/stock-consumer-events /tmp/stock-second-consumer-events /tmp/stock-lock-events \
            /tmp/stock-crash-events /tmp/stock-relock-events /tmp/stock-ime-events /tmp/stock-passive-peer-events \
            /tmp/research-lock-events /tmp/research-ime-events /tmp/research-ime-lock-events \
            /tmp/replacement-peer-events /tmp/replacement-xev-events; do
            echo "EVIDENCE_FILE $file"
            test ! -f "$file" || cat "$file"
          done
          for log in /tmp/hyprland-prerequisite/hypr/*/hyprland.log; do
            test ! -f "$log" || grep -E 'SLEDGE_RESEARCH_REGISTER|SLEDGE_RESEARCH_EXIT' "$log" || true
          done
          journalctl -b -u hyprland-prerequisite --no-pager | grep -E 'SLEDGE_|hook|Hook|plugin|Plugin|error' || true
          echo SLEDGE_STOCK_HOOK_REPORT_END_NOT_PARITY
          systemctl poweroff
        '';
      };
    }).vm;
  vm = makeVm false false false;
  rollbackVm = makeVm true false false;
  researchVm = makeVm false true false;
  replacementVm = makeVm false false true;
  makeRunner =
    guest:
    pkgs.writeShellApplication {
      name = "sledge-stock-qualification-vm";
      runtimeInputs = [
        pkgs.qemu
        pkgs.bubblewrap
        pkgs.coreutils
        pkgs.python3
        pkgs.bash
      ];
      text = ''
        export SLEDGE_VM_EXECUTABLE=${guest}/bin/run-sledge-qualification-vm
        export SLEDGE_VM_IDENTITY=${../../scripts/linux/hyprland/runner_identity.py}
        export SLEDGE_VM_QEMU=${pkgs.qemu}/bin/qemu-system-x86_64
        export SLEDGE_VM_QMP=${../../scripts/linux/hyprland/qmp_ready.py}
        export SLEDGE_VM_MESA=${pkgs.mesa}
        export SLEDGE_VM_GATE=${../../scripts/linux/hyprland/vm-gpu-prerequisite.sh}
        export SLEDGE_VM_DRIVER=${../../scripts/linux/hyprland/qmp_qualification.py}
        exec bash ${../../scripts/linux/hyprland/vm-run.sh} "$@"
      '';
    };
  replacementRunner = makeRunner replacementVm;
  researchRunner = makeRunner researchVm;
  runner = makeRunner vm;
  rollbackRunner = makeRunner rollbackVm;
in
{
  inherit
    replacementRunner
    replacementVm
    researchPlugin
    researchVm
    researchRunner
    plugin
    passivePlugin
    lockClient
    imeClient
    vm
    runner
    rollbackVm
    rollbackRunner
    ;
}
