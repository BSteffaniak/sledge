## Wave5 lifecycle/focus checkpoint

Event identity now comes from actual SeatManager focus surface mapped through
stock getWindowFromSurface: kind1 XDG toplevel m_state.appid, kind2 stock cached
CWindow::m_class (fetchClass's XWayland WM_CLASS). Bounded UTF-8 validation rejects
NUL/malformed/>256-byte identities without truncation; unknown surfaces/locker
remain kind0. No polling, string allocation or serialization in callbacks.

Lifecycle callbacks enqueue ordered Lock9 (before stock lock original and on
confirmed unlock), Reset7 focus reason2 and Retire8 removal reason1. Frozen v1
uses Lock as the only epoch advance, so epoch-establishing Lock precedes same-epoch
Reset/Retire. Queue/send failure is not acknowledgement and never erases holds.

`SLEDGE_STOCK_RESEARCH_DROP_OUTCOME_ONCE=1` drops only the first correlated Replace
proposal's Outcome, after actual native routing. Native suppresses committed
repeat/up normally; Rust must mark missing outcome Uncertain, never replay.
fail-after=mods now restores modifiers WITHOUT bare target Up; fail-after=down
sends target Up only after Down invocation. Shared emission tests cover both.

Approved Reset reason8 reconciliation-complete is emitted after initial or fresh
worker reconnect authentication, at the compositor pre-event linearization point. Router
must have no ownership/pending/fatal state, physical devices must all be accounted
and unchanged/enabled, unlocked. Only the exact current fresh physical Down is
excluded from stock's already-updated physical array. Boundary precedes Event in
the ordered worker stream; the correlated Decision implies peer consumption.
Unknown/unmapped input revokes the connection. Existing holds delay admission;
no whole-state reset. Stale worker-auth-time proof is never used. Worker retries
use fresh nonces and monotonically increasing local connection generations;
notification entries are generation-tagged and Events checked against the
connection nonce before sending. No callback reconnect spin/join/socket IO.

## Connection binding precedes every control

`ensureConnectionBinding` runs before lifecycle sends and key processing.
Only a new authenticated worker generation binds nonce and resets sequence /
wire epoch, before ownership-preserving Router disconnect. Same-generation
controls before the first key retain sequence: focus controls1..4 then boundary5,
never sequence1 again on the same nonce. Fresh generation resets once; stale
worker queue entries remain generation-tagged. Binding and wire increments are
compositor-local; no callback IO or increased deadline. Regression tests cover
both helper state and actual control constructor headers across first key and
fresh connection. Normal7 was a scope regression, not a codec decode failure.

## Frozen encoder / packet evidence

Actual lifecycle controls and Events use shared `stock_wire.hpp` constructors.
Reset8 is opcode7/generation0/body8; Lock9 carries state0/1/2, never reason8.
The ticket epoch notification is intentionally Lock0 (epoch advance), not Reset.
Native states are Up0/Down1/Repeat2; focus encoding updates the body length.
`wire_encoder_test.cpp` emits eight actual constructor packets (Reset8, Lock0,
and all three Event states with unknown/XWayland identity) for current Rust
`Packet::decode` and byte-identical encode verification. This does not establish
that an older packaged peer has the same decoder.

Opt-in `SLEDGE_STOCK_RESEARCH_WIRE_TRACE=1` prints at most sixteen complete
512-byte packet hex records from the socket worker, not callbacks. Notify
records follow successful sends; Event/reply records diagnose transactions.
Tracing may perturb worker timing and is diagnostic only. No wire/deadline/
capability change. Compare captured bytes against the exact packaged peer
before attributing generic InvalidData to any particular field.

## Compatible lifecycle retains authenticated transport

New/destroy/layout hooks no longer unconditionally invalidate the worker.
Before original lifecycle work, speculative tickets are revoked without erasing
ownership and proposal admission is closed. If already reconciled, Reset3 is
queued in-order; initial pre-boundary lifecycle work needs no reset. Equivalent
consumer-map rebuild/registration retains the socket, then exact pre-event
reason8 proof is still required. Unknown/incompatible consumer maps or failed
control enqueue close the stream. Held-state layout changes remain refused.
Unload-only `SLEDGE_RESEARCH_CLOSE` counters label new_keyboard,
destroy_keyboard, layout, consumer_changed, outcome_queue, unknown_input,
and count compatible_lifecycle completions. Worker requested_close is no
longer expected for compatible baseline configuration churn. No deadline or
capability increase, hold reset, app identity guess, or matcher permissive mode.

## Boundary diagnostics / deferred map refresh

`SLEDGE_RESEARCH_BOUNDARY` unload-only logs expose fixed-size callback counters:
ready, maps_incompatible, no_consumer, consumer_unregistered, router_busy,
locked, nested, revoking, device_changed, physical_hold, unaccounted; plus
queued/queue_failed/not_ready and map_deferred/map_failed/map_succeeded.
No callback IO/logging/allocation is added. A lifecycle rebuild deferred by
ownership, lock or nesting no longer poisons an unchanged qualified map;
actual consumer/device pointer changes still fail boundary qualification.
VM diagnostics must establish the observed blocker before claiming baseline
admission fixed. There is no ownership reset or forced reason8 bypass.

## Runtime lookup / neutral boundary repair

Lifecycle hooks select exact pinned ELF names from `SFunctionMatch::signature`:
`_ZN13CInputManager11newKeyboardEN9Hyprutils6Memory14CSharedPointerI9IKeyboardEE`,
`_ZN13CInputManager15destroyKeyboardEN9Hyprutils6Memory14CSharedPointerI9IKeyboardEE`,
`_ZN13CInputManager17setKeyboardLayoutEv`.
The Aquamarine overload is deliberately excluded. Stock lookup filters raw
symbols and associates demangled lines by index; lifecycle selection no longer
relies on that association. Missing/ambiguous symbols still fail installation.

Every verified physical keyboard is now registered, including power/media-only
hardware, provided its map is equivalent to the verified active typing consumer.
The aggregate map is selected from that active consumer before enumeration;
power/media devices never select it. Previously those devices were excluded
while the boundary demanded complete coverage, permanently rejecting the
fixture's enabled power-button.
No holds are reset: exact public `getPressed(evdevKey)` queries remain required;
stock updates its private pressed vector before emitting the key callback.
Equivalent keymaps, full device accounting and native/output quiescence remain
mandatory. This repair is not VM acceptance or production qualification.

## Wave6 lifecycle checkpoint (capabilities remain zero)

Research hooks additionally match exact stock SDK signatures:
`CInputManager::newKeyboard(Hyprutils::Memory::CSharedPointer<IKeyboard>)`
(post-original admission), `destroyKeyboard` with the same argument
(pre-original idempotent retirement), and `setKeyboardLayout()`.
The latter refuses the original while physical/native ownership is held,
locked or uncertain; callers must retry after releases. It has no result
channel and no automatic queued retry in this checkpoint. On released layout
changes, consumer XKB/replacement state is rebuilt outside key callbacks;
only serialization-equivalent enabled device maps qualify. Mixed layouts,
virtual/unaccounted devices, and failed rebuilds block reason8 admission.
Direct per-device config/keymap mutation is detected and rejected, not yet
fully supported by this lifecycle hook set. Held hotplug admission may need a
later explicit layout retry after physical release. No mask-index ORing.

Ordinary unload drain remains unimplemented: stock
`CPluginSystem::unloadPlugin(const CPlugin*, bool)` offers no exit veto.
A future exact unload interception must retain hooks through physical drain
before forwarding the original; `eject=true` bypasses PLUGIN_EXIT. No claim of
held-unload safety is made here. Fatal output remains latched; reconnect
never supplies authority to clear it.

Default production plugin remains passive caps0. Full hotplug/mixed maps/IME replacement, exact uncertain
output recovery, bounded unload preserving held stock ledger, complete timing
qualification and independent VM lifecycle/focus/fault evidence remain unmet.

# R7 replacement-wire fixture

Research ONLY: compile `SLEDGE_STOCK_RESEARCH`, explicitly select
`SLEDGE_STOCK_RESEARCH_MODE=replacement-wire`, private endpoint via
`SLEDGE_HYPRLAND_PASSIVE_SOCKET`. Native client/Rust listener, Hello caps0, SHP1
layouts unchanged. Default remains passive. VM lane alone loads/runs compositor.

Native down callbacks create Router tickets and fixed Event packets (unknown
focus kind/empty string; physical normalized key, state/time, forwarded side mods).
Worker authenticates socket and validates Decision correlation/padding. Callback
wait ends 1ms before the original 10ms deadline, reserving commit budget. Real
Router replacement/stock admission runs synchronously; Outcome queued worker-side
retains exact nonce/generation/epoch/sequence/deadline and original key/state/focus.
Queue failure invalidates speculation, never erases consumed ownership. Missing
or late Outcome is Uncertain at Rust, never original replay. R8 exports every modeled unlocked Down/Repeat/Up in physical order. Fresh Down
alone proposes matching. Repeat/Up use correlation-only settlement tickets even
after fatal replacement uncertainty; Rust replies canonical Pass without action,
and native ignores the reply verdict for routing. Actual Router ownership still
controls suppression/cleanup and retires only on physical Up. Outcome reports
actual lifecycle result with the same full correlation. Rust retires exact matcher
ownership/modifier state only through its settlement API, not rule dispatch.
No release-triggered/tap action is supported in this narrow wire interface.
Disconnected peers cannot receive settlement; fresh-connection admission waits
for native quiescence rather than exporting held-state snapshots. No whole
Router/matcher reset is substituted for exact retirement.
Unlocked epoch changes are explicitly
announced via opcode9 before next unlocked Event. Locked events are not exported.
No socket/file IO or logging in adapter callbacks.

Reservation: strong references to all live keyboard resources of exact focused
seat owner; each must match observed sendEnter surface and owner, good() and
observed keyboard capability. No IME replacement. Stock capability is PRIVATE;
we track exact `CWLSeatProtocol::updateCapabilities(unsigned int)` original and
remain preflight-denied until a real post-load update is observed. Binding must
also be established through a post-load real focus enter. No guessed private
layout, source patches or capability overrides. VM must trigger guest lifecycle
or expect reject-before-output. Mixed keymaps/hotplug admission is still limited.

Commit directly invokes the reserved resource modifier/key methods: isolated
requested mask, down, up, restored forwarded mask. No InputManager/keybind/matcher
reentry. Accepted is resource-route invocation under reservation, NOT app receipt
or reliable Wayland queue success. Independent consumer pair/masks/ordering must
qualify this meaning. The generated sends and SDK return void; output uncertainty
is conservative, and a deadline overrun after output becomes PartialFatal.

Deterministic native failure controls (research process only):
`SLEDGE_STOCK_RESEARCH_FAIL_AFTER=preflight|mods|down`. preflight writes nothing;
mods/down retain consumed ownership, best-effort up/restored modifiers, report
PartialFatal; originals never replay. Target-held/same-key rejection uses Router.
Trigger/target/requested mods, delayed decisions and disconnect belong to explicit
Rust research-peer arguments, not wire-schema changes. Proposal-ready lock race
barrier is NOT implemented; bounded callback waiting does not pump protocol lock
requests, and asynchronous replacement is forbidden.

Host checks build both plugin variants; worker test exercises Hello/Event/Decision/
Outcome over a private seqpacket socket; routing tests exercise held rejection,
partial ownership and restoration contracts. This is not independent native
replacement evidence. Further gaps: synthetic-held ownership from other plugins,
precise app focus identity, complete reset/retire/locked wire notifications,
reconnect admission, capability hook lookup/runtime, aggregate deadline guarantees
through unbounded stock generated writes, and native/Rust research interop fixture.
Production capabilities stay zero.
