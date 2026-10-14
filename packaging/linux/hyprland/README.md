# Hyprland backend packaging

## Home Manager login integration

`homeManagerModules.sledgeHyprland` accepts an independently reviewed exact-build
package, a hotkey config, a real `firstApp`, compositor configuration without
parallel `exec-once` entries, and ordered `autostartCommands`. It installs
`sledge-hyprland-session` and `sledge-hyprland-drain`; it does not activate or
restart the current desktop. Launch the session from a fresh local login, not
inside an existing compositor. A unique private runtime directory supplies the
endpoint environment before Hyprland starts. Application startup is released
only after Supported, qualified and operational daemon status.

The ordinary Hyprland session remains a rollback path. Personal exact-build
acceptance is not general release certification: VM evidence and physical-host
login evidence must be distinguished. Taps/input-source actions remain outside
feature1. Never reuse candidate admission records for this integration.

`candidate.nix` builds an **isolated qualification candidate**, not an installable
supported desktop backend. Do not load its plugin into your active session.
The normal daemon embeds no admission catalogue; runtime configuration cannot
turn a candidate into a supported registration.

Candidate packaging binds the exact stock Hyprland ELF/SDK, native source and
compiler contract, plugin digest, descriptor bytes, and guest scope. The daemon
embeds the catalogue bytes and their SHA-256 through build-time inputs:

- `SLEDGE_HYPRLAND_CATALOGUE_PATH`
- `SLEDGE_HYPRLAND_CATALOGUE_SHA256`

The private installed-session arguments are `--linux-hyprland-registration`,
`--linux-hyprland-pid`, and `--linux-hyprland-socket`. Candidate rules and reloads
accept only fresh-key-down `send_key` hotkeys. Taps and input-source actions are
rejected rather than silently presented as supported. Status always distinguishes
candidate configuration from production qualification.

Guest scope markers are test-package guards, not cryptographic VM attestation.
Use only the isolated runner and manifested transfer documented in
`scripts/linux/hyprland/CANDIDATE-PAYLOAD.md`. Every changed native artifact
requires a new descriptor/catalogue and daemon build, followed by behavioral
requalification. Research-mode test results do not qualify this build.

## Stock compositor lifecycle

The backend targets unmodified Hyprland. Load the plugin before starting
applications so it observes their actual keyboard-enter events. Loading into an
already-focused client can remain `waiting_for_focus`: stock Hyprland's public
API does not expose the exact existing keyboard binding needed for safe replay.
A genuine subsequent enter can establish it; loading alone never grants
operational status.

Focus, device and lock transitions invalidate admission. Replacement resumes
only after authenticated reconciliation for the current connection and epoch;
input arriving while recovery is pending passes through stock handling. Status
separates configured registration, supported qualification and current operational
health. Successful candidate tests do not create a supported catalogue entry.

## Unload controller

`drain-unload.py` requires explicit absolute `hyprctl`/plugin paths and an explicit
compositor instance. It requests drain and waits for **irreversible `detached`**,
not a momentary `quiescent` snapshot, before unloading. Timeout or invalid status
leaves the plugin loaded. Direct administrative unload before detachment and
forced eject are outside this supported unload path.

Controller checks (no compositor contact):

```
python3 packaging/linux/hyprland/test_drain_unload.py
```

## Exact-build qualified factory (no default acceptance)

Two-stage review avoids a hash-dependent artifact loop:
1. `flake.lib.prepareQualifiedHyprlandArtifacts` / `artifacts.nix` takes only
   compositor path/SHA and SDK string (plus explicitly selected pinned pkgs).
   It builds the unaccepted qualified-mode plugin and exports `prospective`:
   final installed registration bytes and `identity.json` with final plugin,
   descriptor, source/build and SDK/compiler identities. It creates no catalogue,
   approval, accepted evidence or daemon grant.
2. External review/tests consume that exact artifact and supply evidence and an
   independent acceptance record. `mkQualifiedHyprland` reconstructs the identical
   plugin derivation (expected hashes/review are NOT its inputs), verifies the
   **post-fixup installed** SO bytes, and uses those exact prospective descriptor
   bytes before creating admission catalogue/daemon. Stripping/RPATH fixups remain
   normal stdenv behavior and cannot escape the final digest check.

Artifact preparation requires no plugin/descriptor hash guess; no fixed-point
search or provisional fake accepted record is needed. Artifact build/VM testing
is still required; evaluation-only checks do not establish release readiness.

`flake.lib.mkQualifiedHyprland` / `qualified.nix` require externally accepted
`contract`, immutable `evidence` and independent `acceptance` inputs. No supplied
example is an accepted record. Contract fields:

- `qualification_revision=1`, `feature_mask=1`, positive `approval_revision`;
- SHA256 `compositor_sha256`, `plugin_sha256`, `descriptor_sha256`,
  `evidence_sha256`, `acceptance_sha256`, `source_hash`, `build_id`;
- exact `compositor_path`, `sdk_drv`, `compiler_drv`, `sdk_api_hash`.

Source hash is SHA256 of the named `sledge-qualified-native` store source path;
`build_id` hashes its source/SDK/compiler/revision/features JSON contract. Factory
asserts closure identity and checks actual compiled plugin, compositor, descriptor
and evidence digests. The independent acceptance file uses the library's strict
`acceptance_revision=1` binding schema. Packaging is a trusted review boundary,
not cryptographic proof against a malicious builder. SDK `unknown_...` is only
corroborating: no broad version/ABI compatibility is inferred from it.

Feature1 is fresh-Down send_key only, not taps/source actions. Existing normal
package remains catalogue-free. Qualified-mode binary is a distinct artifact:
its final descriptor/evidence must be independently reviewed and tested, not
copied from candidate results without accepting the mode-specific artifact.

### Explicit startup barrier

Opt-in `nixosModules.sledgeHyprland` installs startup/drain tools only; it does
**not** create a misleading graphical-session service or change compositor/app
startup. Caller supplies the factory result in `programs.sledgeHyprland.package`.
Integrate `sledge-hyprland-startup` synchronously at compositor startup, before
other apps: supply explicit stock hyprctl, instance, PID, config, private adapter
and IPC paths, and `--first-app /absolute/real/application ...`. The compositor
must already carry `SLEDGE_HYPRLAND_PASSIVE_SOCKET` matching that endpoint.
Plugin loads before the real first app; its natural enter establishes authority.
Only accepted Supported **and** operational daemon status releases the barrier.
Caller must arrange the remaining applications to wait for its successful exit.
Parallel `exec-once` lines do not establish this ordering.

There is intentionally no autonomous systemd unit: no generic user-unit ordering
can promise plugin-before-apps, and failed ExecStop normally proceeds to process
termination, violating retention on failed drain. Startup failure leaves loaded
plugin/daemon intact and reports failure. Explicit drain controller waits for
irreversible detach before unload; timeout never force-unloads or kills daemon.
After successful unload, an administrator may stop the daemon normally. No stale
socket deletion/restart policy is implied.

### Causal diagnostics

Debug `Hyprland causal transaction` records nonce/generation/epoch/sequence and
original deadline plus optional monotonic receive/send/outcome/terminal times.
One bounded record is emitted after settlement; no focus/credentials or wire
extension. Missing clock readings remain null. Correlate with native timing;
logging may perturb later scheduling and is not performance qualification.

Evaluation/controller tests (no services or accepted build):
`test_eval.nix`, `python3 -B test_startup.py`, `python3 -B test_drain_unload.py`.

Host deployment remains gated on final production-path safety/behavior evidence
and separate approval to change the active desktop.
