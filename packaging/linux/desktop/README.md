# Sledge same-user desktop companions

These files are product sources, not automatically installed or enabled. Nothing
here installs services, opens input devices, or grants broker authorization.

## GNOME

`gnome/sledge-desktop@sledge/` is an ESM Shell extension for GNOME 45–48.
The source-manager and screen-shield APIs were checked against upstream 45.0 and
48.0; installation/runtime behavior still needs isolated desktop qualification.
It exports session-bus `org.sledge.Desktop.Gnome`, `/org/sledge/Desktop`,
`org.sledge.Desktop1`:

- `Snapshot() -> s`: bounded JSON protocol version 1.
- `Select(epoch: s, generation: s, id: s) -> b`: inventory-checked activation.

Native source IDs are JSON tuples, e.g. `["xkb","us"]`,
`["xkb","us+intl"]`, `["ibus","mozc-jp"]`. Labels are not identifiers.
The companion consults enabled Shell sources and honors Shell's own activation
path; it does not mutate GSettings or bypass per-window/source-manager policy.
Source activation is asynchronous; the client verifies a subsequent snapshot and
returns a selection failure rather than assuming an activation request succeeded.

Focus is Meta's actual class/app ID, categorized with Meta client type. Missing
identity or overview/shield activity invalidates scope. No title, PID, process,
or Shell.WindowTracker association is used as a fallback.

## KWin

`kwin/` is a KWin 6 declarative script using the registered `DBusCall` QML type,
verified against Plasma/6.3 upstream source. It sends protocol-1 snapshots every
200 ms and on window activation/class changes to session-bus
`org.sledge.DesktopBridge`, `/org/sledge/DesktopBridge`,
`org.sledge.DesktopBridge1.Publish(s) -> b`.

The daemon must start `desktop::companion::KwinBridge` before the script publishes.
The bridge accepts only the pinned unique owner of `org.kde.KWin`, rejects
regressing/duplicate sequence numbers, and expires observations after 500 ms.
KWin restart requires reconnecting the bridge. KWin exposes `resourceClass`
without a scripting-visible protocol discriminator, so focus has category
`kwin_resource_class` and can resolve only explicitly selected legacy Linux
aliases. It must not be guessed to be either Wayland or X11.

Sources use KWin's native `org.kde.keyboard`, `/Layouts`,
`org.kde.KeyboardLayouts`: `getLayoutsList`, `getLayout`, `setLayout`.
IDs are decimal zero-based indexes bound to the exact inventory/unique bus owner.
Changed order or inventory rejects queued selection. Display names are not
persistent layout identities.

## Daemon integration

Linux root declares `pub mod desktop`. Explicitly choose a provider; environment
variables/desktop names are discovery hints, never authority. `Sway::new(path)`
and `Hyprland::new(path)` use bounded same-user UNIX IPC. `X11::connect` takes an
explicit local socket, screen and user authorization cookie; it must not be used
as full-desktop focus on an XWayland connection. `Gnome::connect()` and
`KwinBridge::connect()` pin bus owners and require companions/capabilities.

`DesktopWorker::start(Box<dyn DesktopProvider>)` owns the polling worker.
`focus().current()` returns typed fresh identity or `None`; polling errors clear
scope immediately. Keep the worker guard alive until input/injection workers
stop. `dispatcher()` is cloneable and serializes source calls outside the event
loop. Use `catalog()` at queue time and `select(&catalog, native_id)` for exact
inventory-generation protection. `select_current` is only for actions explicitly
resolved at dispatch time; it cannot preserve a queued source catalog.

`WithInputMethod` optionally combines a compositor's focus provider with explicitly
chosen `IBus` or `Fcitx5` source provider. Never automatically choose whichever
framework happens to own a name. IBus requires a session-bus daemon, the installed
`Engines` and enabled `ActiveEngines` properties; private-bus-only installations
are unavailable. Fcitx5 uses the active group's enabled IDs, not the union of all
groups. Both confirm current selection and never modify persistent configuration.

Sway/Hyprland source IDs are JSON `[device,index]` tuples bound to socket inode,
inventory and versioned fields. Sway needs `xkb_layout_names` and
`xkb_active_layout_index`; Hyprland confirmation needs `active_layout_index`.
Missing/custom-keymap inventories or old protocol versions fail explicitly.
Source catalog `provider` distinguishes namespaces; IDs are not translated
between providers or from Apple's native IDs.

## Security and qualification boundaries

Every snapshot's `strict_lock_authority()` is **false**. Shell shield state,
KScreenLocker state, Hyprland polling and logind LockedHint do not supply atomic
broker capture permission. Sway has no standard reliable lock-state IPC. KWin
publishes unknown lock state. Missing reliable interlock blocks strict broker
operation; these sources must never be used to turn an unlock hint into authority.

Socket credentials and bus unique owners prevent accidental cross-user routing,
not malicious same-user impersonation. Session-bus D-Bus messages are limited by
zbus's native 128 MiB transport ceiling before Sledge's 1 MiB decoded-reply check;
Sledge does not claim a 1 MiB preallocation bound for D-Bus. UNIX protocol frames
are checked before allocation and have absolute request deadlines. X11 uses
bounded stream polling and bounded property reads. Native protocol mutation and
focus changes may race inventory validation; adapters confirm selection and
reject observed changes, but cannot supply atomic compositor transactions where
upstream does not expose them.

Pure/fake transport tests and JavaScript syntax checking are not desktop
acceptance. Pending qualification includes companion load/unload, Shell/KWin API
behavior, native/XWayland focus, source activation timing, custom/per-window/
per-device layouts, framework restart, desktop lock transitions and same-user
adversarial tests. No live session control is part of unit testing.
