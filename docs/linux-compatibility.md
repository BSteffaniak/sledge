# Cross-platform keyboard compatibility

This document defines the Linux port's acceptance contract. It is not a claim that
all listed environments are implemented or qualified. Platform qualification
requires application-visible testing on the named desktop and hardware.

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
- Source work is bounded at queue admission and runs outside the input callback,
  but provider work/shutdown lacks a single aggregate cancellation deadline.
- Packaged broker units are diagnostic-only and deliberately deny device access.
  They are not production-capture installation assets.
- Debian/RPM/Arch and Nix deployment qualification remains outstanding beyond
  the delivered offline staging/archive tooling.
- macOS-specific changes require native macOS regression checks; Linux's
  target-gated macOS crate tests do not exercise them.

A trusted compositor/session capture boundary or an explicit revised isolation
policy is an architectural decision needed before production activation. Do not
use fresh attestation files, negative lock hints, or manual permission grants as
an undocumented bypass. Remaining parity features and behavior qualification
must still be implemented after that decision; approval alone does not complete
this port.

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
