# Native r7 startup output handoff

Source frozen after local validation. No VM/host load, no package/catalogue edits.
F-candidate-output1 proven missing startup binding is addressed, but real output
qualification remains OPEN until rebuilt immutable candidate passes VM validator.

After maps and all observer hooks are installed and BEFORE Hello/worker creation,
startup_output.hpp::refreshStockOutput invokes public pinned SDK APIs:
1. CInputManager::updateCapabilities() -- stock recomputes authoritative HID caps;
   capabilities hook records actual supplied mask even for unchanged stock caps.
2. CSeatManager::resendEnterEvents() -- stock replays current focus through its
   own setKeyboardFocus/sendEnter path; enter hook records exact keyboard resource,
   owner and surface binding. No fabricated binding/capabilities, private member
   access, focuswindow fixture or acceptance override. Runs for production and
   enabled research route, outside physical-key callback.

Stock 0.52.2 ELF exports resendEnterEvents at 0x6d1230; disassembly includes
setKeyboardFocus at 0x6d12b6. SDK SeatManager.hpp exposes resendEnterEvents public.
InputManager.hpp exposes updateCapabilities public. Existing caps, owner/focus,
resource-good, IME, lock, target-held and remaining deadline checks are preserved.
If replay finds no eligible resource, replacement still rejects safely.

## Status interface (additive version1)

`hyprctl sledge:status` includes:
- replacement_capabilities_known: bool
- replacement_capabilities: unsigned actual HID mask
- replacement_preflight: object of cumulative compositor-owned uint64 counters:
  attempted, accepted, partial, fixture, map, focus, locked, ime, target,
  capabilities, deadline, resource, binding, empty.

Rejected counters count first failing native replacement preflight predicate.
Router invalid/same/held target rejection may precede preflight, so attempted0
with a Rejected outcome is not evidence of native preflight success. Counters
are fixed storage/increments only in callback; JSON allocations occur only in
explicit status command. No callback logging or wire/Hello/candidate macro change.

## Validation

`nix shell --offline /nix/store/8v97ngkcpfzgghwnnr7fsz33p2x22gy9-gcc-wrapper-14.3.0 --command bash /hdd/sledge-native-r7-1c6d61da/check.sh`
PASS six native tests, startup SHA256 test, passive/research syntax and three
shared-object compiles with Wall/Wextra/Werror. Foundation includes replay order
and absent-authority-no-binding regression. Structural wiring regression:
`python3 native/hyprland/startup_output_test.py` PASS hooks < refresh < Hello and
strict capability/binding/deadline rejection retained.
`python3 native/hyprland/detached_source_test.py` PASS 18 detours.
`git diff --check -- native/hyprland` PASS. Cargo validation coordinator-owned.

Scratch build hashes (dummy pins; no guest qualification):
research cac2f118366c108fc7eff75e5d99bb4088dd96bc0f796204f830c25b8e6d1dc3
production 566c806c94e3ee11f626c0c24198ca815a22a3633e57089333668a8dd01b471d
candidate f746d64963d5e4946b629b24eb70d9dbe5b59124057daf3c74d8518a0a41e1a2

Coordinator: rebuild actual candidate source/build ID and registration/catalogue,
run unchanged pre-existing focused consumer baseline (no focus-toggle workaround),
inspect status preflight counters and require actual B pair/no A via validator.
