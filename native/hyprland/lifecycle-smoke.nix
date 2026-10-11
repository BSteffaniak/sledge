let
  pkgs = import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/b6018f87da91d19d0ab4cf979885689b469cdd41.tar.gz";
    sha256 = "sha256-twXPFqFsrrY5r28Zh7Homgcp2gUMBgQ6WDS98Q/3xFI=";
  }) { };
in
pkgs.hyprland.overrideAttrs (old: {
  pname = "hyprland-delivered-lifecycle-smoke";
  patches = (old.patches or [ ]) ++ [
    ./lock-boundary.patch
    ./routing.patch
  ];
})
