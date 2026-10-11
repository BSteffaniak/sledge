# Authoritative proposed native transaction/reset wire

**Frozen design research, not an adapter interface shipped to production.** Stock
Hyprland compatibility is required. No patched fork is supported, no transport
exists, and no placeholder will be added. This proposal cannot establish missing
stock lock/input authority. Capture remains disabled.

This supersedes all earlier wire/outcome handoffs. NOT implemented transport;
no production capture or upstream callback qualification. One architecture:
source-integrated compositor bridge plus early lock-boundary.patch, not detours.

## Framing

AF_UNIX SOCK_SEQPACKET, exact 512-byte little-endian packet. Worker authenticates
SO_PEERCRED UID/private endpoint, rejects malformed/truncated packets, unknown
fields and nonzero padding. Header: magic SHP1@0[4], u16 version1@4, u16 opcode@6,
u32 body_len<=448@8, u32 flags0@12, u64 device_generation@16, u64 lock_epoch@24,
u64 sequence@32, u64 monotonic_deadline_ns@40, u64 connection_nonce@48,
u64 reserved0@56. Body@64. Generation0 is global; device generations never reused.
All decision/outcome correlation echoes nonce/generation/epoch/sequence/deadline.
One pending transaction, <=10ms aggregate callback wait: bounded BLOCKING, not
fully nonblocking. Socket I/O stays worker-side. Compositor commits synchronously.

Bodies (relative offsets; unused bytes zero):

- 1 Hello / 2 HelloAck, length400: u64 capabilities@0, u16 hash_len<=192@8,
  u16 artifact_len<=192@10, u32 reserved0@12, UTF8 API hash slot@16[192],
  UTF8 registered closure+digest artifact identity slot@208[192]. No NUL.
  Bits0 capture,1 lock authority,2 isolated injection,3 transactional replacement;
  all other bits reserved. Probe has capabilities0. Identity is not qualification.
- 3 Event, length32+focus_len: u32 evdev key@0,state@4(0up/1down/2repeat),
  compositor time_ms@8,physical side modifiers@12; u64 focus_epoch@16;
  u16 focus_kind@24(0unknown/1Wayland app_id/2X11 class),focus_len<=256@26;
  u32 reserved0@28; UTF8 exact focus@32. Unknown focus length0; never truncate.
  Modifier bits0..7: L/R Shift,L/R Ctrl,L/R Alt,L/R Meta. Not XKB indices.
- 4 Decision / 6 standalone Inject, length16: u32 verdict@0(0Pass/1Swallow/2Replace),
  replacement key@4, requested side mods@8,reserved0@12. Pass/Swallow fields zero.
  Inject requires verdict2,generation0,fresh sequence,live focus/lock epoch.
- 5 Outcome, length24: u32 result@0 (0Replaced,1RejectedPrecommitOriginalAdmitted,
  2PartialUncertainFatal,3TimeoutOriginalAdmitted,4SwallowCommitted,
  5StockConsumed,6OriginalAdmitted,7MergedOriginal,8LockerOwned,9Quarantined);
  u32 reason@4(0none/1targetHeld/2stale/3deadline/4routeUnavailable/5keymapChanged/
  6uncertain/7malformed);u32 original key@8,state@12;u64 actual focus_epoch@16.
  Admission is consumer-route acceptance, NOT application receipt. Merged means
  physical press ownership exists but duplicate seat key emission was suppressed.
  An original proposal's final outcome follows stock keybinding evaluation.
- 7 Reset / 8 Retire, length8: u32 reason@0,reserved0@4. Reasons0connection,
  1removal,2focus,3keymap,4overflow,5transport,6lock,7injection. Retire requires
  nonzero generation; Reset0 invalidates all speculative transactions, nonzero
  invalidates that device's speculative scope. Neither erases committed physical
  press ownership. Retire additionally reconciles output and removes that device.
- 9 Lock,length8: u32 state@0(0unlocked/1locked/2uncertain),reason@4. Epoch advances;
  revoke before locker focus/grab/ACK. Death never unlocks. Real unlock only.

## Single ownership and reset contract

Matcher's opaque proposed outcome token is speculative. Precommit reject/timeout
admits original and rolls back ONLY its proposal, preserving unrelated ownership.
Original owns repeat/up thereafter. Replace commits only after pair route acceptance;
partial output or missing ACK is uncertainty: NEVER replay original, disable future
replacement and explicitly reconcile output. Async core injection is not Replace.

Router returns typed RouteResult: outcome, processStock, deliverClient, reconcile.
StockConsumed repeat/up MUST process stock keybindings/release/longpress cleanup,
while denying client delivery. Merged events likewise process stock but deny
redundant client emission. Swallow/replacement ownership bypasses stock processing.
LockerOwned downs are delivered only within locked routing; after unlock they remain
excluded from app focus-enter/modifiers/repeats until physical up. Quarantined
pre-lock holds remain suppressed through up. Unknown keys bypass unchanged.

Focus, reconnect, transport/overflow resets invalidate speculative nonce/epoch scope
ONLY; committed consumed/forwarded/stock/locker holds survive. Lock revocation is
separate: reconcile old-focus forwarded releases/modifiers synchronously, quarantine
old holds before locker route, invalidate speculation, reset matcher proposals.
Keep native suppression ledger until physical release even if matcher state resets.
Keymap/IME change needs explicit forwarded-XKB rebuilding, not silent ledger clearing.

Preflight rejects any physically held target INCLUDING current original down and
consumed/quarantined holds, and any consumer synthetic/IME held target. Zero output
on reject permits original fallback. After any output, uncertainty requires best-effort
up/modifier restoration but no original replay. Capture stays denied until bridge,
forwarded-only XKB/focus-enter, IME/keymap and VM lock ordering tests qualify.
