# Home module evaluation with real package types, never builds or launches anything.
{ pkgs }:
let
  lib = pkgs.lib;
  base = {
    options.home.packages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
    };
    options.assertions = lib.mkOption {
      type = lib.types.listOf lib.types.anything;
      default = [ ];
    };
  };
  fixture = {
    accepted = true;
    feature_mask = 3;
    compositor = pkgs.hyprland;
    daemon = pkgs.hello;
    plugin = pkgs.hello;
    registration = pkgs.hello;
  };
  evaluate =
    extra:
    lib.evalModules {
      modules = [
        base
        ./home-module.nix
        {
          programs.sledgeHyprland = {
            enable = true;
            bundle = fixture;
            configFile = /nix/store/test-config;
            firstApp = [ "${pkgs.hello}/bin/hello" ];
          };
        }
        extra
      ];
      specialArgs = { inherit pkgs; };
    };
  normal = evaluate { };
  conflicting = evaluate { programs.sledgeHyprland.package = fixture; };
  disabled = lib.evalModules {
    modules = [
      base
      ./home-module.nix
    ];
    specialArgs = { inherit pkgs; };
  };
in
assert
  normal.config.programs.sledgeHyprland.sessionPackage.providedSessions == [ "hyprland-sledge" ];
assert builtins.all (a: a.assertion) normal.config.assertions;
assert builtins.length normal.config.home.packages == 2;
assert !builtins.all (a: a.assertion) conflicting.config.assertions;
assert disabled.config.home.packages == [ ];
true
