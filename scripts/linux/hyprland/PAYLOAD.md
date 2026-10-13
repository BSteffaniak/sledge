# Reusable guest payload (preparation, not production acceptance)

Normal9's immutable wrapper is
`/nix/store/12xbdrrnj2nib2dm4vazghibqvkg77g2-sledge-stock-qualification-vm/bin/sledge-stock-qualification-vm`
(SHA256 `e956e5dcf7446e6969f1fffea0d0076ad172a3edcdf9a1f119abdb478d629d3a`).
Its guest executable is
`/nix/store/0yh0pfw3dd1vsifiqndsjh23p6yx0dp9-nixos-vm/bin/run-sledge-qualification-vm`.
Reuse its declared QEMU/Mesa/gate/identity/driver exports, but invoke the current
`vm-run.sh` explicitly; invoking the old wrapper uses the old stored script.
Do not evaluate a new Nix guest closure when sources are changing or root space
is limited. Prepare logs and artifacts under a lane-unique `/hdd` directory.

The boot entry is selected by a small root-disk `payload-init.sh`, which installs
an explicit runtime service and chains the pinned NixOS init. The old
`systemd.run` generator route was tested and did not activate on this closure.
The pinned base has been restored: see `/hdd/sledge-vm-c4d24/base-runner` and
`base-realize.log` for the current immutable wrapper/hash. Do not use removed
historical normal9 store paths.

Payload contains `boot.sh` (copy `payload-boot.sh`), `run.sh` (guest probe-user
entry), `sledge` (actual daemon), `plugin.so`, config and declared dependencies.
`manifest.sha256` uses lowercase SHA256 plus two spaces plus relative filename.
Every regular file must be declared; no symlinks, traversal, devices, sockets or
undeclared files. Manifest verification occurs before staging and in the guest.
Only trusted qualification scripts belong in payloads; a digest is integrity,
not authorization. Host credentials/configuration are never payload inputs.

`run.sh` must start the pinned stock compositor, independently observing clients
and QMP phase control, then invoke the real daemon with actual TOML config.
Production CLI flags/registration descriptor are supplied by the coordinator.
Existing research flags can establish a daemon functional baseline only.
Keep all guest runtime state under `/tmp`; report evidence via journal/console.
No script may claim production acceptance from startup/handshake alone.

Prepare (tools obtained through pinned `nix shell`, no installation):

```
python3 scripts/linux/hyprland/payload_disk.py PAYLOAD /hdd/LANE/FRESH-SCRATCH \
  --qemu-img /nix/store/.../bin/qemu-img --mkfs /nix/store/.../bin/mkfs.ext4
```

Preparation creates a 512MiB sparse raw ext4 root populated with the payload and
converts it to qcow2. No host mount/root privilege, guest boot, native build or
NixOS realization is performed. Raw image remains inside dedicated scratch.

Set `SLEDGE_VM_PAYLOAD_SCRATCH` to that scratch and retain normal9's exact tool
exports when invoking current `vm-run.sh /dev/dri/renderD129 --kvm`.
It binds only scratch at `/scratch`, uses the prepopulated disk, masks the old
fixture/report services and selects `systemd.run=/sledge-payload/boot.sh`.
The boot entry verifies hashes and runs `run.sh` as the existing probe user on
TTY7. It exports research/worker/production telemetry and powers off via the
systemd run generator. Existing 180s host bound remains in force.

## Required pre-dispatch checks

- Hash pinned runner, QEMU, daemon, plugin and manifest; record exact identities.
- Audit daemon/plugin ELF interpreter and NEEDED libraries against guest store
  image, not merely host-visible `/nix/store`. Host namespace store visibility
  does not mean the guest store image contains newly required dependencies.
- Confirm guest kernel/systemd supports the existing root disk and systemd.run
  mechanism; transfer path is not yet runtime-tested.
- All source writers settled; no native edits/builds during VM.
- Preserve reason8, correlated selected decision/outcome and independent
  consumer evidence; no accepted output based only on summary counters.

Remaining scenarios: scoped correct/wrong `wev`/`SledgeFixtureX11`, Alt-held
replacement, timeout/drop/reconnect, merged devices/layouts/repeats/unknown keys,
lock proposal races, held unload, real daemon reload/source-action integration.
This transfer preparation does not qualify any of those cases.
