"""QMP-only guest keyboard driver; run inside the existing private VM namespace."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time


class QMP:
    def __init__(self, connection):
        self.stream = connection.makefile("rwb", buffering=0)
        greeting = self.receive()
        if "QMP" not in greeting:
            raise RuntimeError("missing QMP greeting")
        self.command("qmp_capabilities")

    def receive(self):
        line = self.stream.readline()
        if not line:
            raise RuntimeError("QMP disconnected")
        return json.loads(line)

    def command(self, execute, arguments=None):
        self.stream.write((json.dumps({"execute": execute, "arguments": arguments or {}}) + "\n").encode())
        while True:
            response = self.receive()
            if "event" in response:
                continue
            if "error" in response:
                raise RuntimeError(str(response["error"]))
            if "return" not in response:
                raise RuntimeError("malformed QMP response")
            return response["return"]

    def key(self, code, down, device=None):
        arguments = {"events": [{"type": "key", "data": {
            "down": down, "key": {"type": "qcode", "data": code}}}]}
        if device is not None:
            if device not in {"display-a", "display-b"}:
                raise ValueError("only explicit qualification guest keyboard paths are admitted")
            arguments["device"] = device
        self.command("input-send-event", arguments)
        time.sleep(0.08)

    def pair(self, code, device=None):
        if device is None:
            self.key(code, True)
            self.key(code, False)
        else:
            self.key(code, True, device)
            self.key(code, False, device)

    def targeted_keyboards(self):
        devices = self.command("qom-list", {"path": "/machine/peripheral"})
        for name in ("keyboard-a", "keyboard-b", "display-a", "display-b"):
            matches = [device for device in devices if device.get("name") == name]
            expected = "child<usb-kbd>" if name.startswith("keyboard-") else "child<secondary-vga>"
            if len(matches) != 1 or matches[0].get("type") != expected:
                raise RuntimeError(f"missing exact guest input route: {name}")
        # QMP's device argument identifies a DISPLAY console, not a keyboard.
        # Separate display-bound keyboard handlers make the two routes distinct.
        return ("display-a", "display-b")

    def merged_hold(self):
        first, second = self.targeted_keyboards()
        # Same physical key held by both devices, then first released while second
        # remains held. Consumer evidence must check union ownership, not count
        # QMP acceptance as delivered input.
        self.key("shift", True, first)
        self.key("shift", True, second)
        self.key("shift", False, first)
        self.pair("b", second)
        self.key("shift", False, second)


def step(qmp, name):
    if name in {"baseline", "loaded", "unloaded", "recovered", "locked", "crashed", "ime", "passive", "rollback"}:
        qmp.pair("a")
        qmp.key("shift", True)
        qmp.pair("b")
        qmp.key("shift", False)
        qmp.key("shift_r", True)
        qmp.pair("c")
        qmp.key("shift_r", False)
        qmp.key("ctrl", True)
        qmp.pair("d")
        qmp.key("ctrl", False)
        qmp.key("alt_r", True)
        qmp.pair("e")
        qmp.key("alt_r", False)
    elif name == "hold":
        qmp.key("shift", True)
        qmp.key("f", True)
    elif name == "release":
        qmp.key("f", False)
        qmp.key("shift", False)
    elif name == "locker-hold":
        qmp.key("shift_r", True)
        qmp.key("j", True)
    elif name == "locker-release":
        qmp.key("j", False)
        qmp.key("shift_r", False)
    elif name == "initial-reconcile":
        qmp.pair("c")
    elif name == "failure-recovery":
        qmp.pair("c")
        qmp.key("shift", True)
        qmp.pair("d")
        qmp.key("shift", False)
    elif name == "capability-hotplug":
        qmp.command("device_add", {"driver": "usb-kbd", "bus": "qualification-usb.0", "id": "capability-keyboard"})
        time.sleep(1)
    elif name == "capability-remove":
        qmp.command("device_del", {"id": "capability-keyboard"})
        time.sleep(1)
    elif name == "candidate-mixed-hold":
        devices = qmp.command("qom-list", {"path": "/machine/peripheral"})
        for name, kind in (("mixed-usb", "usb-kbd"), ("mixed-virtio", "virtio-keyboard-pci")):
            if not any(d.get("name") == name and d.get("type") == f"child<{kind}>" for d in devices):
                raise RuntimeError(f"missing physical mixed route {name}")
        schema = qmp.command("query-qmp-schema")
        print("QMP_MIXED_SCHEMA", json.dumps([s for s in schema if s.get("name") == "input-send-event"]), flush=True)
        qmp.key("alt", True, "display-a")
    elif name == "candidate-mixed-a":
        qmp.pair("a", "display-b")
    elif name == "candidate-mixed-up":
        qmp.key("alt", False, "display-a")
    elif name == "candidate-stress":
        for _ in range(100):
            qmp.pair("a")
    elif name == "candidate-repeat":
        qmp.key("a", True)
        time.sleep(1.2)
        qmp.key("a", False)
    elif name == "candidate-caps":
        qmp.pair("caps_lock")
    elif name == "replacement-alt":
        qmp.key("alt", True)
        qmp.pair("a")
        qmp.key("alt", False)
    elif name == "daemon-a-down":
        qmp.key("a", True)
    elif name == "daemon-a-up":
        qmp.key("a", False)
    elif name == "replacement-a":
        qmp.pair("a")
    elif name == "replacement-extra":
        qmp.key("shift", True)
        qmp.pair("a")
        qmp.key("shift", False)
    elif name == "replacement-target-held":
        qmp.key("b", True)
        qmp.pair("a")
        qmp.key("b", False)
    elif name == "second-keyboard":
        qmp.command("device_add", {"driver": "usb-kbd", "bus": "qualification-usb.0", "id": "keyboard-b"})
        time.sleep(1)
        qmp.pair("g")
    elif name == "remove-second":
        qmp.command("device_del", {"id": "keyboard-b"})
        time.sleep(1)
        qmp.pair("h")
    else:
        raise ValueError(f"unknown guest step: {name}")


def connect(path, child):
    # Store-image creation precedes QEMU; bound readiness within the outer 180s
    # run budget instead of assuming the image always materializes within 40s.
    deadline = time.monotonic() + 100
    while time.monotonic() < deadline:
        if child.poll() is not None:
            raise RuntimeError("VM exited before control sockets")
        connection = socket.socket(socket.AF_UNIX)
        connection.settimeout(30)
        try:
            connection.connect(path)
            return connection
        except (FileNotFoundError, ConnectionRefusedError):
            connection.close()
            time.sleep(0.1)
    raise RuntimeError(f"control socket timeout: {path}")


def main():
    runner = sys.argv[1]
    if not Path(runner).is_absolute() or not runner.startswith("/nix/store/"):
        raise ValueError("immutable VM runner required")
    env = dict(os.environ)
    env["QEMU_OPTS"] += (" -qmp unix:/tmp/qualification-qmp,server=on,wait=off"
                         " -serial unix:/tmp/qualification-control,server=on,wait=off")
    child = subprocess.Popen([runner], env=env)
    completed = False
    try:
        with connect("/tmp/qualification-qmp", child) as monitor, \
             connect("/tmp/qualification-control", child) as control:
            qmp = QMP(monitor)
            stream = control.makefile("rwb", buffering=0)
            while True:
                line = stream.readline()
                if not line:
                    raise RuntimeError("guest exited without fixture completion")
                text = line.decode().strip()
                print(f"QMP_GUEST {text}", flush=True)
                if text == "DONE":
                    stream.write(b"OK\n")
                    completed = True
                    break
                if not text.startswith("STEP "):
                    raise RuntimeError(f"unexpected guest control: {text}")
                step(qmp, text[5:])
                stream.write(b"OK\n")
            status = child.wait(timeout=30)
            if not completed:
                raise RuntimeError("guest fixture did not complete")
            return status
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
