import importlib.util
from pathlib import Path
import unittest
import json
import os
import tempfile

session_spec = importlib.util.spec_from_file_location("session", Path(__file__).with_name("session.py"))
session = importlib.util.module_from_spec(session_spec)
session_spec.loader.exec_module(session)

spec = importlib.util.spec_from_file_location("startup", Path(__file__).with_name("startup.py"))
startup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(startup)


class Readiness(unittest.TestCase):
    def test_exact_instance_publication_race_is_bounded(self):
        from types import SimpleNamespace
        ticks = [0.0]
        responses = iter(["[]", '{"not":"a list"}', '[{"instance":"other","pid":77}]',
                          '[{"instance":"exact","pid":123}]'])
        calls = []
        def command(argv, **kwargs):
            calls.append(argv)
            return SimpleNamespace(stdout=next(responses))
        def sleep(seconds):
            ticks[0] += seconds
        self.assertEqual(startup.discover_pid(Path("/hyprctl"), "exact", command, 1,
                         lambda: ticks[0], sleep), 123)
        self.assertEqual(calls, [["/hyprctl", "-j", "instances"]] * 4)
        ticks[0] = 0
        with self.assertRaisesRegex(RuntimeError, "not published"):
            startup.discover_pid(Path("/hyprctl"), "absent",
                lambda *a, **k: SimpleNamespace(stdout='[{"instance":"other","pid":77}]'),
                0.1, lambda: ticks[0], sleep)
        with self.assertRaisesRegex(RuntimeError, "ambiguous"):
            startup.discover_pid(Path("/hyprctl"), "exact",
                lambda *a, **k: SimpleNamespace(stdout='[{"instance":"exact","pid":1},{"instance":"exact","pid":2}]'))

    def test_invalid_config_stops_before_plugin_or_first_app(self):
        import subprocess
        calls = []
        def rejected(argv, **kwargs):
            calls.append(argv)
            raise subprocess.CalledProcessError(1, argv, stderr="unknown key in TOML")
        with self.assertRaisesRegex(RuntimeError, "unknown key.*plugin/first app not launched"):
            startup.validate_config(Path("/daemon"), Path("/config"), rejected)
        self.assertEqual(calls, [["/daemon", "validate", "/config"]])
        accepted = []
        startup.validate_config(Path("/daemon"), Path("/config"), lambda argv, **kwargs: accepted.append(argv))
        self.assertEqual(accepted, calls)

    def test_wrapper_provenance_without_process_environment(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            session.write_launch_provenance(parent, 123, Path("/compositor"))
            native = parent / "native.sock"
            env = dict(SLEDGE_HYPRLAND_RUNTIME=str(parent), SLEDGE_HYPRLAND_PASSIVE_SOCKET=str(native))
            startup.validate_launch(parent, 123, native, env)
            with self.assertRaisesRegex(RuntimeError, "does not match"):
                startup.validate_launch(parent, 456, native, env)
            os.chmod(parent / "launch.json", 0o644)
            with self.assertRaisesRegex(RuntimeError, "invalid private"):
                startup.validate_launch(parent, 123, native, env)

    def test_bundle_identity_fails_before_loading_unknown_files(self):
        import hashlib
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            plugin, compositor, registration = (parent / name for name in ("plugin", "compositor", "registration"))
            plugin.write_bytes(b"plugin")
            compositor.write_bytes(b"compositor")
            descriptor = dict(wire_version=1, qualification_revision=2, plugin_path=str(plugin),
                plugin_sha256=hashlib.sha256(plugin.read_bytes()).hexdigest(),
                compositor_path=str(compositor), compositor_sha256=hashlib.sha256(compositor.read_bytes()).hexdigest())
            registration.write_text(json.dumps(descriptor))
            startup.validate_bundle(registration, plugin)
            plugin.write_bytes(b"different")
            with self.assertRaisesRegex(RuntimeError, "build mismatch"):
                startup.validate_bundle(registration, plugin)
            descriptor["qualification_revision"] = 999
            registration.write_text(json.dumps(descriptor))
            with self.assertRaisesRegex(RuntimeError, "unsupported"):
                startup.validate_bundle(registration, plugin)

    def test_private_login_directory_and_atomic_marker(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            self.assertEqual(session.private_runtime(parent), parent)
            marker = parent / "session.json"
            startup.publish_session(marker, parent / "health.sock", 123, "exact-instance")
            self.assertEqual(marker.stat().st_mode & 0o777, 0o600)
            self.assertEqual(json.loads(marker.read_text()), dict(version=1, backend="hyprland",
                ipc_socket=str(parent / "health.sock"), compositor_pid=123, instance="exact-instance"))
            with self.assertRaisesRegex(RuntimeError, "already exists"):
                startup.publish_session(marker, parent / "other.sock", 456, "other")
            self.assertEqual(list(parent.iterdir()), [marker])
            os.chmod(parent, 0o755)
            with self.assertRaisesRegex(RuntimeError, "private same-user"):
                session.private_runtime(parent)

    def test_no_marker_overwrite_through_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "session.json"
            marker.symlink_to(Path(directory) / "absent")
            with self.assertRaisesRegex(RuntimeError, "already exists"):
                startup.publish_session(marker, Path(directory) / "health.sock", 123, "instance")

    def test_loader_exit_zero_error_is_not_acknowledgment(self):
        from types import SimpleNamespace
        for output in ("error: ABI mismatch", "", "failed"):
            with self.assertRaisesRegex(RuntimeError, "not acknowledged"):
                startup.load_plugin(Path('/hyprctl'), 'instance', Path('/plugin'),
                                    command=lambda *a, **k: SimpleNamespace(stdout=output))
        startup.load_plugin(Path('/hyprctl'), 'instance', Path('/plugin'),
                            command=lambda *a, **k: SimpleNamespace(stdout='ok\n'))

    def test_hello_candidate_and_missing_proof_are_not_ready(self):
        self.assertFalse(startup.ready({"hyprland_authenticated": True}))
        caps = dict(hyprland_supported_configured=True, hyprland_capture_qualified=True,
                    hyprland_operational=True, hyprland_candidate_configured=False)
        self.assertTrue(startup.ready(caps))
        for key in ("hyprland_supported_configured", "hyprland_capture_qualified", "hyprland_operational"):
            self.assertFalse(startup.ready(dict(caps, **{key: False})))
        self.assertFalse(startup.ready(dict(caps, hyprland_candidate_configured=True)))


if __name__ == "__main__":
    unittest.main()
