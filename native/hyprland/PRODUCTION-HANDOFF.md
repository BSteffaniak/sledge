# Production-r1 native handoff (not qualification)

Build `stock_adapter.cpp` with `SLEDGE_STOCK_PRODUCTION` and quoted
`SLEDGE_STOCK_BUILD_ID` (64 lowercase hex digits). Exports:

- `const char* SLEDGE_STOCK_ARTIFACT()` => `sledge-stock-v1:<build-id>`
- `unsigned SLEDGE_STOCK_REGISTRATION_VERSION()` => 1
- `unsigned SLEDGE_STOCK_WIRE_VERSION()` => 1

Build ID is SHA-256 of the package canonical source/toolchain/build contract,
not plugin self-digest. Registration separately binds actual compositor ELF
SHA-256, SDK API hash, installed plugin SHA-256, wire version and behavioral
qualification revision. No unknown-commit/hash-only capture bypass. Current
production compilation has route code but does NOT set replacementWire; trusted
immutable registration verification remains an integration blocker. Production
research mode/failure environment cannot enable capture. Research is retained.

## F-prod-baseline1 retirement fix (acceptance pending)

Empty/non-output device retirement no longer invokes modifier reconciliation.
Previously removal before a client route existed manufactured fatal uncertainty
from a void/no-route output adapter, despite zero forwarded ownership. Real
forwarded holds still require release and modifier cleanup; failure remains fatal.
Tests contrast zero-output retirement with real held-output failed cleanup.
Unload `SLEDGE_ROUTER` reports owner counts/types, first device/key, fatal,
pending, locked and stock_empty. This separates RouterBusy causes without
callback logging or resetting committed holds. Actual-daemon reason8 + Replace
and consumer B-pair acceptance remains VM-owned and unconfirmed here.

## Revision3 map projection and held hotplug retry

Exact seat selection hook:
`_ZN12CSeatManager11setKeyboardEN9Hyprutils6Memory14CSharedPointerI9IKeyboardEE`.
Registered physical maps no longer require serialization equivalence. Lifecycle
registration prepares bounded per-map XKB states; callback selection projects
forwarded evdev-held keys in the selected consumer map before stock publishes
keymap/output. Replacement modifier preflight selects a prepared state too.
No mask ORing, callback keymap serialization or state construction. Missing
preparation closes projection admission. Cache capacity is16; exhausted caches
fail closed rather than silently reuse stale maps.

A preinstalled SDK CEventLoopTimer retries registration every50ms on compositor
lifecycle thread, after held unregistered devices have actually released. No
release callback allocates/queues work; destruction retires generation first;
slot admission never replays the pre-admission hold. Timer is cancelled at exit
and disarmed after detach. Unknown keys remain unmodeled stock passthrough.

Consumer-switch triggering key currently passes stock and closes speculation;
matching waits for quiescent Reset8 rather than replacing before the client has
received its new keymap. Thus continuous alternating-device held chords may
remain pass-through until quiescence. Full arbitrary latched/locked/group
history and cache recycling after repeated keymap mutations remain acceptance
work, not proven support. Tests project a held Shift across US/German and an
artificial Mod3-index map, restore without stuck rollover, and select group1.
18-detour detached structural check includes the new selection hook.

## Implemented recovery

Exact raw symbol `_ZN15CWLSeatProtocol15destroyResourceEP19CWLKeyboardResource`
tracks retirement of every affected output resource after PartialFatal. Weak
expiry/focus leave do not discharge proof. Deterministic fixture cleanup sends
are invocation evidence, not queue receipt. Overflow permanently blocks recovery.
A fresh binding after all affected retirements, qualified consumer map, unlocked
state, no physical holds and no committed ownership/pending transaction permit
explicit Router fatal retirement. No replay and no generic ownership reset.
Next proposal still requires ordered Reset8 native proof. Tests cover multiple
resources, pre-retirement binding rejection, physical/qualification/lock gates,
and Router consumed-hold preservation. VM acceptance remains required.

## Remaining work

Ordinary drain controls now exist: `hyprctl dispatch sledge:drain` takes no
arguments, irrevocably disables new proposals without erasing ownership.
`hyprctl sledge:status` returns JSON fields `version:1`, `draining`, `quiescent`,
`connection_generation`, `ownership_epoch`. Commands execute on the compositor
thread through SDK addDispatcherV2/registerHyprCtlCommand; no socket Inject
control is accepted. Controller polls exact quiescence before invoking unload
externally. Commands are unregistered before adapter teardown.

Status now commits irreversible Detached on the compositor thread only after
physical/native quiescence, tracked stock seat-ledger emptiness, no pending
output recovery/modifier masks, mailbox Idle and worker idle-park acknowledgement.
Version1 adds `detached` and `drain_blocked_reason` (not_draining,
physical_or_output, stock_ledger, output_uncertain, output_modifiers, mailbox,
worker, none). `detached=true` is durable; later input is stock-only and cannot
create new native ownership. Controller must await detached, not quiescent alone.
All 17 detours either early-call originals or already are pure originals;
resource enter passes the original key array. Lifecycle/wire binding is disabled
while drained. Worker park is nonblocking; join stays in PLUGIN_EXIT.

Stock ledger tracks seat-send path entry before filtering (stock updates private
m_pressed first), plus original focus-enter held keys for conservative load-time
accounting. Physical release is not substituted for stock cleanup evidence.
Missing/unmodeled cleanup evidence or unknown keys blocks detach. Unit/structural
coverage does not constitute VM acceptance; direct unload/forced eject without
durable detached remains outside supported controller policy.
Held hotplug retry, mixed-map active-consumer projection,
registration verification, and release-trigger actions remain unimplemented.
Release actions require negotiated eligibility plus ReleaseReplace original-Up
then replacement-pair semantics, not bare Replace-on-Up. Frozen SHP1 unchanged.

Full Cargo validation belongs to coordinator after lane settlement. Native
validation uses offline Nix GCC/SDK, /hdd scratch, sequential compiles (<=2 cores),
six tests and research/production-test shared objects. Production-test build ID
is test-only all-zero input, not a qualification entry or deployable artifact.
