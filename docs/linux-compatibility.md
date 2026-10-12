# Cross-platform keyboard compatibility

This document defines the Linux port's acceptance contract. It is not a claim that
all listed environments are implemented or qualified. Platform qualification
requires application-visible testing on the named desktop and hardware.

## Hyprland integration decision

Full parity must work with unmodified stock Hyprland; a maintained patched
compositor is not an accepted deployment architecture. Stock 0.52.2 capture
remains disabled pending behavioral qualification. An isolated stock-plugin
investigation established function-hook installation, virtual-key suppression
observed by an independent client, and lock-request entry before the original
handler. This establishes a viable interception route, not modifier isolation,
IME coverage, transactional replacement, or complete lock safety.

The backend must establish authoritative lock revocation and actual seat/IME
delivery ownership, including merged-device releases and focus-enter state.
Do not substitute polled lock hints or infer qualification from hook installation
or matching unknown build hashes. See
[`STOCK-HOOK-INVESTIGATION.md`](../native/hyprland/STOCK-HOOK-INVESTIGATION.md)
for the narrow evidence and remaining acceptance gaps.

The core delivery-result API and native routing/XKB components have deterministic
tests. The isolated Nix VM establishes only the stock compositor prerequisite:
0.52.2 with an enabled Virtual-1 640×480 output. It does not qualify keyboard
capture, synthetic replacement, input methods, or lock behavior. Source patches
and patched build artifacts under `native/hyprland` are research fixtures, not
supported backends or replacements for the running compositor. Additional tooling
is obtained through `nix shell`; the VM uses explicitly selected render-node
acceleration and optional KVM, never host input devices or desktop sockets.


## Shared semantics

- Bindings target normalized physical keys, not produced text. Desktop layouts,
  dead keys, Compose, and input methods remain responsible for text interpretation.
- Generic modifiers match either side. Required modifiers are a subset of held
  modifiers; additional held modifiers do not prevent a match.
- `cmd` denotes Command on macOS and Meta/Super on Linux; it is not translated to
  Control. Right Alt is the physical key even when the layout uses it as AltGr.
- Scoped rules use exact application identities resolved through logical aliases.
  Unknown or disconnected focus must not match scoped rules. Global rules remain
  independent of focus availability.
- Rule precedence is deterministic and preserves configuration order within each
  trigger category. Modifier taps honor each rule's configured `within_ms`.
- A consumed physical press owns its repeats and release, including across focus
  changes and config reload. Independent keyboards must not share press ownership.
- A synthetic `send_key` delivers a down/up pair using the requested modifiers;
  physically held extra modifiers must not accidentally modify the action.
- Synthetic events never re-enter matching. Unmodeled native keys pass unchanged.
- Source switching selects an installed, enabled source, including input methods
  where configured. Unknown, disabled, unavailable, and failed selection are
  distinct failures. No Apple-to-Linux source identifiers are guessed.
- Config reload validates before atomically replacing rules and aliases. Invalid
  reloads keep the previous generation. IPC, SIGHUP, and file watching share the
  same reload path. Incomplete tap sequences reset; outstanding presses do not.

## Explicit isolated Hyprland research listener

The normal daemon remains fail-closed. An isolated guest can explicitly select
`run --linux-hyprland-research-socket ABS_PATH
--linux-hyprland-research-pid PID --linux-hyprland-research-api-hash HASH
--linux-hyprland-research-artifact ARTIFACT`. These flags form one command; the
listener requires a private same-user parent, exact peer PID and zero-capability
Hello identity. They conflict with the input broker and never discover or load a
plugin into the running desktop. Startup admission expires after 60 seconds;
transport failure stops research without automatic reconnect or original replay.

This path uses the real config matcher and typed app aliases under the same
atomic reload lock. Correlated native outcomes settle replacement tokens;
repeat/release notifications only retire physical ownership, never dispatch
release-triggered rules. Source changes use bounded queue admission, not proof
of remote completion. Status reports research configuration separately from
capture qualification, which remains false. Stock research events currently
omit app identity, so app-scoped rules cannot be qualified by this fixture.

