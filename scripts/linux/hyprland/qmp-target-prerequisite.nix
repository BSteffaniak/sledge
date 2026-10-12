# Focused control-plane prerequisite only; no native/Rust source dependency.
let
  pkgs = import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/b6018f87da91d19d0ab4cf979885689b469cdd41.tar.gz";
    sha256 = "sha256-twXPFqFsrrY5r28Zh7Homgcp2gUMBgQ6WDS98Q/3xFI=";
  }) { system = "x86_64-linux"; };
  scripts = builtins.path {
    path = ./.;
    name = "sledge-qmp-target-prerequisite";
  };
in
pkgs.writeShellApplication {
  name = "sledge-qmp-target-check";
  runtimeInputs = [
    pkgs.bubblewrap
    pkgs.coreutils
    pkgs.python3
    pkgs.qemu
  ];
  text = ''
    test -c /dev/kvm
    exec timeout 20 bwrap --unshare-all --die-with-parent --new-session --clearenv \
      --ro-bind /nix/store /nix/store --proc /proc --dev /dev --tmpfs /tmp \
      --dev-bind /dev/kvm /dev/kvm ${pkgs.coreutils}/bin/env -i \
      SLEDGE_VM_QEMU=${pkgs.qemu}/bin/qemu-system-x86_64 \
      ${pkgs.python3}/bin/python3 ${scripts}/qmp_target_ready.py
  '';
}
