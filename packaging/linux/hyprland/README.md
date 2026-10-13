# Hyprland backend packaging

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

Host deployment remains gated on final production-path safety/behavior evidence
and separate approval to change the active desktop.
