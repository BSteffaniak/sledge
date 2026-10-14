# Home Manager installs internal helpers; NixOS installs the same sessionPackage.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.sledgeHyprland;
  selected = if cfg.bundle != null then cfg.bundle else cfg.package;
  # Legacy factory result remains valid; no runtime JSON/hash options are added.
  bundle = if selected ? bundle then selected.bundle else selected;
  session = import ./session-package.nix {
    inherit pkgs bundle;
    inherit (cfg)
      configFile
      firstApp
      compositorConfig
      autostartCommands
      ;
  };
in
{
  options.programs.sledgeHyprland = {
    enable = lib.mkEnableOption "Sledge Hyprland login integration";
    bundle = lib.mkOption {
      type = lib.types.nullOr lib.types.attrs;
      default = null;
      description = "Accepted compatible bundle returned by mkQualifiedHyprland; never prospective artifacts.";
    };
    package = lib.mkOption {
      type = lib.types.nullOr lib.types.attrs;
      default = null;
      description = "Legacy alias for the accepted factory result; prefer bundle.";
    };
    sessionPackage = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      description = "Internal login package for services.displayManager.sessionPackages (hyprland-sledge).";
    };
    configFile = lib.mkOption {
      type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
      default = null;
      description = "Optional expert override; normal runtime config is ~/.config/sledge/config.toml.";
    };
    firstApp = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
    };
    compositorConfig = lib.mkOption {
      type = lib.types.lines;
      default = "";
      description = "Hyprland configuration without parallel exec-once entries.";
    };
    autostartCommands = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = pkgs.stdenv.isLinux && selected != null;
        message = "Sledge Hyprland needs Linux and an accepted bundle.";
      }
      {
        assertion = cfg.bundle == null || cfg.package == null;
        message = "Set bundle or legacy package, not both.";
      }
    ];
    programs.sledgeHyprland.sessionPackage = session;
    home.packages = [
      session
      bundle.daemon
    ];
  };
}
