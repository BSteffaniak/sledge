#!/usr/bin/env python3
"""Explicit compositor startup barrier. Does not configure or restart a desktop."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import time


def status(path):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(1)
        connection.connect(str(path))
        connection.sendall(b'{"op":"status"}\n')
        frame = bytearray()
        while len(frame) <= 65536:
            chunk = connection.recv(4096)
            if not chunk:
                raise RuntimeError("status disconnected")
            frame.extend(chunk)
            if b'\n' in frame:
                response = json.loads(frame.split(b'\n', 1)[0])
                if response.get("kind") != "status":
                    raise RuntimeError("invalid daemon status")
                return response.get("capabilities", {})
        raise RuntimeError("oversized status")


def ready(caps):
    return isinstance(caps, dict) and all(caps.get(k) is True for k in (
        "hyprland_supported_configured", "hyprland_capture_qualified", "hyprland_operational"
    )) and caps.get("hyprland_candidate_configured") is False


def load_plugin(hyprctl, instance, plugin, command=subprocess.run):
    result = command([str(hyprctl), "--instance", instance, "plugin", "load", str(plugin)],
                     check=True, capture_output=True, text=True, timeout=5)
    # hyprctl can report dispatcher/plugin errors in stdout with exit status 0.
    if result.stdout.strip().lower() != "ok":
        raise RuntimeError("plugin load not acknowledged; first app/daemon not launched; inspect compositor status")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ("hyprctl", "plugin", "daemon", "registration", "config", "socket", "ipc_socket"):
        parser.add_argument("--" + key.replace("_", "-"), type=Path, required=True)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--first-app", nargs="+", required=True,
                        help="real initial application: provides natural enter, not synthetic input")
    args = parser.parse_args()
    if args.pid <= 0 or not 0 < args.timeout <= 60 or not args.instance:
        parser.error("explicit live compositor and bounded timeout required")
    if not all(getattr(args, k).is_absolute() for k in (
        "hyprctl", "plugin", "daemon", "registration", "config", "socket", "ipc_socket"
    )) or not Path(args.first_app[0]).is_absolute():
        parser.error("all executable/file paths must be absolute")
    process_env = Path(f"/proc/{args.pid}/environ").read_bytes()
    expected = b"SLEDGE_HYPRLAND_PASSIVE_SOCKET=" + os.fsencode(args.socket)
    if expected not in process_env.split(b'\0'):
        parser.error("compositor must be launched with the exact installed endpoint environment")
    for endpoint in (args.socket, args.ipc_socket):
        parent = endpoint.parent.stat()
        if parent.st_uid != os.getuid() or parent.st_mode & 0o077:
            parser.error("endpoint parents must already be private same-user directories")
    if args.socket.exists() or args.ipc_socket.exists():
        parser.error("existing endpoints require explicit recovery; never unlink")
    load_plugin(args.hyprctl, args.instance, args.plugin)
    # Plugin observes the actual first application's enter. No refocus/key injection.
    subprocess.Popen(args.first_app)
    daemon = subprocess.Popen([str(args.daemon), "--config", str(args.config), "--socket", str(args.ipc_socket), "run",
        "--linux-hyprland-registration", str(args.registration),
        "--linux-hyprland-pid", str(args.pid), "--linux-hyprland-socket", str(args.socket)])
    deadline = time.monotonic() + args.timeout
    while time.monotonic() < deadline and daemon.poll() is None:
        try:
            if ready(status(args.ipc_socket)):
                print(json.dumps({"daemon_pid": daemon.pid, "operational": True}), flush=True)
                return
        except (OSError, ValueError, RuntimeError):
            pass
        time.sleep(0.05)
    # Do not force-unload, kill a potentially output-owning daemon or unlink sockets.
    raise RuntimeError(f"startup not operational; plugin/daemon retained (daemon PID {daemon.pid}); use drain controller")


if __name__ == "__main__":
    main()
