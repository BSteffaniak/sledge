# macOS permissions and installation

sledge needs **Accessibility** to synthesize events and **Input Monitoring** to receive keyboard events. Only grant these permissions to a build you trust.

## Startup behavior

The backend checks both grants before installing `CGEventTap`. If a grant is missing, it requests the corresponding permission once and waits, polling every two seconds. Grant access under **System Settings → Privacy & Security → Accessibility / Input Monitoring**. The daemon continues once both checks succeed.

`check-permissions` reports grants without starting the event tap. `status` reports the running daemon's state. Permission revocation or Secure Input can stop event delivery; watchdog recovery is not a way to bypass either boundary.

## Signing and grant persistence

The bundle identifier is `com.braden.sledge`. `scripts/bundle-macos.sh` creates or reuses a self-signed **Sledge Local Signing** certificate in the login keychain, then signs the bundle with it. Setup may ask for keychain authorization and changes code-signing trust settings. Review `scripts/setup-signing-identity.sh` before running it.

This is deliberately **not ad-hoc signing**: an ad-hoc signature changes identity with the binary hash. Reusing the certificate and a stable path such as `/Applications/Sledge.app` helps macOS recognize successive builds. It is not a guarantee that TCC grants always survive an OS update, certificate change, or rebuild, and it does not provide Apple notarization.

`--no-sign` produces an unsigned bundle for build environments without keychain access; sign it before normal installation.

## Supervision

Create and validate configuration before the first launch. Install the bundle at its final path, grant permissions, and only then opt into the per-user LaunchAgent:

```sh
./scripts/install-launchagent.sh /Applications/Sledge.app
launchctl print "gui/$(id -u)/com.braden.sledge"
```

The installer writes `~/Library/LaunchAgents/com.braden.sledge.plist`, creates the log directory, and loads/starts the agent. `KeepAlive` supervises it. Do not simultaneously run another instance with the same socket.

## Restart-safe IPC

On macOS, each daemon holds an exclusive lock on `<socket>.lock` for its
socket lifetime. SIGTERM requests orderly backend shutdown, including while
waiting for permission grants, then stops IPC and removes the owned socket.
After a crash or forced termination, startup recovers only a same-user socket
whose connection probe returns `ECONNREFUSED`. A live listener (including an
older daemon), symlink, unrelated file, unsafe lock file, or unexpected probe
failure is rejected rather than overwritten. The probe is time-bounded.

The persistent lock file is intentional; do not delete it to force another
instance to start. Socket parents must be same-user directories not writable
by other users, or root-owned sticky directories such as `/tmp`. This also
allows the first upgraded daemon to recover an abandoned legacy
`/tmp/sledge-<uid>.sock` automatically. Linux retains its separate conservative
socket admission policy.

## Secure Input

Password-entry and other Secure Input contexts can prevent user-space keyboard taps from receiving events. Do not rely on remappings there. The daemon does not disable Secure Input.

## Stop and uninstall

Unload the service before removing its files:

```sh
launchctl bootout "gui/$(id -u)/com.braden.sledge"
```

Remove the LaunchAgent plist and `/Applications/Sledge.app` using Finder. Remove their Accessibility/Input Monitoring entries in System Settings. Configuration and logs are separate user data; preserve or remove them deliberately. The locally created signing identity remains in Keychain Access and can be removed separately if no builds use it.
