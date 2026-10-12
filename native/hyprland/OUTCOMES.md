# Transaction outcomes

## Native ownership revision 1 — passive settlement

A concrete `stock_adapter.cpp` now installs the nine exact stock hook signatures
with source-byte change/restoration checks, call-through input/seat/IME routes,
synchronous Router lock revocation before the original, and a same-UID/private
endpoint worker. All capabilities stay zero; no physical devices are registered,
no key events are exported, no replacements or stock focus arrays are modified.
Thus its lock call has no admitted holds to reconcile: it does NOT fix or qualify
pre-lock Shift+f in locker focus-enter.

Host checks in `/tmp/sledge-native-1c6d61da/check.sh` (Nix-shell dependencies only)
built probe.so, adapter.so, SDK identity helper and passive handshake harness;
compiled stock_hooks.hpp against stock 0.52.2; ran stock_foundation_test,
decision_mailbox_test, stock_worker_test, routing_test, forwarded_state_test and
delivered_state_test, all with C++23 -O2 -Wall -Wextra -Werror (SDK headers marked
system because stock Color.hpp has unused parameters). Foundation covers reverse
rollback at nine positions/exception/capacity, device provenance rejection/mapping/
identity reuse, ownership across focus/reset/lock; real XKB covers stock-consumed
Shift exclusion, forwarded Shift, rebuild/failure/lock clearing. Worker tests cover
packet corruption/correlation and private seqpacket Hello/Ack/decision.

Native harness/Rust listener integration passed on the host: exact expected PID,
zero-scope Hello/nonzero nonce/caps0, validated Ack, native_exit=0/rust_exit=0.
Rust artifact: `/nix/store/p7x1v3ay0bbg7ai8xl3n7pn595avclh1-sledge-hyprland-probe-peer-0/bin/hyprland_probe_peer`.
No VM, compositor/plugin loading, desktop or devices were accessed by this lane.
Coordinator owns full Cargo validation.

Remaining active integration: verified evdev/keymap/lifecycle registration;
original admission vs stock consumption evidence; merged stock cleanup with denied
seat/IME output; forwarded-only per-keymap modifier masks and focus-enter arrays;
real old-route release on lock before focus/ACK; actual unlock/crash authority;
complete replacement preflight/commit and partial-output reconciliation; event/
outcome/reset transport integration; real SDK rollback/byte and native/Rust plugin
handshake fixture evidence. Existing Router/IsolatedOutput tests are contracts,
not completed native callback implementation. Capture remains disabled.

## Prior routing outcomes

WIRE.md is the single authoritative transaction/reset proposal. Earlier layouts
in this file were superseded; no parallel protocol or detour design remains.
The typed Router result is NOT final client delivery until the stock route reports
originalOutcome. Stock processing and client emission are separate instructions.
Native physical ownership persists across speculative matcher/transport resets.

Review regressions execute in routing_test.cpp: locker-held modifiers excluded
through unlock/focus/reconnect; stock-consumed repeat/up retains release-binding
and timer cleanup instructions; same-key replacement rejects before writes and
preserves original repeats/up; typed replacement/reject/partial outcomes; consumed
and forwarded ownership survives focus/reconnect. No upstream callbacks are
installed, so VM-native stock cleanup/IME/XKB tests remain required.
