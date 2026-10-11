# Linux installation and packaging

## Status and supported packaging scope

These assets target Linux with systemd and systemd-logind. They create an offline
install image and a root-owned tar archive, not Debian/RPM/Arch dependency-managed
packages. Filesystem layout is `/usr/bin/sledge`,
`/usr/libexec/sledge-linux-broker`, `/usr/lib/systemd/{system,user}`, and
`/usr/share/doc/sledge`. Distributions using another libexec/unit layout need an
explicit downstream package revision; do not silently relocate just one binary.

**This revision is not qualified for live keyboard remapping.** The backend fails
closed without verified session/seat/unlock authority and qualified device routes.
The user daemon currently has no live privileged-broker connection. The shipped
broker unit additionally hides physical devices and grants no evdev/uinput access.
The broker listener is root-only; it is a diagnostic boundary, not a production
user admission policy. Starting these units does not make the port functional.
Do not loosen confinement, make sockets world-writable, add users to `input`,
install setuid binaries, or infer authorization from socket reachability.

See [the compatibility contract](linux-compatibility.md) for required desktop,
hardware, security, lifecycle, and performance qualification. Source builds and
unit syntax checks are not behavioral qualification.

## Three separate consent boundaries

1. **Build**: compile already-reviewed source as an unprivileged user. No services,
   udev rules, groups, permissions, or devices are changed by building.
2. **Package**: explicitly create an offline image/archive as an unprivileged user.
   No package hooks run; nothing is installed or activated. Config is excluded.
3. **Install/activate**: an administrator reviews ownership, artifacts and unit
   policy before manual installation. Installation does not imply consent to
   activation. Activation remains prohibited for remapping until qualification.

The coordinator runs repository verification and release builds after writers
stop. Example release build (do not run concurrently with shared manifest edits):

```sh
CARGO_TARGET_DIR=/tmp/sledge-release-target cargo build --locked --release \
  -p sledge --bin sledge -p sledge_linux --bin sledge-linux-broker
```

Package the resulting Linux artifacts; cross-compilation requires the appropriate
target-specific output directory. This tool does not prove artifact provenance,
architecture, dynamic-library compatibility, or binary correctness. Record build
revision, target, release checks and artifact hashes in the release record.

```sh
# Default is validation plus a printed plan, with no writes.
python3 scripts/linux/package.py --binaries /tmp/sledge-release-target/release \
  --stage /tmp/sledge-image
# Explicit offline write consent; image and archive must not already exist.
python3 scripts/linux/package.py --binaries /tmp/sledge-release-target/release \
  --stage /tmp/sledge-image --archive /tmp/sledge-linux.tar --apply
python3 scripts/linux/test_package.py
```

Choose a new child of a trusted staging parent, not a live install prefix. The
staging manifest includes source SHA-256 hashes; tar members have root ownership
and only 0755 executable / 0644 data modes, without setuid/setgid bits. Binaries
must be regular non-symlink files. The tool refuses root apply, existing image or
archive destinations, relative images, and noncanonical parent paths. Failed
staging can leave an incomplete offline image; inspect it and select a new path
for retry. Do not install an incomplete image. No script invokes systemctl, sudo,
chown, chmod on devices, usermod, udevadm, or a binary being packaged.

## Manual installation (administrator review required)

There is intentionally no live-host installer or post-install hook. A downstream
package must preserve this policy. Inspect archive paths, manifest hashes and
unit contents, then use the following explicit files from the offline image.
These commands are instructions only; packaging never executes them. Administrator
credentials are required for the system paths. Verify all destination parents
are root-owned, not writable by users, and not unexpected symlinks first.

```sh
# Run only after stopping old services as described below.
install -D -o root -g root -m 0755 /tmp/sledge-image/usr/bin/sledge /usr/bin/sledge
install -D -o root -g root -m 0755 /tmp/sledge-image/usr/libexec/sledge-linux-broker /usr/libexec/sledge-linux-broker
install -D -o root -g root -m 0644 /tmp/sledge-image/usr/lib/systemd/system/sledge-broker.service /usr/lib/systemd/system/sledge-broker.service
install -D -o root -g root -m 0644 /tmp/sledge-image/usr/lib/systemd/system/sledge-broker.socket /usr/lib/systemd/system/sledge-broker.socket
install -D -o root -g root -m 0644 /tmp/sledge-image/usr/lib/systemd/user/sledge.service /usr/lib/systemd/user/sledge.service
install -D -o root -g root -m 0644 /tmp/sledge-image/usr/share/doc/sledge/linux-installation.md /usr/share/doc/sledge/linux-installation.md
install -D -o root -g root -m 0644 /tmp/sledge-image/usr/share/doc/sledge/linux-compatibility.md /usr/share/doc/sledge/linux-compatibility.md
systemctl daemon-reload
```

