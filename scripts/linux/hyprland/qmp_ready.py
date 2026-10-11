"""Bounded QMP readiness, not time-alive inference. QEMU must be explicit."""
from pathlib import Path
import hashlib
import json
import os
import selectors
import subprocess
import sys
import time


def probe(command, seconds=5):
    print("QEMU_IDENTITY", command[0], hashlib.sha256(Path(command[0]).read_bytes()).hexdigest(), flush=True)
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + seconds

    pending = bytearray()

    def receive():
        while time.monotonic() < deadline:
            if b"\n" not in pending:
                if not selector.select(max(0, deadline - time.monotonic())):
                    continue
                chunk = os.read(process.stdout.fileno(), 65536)
                if not chunk:
                    raise RuntimeError("QEMU exited before readiness")
                pending.extend(chunk)
            if b"\n" in pending:
                line, _, rest = pending.partition(b"\n")
                pending[:] = rest
                value = json.loads(line)
                if "event" not in value:
                    return value
        raise RuntimeError("QMP readiness deadline expired")

    def request(name):
        process.stdin.write((json.dumps({"execute": name}) + "\n").encode())
        process.stdin.flush()
        result = receive()
        if "return" not in result:
            raise RuntimeError(str(result))
        return result["return"]

    try:
        if "QMP" not in receive():
            raise RuntimeError("missing QMP greeting")
        request("qmp_capabilities")
        status = request("query-status")
        pci = request("query-pci")
        if status.get("status") != "prelaunch" or '"qdev_id": "selected-gpu"' not in json.dumps(pci):
            raise RuntimeError("selected GPU/prelaunch readiness absent")
        print("PASS: QMP query-status + query-pci selected-gpu readiness", flush=True)
    finally:
        selector.close()
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        process.stdin.close()
        process.stdout.close()


if __name__ == "__main__":
    qemu = os.environ["SLEDGE_VM_QEMU"]
    if not os.path.isabs(qemu):
        raise SystemExit("QEMU must be an absolute pinned path")
    probe([qemu, "-nodefaults", "-no-user-config", "-machine", "q35,accel=tcg", "-m", "256", "-S", "-nic", "none", "-monitor", "none", "-serial", "none", "-qmp", "stdio", "-device", "virtio-gpu-gl-pci,id=selected-gpu", "-display", "egl-headless,rendernode=" + sys.argv[1]])
