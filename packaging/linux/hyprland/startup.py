#!/usr/bin/env python3
"""Explicit compositor startup barrier. Does not configure or restart a desktop."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import select
import signal
import sys
import tempfile
import hashlib
import stat


def validate_launch(directory, pid, native_socket, environ=os.environ):
    """Wrapper provenance is a locator check, not backend acceptance authority."""
    directory = Path(directory)
    if environ.get("SLEDGE_HYPRLAND_RUNTIME") != str(directory) or environ.get("SLEDGE_HYPRLAND_PASSIVE_SOCKET") != str(native_socket):
        raise RuntimeError("startup must run inside the internal Sledge login wrapper")
    path = directory / "launch.json"
    # The compositor can dispatch exec-once just before its parent writes proof.
    deadline = time.monotonic() + 1
    while not path.exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd) as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o077 or metadata.st_size > 4096:
            raise RuntimeError("invalid private login provenance")
        proof = json.load(source)
    if not isinstance(proof, dict) or type(proof.get("version")) is not int or proof.get("version") != 1 or type(proof.get("compositor_pid")) is not int or proof.get("compositor_pid") != pid or proof.get("native_socket") != str(native_socket):
        raise RuntimeError("login provenance does not match this compositor/endpoint")



def validate_bundle(registration, plugin):
    descriptor = json.loads(Path(registration).read_text())
    if not isinstance(descriptor, dict) or descriptor.get("wire_version") != 1 or descriptor.get("qualification_revision") not in (1, 2):
        raise RuntimeError("unsupported Sledge bundle revision; select the ordinary Hyprland login")
    if descriptor.get("plugin_path") != str(plugin):
        raise RuntimeError("Sledge bundle plugin path mismatch")
    for path, expected in ((plugin, descriptor.get("plugin_sha256")),
                           (Path(descriptor.get("compositor_path", "")), descriptor.get("compositor_sha256"))):
        if not Path(path).is_absolute() or not isinstance(expected, str) or len(expected) != 64:
            raise RuntimeError("invalid Sledge bundle identity")
        digest = hashlib.sha256()
        with Path(path).open("rb") as source:
            for chunk in iter(lambda: source.read(65536), b""):
                digest.update(chunk)
        if digest.hexdigest() != expected:
            raise RuntimeError("Sledge bundle build mismatch; plugin not loaded")
    # This corroborates files only: daemon still independently proves acceptance,
    # mapped compositor identity, lock/IME eligibility and operational readiness.



def discover_pid(hyprctl, instance, command=subprocess.run, timeout=5,
                 clock=time.monotonic, sleep=time.sleep):
    """Retry only publication of the explicit instance; never guess a PID."""
    deadline = clock() + timeout
    while clock() < deadline:
        try:
            result = command([str(hyprctl), "-j", "instances"], check=True,
                             capture_output=True, text=True, timeout=min(1, deadline-clock()))
            entries = json.loads(result.stdout)
            if not isinstance(entries, list):
                raise ValueError("invalid instance list")
            matches = [entry for entry in entries if isinstance(entry, dict) and entry.get("instance") == instance]
            if len(matches) > 1:
                raise RuntimeError("ambiguous exact compositor instance; startup refused")
            if matches:
                pid = matches[0].get("pid")
                if type(pid) is not int or pid <= 0:
                    raise RuntimeError("invalid exact compositor PID; startup refused")
                return pid
        except (OSError, ValueError, subprocess.SubprocessError):
            pass
        sleep(min(0.05, max(0, deadline-clock())))
    raise RuntimeError("exact compositor instance not published within startup budget; plugin/first app not launched")


def validate_config(daemon, config, command=subprocess.run):
    try:
        command([str(daemon), "validate", str(config)], check=True,
                capture_output=True, text=True, timeout=10)
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or "validation rejected").strip()
        raise RuntimeError(f"invalid Sledge config {config}: {detail}; plugin/first app not launched") from error


def publish_logout(directory, pid, instance):
    path = Path(directory) / "logout.json"
    fd, temporary = tempfile.mkstemp(prefix=".logout-", dir=directory)
    try:
        with os.fdopen(fd, "w") as output:
            json.dump(dict(version=1, compositor_pid=pid, instance=instance), output)
            output.flush()
            os.fsync(output.fileno())
        os.link(temporary, path)
    finally:
        os.unlink(temporary)


def publish_session(path, ipc_socket, pid, instance):
    """Publish only after authoritative Supported/operational proof."""
    path = Path(path)
    if path.exists() or path.is_symlink():
        raise RuntimeError("session marker already exists; explicit recovery required")
    fd, temporary = tempfile.mkstemp(prefix=".session-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as output:
            json.dump(dict(version=1, backend="hyprland", ipc_socket=str(ipc_socket),
                           compositor_pid=pid, instance=instance), output)
            output.flush()
            os.fsync(output.fileno())
        os.link(temporary, path)  # atomic no-clobber publication, even on a racing marker
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def supervise_lifetime(compositor_fd, daemon_fd, directory=None, daemon_pid=None):
    """One passive guardian, no service restart or PID-based termination."""
    child = os.fork()
    if child:
        os.close(compositor_fd)
        os.close(daemon_fd)
        return
    try:
        for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            signal.signal(signum, signal.SIG_IGN)
        if directory is None:
            select.select([compositor_fd], [], [])
        else:
            directory = Path(directory)
            while not select.select([compositor_fd], [], [], 0.05)[0]:
                request = directory / "daemon-stop.request"
                if request.exists():
                    metadata = request.lstat()
                    if stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.getuid() and not metadata.st_mode & 0o077 and metadata.st_size <= 4096:
                        value = json.loads(request.read_text())
                        if value == {"version": 1, "daemon_pid": daemon_pid, "detached": True}:
                            break
        try:
            signal.pidfd_send_signal(daemon_fd, signal.SIGTERM)
        except ProcessLookupError:
            pass
        # Exit readiness is proof about the retained incarnation, not a PID scan.
        if directory is not None and select.select([daemon_fd], [], [], 5)[0]:
            acknowledgment = Path(directory) / "daemon-stop.ack"
            fd, temporary = tempfile.mkstemp(prefix=".stop-ack-", dir=directory)
            try:
                with os.fdopen(fd, "w") as output:
                    json.dump({"version": 1, "daemon_pid": daemon_pid, "exited": True}, output)
                os.link(temporary, acknowledgment)
            finally:
                os.unlink(temporary)
            print(json.dumps({"event": "daemon_stop_acknowledged", "daemon_pid": daemon_pid}), flush=True)
    finally:
        os._exit(0)



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
    parser.add_argument("--pid", type=int)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--first-app", nargs="+", required=True,
                        help="real initial application: provides natural enter, not synthetic input")
    args = parser.parse_args()
    if args.pid is None:
        args.pid = discover_pid(args.hyprctl, args.instance)
    if args.pid <= 0 or not 0 < args.timeout <= 60 or not args.instance:
        parser.error("explicit live compositor and bounded timeout required")
    if not all(getattr(args, k).is_absolute() for k in (
        "hyprctl", "plugin", "daemon", "registration", "config", "socket", "ipc_socket"
    )) or not Path(args.first_app[0]).is_absolute():
        parser.error("all executable/file paths must be absolute")
    for endpoint in (args.socket, args.ipc_socket):
        parent = endpoint.parent.lstat()
        if not stat.S_ISDIR(parent.st_mode) or parent.st_uid != os.getuid() or parent.st_mode & 0o077:
            parser.error("endpoint parents must already be private same-user directories")
    if args.socket.exists() or args.ipc_socket.exists():
        parser.error("existing endpoints require explicit recovery; never unlink")
    if args.socket.parent != args.ipc_socket.parent:
        parser.error("native and IPC endpoints must share the wrapper's private directory")
    compositor_fd = os.pidfd_open(args.pid)
    validate_launch(args.socket.parent, args.pid, args.socket)
    validate_bundle(args.registration, args.plugin)
    validate_config(args.daemon, args.config)
    marker = os.environ.get("SLEDGE_SESSION_FILE")
    if marker and Path(marker) != args.ipc_socket.parent / "session.json":
        parser.error("session marker must be inside the exact private endpoint directory")
    # Publish exact logout selection before loading: interruption during load
    # cannot masquerade as a provably plugin-free session.
    publish_logout(args.socket.parent, args.pid, args.instance)
    load_plugin(args.hyprctl, args.instance, args.plugin)
    # Plugin observes the actual first application's enter. No refocus/key injection.
    subprocess.Popen(args.first_app)
    daemon = subprocess.Popen([str(args.daemon), "--config", str(args.config), "--socket", str(args.ipc_socket), "run",
        "--linux-hyprland-registration", str(args.registration),
        "--linux-hyprland-pid", str(args.pid), "--linux-hyprland-socket", str(args.socket)])
    print(json.dumps({"event": "daemon_started", "daemon_pid": daemon.pid,
                      "daemon_path": str(args.daemon), "compositor_pid": args.pid,
                      "instance": args.instance}), flush=True)
    daemon_pidfd = os.pidfd_open(daemon.pid)
    identity = args.socket.parent / "daemon.json"
    fd = os.open(identity, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as output:
        json.dump({"version": 1, "daemon_pid": daemon.pid}, output)
    supervise_lifetime(compositor_fd, daemon_pidfd, args.socket.parent, daemon.pid)
    deadline = time.monotonic() + args.timeout
    last_status = {}
    while time.monotonic() < deadline and daemon.poll() is None:
        try:
            last_status = status(args.ipc_socket)
        except (OSError, ValueError, RuntimeError):
            time.sleep(0.05)
            continue
        if ready(last_status):
            if marker:
                publish_session(marker, args.ipc_socket, args.pid, args.instance)
            print(json.dumps({"daemon_pid": daemon.pid, "operational": True}), flush=True)
            return
        time.sleep(0.05)
    # Do not force-unload, kill a potentially output-owning daemon or unlink sockets.
    raise RuntimeError(f"startup not operational (daemon exit={daemon.poll()}, capabilities={last_status}); "
                       f"plugin/daemon retained (PID {daemon.pid}); remaining apps withheld; use drain controller")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Sledge startup failed: {error}; no automatic fallback or restart", file=sys.stderr)
        sys.exit(1)