Each relevant user may run `systemctl --user daemon-reload` in their own session.
Neither reload command starts/enables a unit. Do not execute the user daemon as
root. There is no shipped config or overwrite of `~/.config/sledge/config.toml`.
A user chooses and validates their config with `sledge validate PATH` offline.
The user service uses the documented default config path and private
`$XDG_RUNTIME_DIR/sledge.sock` health socket. It has no fallback broker arguments.
Custom config locations require an explicit reviewed user service override.

## Service boundaries and lifetime

- `sledge-broker.socket`: root-owned `/run/sledge-broker/broker.sock`, parent mode
  0700, socket mode 0600, one listening Unix descriptor. No socket is created by
  installation. Socket activation is separate explicit administrator consent.
- `sledge-broker.service`: root-owned executable, inherited listener FD 3,
  `--listen-fd 3`, access to the system bus for logind, no network address families,
  no capabilities, read-only system/home isolation, no physical devices, no
  automatic restart. It reads no user TOML. Runtime errors exit nonzero; an
  unverifiable peer is refused, never upgraded to an unrestricted fallback.
- User `sledge.service`: never root, requires and is bound to
  `graphical-session.target`, stops with that target, no restart loop and no
  `default.target` attachment. Only enable after the desktop demonstrably starts
  and stops that target with the local graphical session. Do not enable user
  lingering for Sledge. On desktops that keep that target alive after logout,
  this unit is not qualified; no persistent capture may be claimed or enabled.

No activation command is supplied for the unqualified broker. A future qualified
release needs an audited admission mechanism for active local users, independent
logind/session/seat/lock proof, device route selection and narrow device policy.
A blanket `DeviceAllow=char-input` rule or a generic input-group grant is not
that mechanism. Broker/service confinement must be tested with the actual
logind system bus and route lifecycle, not weakened to make a test pass.

## Safe stop, upgrade, rollback and uninstall

Stop the user service in **every** affected user's session. Administrators stop
and disable the socket first, then stop the broker; otherwise the socket can
reactivate the broker. Verify both units inactive and all daemon processes gone
before replacing/removing binaries. A qualified future broker must additionally
prove grabs and synthetic key state released on stop. This revision opens none.

```sh
# Each affected user (not root, no --global enable/disable):
systemctl --user disable --now sledge.service
# Administrator, before replacing system files:
systemctl disable --now sledge-broker.socket
systemctl stop sledge-broker.service
systemctl is-active sledge-broker.socket sledge-broker.service
```

`is-active` should report inactive (and returns nonzero); inspect unexpected
failure or surviving processes before continuing. Stop any manually launched
instances too. Upgrade only while stopped, validate the replacement build and
manifest, replace the seven listed files, and daemon-reload both managers. Review
local `/etc/systemd` overrides and per-user overrides rather than overwriting
or deleting them. Preserve config/logs; never automatically restart after
upgrade. Rollback follows the same stopped, reviewed procedure using a verified
previous image. User configuration rollback is a separate user decision.

For uninstall, after the stop procedure remove **only** the seven installed files
listed above that belong to this installation, then daemon-reload both managers.
Do not recursively remove `/usr`, `/run`, users' homes, or unit directories. Socket
`RemoveOnStop=yes` removes its listener; do not unlink a running instance's
socket. Preserve user config/logs and locally authored service overrides by
default. Remove any lingering enablement symlinks via the owning manager, not
by deleting unrelated units. Packages must not start a daemon during uninstall.

## Offline validation and remaining deploy qualification

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/linux/test_package.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/linux/check_units.py
# On a systemd-equipped machine; does not start units:
systemd-analyze verify packaging/linux/systemd/sledge-broker.socket \
  packaging/linux/systemd/sledge-broker.service packaging/linux/systemd/sledge.service
```

Uninstalled ExecStart paths may be reported missing by direct host verification.
`check_units.py` uses a disposable offline root with inert executable and target
fixtures and a deliberate unit search path (including on NixOS hosts); it checks
syntax/dependencies only, never runs services, and does not prove real binary or
host session integration. Validation must not install or run those executables. Remaining release gates include real built-binary
packaging, architecture/library dependencies, logind/bus policy under confinement,
active-session/seat/lock authorization, logout/suspend/crash recovery, restricted
socket admission, device routing and stop-state cleanup, and all desktop/hardware
qualification in the compatibility contract. No distro or desktop is declared
qualified by these packaging assets.
