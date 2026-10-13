# Native r6 source handoff

Frozen VM r5 package is untouched; no host/VM activation here. Candidate macro,
Hello/Decision schema and libcrypto dependency unchanged. New source requires
fresh package source/build ID, descriptor/plugin hash, catalogue and isolated VM
qualification; scratch .so files below are compile-only with dummy scope pins.

## Complete Reset8/drain output proof

`output_reconciliation.hpp::outputReconciled` is shared by
`stock_adapter.cpp::nativeOutputReconciled`, used by reason8 and drain. Requires
empty stock seat ledger, qualified known projection, zero projected holds and
depressed modifiers, no pending synthetic output recovery, no dirty lifecycle.
`physicalQuiescent` independently checks router ownership/pending/fatal state,
physical devices/maps and unknown devices, unlocked and nonnested state.
Forwarded latched/locked bits are genuine native history, not unresolved output;
they are not erased or required to be zero. Forwarded map selection preserves
these bits using modifier names across maps. Failed synthetic restore remains
represented by OutputRecovery and cannot qualify. Intentional CapsLock no longer
permanently blocks drain or reconciliation.

Fatal retirement additionally requires exact affected resource destruction,
post-retirement enter and current good keyboard resources with exact seat owner/
focused surface bindings and keyboard capability, no IME. The same native proof
(except pending recovery/fatal alone) must pass, then router recovery and recovery
completion precede a new Reset8. Null affected resource is unprovable and poisons
recovery. No snapshots/ownership are cleared to force admission.

## Callback changes

Mailbox uses aligned lock-free uint32 state + FUTEX_WAIT_PRIVATE/FUTEX_WAKE_PRIVATE.
No producer busy-spin, condvar, allocations, thread management or mutex. Deadline
recomputed after EINTR/spurious wake; no budget extension. Late completion cannot
reuse the abandoned slot until worker acknowledgement. It remains an explicitly
bounded synchronous kernel wait, NOT a hard real-time scheduler guarantee.

Lifecycle hooks mark dirty state only; map reconstruction/registration/preparation
runs in existing compositor 50ms timer, not in hook stack. Unknown/dirty maps deny
reason8. Registration Debug::log removed (status counters remain). Timer work may
still occupy compositor time; total latency needs VM measurement and SDK original
calls are not claimed allocation-free. Broad zero-allocation compositor guarantee
is not established.

## Regression coverage

Mailbox tests: stock ledger/output uncertainty/projection hold/depressed gating,
5ms timeout CPU usage <3ms, EINTR wake, late completion discard, slot abandonment,
100 immediate completion/lost-wake races. Forwarded test: native CapsLock survives
same map and alternate prepared map selection; output proof permits it. Foundation
test: null affected route cannot retire; existing exact retirement/fresh-binding
and fatal reset tests remain.

Validation command:
`nix shell --offline /nix/store/8v97ngkcpfzgghwnnr7fsz33p2x22gy9-gcc-wrapper-14.3.0 --command bash /hdd/sledge-native-r6-1c6d61da/check.sh`
Runs six native tests plus candidate SHA256 test, passive/research syntax and
research/production/candidate shared-object compile, Wall/Wextra/Werror.
Also `python3 native/hyprland/detached_source_test.py` and
`git diff --check -- native/hyprland`. Cargo validation belongs to coordinator.

Scratch hashes (dummy scope pins, not qualifying builds):
research e1d500ff8c7e807ee5c6751ddfd0070084f890f9a99fc8f78e1f7131199f50bd
production 9162146eaa2f006a81b828dc02904746d8d1383e4c656498bdac9aac287cabc9
candidate 913dc1923246d22f24c3bee7cc72d41b733faec81bcc9a814006929ace30a168
