# Candidate payload handoff (VM-only, not acceptance)

Pinned guest fstab binds EROFS `/nix/.ro-store` read-only at `/nix/store`.
There is no writable store overlay by default.

Add these **regular, manifested files** to the existing payload:

- `scope/machine-id`: exactly `736c656467652d63616e646964617465` plus newline.
- `scope/boot-marker`: exactly `sledge-hyprland-isolated-candidate-v1` plus newline.
- `store/<32-character-store-hash>-<name>/...`: candidate plugin and registration
  files at paths matching the immutable descriptor. Transfer no symlinks/devices.
- `candidate-registration-path`: exact newly handed-off `/nix/store/.../registration.json`
  path plus newline; launcher reads plugin path from this immutable descriptor.
- Candidate-built daemon, its declared loader/libs, and the catalogue embedded
  at build time. The existing research daemon cannot substitute for this build.

`payload_disk.py` verifies every file digest and exact scope, rejecting unknown
scope files and malformed top-level store names. Generated guest init copies
scope files into `/etc`, chowns root:root and chmods 0444 **before** chaining the
pinned NixOS init. Boot checks bytes/ownership/mode again, so regenerated machine
identity or activation-overwritten scope aborts before any client/plugin starts.

Boot mounts a guest-only overlay at `/nix/store`, lower `/nix/.ro-store`, disposable
root-disk `/nix/payload-upper` and `/nix/payload-work`. Each declared new store tree
must be absent in the lower store. Files are copied root-owned 0444 and directories
0555, then rehashed at their exact `/nix/store/...` locations. Scope inputs are
also staged directly into the root-disk `/etc` before boot and protected with
read-only bind mounts by pre-init; checks still require unchanged bytes after
activation. A candidate-only kernel machine-id pin matches the same approved
bytes. Host `/nix/store`
remains read-only and unchanged. No host share, networking or desktop is added.
Executable runtime dependencies belong in the existing daemon-local `lib/`
transfer, not the read-only candidate store trees (whose inputs are .so/JSON).

Installed startup must use `run --linux-hyprland-registration EXACT_GUEST_PATH
--linux-hyprland-pid PID --linux-hyprland-socket PRIVATE_PATH --stdout-logs`.
Do not set research mode/fault variables or pass research flags. The current
`daemon-payload-run.sh` still uses research startup and therefore is NOT the
candidate launcher. Its consumer phases can be reused only after explicitly
switching startup and plugin paths with the coordinator's artifact handoff.

Preparation tests pass. Boot attempts candidate1–7 abort before plugin load:
the supplied machine-id is **34 hex characters (17 bytes)**, not systemd's
required 32 hex characters. Pre-init logs prove the old approved bytes were staged;
systemd rejects them and overlays a generated `/run/machine-id`, even over a
read-only bind. F-candidate-scope1 correction approved: current pin is the valid
32-hex value above, with unchanged marker. The helper rejects the old malformed
34-hex value before disk creation, separately from exact-pin checks. Old native,
registration, catalogue and daemon artifacts/disks remain invalid and must not
be retried; await the rebuilt immutable artifact/hash handoff. Do not rewrite
identity post-activation. Corrected-scope boot candidate10 verifies unchanged scope
bytes and exact store hashes. Candidate12 exposed overlayfs stat/mmap device
mismatch; candidate13 resolves it by root-owned0444 read-only file binds at exact
store destinations (no verification bypass), and reaches qualification=Candidate
feature_mask=1. Installed admission then fails Broken pipe before reconciliation
and health: native Hello exchange budget is10ms while post-bind Rust admission
rehashes artifacts. This diagnosis is routed to Rust/native owners; no consumer
replacement baseline is accepted yet. Evidence lives in
`/hdd/sledge-vm-c4d24/candidate13-vm.log`. Recovery, layout/repeat/lock coverage
remains open. Compile pins and catalogue membership alone are not acceptance.
