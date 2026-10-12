"""Bounded QMP targeting prerequisite, not compositor/consumer qualification.

Invoke only inside a private networkless namespace with immutable pinned QEMU.
No disk, guest network, host input, desktop sockets or shared filesystem device.
"""
import hashlib
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile

from qmp_qualification import QMP, connect


def main():
    executable = Path(os.environ["SLEDGE_VM_QEMU"])
    if not str(executable).startswith("/nix/store/") or not executable.is_file():
        raise ValueError("explicit pinned QEMU required")
    print("QEMU_TARGET_IDENTITY", executable, hashlib.sha256(executable.read_bytes()).hexdigest(), flush=True)
    with tempfile.TemporaryDirectory() as directory:
        path = str(Path(directory) / "qmp.sock")
        child = subprocess.Popen([str(executable), "-nodefaults", "-no-user-config",
            "-machine", "q35,accel=kvm", "-m", "256", "-smp", "1", "-display", "none",
            "-nic", "none", "-monitor", "none", "-serial", "none", "-S",
            "-device", "secondary-vga,id=display-a",
            "-device", "secondary-vga,id=display-b",
            "-device", "qemu-xhci,id=qualification-usb",
            "-device", "usb-kbd,bus=qualification-usb.0,id=keyboard-a,display=display-a",
            "-device", "usb-kbd,bus=qualification-usb.0,id=keyboard-b,display=display-b",
            "-qmp", f"unix:{path},server=on,wait=off"])
        try:
            with connect(path, child) as connection:
                qmp = QMP(connection)
                first, second = qmp.targeted_keyboards()
                print("QMP_TARGETS", first, second, flush=True)
                qmp.command("cont")
                qmp.merged_hold()
                try:
                    qmp.command("input-send-event", {"device": "/machine/peripheral/missing-keyboard",
                        "events": [{"type": "key", "data": {"down": True,
                            "key": {"type": "qcode", "data": "a"}}}]})
                except RuntimeError:
                    print("QMP_MISSING_TARGET_REJECTED", flush=True)
                else:
                    raise RuntimeError("QEMU silently accepted missing target")
                qmp.command("quit")
                if child.wait(timeout=3) != 0:
                    raise RuntimeError("QEMU failed")
                print("PASS_QMP_TARGET_ACCEPTANCE_NOT_CONSUMER_DELIVERY", flush=True)
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()


if __name__ == "__main__":
    sys.exit(main())
