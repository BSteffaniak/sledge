# sledge

**A native Rust keyboard-automation daemon for macOS.**

sledge provides declarative hotkeys, application-scoped remapping, modifier-tap sequences, and input-source switching without an embedded scripting runtime. A platform-neutral matching engine is connected directly to macOS's keyboard event tap.

> **Alpha software.** The daemon can observe keyboard events and synthesize keystrokes. Review your configuration and only grant permissions to a build you trust. macOS is the supported keyboard daemon. Linux has an experimental broker, desktop adapters, and diagnostic CLI, but capture is deliberately denied pending a qualified session/lock authority; native Linux `send_key` is not implemented. Windows remains a stub. See the [Linux compatibility contract](docs/linux-compatibility.md) and [installation notes](docs/linux-installation.md).

## Design

- Normalized key events and a modifier-tap state machine in [`sledge_core`](packages/core).
- TOML parsing and rule validation in [`sledge_config`](packages/config).
- Native `CGEventTap` integration, focus tracking, and watchdog recovery in [`sledge_macos`](packages/macos).
- Synthetic-event tagging to prevent the daemon from matching its own output.
- Atomic rule/alias reload through IPC, SIGHUP, or a debounced file watcher.
- Optional per-user `launchd` supervision; no background service is installed by a Cargo build.

## Build and configure

Requirements: macOS, Git, stable Rust, and the Xcode Command Line Tools. Packaging uses the system code-signing and keychain tools.

```sh
git clone https://github.com/BSteffaniak/sledge.git
cd sledge
cargo build --locked --release
mkdir -p ~/.config/sledge
# For a new installation only; do not overwrite an existing configuration.
cp -n config.example.toml ~/.config/sledge/config.toml
./target/release/sledge validate ~/.config/sledge/config.toml
```

Review the [annotated configuration](config.example.toml) before starting. For example:

```toml
[app_aliases.ghostty]
macos = "com.mitchellh.ghostty"

[[binding]]
when = { app_in = ["ghostty"] }
trigger = { key = "Semicolon", mods = ["ctrl"] }
action = { type = "send_key", key = "KeyS", mods = ["ctrl"] }

[[binding]]
trigger = { tap = "RightAlt", count = 3, within_ms = 600 }
action = { type = "send_key", key = "Return", mods = ["ctrl"] }
```

## Run

For a foreground evaluation:

```sh
./target/release/sledge run --stdout-logs
```

For a stable application path and signing identity:

```sh
./scripts/bundle-macos.sh target/release/sledge ./dist
# Move dist/Sledge.app to /Applications/Sledge.app using Finder.
open /Applications/Sledge.app
```

The bundling script creates/reuses a self-signed **Sledge Local Signing** identity in your login keychain and may request authorization for trust settings. It is not Apple notarization. A stable certificate and installation path help preserve TCC grants across rebuilds, but reauthorization can still be necessary. See [macOS permissions](docs/macos-permissions.md).

Grant **Accessibility** and **Input Monitoring** in System Settings. When grants are missing at startup, the daemon requests them once and waits; it does not install the tap until both are granted.

To opt into login startup, stop the foreground instance first, then run:

```sh
./scripts/install-launchagent.sh /Applications/Sledge.app
```

This writes and loads a per-user LaunchAgent with `KeepAlive`. Do not run a foreground daemon and a supervised instance simultaneously.

## Commands

Use `./target/release/sledge` below, or put the installed binary on `PATH`:

```text
sledge run                       Run in the foreground (also the default)
sledge status                    Query the daemon over its Unix socket
sledge reload                    Request a reload over that socket
sledge validate <path>           Validate without starting a keyboard tap
sledge check-permissions         Report current permission grants
```

The default configuration is `~/.config/sledge/config.toml`; override it with `--config` or `SLEDGE_CONFIG`. Successful reload replaces rules and app aliases together and updates the log level. Invalid configuration leaves the previous configuration active. SIGHUP and file watching use the same reload path.

## Limitations and security

- Secure Input and OS permission boundaries can prevent events from reaching the tap. Watchdog recovery cannot bypass those protections.
- App scoping uses foreground application identity, not window titles or document contents.
- Debug/trace logging can expose key codes; keep normal logging at `info` or lower.
- A syntactically valid action is not a guarantee that macOS will permit it or that an input-source identifier exists.

Read the [security policy](SECURITY.md), [configuration schema](docs/config-schema.md), and [architecture](docs/architecture.md).

## Development

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo machete --with-metadata
```

Automated tests cover rule handling, configuration, and backend helpers. Permission prompts, event-tap recovery, signing, and login startup also need manual macOS testing. Report non-sensitive bugs through [GitHub Issues](https://github.com/BSteffaniak/sledge/issues).

## License

[Mozilla Public License 2.0](LICENSE).
