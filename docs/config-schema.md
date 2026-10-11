# Config schema

`sledge` is configured via TOML. Default location:

    ~/.config/sledge/config.toml

Override with `sledge run --config <path>` or `SLEDGE_CONFIG=<path>`.

## Top-level tables

### `[daemon]`

```toml
[daemon]
log_level = "info"   # one of: trace, debug, info, warn, error
```

### `[app_aliases.<name>]`

Logical name to per-platform identifier. Reference the logical name from
`when.app_in` in your bindings.

```toml
[app_aliases.ghostty]
macos   = "com.mitchellh.ghostty"
# linux = "com.mitchellh.ghostty"   # Wayland app_id (future)
# windows = "Ghostty.exe"           # window class / exe (future)
```

On macOS, only `macos` is consulted. Linux adapters distinguish exact Wayland
application IDs from X11 classes; legacy `linux` strings remain available as an
explicit fallback in the observed identity category. There is no fuzzy matching
or window-title/process-name inference.

```toml
[app_aliases.editor.linux_ids]
wayland = "org.example.Editor"
x11_class = "ExampleEditor"
```

Identifiers are case-sensitive. Duplicate identities within a category are
rejected deterministically. Validation rejects unknown logical aliases; an
alias without an identifier for the current platform cannot match its app.
Missing or stale desktop focus never matches scoped rules.

### `[[binding]]`

Array of tables. Each entry has a `trigger` and `action`, and optionally a
`when` scope.

```toml
[[binding]]
when    = { app_in = ["ghostty"] }           # optional
trigger = { key = "KeyK", mods = ["alt"] }
action  = { type = "send_key", key = "KeyT", mods = ["alt"] }
```

## Trigger shapes

### Hotkey

```toml
trigger = { key = "Digit1", mods = ["cmd", "alt"] }
```

- `key`: an HID-style key name. See "Key names" below.
- `mods`: array of modifier names. One of: `ctrl`, `shift`, `alt`, `cmd`,
  `fn`, `left_ctrl`, `right_ctrl`, `left_shift`, `right_shift`, `left_alt`,
  `right_alt`, `left_cmd`, `right_cmd`.
  - Plain `ctrl` matches either side. `left_ctrl` / `right_ctrl` require
    that specific side.

### Modifier tap

```toml
trigger = { tap = "RightAlt", count = 3, within_ms = 600 }
```

- `tap`: a modifier key name. Must be a modifier (e.g. `RightAlt`,
  `LeftShift`, `RightCmd`), not a regular key.
- `count`: number of taps required (>= 2).
- `within_ms`: maximum milliseconds between consecutive taps.

Pressing any non-modifier key or holding another modifier resets the counter.

## Action shapes

### `send_key`

```toml
action = { type = "send_key", key = "Return", mods = ["ctrl"] }
```

Synthesizes a key-down followed by a key-up of the given key with the given
modifiers. The synthesized events carry the self-event sentinel so they
don't re-trigger the daemon's own rules.

### `set_input_source`

```toml
action = { type = "set_input_source", id = "com.apple.keylayout.Dvorak" }
```

Native identifiers are passed unchanged to the platform provider. `sledge
validate` does not verify that a native identifier is installed or enabled.
Linux source providers are subject to the [compatibility qualification
contract](linux-compatibility.md); an unavailable provider is not silently
substituted with another source.

Logical source aliases allow the same binding to select platform-specific IDs:

```toml
[input_source_aliases.us]
macos = "com.apple.keylayout.US"
linux = "xkb:us::eng" # example IBus engine ID; use the selected provider's inventory

[[binding]]
trigger = { tap = "RightAlt", count = 3, within_ms = 600 }
action = { type = "set_input_source", source = "us" }
```

Exactly one of `id` or `source` is required. Logical aliases must exist and
provide an identifier for the selected platform. Compilation resolves the ID
before the matching generation is swapped. IDs are not guessed from labels or
translated from Apple identifiers. Provider-specific indexed inventories must
be checked for changes before selection.

## Scope predicate

```toml
when = { app_in = ["ghostty", "kitty"] }
```

The binding only fires when the focused application's identifier (resolved
via `app_aliases`) matches one of the listed logical names. Omit `when` to
fire globally.

## Key names

`sledge` uses USB-HID-style key names. The core set:

- Letters: `KeyA` through `KeyZ`
- Digits: `Digit0` through `Digit9`
- Function keys: `F1` through `F24`
- Arrows: `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`
- Modifiers: `LeftShift`, `RightShift`, `LeftCtrl`, `RightCtrl`,
  `LeftAlt`, `RightAlt`, `LeftCmd`, `RightCmd`, `Fn`, `CapsLock`
- Punctuation: `Return`, `Tab`, `Space`, `Backspace`, `Delete`, `Escape`,
  `Semicolon`, `Quote`, `Comma`, `Period`, `Slash`, `Backslash`,
  `Backquote`, `Minus`, `Equal`, `LeftBracket`, `RightBracket`
- Navigation: `Home`, `End`, `PageUp`, `PageDown`, `Insert`

The full list is in `packages/core/src/event.rs`.
