# Unaccepted qualified-mode artifact preparation: never creates admission grants.
{ pkgs, contract }:
let
  lib = pkgs.lib;
  source = ../../../native/hyprland;
  sourceHash = builtins.hashString "sha256" (
    toString (
      builtins.path {
        path = source;
        name = "sledge-qualified-native";
      }
    )
  );
  buildId = builtins.hashString "sha256" (
    builtins.toJSON {
      inherit sourceHash;
      compiler = pkgs.stdenv.cc.drvPath;
      sdk = pkgs.hyprland.drvPath;
      qualification_revision = 1;
      feature_mask = 1;
    }
  );
  header = pkgs.writeText "sledge-qualified-pins.hpp" ''
    #define SLEDGE_STOCK_PRODUCTION
    #define SLEDGE_STOCK_QUALIFIED
    #define SLEDGE_STOCK_BUILD_ID "${buildId}"
    #define SLEDGE_STOCK_ELF_SHA256 "${contract.compositor_sha256}"
    #define SLEDGE_STOCK_SDK_API_HASH "${contract.sdk_api_hash}"
    #define SLEDGE_STOCK_QUALIFICATION_REVISION 1
    #define SLEDGE_STOCK_FEATURE_MASK 1
  '';
  plugin = pkgs.stdenv.mkDerivation {
    pname = "sledge-hyprland-exact-qualified";
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
      c++ -std=c++23 -O2 -fPIC -shared stock_adapter.cpp -o qualified.so \
        -include ${header} $(pkg-config --cflags hyprland libdrm openssl) \
        $(pkg-config --libs libcrypto)
      echo '${contract.compositor_sha256}  ${contract.compositor_path}' | sha256sum -c -
    '';
    installPhase = "mkdir -p $out/lib; cp qualified.so $out/lib/qualified.so";
  };
  prospective =
    pkgs.runCommand "sledge-qualified-prospective" { nativeBuildInputs = [ pkgs.python3 ]; }
      ''
        mkdir -p $out
        python3 - "$out" <<'PYTHON'
        import hashlib,json,pathlib,sys
        out=pathlib.Path(sys.argv[1])
        plugin=pathlib.Path('${plugin}/lib/qualified.so')
        descriptor=dict(wire_version=1,qualification_revision=1,build_id='sledge-stock-v1:${buildId}',
          compositor_path='${contract.compositor_path}',compositor_sha256='${contract.compositor_sha256}',
          sdk_api_hash='${contract.sdk_api_hash}',plugin_path=str(plugin),
          plugin_sha256=hashlib.sha256(plugin.read_bytes()).hexdigest(),capabilities=0)
        raw=json.dumps(descriptor,sort_keys=True,separators=(',',':')).encode()
        (out/'registration.json').write_bytes(raw)
        (out/'identity.json').write_text(json.dumps(dict(source_hash='${sourceHash}',build_id='${buildId}',
          sdk_drv='${builtins.unsafeDiscardStringContext pkgs.hyprland.drvPath}',compiler_drv='${builtins.unsafeDiscardStringContext pkgs.stdenv.cc.drvPath}',
          plugin_sha256=descriptor['plugin_sha256'],descriptor_sha256=hashlib.sha256(raw).hexdigest()),sort_keys=True))
        PYTHON
      '';
in
assert lib.assertMsg (
  builtins.match "[0-9a-f]{64}" contract.compositor_sha256 != null
) "exact ELF SHA256 required";
assert lib.assertMsg (
  contract.compositor_path == "${pkgs.hyprland}/bin/.Hyprland-wrapped"
) "exact stock SDK/runtime path required";
assert lib.assertMsg (
  builtins.match "[A-Za-z0-9_.:+-]+" contract.sdk_api_hash != null
  && builtins.stringLength contract.sdk_api_hash <= 192
) "bounded SDK literal required";
{
  inherit
    plugin
    prospective
    sourceHash
    buildId
    ;
}
