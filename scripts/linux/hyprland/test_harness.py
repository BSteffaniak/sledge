import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from runner_identity import admit
from qmp_ready import probe
from validate_guest import validate


class HarnessTests(unittest.TestCase):
    def test_fake_runner_qemu_comment_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "run-sledge-qualification-vm"
            fake.write_text("#!/bin/sh\n# /nix/store/expected-qemu/bin/qemu-system-x86_64\nexit 0\n")
            fake.chmod(0o755)
            expected = "/nix/store/" + "a" * 32 + "-nixos-vm/bin/run-sledge-qualification-vm"
            with patch.dict(os.environ, {"PATH": directory}):
                with self.assertRaises(ValueError):
                    admit(str(fake), expected)
                with self.assertRaises(ValueError):
                    admit(str(fake), str(fake))

    def test_store_runner_symlink_admission(self):
        expected = "/nix/store/" + "a" * 32 + "-nixos-vm/bin/run-sledge-qualification-vm"
        immutable = Path("/nix/store/" + "b" * 32 + "-run-nixos-vm")
        with patch.object(Path, "resolve", return_value=immutable), \
             patch.object(Path, "is_file", return_value=True), \
             patch.object(Path, "read_bytes", return_value=b"runner"), \
             patch("runner_identity.os.access", return_value=True):
            self.assertEqual(len(admit(expected, expected)), 64)
        with patch.object(Path, "resolve", return_value=Path("/tmp/mutable-runner")):
            with self.assertRaises(ValueError):
                admit(expected, expected)

    def test_guest_rejects_bad_outputs_and_identity(self):
        good = {"name": "Virtual-1", "width": 640, "height": 480, "scale": 1, "dpmsStatus": True}
        validate({"version": "0.52.2"}, [good], "/exact", "/exact")
        for monitors in [[], [dict(good, disabled=True)], [dict(good, width=800)]]:
            with self.assertRaises(ValueError):
                validate({"version": "0.52.2"}, monitors, "/exact", "/exact")
        for version, executable in [("0.53", "/exact"), ("0.52.2", "/wrong")]:
            with self.assertRaises(ValueError):
                validate({"version": version}, [good], executable, "/exact")

    def test_qmp_delayed_failed_and_ready_with_path_poison(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "selected"
            poison = Path(directory) / "qemu-system-x86_64"
            poison.write_text("#!/bin/sh\nexit 99\n")
            poison.chmod(0o755)
            scripts = [
                "import time; time.sleep(2)",
                'import sys,time; sys.stdout.write("{"); sys.stdout.flush(); time.sleep(2)',
                "raise SystemExit(1)",
                'import json,sys; print(json.dumps({"QMP":{}}),flush=True); responses=[{}, {"status":"prelaunch"}, [{"qdev_id":"selected-gpu"}]];\nfor response in responses:\n sys.stdin.readline(); print(json.dumps({"return":response}),flush=True)',
            ]
            for index, script in enumerate(scripts):
                fake.write_text("#!" + os.sys.executable + "\n" + script)
                fake.chmod(0o755)
                with patch.dict(os.environ, {"PATH": directory}):
                    if index < 3:
                        with self.assertRaises(RuntimeError):
                            probe([str(fake)], seconds=0.2)
                    else:
                        probe([str(fake)], seconds=1)


if __name__ == "__main__":
    unittest.main()
