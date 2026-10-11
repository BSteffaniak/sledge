# Isolated runtime checkpoint (not parity qualification)

Pin: nixpkgs `b6018f87da91d19d0ab4cf979885689b469cdd41`.
All tools and artifacts are obtained through `nix shell`, never profiles,
manual installs, `nix develop`, or `nix build`.

## Nested failure observed

Private Weston 14.0.1 headless/pixman starts, and Hyprland connects to it.
Parent registry lacks `wl_seat` and `zwp_linux_dmabuf_v1`; compositor/xdg are v5.
Aquamarine requires those protocols and binds compositor/xdg v6, so reports
`Missing protocols` / `no allocator available`. No plugin load reached.

## Selected host renderer and opt-in KVM

User explicitly authorized render-node-only QEMU graphics, then optional KVM.
No host input, DRM card nodes, display sockets, home or runtime sharing.
The script requires an explicit `/dev/dri/renderDNUMBER` argument, rejects
symlinks/non-character devices, and binds ONLY that node under private /dev/dri.
Read-only /sys supplies device metadata; it does not supply device nodes.

- renderD128: 226:128, mode0666 root/group303, NVIDIA10de:2860; pinned Mesa
  renderer failed initialization (not selected in the successful run).
- **renderD129**: 226:129, mode0666 root/group303, AMD1002:164e/amdgpu;
  renderer gate passed with pinned Mesa GBM/DRI/GLVND paths.
- `/dev/kvm`: mode0666 root/group302; bound ONLY with explicit `--kvm`.

Budget announced before realization: 2CPUs/4GiB guest RAM/8GiB disposable disk,
180s boot bound; estimated5–15GiB extra closure/image. At inspection23GiB RAM
and127GiB disk available. Namespace /tmp is RAM-backed; image copying and disk
writes add host RAM demand. Remaining cached closure download was about36MiB
and427MiB copied on the first observed guest realization, not total image size.

```sh
nix shell --no-write-lock-file path:.#hyprland-vm-prepare --command \
  sledge-hyprland-vm-prepare . /dev/dri/renderD129 --kvm
```

The small executable prints budget and runs a five-second nonbooting renderer
gate BEFORE realizing the guest image through another `nix shell`. The guest
carries exact host runtime n13h1pmiv6n05w0lkd7fbaywg1fc9b4p and SDK
7lvv1y5wdzzlrjy14cgqk6rvyf9hp07i, plus the same sledge-hyprland-sdk wrapper.
No network, forwarded ports, host shares or device passthrough. QEMU's host
render node renders a virtual virgl GPU; guest card0/renderD128 and keyboards
are virtual hardware, not host card/input bindings.

## Actual guest observations

TCG booted the kernel but had timer/RCU stalls; not prolonged. KVM booted NixOS,
then guest prerequisite activated virtual VT7, started stock Hyprland, and
returned service exit0. Logs show virtual virtio_gpu/card0, qemu virtual keyboard,
virgl AMD Radeon610M renderer, Virtual-1 modeset640x480@60, and Hyprctl
connections. Some output commit errors occurred during startup. Guest cleanly
powered off at about6s kernel uptime; host runner exit0, no child remains.

**Not qualification:** expected IPC/version/monitor result markers were sent to
guest tty7 and were not captured in serial evidence. Source now saves these
results to a file and prints them through the serial report, but this final
source correction has NOT been rerun pending integration review. No independent
input consumer/lock client, plugin refusal/load/unload, or application-visible
parity tests ran. Guest boot and service exit0 alone are not an acceptance verdict.
The namespace disk/state is discarded on poweroff or bounded timeout.

## Harness review correction

Graphics gate now requires bounded QMP greeting, capabilities, query-status
`prelaunch`, and query-pci identifying `selected-gpu` on the exact pinned QEMU.
Timeout124 alone is failure, never readiness. Delayed greeting, partial-line
stall, early failure and successful QMP replies are deterministic tests; hostile
PATH substitution is tested against explicit artifact selection. Guest runner
explicitly uses the same qemu package and verifies its generated executable
path before boot. QEMU/runner hashes are printed.

