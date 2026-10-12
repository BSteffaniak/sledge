# Stock Hyprland hook investigation — checkpoint

The earlier assertion that stock Hyprland necessarily needs upstream changes was
premature. Stock 0.52.2 exposes documented x86_64 function trampolines, including
private function lookup. No patched compositor was used in this investigation.

## Observed in the isolated VM

Stock ELF SHA256:
`8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca`.

- Exact-symbol hooks installed for InputManager key/modifier, SeatManager
  key/modifier/focus, IME grab key/modifier, SessionLock protocol entry and query.
- Query trampoline call-through self-test passed. Plugin load/list/unload passed.
- Independent ext-session-lock client request reached the lock-entry hook once,
  before the original handler: protocol isLocked was false at entry. No surfaces
  were created; no lock ACK was received. This does NOT prove completed locking,
  revocation, old-focus reconciliation, or unlock recovery.
- Independent `wev` client baseline received virtual `wtype` a/b press/release.
  A subsequent fixed-fixture key-entry hook suppressed both a events while b
  press/release still arrived. The actual InputManager event code was 1, while
  the consumer received code 9; initial guessed-code tests FAILED and were not
  counted as successful suppression. Mapping must be derived from device keymap.
- Key/modifier/seat/focus hooks were exercised. IME grab hook counters stayed zero:
  installation is established, IME behavior is NOT established.

Retained logs:
- `/tmp/sledge-stock-expanded-hook-investigation.log`
- `/tmp/sledge-stock-input-hook-baseline.log`
- `/tmp/sledge-stock-input-hook-suppression-v3.log`

Final suppression VM: `/nix/store/6vhiqmmcx0dnzxx0vgvd8jkqf6k4h856-nixos-vm`.

## Scope and unresolved work

Research only. Fixed virtual-keyboard fixture, no daemon transport, no general
remapping, no production capture. Do not load into the active desktop. The probe
has file logging in callbacks for isolated evidence; that is NOT a production
hot-path design. Exact store path plus matching SDK hash gates this test; it does
not itself prove sandbox provenance, and unknown commit equality is insufficient.

Still required: virtual modifier semantics (wtype baseline did not show Shift
active), physical guest keyboard input, multiple keymaps/devices, held-key focus
switches, actual IME grab client, transactional replacement and partial failure,
complete locker surfaces/ACK/unlock/crash tests, hook conflict/rollback/unload
recovery. Failed partial initialization also needs explicit all-or-nothing hook
rollback before this fixture can become a reusable adapter.

Nix dependencies only via `nix shell`. Stock VM uses the existing private namespace
runner, explicit renderD129 and KVM, no host input/card/display sockets or network.
The runner admission bug was corrected: a pinned Nix-generated executable may be
a symlink to an immutable top-level store executable; mutable targets still reject.

## Conclusion

A stock plugin route is demonstrably possible for these narrow checkpoints.
Full parity is unqualified, not proven impossible. Official function-hook support
is evidence against a blanket intentional prohibition on trusted plugins;
unprivileged client security restrictions are a different boundary.
