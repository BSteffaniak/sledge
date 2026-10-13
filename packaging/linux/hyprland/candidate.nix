# Explicit isolated qualification candidate. Never a host backend package.
{
  pkgs,
  machineId,
  bootMarker,
  evidenceSha256,
}:
let
  lib = pkgs.lib;
  runtime = "${pkgs.hyprland}/bin/.Hyprland-wrapped";
  sdkHash = "unknown_aq_0.9_hu_0.10_hg_0.4_hc_0.1_hlg_0.6";
  elfHash = "8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca";
  markerPath = "/etc/sledge-qualification-candidate";
  source = ../../../native/hyprland;
  buildId = builtins.hashString "sha256" (
    builtins.toJSON {
      source = builtins.hashString "sha256" (
        toString (
          builtins.path {
            path = source;
            name = "sledge-candidate-native";
          }
        )
      );
      compiler = pkgs.stdenv.cc.drvPath;
      sdk = pkgs.hyprland.drvPath;
      inherit machineId bootMarker;
      policy = 1;
    }
  );
  header = pkgs.writeText "sledge-candidate-pins.hpp" ''
    #define SLEDGE_STOCK_PRODUCTION
    #define SLEDGE_STOCK_CANDIDATE
    #define SLEDGE_STOCK_BUILD_ID "${buildId}"
    #define SLEDGE_STOCK_ELF_SHA256 "${elfHash}"
    #define SLEDGE_STOCK_SDK_API_HASH "${sdkHash}"
    #define SLEDGE_STOCK_MACHINE_ID_SHA256 "${builtins.hashString "sha256" machineId}"
    #define SLEDGE_STOCK_BOOT_MARKER_PATH "${markerPath}"
    #define SLEDGE_STOCK_BOOT_MARKER_SHA256 "${builtins.hashString "sha256" bootMarker}"
  '';
  plugin = pkgs.stdenv.mkDerivation {
    pname = "sledge-hyprland-isolated-candidate";
    version = "1";
    src = source;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [
      pkgs.hyprland.dev
      pkgs.openssl
    ]
    ++ pkgs.hyprland.buildInputs;
    buildPhase = ''
      export PKG_CONFIG_PATH=${pkgs.hyprland.dev}/share/pkgconfig:$PKG_CONFIG_PATH
      test "$(sha256sum ${runtime} | cut -d' ' -f1)" = ${elfHash}
      c++ -std=c++23 -O2 -Wall -Wextra -Werror -pthread -fPIC -shared \
        -include ${header} $(pkg-config --cflags hyprland libdrm openssl) \
        stock_adapter.cpp -lcrypto -o candidate.so
    '';
    installPhase = "mkdir -p $out/lib; cp candidate.so $out/lib/candidate.so";
  };
  registration =
    pkgs.runCommand "sledge-candidate-registration"
      {
        nativeBuildInputs = [ pkgs.python3 ];
      }
      ''
        mkdir -p "$out"
        python3 - "$out" <<'PY'
        import hashlib, json, pathlib, sys
        output = pathlib.Path(sys.argv[1])
        plugin = pathlib.Path('${plugin}/lib/candidate.so')
        descriptor = dict(wire_version=1, qualification_revision=1,
          build_id='sledge-stock-v1:${buildId}', compositor_path='${runtime}',
          compositor_sha256='${elfHash}', sdk_api_hash='${sdkHash}',
          plugin_path=str(plugin), plugin_sha256=hashlib.sha256(plugin.read_bytes()).hexdigest(), capabilities=0)
        raw = json.dumps(descriptor, sort_keys=True, separators=(',', ':')).encode()
        (output / 'registration.json').write_bytes(raw)
        entry = dict(descriptor_sha256=hashlib.sha256(raw).hexdigest(), build_id=descriptor['build_id'],
          evidence_sha256='${evidenceSha256}', feature_mask=1, status='candidate', isolated_vm=True,
          machine_id_sha256='${builtins.hashString "sha256" machineId}', boot_marker_path='${markerPath}',
          boot_marker_sha256='${builtins.hashString "sha256" bootMarker}')
        catalogue = json.dumps(dict(policy_revision=1, entries=[entry]), sort_keys=True, separators=(',', ':')).encode()
        (output / 'catalogue.json').write_bytes(catalogue)
        (output / 'catalogue.sha256').write_text(hashlib.sha256(catalogue).hexdigest())
        PY
      '';
in
assert lib.assertMsg (
  builtins.match "[0-9a-f]{32}\n" machineId != null
) "candidate requires a valid 128-bit machine-id plus newline";
assert lib.assertMsg (
  builtins.match "[0-9a-f]{64}" evidenceSha256 != null
) "candidate requires evidence SHA256";
{
  inherit
    plugin
    registration
    buildId
    markerPath
    ;
}
