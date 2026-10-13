# Factory only: the caller supplies an externally reviewed exact artifact contract.
{
  pkgs,
  rustPlatform,
  contract,
  evidence,
  acceptance,
}:
let
  lib = pkgs.lib;
  hash = s: builtins.match "[0-9a-f]{64}" s != null;
  artifacts = import ./artifacts.nix {
    inherit pkgs;
    contract = {
      inherit (contract) compositor_path compositor_sha256 sdk_api_hash;
    };
  };
  inherit (artifacts) sourceHash buildId plugin;
  contractFile = pkgs.writeText "sledge-accepted-contract.json" (builtins.toJSON contract);
  registration =
    pkgs.runCommand "sledge-qualified-registration" { nativeBuildInputs = [ pkgs.python3 ]; }
      ''
        mkdir -p $out
        python3 - "$out" <<'PY'
        import hashlib,json,pathlib,sys,runpy
        verify_final_plugin=runpy.run_path('${./final_identity.py}')['verify_final_plugin']
        c=json.loads(pathlib.Path('${contractFile}').read_text())
        out=pathlib.Path(sys.argv[1])
        evidence=pathlib.Path('${evidence}').read_bytes()
        approval=pathlib.Path('${acceptance}').read_bytes()
        assert hashlib.sha256(evidence).hexdigest()==c['evidence_sha256']
        assert hashlib.sha256(approval).hexdigest()==c['acceptance_sha256']
        descriptor=dict(wire_version=1,qualification_revision=1,build_id='sledge-stock-v1:${buildId}',
          compositor_path=c['compositor_path'],compositor_sha256=c['compositor_sha256'],sdk_api_hash=c['sdk_api_hash'],
          plugin_path='${plugin}/lib/qualified.so',plugin_sha256=c['plugin_sha256'],capabilities=0)
        verify_final_plugin(pathlib.Path('${plugin}/lib/qualified.so'),c['plugin_sha256'])
        raw=pathlib.Path('${artifacts.prospective}/registration.json').read_bytes()
        assert json.loads(raw)==descriptor, 'prospective descriptor differs from accepted binding' 
        assert hashlib.sha256(raw).hexdigest()==c['descriptor_sha256']
        binding=dict(descriptor_sha256=c['descriptor_sha256'],build_id=descriptor['build_id'],
          evidence_sha256=c['evidence_sha256'],qualification_revision=1,feature_mask=1,approval_revision=c['approval_revision'])
        accepted=json.loads(approval)
        assert accepted['acceptance_revision']==1 and accepted['entries'].count(binding)==1
        catalogue=json.dumps(dict(policy_revision=2,entries=[dict(binding,status='supported')]),sort_keys=True,separators=(',',':')).encode()
        (out/'registration.json').write_bytes(raw)
        (out/'catalogue.json').write_bytes(catalogue)
        (out/'acceptance.json').write_bytes(approval)
        PY
      '';
  daemon = rustPlatform.buildRustPackage {
    pname = "sledge-hyprland-qualified-daemon";
    version = "1";
    src = lib.cleanSourceWith {
      src = ../../..;
      filter =
        path: type:
        let
          name = builtins.baseNameOf path;
        in
        name != "target" && name != ".git" && type != "symlink";
    };
    cargoLock.lockFile = ../../../Cargo.lock;
    cargoBuildFlags = [
      "-p"
      "sledge"
      "--bin"
      "sledge"
    ];
    cargoTestFlags = [
      "-p"
      "sledge"
    ];
    preBuild = ''
      export SLEDGE_HYPRLAND_CATALOGUE_PATH=${registration}/catalogue.json
      export SLEDGE_HYPRLAND_CATALOGUE_SHA256=$(sha256sum "$SLEDGE_HYPRLAND_CATALOGUE_PATH" | cut -d' ' -f1)
      export SLEDGE_HYPRLAND_ACCEPTANCE_PATH=${registration}/acceptance.json
      export SLEDGE_HYPRLAND_ACCEPTANCE_SHA256=${contract.acceptance_sha256}
    '';
  };
in
assert lib.assertMsg (
  contract.qualification_revision == 1 && contract.feature_mask == 1 && contract.approval_revision > 0
) "only independently reviewed revision1 feature1 is supported";
assert lib.assertMsg (builtins.all hash (
  map (k: contract.${k}) [
    "compositor_sha256"
    "plugin_sha256"
    "descriptor_sha256"
    "evidence_sha256"
    "acceptance_sha256"
    "source_hash"
    "build_id"
  ]
)) "all accepted identities must be SHA256";
assert lib.assertMsg (
  contract.source_hash == sourceHash
  && contract.build_id == buildId
  && contract.sdk_drv == pkgs.hyprland.drvPath
  && contract.compiler_drv == pkgs.stdenv.cc.drvPath
) "source/SDK/toolchain build contract mismatch";
assert lib.assertMsg (
  builtins.match "[A-Za-z0-9_.:+-]+" contract.sdk_api_hash != null
) "SDK corroborating identity must be a bounded literal";
assert lib.assertMsg (builtins.stringLength contract.sdk_api_hash <= 192) "SDK identity oversized";
assert lib.assertMsg (
  contract.compositor_path == "${pkgs.hyprland}/bin/.Hyprland-wrapped"
) "only exact stock compositor path is supported";
{
  inherit
    plugin
    registration
    daemon
    buildId
    sourceHash
    ;
  inherit (contract) feature_mask;
}
