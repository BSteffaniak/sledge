#!/usr/bin/env python3
"""Internal display-manager launcher; no restart, fallback or socket deletion."""
import argparse
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import json
import importlib.util
import stat
import time


def private_json(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd) as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o077 or metadata.st_size > 4096:
            raise RuntimeError("invalid private daemon identity/acknowledgment")
        return json.load(source)


def stop_detached_daemon(directory, timeout=6):
    """Only after irreversible detach; guardian retains the exact daemon pidfd."""
    directory = Path(directory)
    identity = private_json(directory / "daemon.json")
    if not isinstance(identity, dict) or identity.get("version") != 1 or type(identity.get("daemon_pid")) is not int or identity["daemon_pid"] <= 0:
        raise RuntimeError("invalid exact daemon identity; plugin retained")
    request = directory / "daemon-stop.request"
    if not request.exists():
        fd, temporary = tempfile.mkstemp(prefix=".stop-", dir=directory)
        try:
            with os.fdopen(fd, "w") as output:
                json.dump(dict(version=1, daemon_pid=identity["daemon_pid"], detached=True), output)
            os.link(temporary, request)
        finally:
            os.unlink(temporary)
    deadline = time.monotonic() + timeout
    acknowledgment = directory / "daemon-stop.ack"
    while time.monotonic() < deadline:
        if acknowledgment.exists():
            result = private_json(acknowledgment)
            if result != dict(version=1, daemon_pid=identity["daemon_pid"], exited=True):
                raise RuntimeError("daemon stop acknowledgment mismatch; plugin retained")
            print(json.dumps({"event": "daemon_stop_acknowledged", "daemon_pid": identity["daemon_pid"]}), flush=True)
            return
        time.sleep(0.05)
    raise TimeoutError("exact daemon exit not acknowledged; detached plugin/compositor retained")


def orderly_logout(directory, pid, compositor_fd, hyprctl, plugin, signum,
                    command=subprocess.run, send=signal.pidfd_send_signal, controller_path=None):
    try:
        path = Path(directory) / "logout.json"
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd) as source:
            metadata = os.fstat(source.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o077 or metadata.st_size > 4096:
                raise RuntimeError("invalid private logout selection")
            selection = json.load(source)
        if not isinstance(selection, dict) or type(selection.get("version")) is not int or selection.get("version") != 1 or type(selection.get("compositor_pid")) is not int or selection.get("compositor_pid") != pid or not isinstance(selection.get("instance"), str) or not selection["instance"]:
            raise RuntimeError("logout selection does not identify this compositor")
        spec = importlib.util.spec_from_file_location("drain_unload", controller_path or Path(__file__).with_name("drain-unload.py"))
        controller = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(controller)
        def invoke(executable, instance, arguments, timeout):
            return command([str(executable), "--instance", instance, *arguments],
                           check=True, capture_output=True, text=True, timeout=timeout).stdout
        controller.drain_unload(hyprctl, selection["instance"], plugin, 10, invoke,
                                after_detach=lambda: stop_detached_daemon(directory))
        send(compositor_fd, signum)
        return True
    except (OSError, RuntimeError, ValueError, TimeoutError, subprocess.SubprocessError) as error:
        print(f"Sledge logout refused: {error}; compositor/daemon retained; release held keys and retry logout or explicit drain", file=sys.stderr)
        return False


def write_launch_provenance(directory, pid, compositor):
    path = directory / "launch.json"
    fd, temporary = tempfile.mkstemp(prefix=".launch-", dir=directory)
    try:
        with os.fdopen(fd, "w") as output:
            json.dump(dict(version=1, compositor_pid=pid, compositor_path=str(compositor),
                           native_socket=str(directory / "native.sock")), output)
            output.flush()
            os.fsync(output.fileno())
        os.link(temporary, path)
    finally:
        os.unlink(temporary)


def private_runtime(path):
    path = Path(path)
    metadata = path.lstat()
    import stat
    if not path.is_absolute() or not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid() or metadata.st_mode & 0o077:
        raise RuntimeError("login requires a private same-user XDG_RUNTIME_DIR")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compositor", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--hyprctl", type=Path, required=True)
    parser.add_argument("--plugin", type=Path, required=True)
    parser.add_argument("--drain-controller", type=Path, default=Path(__file__).with_name("drain-unload.py"))
    args = parser.parse_args()
    if not all(path.is_absolute() for path in (args.compositor, args.config, args.hyprctl, args.plugin)):
        parser.error("internal compositor/config paths must be absolute")
    runtime = private_runtime(os.environ.get("XDG_RUNTIME_DIR", ""))
    directory = Path(tempfile.mkdtemp(prefix="sledge.", dir=runtime))
    env = dict(os.environ, SLEDGE_HYPRLAND_RUNTIME=str(directory),
               SLEDGE_HYPRLAND_PASSIVE_SOCKET=str(directory / "native.sock"),
               SLEDGE_SESSION_FILE=str(directory / "session.json"))
    # Isolate the child group so ordinary signals to the login wrapper do not
    # bypass drain by reaching the compositor first. Forced cgroup teardown is
    # abrupt session loss, not an orderly logout guarantee.
    compositor = subprocess.Popen([str(args.compositor), "--config", str(args.config)], env=env, start_new_session=True)
    compositor_fd = os.pidfd_open(compositor.pid)
    write_launch_provenance(directory, compositor.pid, args.compositor)
    requested = None
    def stop(signum, _frame):
        nonlocal requested
        requested = signum
    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(signum, stop)
    while True:
        if requested is not None:
            signum, requested = requested, None
            orderly_logout(directory, compositor.pid, compositor_fd, args.hyprctl, args.plugin, signum,
                           controller_path=args.drain_controller)
        try:
            code = compositor.wait(timeout=0.25)
            break
        except subprocess.TimeoutExpired:
            continue
    os.close(compositor_fd)
    # Endpoint directory/sockets are deliberately retained, never reused/unlinked.
    if code:
        print(f"Sledge login: compositor exited ({code}); no automatic fallback. Select Hyprland at login for rollback. Diagnostics: {directory}", file=sys.stderr)
    return code


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError) as error:
        print(f"Sledge login failed: {error}", file=sys.stderr)
        sys.exit(1)
