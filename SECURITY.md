# Security

`sledge` installs a system-wide keyboard event tap on macOS. This means every
keystroke on the machine passes through the daemon process while it is
running. Treat the binary as trust-sensitive:

- Only grant Accessibility and Input Monitoring to a build of `sledge` you
  built from source yourself, or whose provenance you trust.
- The daemon never logs key contents at `info` or lower. `debug` and `trace`
  levels may log key codes (not text) for troubleshooting \u2014 do not leave
  trace-level logging enabled in normal use.
- The daemon does not transmit any data off-host. There is no network code
  path in `sledge_core`, `sledge_macos`, or `sledge` (the binary). The only
  IPC is a per-user Unix domain socket for `sledge status` / `sledge reload`.

## Reporting a vulnerability

Email the maintainer directly rather than opening a public issue. Please
include a minimal reproducer and, where applicable, the `sledge --version`
output.