Workspace checks and mock transaction tests are distinct from consumer-visible
VM acceptance. See [`REPLACEMENT-WIRE.md`](../native/hyprland/REPLACEMENT-WIRE.md).

## Linux privilege and lifecycle contract

The unprivileged user daemon owns user config, matching, focus integration,
source selection, and logs. A narrowly privileged input broker owns verified
physical-device access, exclusive grabs, virtual-device output, peer/session/seat
authorization, output state, and bounded recovery. The broker must not read user
TOML, run shell commands, load plugins, or accept arbitrary device paths/ioctls.

Remapping must not activate without authorization for the active local graphical
session and seat. Remote, inactive, wrong-seat, and stale clients cannot receive
keyboard input or inject into another session. Lock, VT switch, logout,
suspend/resume, device removal, provider failure, and protocol disconnection
require explicit state transitions. Missing verified authority must not become
an unrestricted fallback.

Builds never install services, change device permissions, or grant input access.
Installation does not overwrite user config. Broad input-group membership,
world-readable input devices, world-writable uinput, and a setuid main daemon are
not default installation mechanisms.

Before a physical keyboard is grabbed, its virtual route must be ready and
capable of forwarding all affected events. Composite devices that cannot be
faithfully proxied stay ungrabbed with an explicit coverage error. Handle native
frames, repeats, scan metadata, LED feedback, `SYN_DROPPED`, hotplug, and device
generations. A failed output route must not retain a physical grab.

## Qualification targets

Each target must have a recorded version range and runtime capability probes:

| Target             | Input        | Application identity      | Source selection                                |
| ------------------ | ------------ | ------------------------- | ----------------------------------------------- |
| X11                | evdev/uinput | EWMH/WM_CLASS             | configured XKB groups and input-method provider |
| GNOME Wayland      | evdev/uinput | supported Shell companion | desktop source integration                      |
| KDE Plasma Wayland | evdev/uinput | supported KWin companion  | Plasma/input-method integration                 |
| Sway               | evdev/uinput | Sway IPC                  | configured keyboard layouts/input methods       |
| Hyprland           | evdev/uinput | Hyprland IPC              | configured keyboard layouts/input methods       |

Sway support does not imply all wlroots compositors. Observing only XWayland
windows does not qualify a Wayland desktop. A focus adapter alone does not qualify
source switching. A compilation or fake-transport test does not qualify any
physical-device or desktop behavior.

## Required evidence

1. Shared deterministic tests: modifier sides/subsets, precedence, scope,
   per-rule tap windows and boundary timing, Fn/Caps physical transitions,
   press ownership, multiple devices, reload, and discontinuity reset.
2. Fake transport/state tests: partial writes, bounded queues, timeout/late
   decisions, old generations, session revocation, output reconciliation,
   balanced synthetic state, and device loss.
3. Isolated privileged VM tests: synthetic physical input, independent output
   consumer, grab/proxy lifecycle, stream loss, crash injection, authorization
   revocation, and recovery. Never grab the developer's keyboard by default.
4. Application-visible desktop tests: native and XWayland apps, focus changes,
   held-modifier replacements, repeat, desktop shortcuts, layouts, input methods,
   lock/unlock, compositor restart, per-window/per-device policies.
5. Physical devices: USB, Bluetooth, laptop, dock, ANSI/ISO, non-US layouts,
   multiple keyboards, composite interfaces, LED feedback, reconnect and sleep.
6. Security: wrong UID/session/seat, forged identity, stale epochs, malformed or
   oversized messages, socket races, permission loss, and resource exhaustion.
7. Performance: input latency distributions, idle/typing CPU, queue timeouts,
   memory/descriptor endurance, and logging backpressure.
8. macOS regression: unchanged sentinel pairing and TCC startup invariants;
   blocking desktop work remains outside the tap callback.

## Honest boundaries

