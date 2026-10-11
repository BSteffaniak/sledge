# Compositor integration boundary (v0.52.2)

**Frozen research only.** User architectural decision requires stock Hyprland;
no patched compositor/fork is supported. The source integration discussion below
records missing upstream boundaries, not an implementation roadmap or supported
adapter. No patch expansion/transport implementation is proceeding. Stock capture
stays disabled pending authoritative stock hooks and independent qualification.

`routing.hpp` and `isolated_output.hpp` are implemented, deterministic routing
components, not installed callbacks. The probe remains separate. No stock
compositor artifact is admitted for production capture. Review outcome/rollback
and native VM test requirements are refined in OUTCOMES.md. Router now rejects
already-held replacement targets, provides delivered-only focus-enter keys and
records stock-consumed original outcomes; bridge must call originalOutcome
synchronously before any focus/keymap/IME state can change. No runtime detour is
assumed safe simply because `findFunctionsByName` returns an address.

## Smallest authoritative lock patch

`lock-boundary.patch` applies to upstream
`386376400119dd46a767c9f8c8791fd22c7b6e61` (context checked). It adds a synchronous,
non-cancellable signal before `newLock.emit`, not a listener on that existing
signal. A Router listener must call `lock(oldFocusOutput)` synchronously, without
IPC. This closes the gate, invalidates the epoch, releases forwarded keys to the
OLD route, restores modifiers, and quarantines held input before stock listeners
can switch to locker focus or acknowledge the lock.

The false boundary runs after genuine unlock callbacks, never on destruction.
Consumer attachment during an existing lock starts locked and must not guess an
unlock. Multiple lock requests can conservatively revoke again. This is a
minimum **lock boundary** patch only, not a complete parity patch. It passed a
full pinned compositor compile/link/install smoke build, but the patched runtime
was never loaded or executed. Its digest/closure is distinct from stock and is
research evidence only, not production qualification.

## Historical source integration points (superseded by stock-only architecture)

The source-patch proposal below is historical research, not a supported deployment
plan. Stock-only architecture supersedes it. WIRE.md records a research
reset/transaction contract; no transport or detour architecture is implemented.
Bridge must execute RouteResult.processStock independently of deliverClient:
StockConsumed repeat/up runs keybind release/longpress/timer cleanup without
client delivery. originalOutcome returns the actual typed outcome for acknowledgement.
LockerOwned state never becomes app-forwarded merely because unlock occurred.

1. `CKeyboard` physical pressed/XKB states must remain physical for keybinding,
   layout, LED state. `InputManager::onKeyboardKey` receives the original device
   and raw event before keybindings. The router runs there, with monotonic device
   generation, normalized known-key table and focus observation from the SAME
   compositor turn. Unknown/unmodeled codes bypass unchanged, not via the daemon.
   Router's result chooses original routing vs consumption; existing keybindings
   still determine whether a passed key actually reaches the client. Ownership of
   client-forwarded state therefore needs a callback AFTER keybinding admission;
   current Router tests model a route where all Pass events reach the consumer.
   Treating Pass-before-keybind as actual client delivery is not yet valid.
2. `InputManager::onKeyboardMod` merges physical modifiers. It cannot be used
   unchanged when modifiers are consumed. Deliver a separate forwarded-only XKB
   state to seat/IME, preserving layout group/latched/locked semantics according
   to explicit policy. Router's eight side bits are physical flags, NOT native
   XKB modifier indices; do not substitute them into `sendKeyboardMods`.
3. `SeatManager::setKeyboardFocus/resendEnterEvents` must enumerate only
   forwarded held keys and send forwarded XKB modifiers. A cancellable key hook
   does not fix focus-enter key arrays. Keymap replacement and active keyboard
   changes invalidate tickets and must reconstruct forwarded state atomically.
4. `CInputMethodV2::hasGrab/sendKey/sendMods/setKeyboard` is the IME route used
   by stock `InputManager`. Injection must pin the same route/keyboard/focus for
   requested modifiers -> down -> up -> restored modifiers, without yielding.
   `SeatManager::sendKeyboardKey/sendKeyboardMods` and IME methods return void:
   they prove neither allocation success nor application receipt. A full-pair
   admission/preflight contract is required BEFORE the first observable write;
   standalone queue admission is insufficient. No output after preflight failure
   means original-pass fallback. Once output begins, any failure is partial/fatal,
   with best-effort up/modifier restoration, NEVER original-pass duplication.
5. Physical repeated-down events and compositor/client-generated repeats differ.
   Consumed presses must never enter seat/IME repeat ownership; forwarded presses
   must retain the normal repeat contract. Two keyboards' overlapping keys require
   merged release handling (implemented in Router) plus matching focus-enter state.
6. Device destruction uses monotonic generations; retire reconciles only keys
   not held forwarded by another device. Transport failure, focus/keymap change,
   and reload invalidate pending tuples without losing committed ownership.
   Disconnect no longer releases/quarantines unrelated forwarded presses; lock
   revocation is the explicit old-focus reconciliation/quarantine boundary.

These are genuine blockers for a stock plugin. The implemented host-independent
component is useful executable behavior but cannot replace these source paths.
No always-disabled production backend or speculative hook package is added.

## Deterministic test evidence

Build dependencies are only obtained through nix shell:

```sh
nix shell --no-write-lock-file .#hyprland-sdk --command sledge-hyprland-sdk sh -eu -c '
 d=$(mktemp -d /tmp/sledge-routing.XXXXXX)
 "$CXX" -std=c++23 -O2 -Wall -Wextra -Werror native/hyprland/routing_test.cpp -o "$d/test"
 env -i "$d/test"
'
```

Tests cover consumed repeat/release across focus, independent devices, unknown
codes, deadline/stale replies, merged modifier/release state, replacement
preflight rejection/pass fallback vs partial-fatal/no-fallback, requested-only
modifier sequence/restoration, lock epoch revocation/quarantine, and retirement.
These tests exercise the actual Router/IsolatedOutput implementation against an
observable fake output route. They are not native application-visible evidence.

## Test-only admission

Unknown commit can be distinguished by exact closure plus executable digest;
that is not sufficient to qualify detours, listener ordering, or routing. No
identity-only override is implemented. A future isolated probe admission must be
in a separately compiled TEST-only target with a verified `/proc/self/exe` digest,
closure/compiler/API hash match and independently established sandbox provenance.
Environment flags or an arbitrary daemon-provided digest cannot establish sandbox
provenance. Current probe rejection remains intentional and does not admit capture.
