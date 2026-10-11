# Hyprland 0.52.2 qualification probe and routing component

## Architecture boundary: stock Hyprland only

The user requires stock compatibility; no maintained compositor fork or patched
runtime is supported. Source patches and their Nix build expressions below are
frozen RESEARCH FIXTURES only, never deploy/load instructions. Full parity is
blocked on authoritative upstream stock hooks for pre-lock revocation and complete
key/modifier/focus/IME routing. Stock capture remains disabled. No adapter transport
exists or will be fabricated as a placeholder. Component tests and VM prerequisites
are not a production integration claim. See LIFECYCLE.md for settled observational
repair evidence; future source-patch expansion is stopped.

The probe below is separate from the executable host-independent routing component
in `routing.hpp` / `isolated_output.hpp`; see INTEGRATION.md for its tests, concrete
stock API blockers, and the minimum authoritative lock-boundary source patch.
Routing tests passed strict optimized compilation and AddressSanitizer/UBSan.
No native compositor routing bridge is installed or qualified.

**Not an input backend. No hooks, listeners, sockets, injection, or capture.**
`probe.cpp` is a real Hyprland plugin entrypoint that rejects unknown SDK commit
identity and all unqualified artifacts. Its default behavior is refusal, including
the current host. No allowlist or override exists. Loading code necessarily runs
C++ header initialization before pluginInit; this refusal is not a general-purpose
sandbox for incompatible binaries. Do not load it into an active desktop.

## Evidence

Upstream tag v0.52.2 resolves to
`386376400119dd46a767c9f8c8791fd22c7b6e61`. Source research is read-only;
installed SDK signatures were independently checked with static_asserts.

Exact NixOS SDK/runtime metadata:

- nixpkgs `b6018f87da91d19d0ab4cf979885689b469cdd41`
- derivation `/nix/store/4zwfhvvhkmlpsc4576z6wb1wa45nrmc8-hyprland-0.52.2.drv`
- dev `/nix/store/7lvv1y5wdzzlrjy14cgqk6rvyf9hp07i-hyprland-0.52.2-dev`
- runtime `/nix/store/n13h1pmiv6n05w0lkd7fbaywg1fc9b4p-hyprland-0.52.2`
- executable SHA-256 `f1ca58664104e1facf2e3bdb76b4b9499174e2688357439b23fc526ac479576c`
- GCC 14.3.0, C++23; installed version.h reports `GIT_COMMIT_HASH="unknown"`

The stock dependency-qualified API hash includes commit and major/minor versions
of aquamarine, hyprutils, hyprgraphics, hyprcursor, hyprlang. An `unknown` commit
plus those versions is not sufficient exact ABI identity. Future qualification
must bind the full closure and executable digest, API version/hash, compiler ABI,
and tested function signatures; Nix package identity is not routing qualification.

## Concrete API ordering blockers

Pinned upstream source:

- `src/devices/Keyboard.cpp:31-41`: update physical pressed state, emit key,
  then update XKB. Cancelling the key hook does not cancel later modifiers.
- `src/managers/input/InputManager.cpp:1423-1483`: cancellable `keyPress` runs
  before keybindings, then IME/seat delivery; raw keycodes are available.
- `InputManager.cpp:1485-1525`: separate modifier routing merges keyboards and
  delivers to IME/seat. Key cancellation alone is not isolated suppression.
- `src/protocols/SessionLock.cpp:185-201`: newLock emits BEFORE m_locked=true.
- `src/managers/SessionLockManager.cpp:45-46,49-129`: stock newLock listener
  already registered; it changes focus/grab and can sendLocked synchronously.
  A later plugin listener does not prove reset before lock delivery.
- `SessionLock.cpp:128-139`: genuine unlock clears m_locked then emits
  unlockAndDestroy. Destruction alone must not reopen matching.

Complete interception would require verified early lock/key/modifier detours or
a compositor patch, per-device ownership, forwarded-only modifier aggregation,
focus-enter held-state filtering, IME-aware isolated delivery, safe hot-unplug,
and synthetic bypass. None is implemented or behaviorally qualified here.
See WIRE.md for the initial distinct native transport/ACK/reset contract.

## Reproducible build (dependencies only via nix shell)

The coordinator/Nix lane owns the `hyprland-sdk` wrapper in flake.nix. No package
manifest is added here. Build outputs belong in an isolated temporary directory:

```sh
nix shell --no-write-lock-file .#hyprland-sdk --command sledge-hyprland-sdk sh -eu -c '
  d=$(mktemp -d /tmp/sledge-hyprland-probe.XXXXXX)
  "$CXX" -std=c++23 -O2 -Wall -Wextra -Werror -fPIC -shared \
    $(pkg-config --cflags hyprland) native/hyprland/probe.cpp -o "$d/probe.so"
  "$CXX" -std=c++23 -Wall -Wextra -Werror \
    native/hyprland/probe_test.cpp -ldl -o "$d/probe-test"
  sha256sum "$d/probe.so" "$d/probe-test"
  env -i "$d/probe-test" "$d/probe.so"
'
```

Build passed, including `-O2 -Wall -Wextra -Werror`. The standalone test executes
the same host-independent identity policy used by pluginInit: empty, unknown,
and known-but-unqualified identities all reject (PASS). It then attempts a real
RTLD_NOW load without host symbol stubs and returns **77 (BLOCKED)**: real SDK
headers require the host `CHyprColor` constructor at load time. No fake
host symbol is supplied. This is policy evidence, not native pluginInit or
compositor qualification; compositor load/unload must use an explicit isolated
headless environment supplied by the Nix lane, never inherited sockets/devices.
The current optimized plugin SHA-256 is
`1994bf3d784ac64391c2993989df1fe77c65bac7808e46ad618a4846becd4c8a`.
No compositor plugin load/unload test or physical behavior test has passed yet.
The Nix lane reports an actual device-free, private bwrap headless attempt against
this host binary aborting with `CBackend::create() failed` (exit 134). This is
reported cross-lane evidence, not a test executed by the native lane. The upstream
backend selects headless plus DRM-if-available/Wayland fallback rather than a
qualified headless-only route. Accessing host devices is not an allowed workaround.
