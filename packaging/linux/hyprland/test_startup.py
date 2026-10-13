import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("startup", Path(__file__).with_name("startup.py"))
startup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(startup)


class Readiness(unittest.TestCase):
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