Guest validator parses version+monitor JSON, requires version0.52.2 and actual
/proc executable equal to the expected stock ELF, and exactly one enabled
Virtual-1 output640x480,scale1,DPMS true. Empty/wrong geometry/disabled/wrong
runtime fixtures reject. One bounded actual KVM run after these corrections
cleanly powered off exit0, but retained tool output truncated the result section;
**PASS marker/JSON was not verified from retained evidence**, so no marker
qualification claimed. No second actual retry performed. Serial log filtering
was reduced afterward for the next integration-review run.

SDK wrapper deliberately clears loader/compiler search and flag overrides,
sets its own compiler/includes/pkg-config paths. Prepare clears loader/compiler
variables too. This does not make invocation or compilation hermetic.

Provenance correction: stock `bin/Hyprland` is a shell wrapper, SHA256
`f1ca58664104e1facf2e3bdb76b4b9499174e2688357439b23fc526ac479576c`.
Actual stock ELF `bin/.Hyprland-wrapped` SHA256
`8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca`.
Patched ELF interpreter verified:
`/nix/store/7nbi22pcc92y2fqbkyp7h3srvvklmckb-glibc-2.40-224/lib/ld-linux-x86-64.so.2`.
Review history locator8cfdeecd-73f2-46dc-b5eb-a60bb45c1b02 seq2959 was attempted
through supported Bcode API but unavailable (no verified live owner); user
four-item handoff governed these corrections.

## Immutable runner admission follow-up

Prepare embeds the exact generated VM executable store path from the shared
Nix `vm` value (string context discarded only to defer large realization until
after the graphics gate). Execution does not resolve a VM runner through PATH
or inspect QEMU substrings/comments. Validator requires the exact canonical
Nix-store executable path and prints its SHA256. A fake PATH runner containing
the expected QEMU in a comment is rejected deterministically, as is a mutable
expected path. No VM rerun or broad realization for this narrow source fix.

Previous genuine guest prerequisite log retained for independent read:
`/tmp/sledge-kvm-prerequisite-8aa348ce-review.log` (10034 bytes), SHA256
`b0fb8d595a4fe831770a01128270385ae956c1902859527cb574045cad446e07`.
Lines98–164 contain IPC_READY, exactversion0.52.2, enabled Virtual-1 640x480
scale1 DPMStrue, and the validator PASS_NOT_PARITY marker. QMP readiness lines8/31.
This proves the previous guest prerequisites, not execution of the newest
runner-admission source or plugin/input/lock qualification.

## Full patched compositor compile smoke

Estimate announced:2–6GiB workspace,2–4GiB RAM at2jobs, several minutes.

```sh
nix shell --no-write-lock-file --max-jobs 1 --cores 2 \
  path:.#hyprland-lock-smoke --command sh -c true
```

**PASS:** all391 Meson/Ninja targets compiled/linked, installation/fixup completed,
command exit0. First200s observation cancelled at263/391; no live compiler remained.
Final bounded600s observation completed the full build. No unobserved work remains.
Stock source: `/nix/store/8njasxpjmb1n25n5kanrh9ga04a19dsw-source`.
Applied only `native/hyprland/lock-boundary.patch`, hash
`acb4491398f30f47dae94bf1cd90b15aa5c15ad58319fa88ec096a38529a9e32`.

Distinct patch-smoke artifact (NOT stock ABI):
- drv: `/nix/store/77g7wypfi7vmpip3bsvr60ykxgzcyzm6-hyprland-lock-smoke-0.52.2.drv`
- out: `/nix/store/snj2zivkgxm4sr4wv258x7lyj5f356v5-hyprland-lock-smoke-0.52.2`
- SDK: `/nix/store/xpj5wh7qiyhh1h9i0jcjxasaxlhlnlz7-hyprland-lock-smoke-0.52.2-dev`
- actual ELF SHA256: `4088b8b2df176f1d9e8f362a8d3fadffd32bb3fce71684d837e9747ba5482f54`

This proves full-source compile of the lock patch, not runtime lock ordering,
forwarded-XKB integration or ABI qualification. Patched binary was never loaded
or run. Stock package also exposed as `hyprland-source-build`.
