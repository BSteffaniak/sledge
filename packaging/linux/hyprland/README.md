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
