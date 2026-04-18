# sledge

Low-level keyboard-event daemon. A robust, minimal replacement for
Hammerspoon-style key-remap and hotkey setups.

> **Alpha software.** This tool installs a system-wide keyboard event tap and
> synthesizes keystrokes. Read the source, review the config, and only grant
> permissions to builds you trust.

## Why

Hammerspoon is great for throwing together Lua scripts, but the pieces you
actually depend on every day — global hotkeys, app-scoped remaps, modifier-tap
tricks — sit on top of a Lua VM that can silently die and an event tap that
macOS happily disables without telling anyone. When that happens, typing just
stops working right. You don't get a crash dialog. You get a keyboard that
_almost_ behaves.

`sledge` does the small subset you actually use, but does it close to the
metal:

- One Rust binary, no embedded scripting runtime, no plugin loader.
- CGEventTap installed at the HID level with an active watchdog that
  re-enables the tap whenever macOS disables it.
- Injected events are tagged with a private event-source userdata so the
  daemon never re-processes its own output.
- Permissions (Accessibility + Input Monitoring) are checked up-front; if
  they're missing, the daemon says so loudly and exits.
- Config is declarative TOML. No arbitrary code paths. If the config parses,
  it runs; if it doesn't, the daemon refuses to start.
- Runs under `launchd` with `KeepAlive` so it auto-restarts on crash.

## How it works

Three layers:

1. **Core engine** (`sledge_core`) — platform-agnostic. Owns the rule set,
   modifier-tap state machine, and app-scope filter. Speaks in normalized
   `KeyEvent`s keyed by USB-HID usage codes, not per-OS codes.
2. **Platform backend** — currently just `sledge_macos`. Implements the
   `InputBackend` trait: install a tap, deliver events, inject synthesized
   events, report focus changes, switch input sources.
3. **Daemon** (`sledge`) — wires core + backend together, owns the main loop,
   handles SIGHUP reloads, exposes a `status` socket, emits structured logs.

`sledge_linux` and `sledge_windows` exist as target-gated stubs so the
workspace shape is right for future backends. They don't do anything yet.

## Install (from source)

```sh
git clone https://github.com/BSteffaniak/sledge ~/GitHub/sledge
cd ~/GitHub/sledge
cargo build --release
```

Then either invoke the binary directly, or wrap it into a `.app` bundle for
stable TCC grants:

```sh
./scripts/bundle-macos.sh target/release/sledge ./dist
open ./dist/Sledge.app   # triggers permission prompts on first run
```

## Configure

Drop a config at `~/.config/sledge/config.toml`. An annotated example lives
at [`config.example.toml`](./config.example.toml).

```toml
[daemon]
log_level = "info"

[app_aliases.ghostty]
macos = "com.mitchellh.ghostty"

# Cmd+Alt+1 -> switch to US layout
[[binding]]
trigger = { key = "Digit1", mods = ["cmd", "alt"] }
action  = { type = "set_input_source", id = "com.apple.keylayout.US" }

# Triple-tap right Option -> Ctrl+Enter
[[binding]]
trigger = { tap = "RightAlt", count = 3, within_ms = 600 }
action  = { type = "send_key", key = "Return", mods = ["ctrl"] }

# Terminal-only: Ctrl+; -> Ctrl+s
[[binding]]
when    = { app_in = ["ghostty"] }
trigger = { key = "Semicolon", mods = ["ctrl"] }
action  = { type = "send_key", key = "KeyS", mods = ["ctrl"] }
```

## Commands

```
sledge                           # run the daemon in the foreground
sledge run                       # same as above, explicit
sledge status                    # query a running daemon via its socket
sledge reload                    # send SIGHUP to a running daemon
sledge validate <path>           # parse and validate a config file
sledge check-permissions         # report Accessibility + Input Monitoring status
```

## Permissions

On first run, macOS will prompt for **Accessibility** and **Input
Monitoring**. Both are required. If either is missing or revoked, `sledge`
logs an error and exits non-zero — it never silently "sort of" runs.

The app bundle uses a stable `CFBundleIdentifier` (`com.braden.sledge`) and
is ad-hoc codesigned so TCC preserves grants across rebuilds. See
[`docs/macos-permissions.md`](./docs/macos-permissions.md) for detail.

## Project layout

```
packages/
  core/              sledge_core \u2014 engine, types, rule matching
  config/            sledge_config \u2014 TOML parser + validation
  config/models/     sledge_config_models \u2014 serde types
  macos/             sledge_macos \u2014 CGEventTap backend
  linux/             sledge_linux \u2014 stub
  windows/           sledge_windows \u2014 stub
  cli/               sledge binary
```

## License

MPL-2.0. See [`LICENSE`](./LICENSE).
