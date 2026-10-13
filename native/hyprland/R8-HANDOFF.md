# Native r8 startup output handoff (supersedes r7 replay choice)

Implemented, locally compiled/tested; VM qualification remains OPEN. No host load,
commit, catalogue/package edit or VM operation. Source-ready/frozen.

Exact v0.52.2 upstream SeatManager.cpp inspection proves resendEnterEvents clears
keyboard AND pointer focus, calls setKeyboardFocus(nullptr), then restores focus.
Therefore r7's resendEnterEvents call was REMOVED. r8 never mutates focus, emits
leave or synthetic release as startup reconciliation.

After observer hooks are installed and before worker admission:
- public CInputManager::updateCapabilities() recomputes authoritative caps.
- reconcileStartupOutput uses public SeatManager m_state keyboardFocus and
  keyboardFocusResource, owner client matching surface client, resource good and
  exact resource m_owner, physicalQuiescent, getKeysFromAllKBs empty, native
  depressed0. It invokes each exact current seat resource's public sendEnter with
  the released current key snapshot and sendMods with native latched/locked/group.
  Actual intercepted enter records binding; no acceptance override/private state.
- Genuine Caps/latched native bits are adopted into forwarded XKB at this released
  boundary. They are not cleared.
- Held physical/native keys prevent startup replay. Existing native key/modifier
  delivery and real enters remain intact while startup is pending. Production
  registration defers held devices instead of throwing. Existing timer retries
  once released; no manual focus choreography is required for the supported seat.
- Empty/no-focused/invalid owner/resources or unsupported route remains waiting.
  No reason8 is sent until startup output reconciles. Thus daemon cannot infer
  operational admission from Reset8 while enter authority is absent.

Status `hyprctl sledge:status` adds waiting_for_focus (startup output pending) and
native_operational (startup reconciled AND initialReconciled AND worker connected).
waiting_for_focus also covers waiting for held keys to release; not a claim that
client needs manual refocus. R7 additive capability/preflight counters retained.
Cross-lane requirement: daemon health must keep candidate operational false until
native admission/reconciliation, not merely Hello authentication. Native lane does
not change Rust health code.

Regression: foundation startupReplayAllowed rejects native/physical holds,
unknown resources and depressed native mods, preserving held client without any
replay until released. startup_output_test.py verifies production observers before
refresh before Hello, direct sendEnter with unchanged resource owner, no focus
reset API, preserved native latched/locked/group and strict preflight retained.
Six native tests + SHA256 gate test, passive/research syntax, 3 shared-object
compiles PASS via /hdd/sledge-native-r8-1c6d61da/check.sh in offline GCC nix shell.
Startup wiring test, 18-detour test and native diff check PASS. An initial scratch
compile failed because old OpenSSL3.6.1 inputs vanished; scratch script updated to
available OpenSSL3.6.3 headers/libcrypto and complete validation rerun PASS. No
project dependency/packaging mutation. Cargo checks remain coordinator-owned.

Scratch compile-only hashes (dummy scope pins; not VM-qualified):
research b89b0b688e349e60c606a32918ea60ba05d69f63f71b7b8775f61d095baa0e75
production a5c8ece191dfa5f620447f5fd4abbba96ac0495eaffd26f6392ce35dca1160ac
candidate a986d0dcceeba7be51ac7e5a1df1fe80eac681554a4bf0670533e3c6a8eb5387

Rebuild fresh immutable source/build ID/descriptor/catalogue; baseline must use
pre-existing focused client without external focus dance and require real B/no A.
Also qualify held startup release and Caps snapshot. Counters diagnose any further
preflight failure; do not treat source tests as live compositor success.
