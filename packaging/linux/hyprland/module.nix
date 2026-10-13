# Opt-in startup tools, NOT an automatically ordered graphical user service.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.sledgeHyprland;
  startup = pkgs.writeShellApplication {
    name = "sledge-hyprland-startup";
    runtimeInputs = [ pkgs.python3 ];
    text = ''exec python3 ${./startup.py} --daemon ${cfg.package.daemon}/bin/sledge --registration ${cfg.package.registration}/registration.json --plugin ${cfg.package.plugin}/lib/qualified.so "$@"'';
  };
  drain = pkgs.writeShellApplication {
    name = "sledge-hyprland-drain-unload";
    runtimeInputs = [ pkgs.python3 ];
    text = ''exec python3 ${./drain-unload.py} --plugin ${cfg.package.plugin}/lib/qualified.so "$@"'';
  };
in
{
  options.programs.sledgeHyprland = {
    enable = lib.mkEnableOption "explicit exact-build Hyprland startup/drain tools";
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
          cfg.package != null
          && cfg.package ? daemon
          && cfg.package ? registration
          && cfg.package ? plugin
          && cfg.package.feature_mask == 1;
        message = "sledgeHyprland requires an explicitly accepted feature1 qualified factory result";
      }
    ];
    environment.systemPackages = [
      startup
      drain
    ];
  };
}
