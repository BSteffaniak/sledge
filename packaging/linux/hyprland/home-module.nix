# User-session integration. The caller supplies an independently accepted package.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.sledgeHyprland;
  startup = pkgs.writeShellApplication {
    name = "sledge-hyprland-session-start";
    runtimeInputs = [
      pkgs.python3
      pkgs.hyprland
    ];
    text = ''
      : "''${HYPRLAND_INSTANCE_SIGNATURE:?compositor instance required}"
      : "''${SLEDGE_HYPRLAND_RUNTIME:?private session directory required}"
      pid=$(hyprctl -j instances | python3 -c 'import json,os,sys;print(next(i["pid"] for i in json.load(sys.stdin) if i["instance"]==os.environ["HYPRLAND_INSTANCE_SIGNATURE"]))')
      python3 ${./startup.py} --daemon ${cfg.package.daemon}/bin/sledge \
        --registration ${cfg.package.registration}/registration.json \
        --plugin ${cfg.package.plugin}/lib/qualified.so \
        --hyprctl ${pkgs.hyprland}/bin/hyprctl --instance "$HYPRLAND_INSTANCE_SIGNATURE" \
        --pid "$pid" --config ${cfg.configFile} \
        --socket "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" --ipc-socket "$SLEDGE_HYPRLAND_RUNTIME/health.sock" \
        --first-app ${lib.escapeShellArgs cfg.firstApp}
      ${lib.concatMapStringsSep "\n" (
        command: "hyprctl dispatch exec ${lib.escapeShellArg command}"
      ) cfg.autostartCommands}
    '';
  };
  drain = pkgs.writeShellApplication {
    name = "sledge-hyprland-drain";
    runtimeInputs = [ pkgs.python3 ];
    text = ''
      : "''${HYPRLAND_INSTANCE_SIGNATURE:?run from this Sledge Hyprland session}"
      exec python3 ${./drain-unload.py} --hyprctl ${pkgs.hyprland}/bin/hyprctl \
        --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin ${cfg.package.plugin}/lib/qualified.so
    '';
  };
  sessionConfig = pkgs.writeText "sledge-hyprland.conf" (
    cfg.compositorConfig
    + ''

      # All other application startup is released only after native proof.
      exec-once = ${startup}/bin/sledge-hyprland-session-start
    ''
  );
  session = pkgs.writeShellApplication {
    name = "sledge-hyprland-session";
    runtimeInputs = [ pkgs.coreutils ];
    text = ''
      : "''${XDG_RUNTIME_DIR:?login runtime directory required}"
      umask 077
      export SLEDGE_HYPRLAND_RUNTIME
      SLEDGE_HYPRLAND_RUNTIME=$(mktemp -d "$XDG_RUNTIME_DIR/sledge.XXXXXXXX")
      export SLEDGE_HYPRLAND_PASSIVE_SOCKET="$SLEDGE_HYPRLAND_RUNTIME/native.sock"
      exec ${pkgs.hyprland}/bin/Hyprland --config ${sessionConfig} "$@"
    '';
  };
in
{
  options.programs.sledgeHyprland = {
    enable = lib.mkEnableOption "explicit Sledge Hyprland login session";
    package = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.submodule {
          options = {
            daemon = lib.mkOption { type = lib.types.package; };
            plugin = lib.mkOption { type = lib.types.package; };
            registration = lib.mkOption { type = lib.types.package; };
            feature_mask = lib.mkOption { type = lib.types.ints.positive; };
          };
        }
      );
      default = null;
      description = "Independently accepted exact-build package components; never a candidate.";
    };
    configFile = lib.mkOption {
      type = lib.types.path;
      description = "Validated Linux hotkey configuration.";
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
        assertion =
          pkgs.stdenv.isLinux
          && cfg.package != null
          && builtins.elem cfg.package.feature_mask [
            1
            3
          ];
        message = "Sledge Hyprland requires Linux and an accepted feature1/feature3 package.";
      }
      {
        assertion = cfg.firstApp != [ ] && lib.hasPrefix "/" (builtins.head cfg.firstApp);
        message = "Sledge Hyprland requires an absolute real first application.";
      }
      {
        assertion =
          !lib.any (line: lib.hasPrefix "exec-once" (lib.strings.trim line)) (
            lib.splitString "\n" cfg.compositorConfig
          );
        message = "Parallel exec-once entries must be moved to sledgeHyprland.autostartCommands.";
      }
    ];
    home.packages = [
      session
      drain
      cfg.package.daemon
    ];
  };
}
