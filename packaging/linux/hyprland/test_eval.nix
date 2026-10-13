# Evaluation-only checks: no accepted contract, native build or services started.
{ lib }:
let
  module = import ./module.nix;
  base = {
    options.environment.systemPackages = lib.mkOption {
      type = lib.types.listOf lib.types.anything;
      default = [ ];
    };
    options.assertions = lib.mkOption {
      type = lib.types.listOf lib.types.anything;
      default = [ ];
    };
  };
  disabled = lib.evalModules {
    modules = [
      base
      module
    ];
    specialArgs.pkgs = { };
  };
  missing = lib.evalModules {
    modules = [
      base
      module
      { programs.sledgeHyprland.enable = true; }
    ];
    specialArgs.pkgs = { };
  };
  enabled = lib.evalModules {
    modules = [
      base
      module
      {
        programs.sledgeHyprland = {
          enable = true;
          package = {
            daemon = "/nix/store/test-daemon";
            plugin = "/nix/store/test-plugin";
            registration = "/nix/store/test-registration";
            feature_mask = 1;
          };
        };
      }
    ];
    specialArgs.pkgs = {
      python3 = "/nix/store/test-python";
      writeShellApplication = spec: spec.text;
    };
  };
  wrongRuntime = builtins.tryEval (
    import ./artifacts.nix {
      pkgs = {
        inherit lib;
        hyprland = "/nix/store/exact-sdk";
      };
      contract = {
        compositor_sha256 = builtins.concatStringsSep "" (builtins.genList (_: "a") 64);
        compositor_path = "/nix/store/different-runtime/bin/.Hyprland-wrapped";
        sdk_api_hash = "unknown";
      };
    }
  );
  rejected = builtins.tryEval (
    import ./qualified.nix {
      pkgs = { inherit lib; };
      rustPlatform = { };
      contract = {
        qualification_revision = 0;
        feature_mask = 1;
        approval_revision = 1;
      };
      evidence = "";
      acceptance = "";
    }
  );
in
assert disabled.config.environment.systemPackages == [ ];
assert !(builtins.head missing.config.assertions).assertion;
assert builtins.length enabled.config.environment.systemPackages == 2;
assert (builtins.head enabled.config.assertions).assertion;
assert !wrongRuntime.success;
assert !rejected.success;
true
