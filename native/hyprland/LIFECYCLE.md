# Observational XKB lifecycle checkpoint — NOT delivered-state routing

**Frozen research fixture only:** stock Hyprland is the supported architecture;
these patches/binaries are not supported integration and must never deploy/load.
Source expansion stopped. Commands below record completed validation, not a
production build recommendation. No native-lane background build is retained.

Review rejected the previous focus-enter filter. `routing.patch` now leaves stock
getKeysFromAllKBs, shareKeyFromAllKBs and onKeyboardMod byte-for-byte unchanged.
Shadow state records stock admission only and NEVER controls focus-enter, client
keys/modifiers or suppression. Physical up observation precedes disabled/disallowed
early return. Shadow clears on config disable, immediate/asynchronous permission
revoke and destruction. Those clears are observation resets, NOT native output
reconciliation or lock quarantine. No shadow lock/quarantine is wired.

Own XKB keymap replay is retained, but its depressed mask is unused by stock
delivery. Refactor remains incomplete: no capture/transport/outcome bridge.
Do not run patched runtime until independent recheck. Lock patch unchanged.

Validation:
- `observational_source_test.py` applies patch to exact copied source and verifies
  stock focus/modifier/merge bodies unchanged, clear hooks, and stock multidevice
  A-down/B-consumed-down/A-up/B-up model. Shadow may become empty while stock
  seat remains held: focus-enter continues using stock seat, never shadow.
  This is source/model evidence, NOT guest application-visible qualification.
- Real XKB lifecycle regression PASS: strict GCC14.3 optimized and ASan/UBSan.
- Corrected full compositor build PASS exit0, 391 targets compile/link/install/fixup.

```sh
nix shell --impure --no-write-lock-file --max-jobs 1 --cores 2 \
 --expr 'import ./native/hyprland/lifecycle-smoke.nix' --command sh -c true
```

Corrected artifact:
- drv `/nix/store/g7kmmzdhngh348bqw6i94wjk7qcqf27n-hyprland-delivered-lifecycle-smoke-0.52.2.drv`
- runtime `/nix/store/hlzh5d09pyv5i81ym1kbzs8bgzqnj5qa-hyprland-delivered-lifecycle-smoke-0.52.2`
- ELF `bin/.Hyprland-wrapped` SHA256 `17d048667f4ab7851e1752ced5506ea9afe550ac869257b07cf1b28fcdf57d50`

Previous rejected l57mr... runtime ELF SHA256 is
`db59c688086c174d141c6f8d4b53dc441188dec7f676b85edd3f95d4ce7fea34`.
80912... was mistakenly described as ELF: it hashes wrapper script bin/Hyprland.
Neither runtime executed by native lane; no host device/socket access.

Dependencies obtained only through pinned nix shell:

```sh
nix shell --no-write-lock-file \
 github:NixOS/nixpkgs/b6018f87da91d19d0ab4cf979885689b469cdd41#xkeyboard_config \
 path:.#hyprland-sdk --command sledge-hyprland-sdk sh -eu -c '
 python3 native/hyprland/generate_lifecycle_patch.py
 python3 native/hyprland/observational_source_test.py
 d=$(mktemp -d /tmp/sledge-observation-test.XXXXXX)
 "$CXX" -std=c++23 -O2 -Wall -Wextra -Werror $(pkg-config --cflags xkbcommon) \
 native/hyprland/delivered_state_test.cpp $(pkg-config --libs xkbcommon) -o "$d/test"
 env -i "$d/test"
'
```

Sanitized variant: -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer,
env -i ASAN_OPTIONS=detect_leaks=1. Covers duplicate down, per-device holds,
keymap replay, null-keymap failure preserving snapshot, clear, unknown code.

Next (NOT implemented): actual consumer outcome commit bridge, seat/IME delivered
union, forwarded-only active-consumer XKB/group/locked/latched policy, focus-enter
modifiers, early lock output reconciliation + quarantine, bounded authenticated
transport/core tokens, isolated VM behavioral tests. Capture remains denied.
