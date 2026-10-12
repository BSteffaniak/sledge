{
  description = "sledge: low-level keyboard-event daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        # Separate SDK pin: reproduces the NixOS 25.11 host's exact Hyprland
        # derivation without updating the workspace's flake.lock.
        hyprlandPkgs = import (builtins.fetchTarball {
          url = "https://github.com/NixOS/nixpkgs/archive/b6018f87da91d19d0ab4cf979885689b469cdd41.tar.gz";
          sha256 = "sha256-twXPFqFsrrY5r28Zh7Homgcp2gUMBgQ6WDS98Q/3xFI=";
        }) { inherit system; };
        sdkInputs = hyprlandPkgs.lib.closePropagation (
          [ hyprlandPkgs.hyprland.dev ] ++ map hyprlandPkgs.lib.getDev hyprlandPkgs.hyprland.buildInputs
        );
        sdk = hyprlandPkgs.writeShellApplication {
          name = "sledge-hyprland-sdk";
          runtimeInputs = [
            hyprlandPkgs.stdenv.cc
            hyprlandPkgs.cmake
            hyprlandPkgs.ninja
            hyprlandPkgs.pkg-config-unwrapped
            hyprlandPkgs.python3
            hyprlandPkgs.coreutils
            hyprlandPkgs.binutils
            hyprlandPkgs.hyprland
            hyprlandPkgs.bubblewrap
            hyprlandPkgs.weston
          ];
          text = ''
            # Deliberately clear loader/compiler overrides; this is not a sandbox.
            unset LD_PRELOAD LD_LIBRARY_PATH LIBRARY_PATH CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH
            unset CFLAGS CXXFLAGS CPPFLAGS LDFLAGS NIX_LDFLAGS NIX_CFLAGS_LINK
            export CC=${hyprlandPkgs.stdenv.cc}/bin/cc
            export CXX=${hyprlandPkgs.stdenv.cc}/bin/c++
            export PKG_CONFIG_PATH="${hyprlandPkgs.lib.makeSearchPath "lib/pkgconfig" sdkInputs}:${hyprlandPkgs.lib.makeSearchPath "share/pkgconfig" sdkInputs}"
            export NIX_CFLAGS_COMPILE="${
              hyprlandPkgs.lib.concatMapStringsSep " " (input: "-isystem ${input}/include") sdkInputs
            }"
            export SLEDGE_HYPRLAND_SDK=${hyprlandPkgs.hyprland.dev}
            export SLEDGE_HYPRLAND_RUNTIME=${hyprlandPkgs.hyprland}
            export SLEDGE_HYPRLAND_DRV=${builtins.unsafeDiscardStringContext hyprlandPkgs.hyprland.drvPath}
            export SLEDGE_HYPRLAND_NIXPKGS=b6018f87da91d19d0ab4cf979885689b469cdd41
            if [ "$#" -eq 0 ]; then
              echo 'usage: sledge-hyprland-sdk COMMAND [ARG...]' >&2
              exit 2
            fi
            exec "$@"
          '';
        };
        vm =
          (import "${hyprlandPkgs.path}/nixos" {
            inherit system;
            configuration = {
              virtualisation.qemu.package = hyprlandPkgs.qemu;
              imports = [ ./scripts/linux/hyprland/vm.nix ];
              environment.systemPackages = [ sdk ];
            };
          }).vm;
      in
      {
        packages = pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          hyprland-sdk = sdk;
          # Stock build is the host derivation; patched build has a distinct ABI.
          hyprland-source-build = hyprlandPkgs.hyprland;
          hyprland-lock-smoke = hyprlandPkgs.hyprland.overrideAttrs (old: {
            pname = "hyprland-lock-smoke";
            patches = (old.patches or [ ]) ++ [ ./native/hyprland/lock-boundary.patch ];
          });
          hyprland-vm = vm;
          hyprland-vm-prepare = hyprlandPkgs.writeShellApplication {
            name = "sledge-hyprland-vm-prepare";
            runtimeInputs = [
              hyprlandPkgs.nix
              hyprlandPkgs.qemu
              hyprlandPkgs.bubblewrap
              hyprlandPkgs.coreutils
              hyprlandPkgs.python3
              hyprlandPkgs.bash
            ];
            text = ''
              unset LD_PRELOAD LD_LIBRARY_PATH LIBRARY_PATH CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH
              unset CC CXX CFLAGS CXXFLAGS CPPFLAGS LDFLAGS NIX_CFLAGS_COMPILE NIX_LDFLAGS
              export SLEDGE_VM_EXECUTABLE=${builtins.unsafeDiscardStringContext vm.outPath}/bin/run-sledge-qualification-vm
              export SLEDGE_VM_IDENTITY=${./scripts/linux/hyprland/runner_identity.py}
              export SLEDGE_VM_QEMU=${hyprlandPkgs.qemu}/bin/qemu-system-x86_64
              export SLEDGE_VM_QMP=${./scripts/linux/hyprland/qmp_ready.py}
              export SLEDGE_VM_MESA=${hyprlandPkgs.mesa}
              export SLEDGE_VM_GATE=${./scripts/linux/hyprland/vm-gpu-prerequisite.sh}
              export SLEDGE_VM_RUNNER=${./scripts/linux/hyprland/vm-run.sh}
              exec bash ${./scripts/linux/hyprland/vm-prepare.sh} "$@"
            '';
          };
          hyprland-stock-qualification =
            (import ./native/hyprland/stock-hook-investigation.nix {
              inherit system;
              peer = self.packages.${system}.hyprland-probe-peer;
            }).runner;
          hyprland-stock-research =
            (import ./native/hyprland/stock-hook-investigation.nix {
              inherit system;
              peer = self.packages.${system}.hyprland-probe-peer;
            }).researchRunner;
          hyprland-stock-replacement =
            (import ./native/hyprland/stock-hook-investigation.nix {
              inherit system;
              peer = self.packages.${system}.hyprland-probe-peer;
            }).replacementRunner;
          hyprland-stock-preflight =
            (import ./native/hyprland/stock-hook-investigation.nix {
              inherit system;
              peer = self.packages.${system}.hyprland-probe-peer;
            }).preflightRunner;
          hyprland-stock-mods-failure =
            (import ./native/hyprland/stock-hook-investigation.nix {
              inherit system;
              peer = self.packages.${system}.hyprland-probe-peer;
            }).modsFailureRunner;
          hyprland-stock-down-failure =
            (import ./native/hyprland/stock-hook-investigation.nix {
              inherit system;
              peer = self.packages.${system}.hyprland-probe-peer;
            }).downFailureRunner;
          hyprland-stock-rollback =
            (import ./native/hyprland/stock-hook-investigation.nix {
              inherit system;
              peer = self.packages.${system}.hyprland-probe-peer;
            }).rollbackRunner;
          hyprland-probe-peer =
            (pkgs.makeRustPlatform {
              cargo = rustToolchain;
              rustc = rustToolchain;
            }).buildRustPackage
              {
                pname = "sledge-hyprland-probe-peer";
                version = "0";
                src = pkgs.lib.cleanSourceWith {
                  src = ./.;
                  filter =
                    path: type:
                    let
                      name = builtins.baseNameOf path;
                    in
                    name != "target" && name != ".git" && type != "symlink";
                };
                cargoLock.lockFile = ./Cargo.lock;
                cargoBuildFlags = [
                  "-p"
                  "sledge_linux"
                  "--example"
                  "hyprland_probe_peer"
                  "--example"
                  "hyprland_research_peer"
                ];
                # This is a passive guest fixture, not a qualified runtime backend.
                # Workspace and behavioral checks are run separately by contributors.
                doCheck = false;
                installPhase = ''
                  runHook preInstall
                  mkdir -p "$out/bin"
                  cp target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/examples/hyprland_probe_peer "$out/bin/"
                  cp target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/examples/hyprland_research_peer "$out/bin/"
                  runHook postInstall
                '';
              };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-deny
            pkgs.cargo-machete
            pkgs.cargo-nextest
            pkgs.pkg-config
            pkgs.taplo
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];

          RUST_BACKTRACE = "1";

          shellHook = ''
            echo "sledge development environment loaded"
            echo "  rustc     $(rustc --version)"
            echo "  cargo     $(cargo --version)"
          '';
        };
      }
    );
}
