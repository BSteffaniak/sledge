# Architecture

`sledge` is split into three conceptual layers plus platform backends.

```
+-------------------------------------------------+
|  Config (TOML on disk)                          |
|  sledge_config_models + sledge_config           |
+-------------------------------------------------+
|  Core engine (platform-agnostic)                |
|  sledge_core                                    |
|    * KeyEvent / KeyCode / Modifiers             |
|    * RuleSet compile + match                    |
|    * Modifier-tap FSM                           |
|    * InputBackend trait                         |
+-------------------------------------------------+
|  Platform backend (target-gated)                |
|  sledge_macos  (CGEventTap + TIS + NSWorkspace) |
|  sledge_linux  (stub)                           |
|  sledge_windows (stub)                          |
+-------------------------------------------------+
|  Daemon (binary)                                |
|  sledge                                         |
|    * main loop, signal handling                 |
|    * IPC socket (sledge status/reload)          |
|    * tracing logger                             |
+-------------------------------------------------+
```

## Normalized key model

The core engine speaks in **USB-HID usage codes**, not per-OS key codes. This
is the lowest-common-denominator across macOS, Linux, and Windows. Each
platform backend is responsible for translating its native key code into the
HID enum on the way in, and back on the way out.

`Modifiers` is a bitflag with left/right distinction. A triple-tap on "Right
Option" is distinguishable from a triple-tap on "Left Option".

## Verdicts

When the backend hands the core a `KeyEvent`, the core returns a `Verdict`:

- `Pass` \u2014 let the event through unmodified.
- `Swallow` \u2014 drop the event; do not deliver it to any app.
- `Replace(event)` \u2014 drop the original and inject a synthesized event.

The macOS tap callback applies the verdict synchronously.

## Self-event filtering

Synthetic events are injected through a private `CGEventSource` whose
`sourceUserData` is set to a sentinel value (`SLEDGE_SOURCE_TAG`). The tap
callback checks this field on every event and, if it matches, passes the
event through without running rule matching. This breaks any potential
feedback loop where a synthesized event re-triggers the same rule that
created it.

## Tap resilience

macOS disables an event tap when:

- The callback exceeds the tap's timeout budget.
- The hosting process loses Input Monitoring / Accessibility trust.
- A user explicitly toggles the tap off.

`sledge_macos` installs a watchdog thread that polls `CGEventTapIsEnabled`
every 500ms and re-enables the tap whenever it has been disabled. The
watchdog logs the re-enable so degradation is visible in the daemon's log
stream.

## Config reload

`SIGHUP` triggers config reload. The new config is parsed and validated
before the old `RuleSet` is swapped. If validation fails, the old rules stay
live and the failure is logged. This makes `kill -HUP <pid>` safe to run
during experimentation.

The daemon also runs a background file watcher on the config file's parent
directory. Events are debounced ~250 ms to coalesce the multi-write saves
most editors perform. The watcher invokes the same reload closure SIGHUP
uses, so all triggers behave identically: atomic swap on success, no
change on failure.
