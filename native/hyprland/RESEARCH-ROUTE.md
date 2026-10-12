# Isolated stock lock-quarantine fixture

This is a research mode in `stock_adapter.cpp`, not production capture. Default
build and runtime remain passive; Hello capabilities remain zero in both builds.
No matcher events, replacement, asynchronous injection or delivery acknowledgments
are enabled by this fixture.

Compile the same stock SDK target with `-DSLEDGE_STOCK_RESEARCH`, include
`stock_research_route.hpp`, and set `SLEDGE_STOCK_RESEARCH_MODE=lock-quarantine`
explicitly in the isolated compositor process. No active-desktop loading. There
is no identity-only production override. Initial load requires unlocked stock
protocol state and released physical keys, with at least one verified keyboard.

Registration uses existing nonvirtual IKeyboard → Aquamarine IKeyboard → nonnull
libinput device with keyboard capability. Mapping is evdev code identity only
where libinput reports key support and XKB has code+8. Virtual keymaps never use
this mapping. Generations are monotonic, device destruction retires ownership.
Only exactly serialized-equivalent keymaps on verified typing keyboards are
admitted; power/media-only devices cannot select the aggregate map. Serialization
runs at initialization only, never on callbacks. Other keyboards/hotplug bypass.
This limited fixture is not a mixed-device/keymap coverage claim. Keymap/enable change
revokes rather than silently discarding committed ownership; recovery is not yet
implemented.

Three additional exact stock signatures are resolved and byte-checked:

- `CWLKeyboardResource::sendEnter(Hyprutils::Memory::CSharedPointer<CWLSurfaceResource>, wl_array*)`
- `CWLKeyboardResource::sendMods(unsigned int, unsigned int, unsigned int, unsigned int)`
- `CKeybindManager::onKeyEvent(std::any, Hyprutils::Memory::CSharedPointer<IKeyboard>)`

Input entry routes through Router; actual stock keybind boolean admission is
recorded before seat/IME emission, separate from consumer delivery. Stock-consumed
release/timer processing is preserved with denied client output. Quarantined up
also executes stock cleanup under the denied gate. Forwarded/locker union drives
real XKB masks and stack-backed focus-enter arrays. New locker holds appear only
in locker routing; pre-lock holds are quarantined, and locker-held keys cannot
enter app focus after protocol-confirmed unlock.

Actual keybind correlation includes IKeyboard pointer, evdev key and state; stock
std::any contains IKeyboard::SKeyEvent. Lifecycle-only research logs emit
SLEDGE_RESEARCH_REGISTER (identity/generation/name/keymap) and SLEDGE_RESEARCH_EXIT
(registered/modeled/unmapped/keymap-change/correlated-admission counters). VM must
retain compositor logs through plugin unload; no callback file logging is added.

Lock-entry calls recorded old seat-resource/IME-owner release/modifier methods
synchronously before original lock processing, then clears forwarded state. Fresh
locked keys explicitly execute stock keybind processing and go to the seat;
app IME grab key/modifier emissions are suppressed while locked except revocation.
SDK sends return void:
these attempts are UNCONFIRMED and deliberately mark Router output uncertainty,
not Accepted outcomes. Protocol query is used only to confirm true unlock; lock
entry sets authority before the original, destruction/focus never unlocks.
`LockPhase` explicitly distinguishes Entering from Locked: nested protocol false
queries during revocation/focus before the original returns never confirm unlock.
The original must return with protocol true to establish Locked; only subsequent
false can reopen. Rejected/uncertain entry stays closed. The same phase controls
seat-only locked input, IME suppression and locker-only XKB selection in both
lock cycles. Two-cycle host regressions preserve locker-held keys through actual
unlock while denying app enter/repeat/up; this is not VM qualification.
Quarantined pre-lock physical releases run the FULL original InputManager route
under denied seat/IME output, even while locked. Keybind-only release cleanup
leaves InputManager's private m_pressed ledger stuck, suppressing the next fresh
down while permitting its up. Focused host recovery model checks full-path
ledger removal and fresh down/up after unlock; VM recovery remains pending.

Independent VM acceptance needed: Shift+f held across lock, old-client up/zero
modifiers before locker focus, empty pre-lock locker enter/modifiers; physical
release cleanup; fresh locked Shift/letter behavior; unlock while locker Shift
held; keybind-consumed press/release; actual IME grab lock sequence; hook byte
rollback/unload. No callback file logging exists in this adapter. Existing probe
remains the separately traced research target. Compiler success is not signature
lookup, lock-order, IME or wire-delivery qualification.

Replacement remains zero-output rejection. Stock CWLKeyboardResource::sendKey/
sendMods silently return for private missing owner/currentSurface or seat caps,
then use void generated protocol writes. A complete pair preflight/resource
reservation and partial-output recovery has not been implemented; public good()
and void send return cannot be converted to route admission.
