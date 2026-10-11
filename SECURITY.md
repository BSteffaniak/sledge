# Security

`sledge` installs a system-wide keyboard event tap on macOS. This means every
keystroke on the machine passes through the daemon process while it is
running. Treat the binary as trust-sensitive:

- Only grant Accessibility and Input Monitoring to a build of `sledge` you
  built from source yourself, or whose provenance you trust.
- The daemon never logs key contents at `info` or lower. `debug` and `trace`
  levels may log key codes (not text) for troubleshooting — do not leave
  trace-level logging enabled in normal use.
- Physical-key capture, matching, and synthetic output must never perform network
  transmission. Linux desktop integration uses local Unix sockets and D-Bus; no
  off-host telemetry is permitted.
- The Linux port separates the unprivileged user daemon from narrowly privileged
  input-device handling. Kernel peer credentials alone are not proof of an active,
  authorized graphical session or seat. Unknown session/lock authority must not
  silently permit raw keyboard access.
- Do not grant world-readable `/dev/input`, world-writable `/dev/uinput`, or broad
  input-group membership as a default installation workaround. Do not run the
  config-reading user daemon as root.
- Builds do not install or activate services. Privileged/device integration tests
  must use isolated test devices/VMs, not grab a developer's live keyboard.
- Linux full compatibility remains subject to the qualification gates in
  [the compatibility contract](docs/linux-compatibility.md). Compilation and fake
  transport tests are not evidence of lock-screen isolation or crash recovery.

## Reporting a vulnerability

Email [bradensteffaniak@gmail.com](mailto:bradensteffaniak@gmail.com) rather than opening a public issue. Please
include a minimal reproducer and, where applicable, the `sledge --version`
output.
