"""Prepare a small payload root disk; never build or launch a guest closure."""
import argparse
import hashlib
from pathlib import Path
import re
import shutil
import subprocess

from candidate_payload import candidate_scope


def verify_payload(root):
    root = Path(root).resolve(strict=True)
    manifest = root / "manifest.sha256"
    records = {}
    for line in manifest.read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_./-]+)", line)
        if not match:
            raise ValueError("invalid payload manifest record")
        digest, name = match.groups()
        relative = Path(name)
        if relative.is_absolute() or ".." in relative.parts or name in records:
            raise ValueError("unsafe or duplicate payload path")
        records[name] = digest
    actual = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ValueError("payload symlinks are forbidden")
        if path.is_file() and path != manifest:
            name = path.relative_to(root).as_posix()
            actual.add(name)
            if records.get(name) != hashlib.sha256(path.read_bytes()).hexdigest():
                raise ValueError(f"payload checksum mismatch: {name}")
        elif not path.is_file() and not path.is_dir():
            raise ValueError("payload special files are forbidden")
    if actual != set(records) or not {"run.sh", "boot.sh", "sledge", "plugin.so"} <= actual:
        raise ValueError("payload manifest incomplete")
    candidate_scope(root)
    return root


def prepare(payload, scratch, qemu_img, mkfs):
    payload = verify_payload(payload)
    scratch = Path(scratch)
    if not scratch.is_absolute() or not str(scratch).startswith("/hdd/") or scratch.exists():
        raise ValueError("fresh absolute /hdd scratch directory required")
    for tool in (qemu_img, mkfs):
        if not str(Path(tool).resolve(strict=True)).startswith("/nix/store/"):
            raise ValueError("immutable Nix tools required")
    scratch.mkdir(parents=True)
    staged = scratch / "root/sledge-payload"
    shutil.copytree(payload, staged)
    verify_payload(staged)
    if candidate_scope(staged):
        etc = staged.parent / "etc"
        etc.mkdir()
        for source, destination in (("machine-id", "machine-id"), ("boot-marker", "sledge-qualification-candidate")):
            target = etc / destination
            shutil.copyfile(staged / "scope" / source, target)
            target.chmod(0o444)
            (staged / "scope" / source).chmod(0o444)
    init = staged.parent / "payload-init.sh"
    init.write_text("#!/nix/store/lfbzxs5wyqd2122mpbj5azkxhxspw9cd-bash-interactive-5.3p3/bin/bash\n"
                    "set -eu\n"
                    "export PATH=/nix/store/hqkszxk2c0cxvd04xa4gsaqs182dw8l2-coreutils-9.8/bin:/nix/store/vh24fxc958hvngx7hzm96q8w1h1hiwgl-util-linux-2.41.4-bin/bin\n"
                    "mount -n -o remount,rw none /\n"
                    "mkdir -p /run/systemd/system\n"
                    "if test -d /sledge-payload/scope; then\n"
                    " mkdir -p /etc\n"
                    " cp /sledge-payload/scope/machine-id /etc/machine-id\n"
                    " cp /sledge-payload/scope/boot-marker /etc/sledge-qualification-candidate\n"
                    " chown 0:0 /etc/machine-id /etc/sledge-qualification-candidate\n"
                    " chmod 444 /etc/machine-id /etc/sledge-qualification-candidate\n"
                    " chown 0:0 /sledge-payload/scope/machine-id /sledge-payload/scope/boot-marker\n"
                    " chmod 444 /sledge-payload/scope/machine-id /sledge-payload/scope/boot-marker\n"
                    " mount --bind /sledge-payload/scope/machine-id /etc/machine-id\n"
                    " mount -o remount,bind,ro /etc/machine-id\n"
                    " mount --bind /sledge-payload/scope/boot-marker /etc/sledge-qualification-candidate\n"
                    " mount -o remount,bind,ro /etc/sledge-qualification-candidate\n"
                    " echo SLEDGE_SCOPE_PRE_INIT > /sledge-payload/pre-init-scope.log; cat /etc/machine-id >> /sledge-payload/pre-init-scope.log; cat /proc/cmdline >> /sledge-payload/pre-init-scope.log\nfi\n"
                    "cat > /run/systemd/system/sledge-payload-boot.service <<'UNIT'\n"
                    "[Unit]\nAfter=systemd-user-sessions.service\nRequires=systemd-user-sessions.service\n"
                    "[Service]\nType=oneshot\nExecStart=/bin/sh /sledge-payload/boot.sh\n"
                    "ExecStopPost=/run/current-system/sw/bin/systemctl poweroff\nUNIT\n"
                    'for word in $(cat /proc/cmdline); do case "$word" in sledge.base_init=*) base=${word#*=};; esac; done\n'
                    'exec "$base" --unit=sledge-payload-boot.service\n')
    init.chmod(0o755)
    raw = scratch / "payload-root.raw"
    subprocess.run([qemu_img, "create", "-f", "raw", str(raw), "512M"], check=True)
    subprocess.run([mkfs, "-F", "-L", "nixos", "-d", str(staged.parent), str(raw)], check=True)
    disk = scratch / "qualification.qcow2"
    subprocess.run([qemu_img, "convert", "-f", "raw", "-O", "qcow2", str(raw), str(disk)], check=True)
    print(f"PAYLOAD_DISK={disk}")
    print(f"PAYLOAD_MANIFEST_SHA256={hashlib.sha256((staged / 'manifest.sha256').read_bytes()).hexdigest()}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("payload")
    parser.add_argument("scratch", nargs="?")
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--qemu-img")
    parser.add_argument("--mkfs")
    args = parser.parse_args()
    if args.verify:
        verify_payload(args.payload)
    else:
        if not args.scratch or not args.qemu_img or not args.mkfs:
            parser.error("preparation requires scratch, --qemu-img and --mkfs")
        prepare(args.payload, args.scratch, args.qemu_img, args.mkfs)