Firmware-only Fn cannot be matched when the OS receives no event. Linux modifier
transitions may be observable in ways macOS per-event flags are not; test and
document these differences rather than claiming event-for-event equivalence.
Virtual proxies may require migration of compositor device-specific settings.
Source scope and lock authority vary by desktop. Unavailable capability is an
explicit degraded/blocked state, not a silently disabled binding.

## Current implementation status

The code includes shared semantic regressions, backward-compatible app/source
aliases, conservative Linux CLI lifecycle and IPC, evdev/uinput broker plumbing,
desktop adapters/companions, and offline installation assets. It is **not a
functional Linux keyboard daemon yet**:

- Production capture is denied: logind inspection does not establish the required
  lock/capture authority, and no qualified authority is installed.
- Native Linux `send_key` is unsupported; privileged `Replace` decisions are
  rejected rather than exposing untracked synthetic-write failure paths.
- Virtual output consumer readiness needs qualified evidence; LED feedback and
  repeat-setting cloning are not implemented.
- Desktop/provider APIs and companion syntax have automated checks, not native
  desktop/hardware qualification.
- Source work is bounded at queue admission and runs outside the input callback.
  Built-in adapters share a 400 ms aggregate operation budget across mutex waits,
  inventory validation, selection, and confirmation. Shutdown cooperatively cancels
  waits and joins workers; no provider threads are detached. A non-cooperative
  custom provider can delay joining indefinitely. Already transmitted remote requests cannot be
  recalled, so this is not a guarantee against delayed remote side effects.
- Packaged broker units are diagnostic-only and deliberately deny device access.
  They are not production-capture installation assets.
- Debian/RPM/Arch and Nix deployment qualification remains outstanding beyond
  the delivered offline staging/archive tooling.
- macOS-specific changes require native macOS regression checks; Linux's
  target-gated macOS crate tests do not exercise them.

The production architecture is a trusted compositor/session capture boundary,
retaining strict lock isolation. Session-owned evdev capture with polled lock
hints is not an approved fallback. Before activation, a concrete adapter must
establish suppression, injection, and revocation atomically with the compositor's
lock state. The current focus/source companions do not establish that boundary.
Do not use fresh attestation files, negative lock hints, or manual permission grants as
an undocumented bypass. Remaining parity features and behavior qualification
must still be implemented after that decision; approval alone does not complete
this port.

### Trusted-boundary implementation prerequisite

Production capture must move event suppression and lock revocation into the
compositor's event-delivery path, not grant the current evdev broker authority
from desktop snapshots. The first concrete candidate is a native, version-pinned
KWin input-filter plugin. Its existing QML focus/source companion is not such a
plugin. Hyprland likewise needs native, release-specific integration; GNOME and
Sway require additional compositor hooks. An ordinary X11 client cannot establish
an atomic session-lock boundary.

The native adapter must own per-device press/repeat/release routing, bypass
unknown events, isolate replacement modifiers, exclude synthetic re-entry, and
reset state synchronously before lock delivery. It must also bound any daemon
round trip and pass original input on timeout/disconnection. These requirements
are not implemented or qualified by the current source checkpoint. Implementing
and testing the first adapter requires a selected compositor release, its native
development APIs, and an isolated compositor test environment.

Relevant upstream interfaces (capabilities, not qualified version ranges):
[KWin input filters](https://github.com/KDE/kwin/blob/master/src/input.h),
[KWin lock routing](https://github.com/KDE/kwin/blob/master/src/input.cpp), and
[the InputCapture portal contract](https://github.com/flatpak/xdg-desktop-portal/blob/main/data/org.freedesktop.portal.InputCapture.xml).
The portal's compositor-controlled capture does not by itself establish an
always-on physical keyboard remapping transaction.

## Repository verification

For a coherent final source checkpoint, run in order:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo machete --with-metadata
```

All must pass before a code checkpoint is accepted. Linux CI additionally needs
release builds and isolated integration evidence; macOS remains a regression
platform. Behavioral qualification is separate from these commands.
