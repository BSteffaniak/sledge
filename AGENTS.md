# Agent / contributor notes

This repository is a Cargo workspace. Crates live under `packages/`.

## Layout

```
packages/
  core/              sledge_core — engine, types, rule matching
  config/            sledge_config — TOML parser + validation
  config/models/     sledge_config_models — serde types
  macos/             sledge_macos — CGEventTap backend (macOS only)
  linux/             sledge_linux — target-gated stub
  windows/           sledge_windows — target-gated stub
  cli/               sledge binary
```

## Verification (required after every change)

Run in this order. All four must pass.

1. `cargo fmt --all`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --all`
4. `cargo machete --with-metadata`

`cargo fmt` first — formatting can reflow lines and push functions over
clippy's `too_many_lines` threshold. Fix the root cause (extract helpers), do
not compress formatting to satisfy clippy.

## Conventions

- Edition 2024.
- Lints: `clippy::all`, `pedantic`, `nursery`, `cargo` as warn at the workspace
  level. Do not add crate-level `#![allow(clippy::...)]` to paper over
  warnings — fix the cause. Item-level `#[allow(...)]` with a one-line
  justifying comment is acceptable for genuine false positives.
- Error handling: `anyhow` in the binary crate (`packages/cli`), `thiserror`
  in library crates (`packages/core`, `packages/config`, platform crates).
- Logging: `tracing` crate for structured logs. Subscriber setup lives in
  `packages/cli/src/logging.rs`.
- No wildcard imports in production code. `use super::*;` inside
  `#[cfg(test)] mod tests { ... }` is fine.
- Platform-specific code lives only in platform crates. `sledge_core` and
  `sledge_config` must compile on all three targets with the same feature
  set.

## Hot-path discipline (macOS backend)

The `CGEventTap` callback runs on the `CFRunLoop` main thread and must return
within macOS's event-tap timeout (historically ~70ms). Anything that can
block — file I/O, DNS, synchronous IPC, long allocations — must be
pushed to a worker thread via `crossbeam_channel`. The tap callback's
responsibility is: map the `CGEvent` to a `KeyEvent`, ask the core for a
verdict, apply it, return. Nothing else.

## Self-event filtering

All synthetic events injected by the daemon are posted through a private
`CGEventSource` whose `sourceUserData` is set to a sentinel value. The tap
callback reads that field on every event and passes through (does not
rule-match) any event carrying the sentinel. Do not change the sentinel value
without updating both the source constructor and the tap filter in the same
commit.

## Permissions

On startup the daemon checks Accessibility and Input Monitoring. If either is
missing, it requests the corresponding grant once, logs the waiting state, and
polls until both are granted. Do not install the event tap before both checks
succeed, repeatedly prompt, or claim that watchdog recovery bypasses TCC.

## Config reload

`SIGHUP` triggers an atomic config reload. The new config is parsed and
validated first; only if validation succeeds are the rules and app aliases swapped
together under one matching-state lock. If validation fails the daemon keeps the old rules live and logs the
error.

In addition to SIGHUP, the daemon runs a background file watcher on the
config file's parent directory (see `packages/cli/src/config_watcher.rs`).
Events are debounced (~250 ms) to coalesce multi-write saves by editors
that rename-over-target. The watcher invokes the same `reload_fn` as
SIGHUP and the IPC `reload` request, so all three triggers share identical
behaviour.
