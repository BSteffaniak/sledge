# Internal login package. Only an independently accepted factory bundle is valid.
{
  pkgs,
  bundle,
  configFile ? null,
  firstApp,
  compositorConfig ? "",
  autostartCommands ? [ ],
}:
let
  lib = pkgs.lib;
  compositor = bundle.compositor;
  startup = pkgs.writeShellApplication {
    name = "sledge-hyprland-session-start";
    runtimeInputs = [
      pkgs.python3
      compositor
    ];
    text = ''
      : "''${HYPRLAND_INSTANCE_SIGNATURE:?compositor instance required}"
      : "''${SLEDGE_HYPRLAND_RUNTIME:?private session directory required}"
      umask 077
      exec >> "$SLEDGE_HYPRLAND_RUNTIME/startup.log" 2>&1
      python3 ${./startup.py} --daemon ${bundle.daemon}/bin/sledge \
        --registration ${bundle.registration}/registration.json \
        --plugin ${bundle.plugin}/lib/qualified.so \
        --hyprctl ${compositor}/bin/hyprctl --instance "$HYPRLAND_INSTANCE_SIGNATURE" \
        --config ${
          if configFile == null then
            ''"''${XDG_CONFIG_HOME:-$HOME/.config}/sledge/config.toml"''
          else
            lib.escapeShellArg (toString configFile)
        } \
        --socket "$SLEDGE_HYPRLAND_PASSIVE_SOCKET" --ipc-socket "$SLEDGE_HYPRLAND_RUNTIME/health.sock" \
        --first-app ${lib.escapeShellArgs firstApp}
      ${lib.concatMapStringsSep "\n" (
        command: "hyprctl dispatch exec ${lib.escapeShellArg command}"
      ) autostartCommands}
    '';
  };
  drain = pkgs.writeShellApplication {
    name = "sledge-hyprland-drain";
    runtimeInputs = [ pkgs.python3 ];
    text = ''
      : "''${HYPRLAND_INSTANCE_SIGNATURE:?run from this Sledge Hyprland session}"
      exec python3 ${./drain-unload.py} --hyprctl ${compositor}/bin/hyprctl \
        --instance "$HYPRLAND_INSTANCE_SIGNATURE" --plugin ${bundle.plugin}/lib/qualified.so
    '';
  };
  sessionConfig = pkgs.writeText "sledge-hyprland.conf" (
    compositorConfig
    + ''

      # Natural first-app enter establishes authority; all other apps wait.
      exec-once = ${startup}/bin/sledge-hyprland-session-start
    ''
  );
  launcher = pkgs.writeShellApplication {
    name = "sledge-hyprland-session";
    runtimeInputs = [ pkgs.python3 ];
    text = ''
      exec python3 ${./session.py} --compositor ${compositor}/bin/Hyprland --config ${sessionConfig} \
        --hyprctl ${compositor}/bin/hyprctl --plugin ${bundle.plugin}/lib/qualified.so \
        --drain-controller ${./drain-unload.py} "$@"
    '';
  };
in
assert lib.assertMsg (bundle.accepted or false
) "Sledge login requires an accepted bundle, not a prospective or candidate artifact";
assert lib.assertMsg (builtins.elem bundle.feature_mask [
  1
  3
]) "unsupported Sledge bundle features";
assert lib.assertMsg (
  firstApp != [ ] && lib.hasPrefix "/" (builtins.head firstApp)
) "Sledge login requires an absolute real first application";
assert lib.assertMsg (
  !lib.any (line: lib.hasPrefix "exec-once" (lib.strings.trim line)) (
    lib.splitString "\n" compositorConfig
  )
) "move parallel exec-once entries to autostartCommands";
pkgs.runCommand "sledge-hyprland-login"
  {
    passthru = {
      providedSessions = [ "hyprland-sledge" ];
      inherit launcher drain;
    };
  }
  ''
    mkdir -p $out/bin $out/share/wayland-sessions
    ln -s ${launcher}/bin/sledge-hyprland-session $out/bin/sledge-hyprland-session
    ln -s ${drain}/bin/sledge-hyprland-drain $out/bin/sledge-hyprland-drain
    cat > $out/share/wayland-sessions/hyprland-sledge.desktop <<EOF
    [Desktop Entry]
    Name=Hyprland (Sledge)
    Comment=Hyprland with the accepted Sledge keyboard backend
    Exec=${launcher}/bin/sledge-hyprland-session
    TryExec=${launcher}/bin/sledge-hyprland-session
    Type=Application
    DesktopNames=Hyprland
    EOF
  ''
