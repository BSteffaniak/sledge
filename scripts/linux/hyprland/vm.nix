{ pkgs, modulesPath, ... }:
{
  imports = [ "${modulesPath}/virtualisation/qemu-vm.nix" ];
  system.stateVersion = "25.11";
  networking.hostName = "sledge-qualification";
  networking.useDHCP = false;
  virtualisation = {
    memorySize = 4096;
    cores = 2;
    diskSize = 8192;
    graphics = true;
    useNixStoreImage = true;
    mountHostNixStore = false;
    sharedDirectories = pkgs.lib.mkForce { };
    qemu.networkingOptions = pkgs.lib.mkForce [ "-nic none" ];
    qemu.options = [
      "-device virtio-gpu-gl-pci"
      "-serial stdio"
      "-monitor none"
    ];
    qemu.virtioKeyboard = true;
  };
  boot.kernelModules = [ "virtio_gpu" ];
  boot.kernelParams = [ "console=ttyS0" ];
  hardware.graphics.enable = true;
  programs.hyprland = {
    enable = true;
    package = pkgs.hyprland;
  };
  environment.systemPackages = [
    pkgs.hyprland.dev
    pkgs.stdenv.cc
    pkgs.cmake
    pkgs.pkg-config
    pkgs.wayland-utils
    pkgs.weston
  ];
  users.users.probe = {
    isNormalUser = true;
    extraGroups = [
      "video"
      "render"
    ];
  };
  systemd.services.hyprland-prerequisite = {
    wantedBy = [ "multi-user.target" ];
    after = [ "systemd-user-sessions.service" ];
    preStart = "${pkgs.kbd}/bin/chvt 7";
    serviceConfig = {
      Type = "oneshot";
      TimeoutStartSec = 90;
      User = "probe";
      PermissionsStartOnly = true;
      PAMName = "login";
      TTYPath = "/dev/tty7";
      StandardInput = "tty";
      StandardOutput = "journal+console";
      StandardError = "journal+console";
      UtmpIdentifier = "tty7";
    };
    script = ''
      export XDG_RUNTIME_DIR=/tmp/hyprland-prerequisite
      mkdir -m 700 "$XDG_RUNTIME_DIR"
      cat > /tmp/hyprland.conf <<'CONFIG'
      monitor = , 640x480, auto, 1
      debug {
        disable_logs = false
      }
      misc {
        disable_hyprland_logo = true
        disable_splash_rendering = true
      }
      CONFIG
      ${pkgs.hyprland}/bin/Hyprland --config /tmp/hyprland.conf &
      child=$!
      trap 'kill "$child" 2>/dev/null || true; wait "$child" || true' EXIT
      for attempt in $(seq 1 60); do
        for socket in "$XDG_RUNTIME_DIR"/hypr/*/.socket.sock; do
          if [ -S "$socket" ]; then
            export HYPRLAND_INSTANCE_SIGNATURE
            HYPRLAND_INSTANCE_SIGNATURE=$(basename "$(dirname "$socket")")
            {
              echo 'SLEDGE_GUEST_IPC_READY'
              timeout 5 ${pkgs.hyprland}/bin/hyprctl -j version > /tmp/guest-version.json
              timeout 5 ${pkgs.hyprland}/bin/hyprctl -j monitors > /tmp/guest-monitors.json
              cat /tmp/guest-version.json /tmp/guest-monitors.json
              actual=$(readlink -f /proc/$child/exe)
              ${pkgs.python3}/bin/python3 ${./validate_guest.py} /tmp/guest-version.json /tmp/guest-monitors.json "$actual" ${pkgs.hyprland}/bin/.Hyprland-wrapped
            } > /tmp/hyprland-prerequisite-result
            exit 0
          fi
        done
        kill -0 "$child" || exit 77
        sleep 1
      done
      echo 'SLEDGE_GUEST_COMPOSITOR_PREREQUISITE_TIMEOUT'
      exit 77
    '';
  };
  systemd.services."serial-getty@ttyS0".enable = false;
  systemd.services.qualification-report = {
    wantedBy = [ "multi-user.target" ];
    after = [ "hyprland-prerequisite.service" ];
    serviceConfig = {
      StandardOutput = "tty";
      StandardError = "tty";
      TTYPath = "/dev/ttyS0";
    };
    script = ''
      echo 'SLEDGE_GUEST_REPORT_BEGIN'
      ls -l /dev/dri /dev/input || true
      timeout 5 systemctl status hyprland-prerequisite --no-pager || true
      timeout 5 journalctl -b -u hyprland-prerequisite --no-pager || true
      test ! -f /tmp/hyprland-prerequisite-result || cat /tmp/hyprland-prerequisite-result
      for log in /tmp/hyprland-prerequisite/hypr/*/hyprland.log; do
        test ! -f "$log" || grep -E 'Renderer:|Session is|ERR|CRITICAL|Hyprctl' "$log" | tail -20 || true
      done
      echo 'SLEDGE_GUEST_REPORT_END_NOT_PARITY'
      systemctl poweroff
    '';
  };
  # No capture, plugin auto-load, host shares, or qualification verdict.
  # Independent input consumer and lock-client tests remain future work.
}
