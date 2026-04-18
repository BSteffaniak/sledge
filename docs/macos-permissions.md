# macOS permissions

`sledge` requires two separate TCC grants on macOS:

- **Accessibility** (`com.apple.security.accessibility`) \u2014 lets the
  daemon synthesize keystrokes and query foreground-app identity.
- **Input Monitoring** (`com.apple.security.input-monitoring`) \u2014 lets
  the daemon read keystrokes via CGEventTap.

Both are required. Without Input Monitoring the tap never receives events.
Without Accessibility, injection returns silently without effect.

## Grant persistence across rebuilds

TCC keys grants off a binary's code-signing identity and bundle path. Every
time you rebuild, the binary's hash changes; if the signing identity is also
unstable, macOS treats the new build as an unknown application and silently
revokes the previous grant. This is the root cause of the "Hammerspoon
stopped working after a nix rebuild" class of failure.

`sledge` mitigates this by:

1. **Stable bundle id.** The `.app` bundle uses `CFBundleIdentifier =
com.braden.sledge`. This identifier does not change across builds.
2. **Ad-hoc code signing with a stable identifier.** `scripts/bundle-macos.sh`
   runs `codesign --sign - --identifier com.braden.sledge --force ...` on
   every build. The ad-hoc signature ("-") plus a stable identifier is
   sufficient for TCC to treat successive builds as the same application.
3. **Stable install path.** The bundle is installed at
   `/Applications/Sledge.app`. Do not install under
   `/nix/store/...-sledge/bin/sledge` directly \u2014 TCC partly keys off
   the bundle path.

If you need to re-grant after a rebuild that did lose permissions, remove
the stale entries from _System Settings \u2192 Privacy & Security \u2192
Accessibility_ and _Input Monitoring_, then re-launch the bundle.

## Preflight

On every startup `sledge` calls:

- `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)` \u2014 Input Monitoring.
- `AXIsProcessTrusted()` \u2014 Accessibility.

If either is denied, the daemon logs an error (with the specific missing
grant) and exits with a non-zero status. `launchd`'s backoff kicks in; you'll
see the daemon restart-loop in `log stream --predicate 'subsystem contains
"sledge"'`. Grant the permissions and the daemon will come up on the next
launchd restart.

## Secure input mode

Password fields (1Password, `sudo`, Keychain prompts, etc.) cause macOS to
enter _secure input mode_ for the duration of the prompt. No user-space
event tap sees events during secure input \u2014 this is a kernel-level
boundary, not something the daemon can route around. It's the same
limitation every CGEventTap-based tool has, including Karabiner and
Hammerspoon.

## Dropping permissions

To fully uninstall:

```
launchctl unload ~/Library/LaunchAgents/com.braden.sledge.plist
rm ~/Library/LaunchAgents/com.braden.sledge.plist
rm -rf /Applications/Sledge.app
```

Then manually remove the entries under System Settings \u2192 Privacy &
Security \u2192 Accessibility / Input Monitoring.
