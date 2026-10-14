# Opt-in startup tools, NOT an automatically ordered graphical user service.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.sledgeHyprland;
  selected = if cfg.bundle != null then cfg.bundle else cfg.package;
  bundle = if selected != null && selected ? bundle then selected.bundle else selected;
  startup = pkgs.writeShellApplication {
    name = "sledge-hyprland-startup";
    runtimeInputs = [ pkgs.python3 ];
    text = ''exec python3 ${./startup.py} --daemon ${bundle.daemon}/bin/sledge --registration ${bundle.registration}/registration.json --plugin ${bundle.plugin}/lib/qualified.so "$@"'';
  };
  drain = pkgs.writeShellApplication {
    name = "sledge-hyprland-drain-unload";
    runtimeInputs = [ pkgs.python3 ];
    text = ''exec python3 ${./drain-unload.py} --plugin ${bundle.plugin}/lib/qualified.so "$@"'';
  };
in
{
  options.programs.sledgeHyprland = {
    enable = lib.mkEnableOption "explicit exact-build Hyprland startup/drain tools";
    bundle = lib.mkOption {
      type = lib.types.nullOr lib.types.attrs;
      default = null;
      description = "Accepted bundle returned by qualified.nix; runtime hashes are internal.";
    };
    package = lib.mkOption {
      type = lib.types.nullOr lib.types.attrs;
      default = null;
      description = "Result of qualified.nix factory with externally accepted exact artifact contract.";
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion =
          bundle != null
          && bundle ? daemon
          && bundle ? registration
          && bundle ? plugin
          && builtins.elem bundle.feature_mask [
            1
            3
          ];
        message = "sledgeHyprland requires an explicitly accepted feature1 or feature3 qualified factory result";
      }
    ];
    environment.systemPackages = [
      startup
      drain
    ];
  };
}
