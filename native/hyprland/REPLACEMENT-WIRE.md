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
Disconnected peers cannot receive settlement; reconnect snapshot admission remains
a gap, and no whole Router/matcher reset is substituted for exact retirement.
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
