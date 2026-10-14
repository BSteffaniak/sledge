import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch, Mock


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


session = load("session")
startup = load("startup")


class Login(unittest.TestCase):
    def test_launches_once_with_private_locator_and_no_restart(self):
        with tempfile.TemporaryDirectory() as directory:
            compositor = Mock(pid=123)
            compositor.wait.return_value = 0
            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": directory}), \
                 patch("sys.argv", ["session", "--compositor", "/exact/Hyprland", "--config", "/exact/config", "--hyprctl", "/exact/hyprctl", "--plugin", "/exact/plugin"]), \
                 patch.object(session.os, "pidfd_open", return_value=41), patch.object(session.os, "close"), \
                 patch.object(session.subprocess, "Popen", return_value=compositor) as launch, \
                 patch.object(session.signal, "signal"):
                self.assertEqual(session.main(), 0)
            launch.assert_called_once()
            args, kwargs = launch.call_args
            self.assertEqual(args[0], ["/exact/Hyprland", "--config", "/exact/config"])
            env = kwargs["env"]
            runtime = Path(env["SLEDGE_HYPRLAND_RUNTIME"])
            self.assertEqual(runtime.parent, Path(directory))
            self.assertEqual(runtime.stat().st_mode & 0o777, 0o700)
            self.assertEqual(env["SLEDGE_SESSION_FILE"], str(runtime / "session.json"))
            self.assertEqual(env["SLEDGE_HYPRLAND_PASSIVE_SOCKET"], str(runtime / "native.sock"))
            self.assertFalse((runtime / "session.json").exists())
            (runtime / "launch.json").unlink()
            runtime.rmdir()

    def test_failure_keeps_diagnostics_and_never_restarts(self):
        with tempfile.TemporaryDirectory() as directory:
            compositor = Mock(pid=123)
            compositor.wait.return_value = 1
            with patch.dict(os.environ, {"XDG_RUNTIME_DIR": directory}), \
                 patch("sys.argv", ["session", "--compositor", "/exact/Hyprland", "--config", "/exact/config", "--hyprctl", "/exact/hyprctl", "--plugin", "/exact/plugin"]), \
                 patch.object(session.os, "pidfd_open", return_value=41), patch.object(session.os, "close"), \
                 patch.object(session.subprocess, "Popen", return_value=compositor) as launch, \
                 patch.object(session.signal, "signal"), patch("sys.stderr") as errors:
                self.assertEqual(session.main(), 1)
            launch.assert_called_once()
            self.assertIn("no automatic fallback", str(errors.write.call_args_list))
            for path in Path(directory).iterdir():
                (path / "launch.json").unlink()
                path.rmdir()

    def test_controlled_logout_drains_before_signal_and_timeout_retains(self):
        from types import SimpleNamespace
        import json
        with tempfile.TemporaryDirectory() as directory:
            startup.publish_logout(directory, 123, "exact-instance")
            order = []
            def command(argv, **kwargs):
                order.append(argv[3:])
                if argv[3:] == ["sledge:status"]:
                    return SimpleNamespace(stdout=json.dumps(dict(version=1, draining=True, detached=True,
                        quiescent=True, connection_generation=1, ownership_epoch=1)))
                return SimpleNamespace(stdout="ok")
            def send(fd, signum):
                self.assertEqual(fd, 41)
                order.append("signal")
            with patch.object(session, "stop_detached_daemon", side_effect=lambda d: order.append("daemon stop ack")):
                self.assertTrue(session.orderly_logout(directory, 123, 41, "/hyprctl", "/plugin", 15, command, send))
            self.assertEqual(order, [["dispatch", "sledge:drain"], ["sledge:status"], "daemon stop ack", ["plugin", "unload", "/plugin"], "signal"])
            sender = Mock()
            def timeout(*args, **kwargs):
                raise session.subprocess.TimeoutExpired("drain", 2)
            with patch("sys.stderr"):
                self.assertFalse(session.orderly_logout(directory, 123, 41, "/hyprctl", "/plugin", 15, timeout, sender))
            sender.assert_not_called()
            order.clear()
            with patch.object(session, "stop_detached_daemon", side_effect=TimeoutError("stop ack timeout")), patch("sys.stderr"):
                self.assertFalse(session.orderly_logout(directory, 123, 41, "/hyprctl", "/plugin", 15, command, sender))
            self.assertFalse(any(isinstance(step, list) and step[:2] == ["plugin", "unload"] for step in order))
            sender.assert_not_called()

    def test_daemon_stop_requires_exact_exit_ack(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            for name, value in (("daemon.json", dict(version=1, daemon_pid=123)),
                                ("daemon-stop.ack", dict(version=1, daemon_pid=123, exited=True))):
                path = parent / name
                path.write_text(json.dumps(value))
                os.chmod(path, 0o600)
            session.stop_detached_daemon(parent)
            self.assertEqual(json.loads((parent / "daemon-stop.request").read_text()), dict(version=1, daemon_pid=123, detached=True))
            (parent / "daemon-stop.ack").unlink()
            with self.assertRaises(TimeoutError):
                session.stop_detached_daemon(parent, timeout=0)

    def test_guardian_signals_exact_daemon_only_after_compositor_exit(self):
        order = []
        def waited(read, _write, _error):
            self.assertEqual(read, [41])
            order.append("compositor exited")
            return [41], [], []
        def stopped(fd, signum):
            self.assertEqual(fd, 42)
            self.assertEqual(signum, startup.signal.SIGTERM)
            order.append("daemon stopped")
        with patch.object(startup.os, "fork", return_value=0), \
             patch.object(startup.signal, "signal"), \
             patch.object(startup.select, "select", side_effect=waited), \
             patch.object(startup.signal, "pidfd_send_signal", side_effect=stopped), \
             patch.object(startup.os, "_exit", side_effect=SystemExit(0)):
            with self.assertRaises(SystemExit):
                startup.supervise_lifetime(41, 42)
        self.assertEqual(order, ["compositor exited", "daemon stopped"])


if __name__ == "__main__":
    unittest.main()
