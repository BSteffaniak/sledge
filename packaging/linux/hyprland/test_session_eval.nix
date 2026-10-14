# No services, accepted build or desktop loading: pure package contract evaluation.
{ lib }:
let
  pkgs = {
    inherit lib;
    python3 = "/nix/store/python";
    writeShellApplication = spec: "/nix/store/${spec.name}";
    writeText = name: text: "/nix/store/${name}";
    runCommand =
      name: attrs: text:
      attrs.passthru // { script = text; };
  };
  bundle = {
    accepted = true;
    feature_mask = 3;
    compositor = "/nix/store/exact-compositor";
    daemon = "/nix/store/daemon";
    plugin = "/nix/store/plugin";
    registration = "/nix/store/registration";
  };
  make =
    overrides:
    import ./session-package.nix (
      {
        inherit pkgs bundle;
        configFile = /nix/store/config;
        firstApp = [ "/nix/store/app/bin/app" ];
      }
      // overrides
    );
  session = make { };
in
assert session.providedSessions == [ "hyprland-sledge" ];
assert lib.hasInfix "Name=Hyprland (Sledge)" session.script;
assert lib.hasInfix "wayland-sessions/hyprland-sledge.desktop" session.script;
assert
  !(builtins.tryEval (make {
    bundle = bundle // {
      accepted = false;
    };
  })).success;
assert
  !(builtins.tryEval (make {
    firstApp = [ ];
  })).success;
assert
  !(builtins.tryEval (make {
    compositorConfig = "exec-once = unsafe";
  })).success;
assert
  !(builtins.tryEval (make {
    bundle = bundle // {
      feature_mask = 2;
    };
  })).success;
true
