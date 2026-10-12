import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from runner_identity import admit
from qmp_ready import probe
from validate_guest import validate
from qmp_qualification import QMP, step
from validate_qualification import require_ime_lock_exclusion, require_passive, require_restoration, validate_research, validate as validate_qualification


class HarnessTests(unittest.TestCase):
    def test_ime_lock_exclusion_rejects_overlap_and_extra_keys(self):
        ime = "".join(f"IME_KEY code=30 state={i % 2} time={i}\n" for i in range(22))
        locker = "LOCK_KEY code=30 state=1 time=100 ack=1\nLOCK_KEY code=30 state=0 time=200 ack=1\n"
        require_ime_lock_exclusion(ime, locker)
        for bad in (ime + "IME_KEY code=30 state=1 time=150\n", ime.replace("time=21", "time=150")):
            with self.assertRaises(ValueError):
                require_ime_lock_exclusion(bad, locker)

    def test_research_requires_completed_independent_fixture(self):
        with self.assertRaisesRegex(ValueError, "incomplete"):
            validate_research("Plugin could not be loaded: missing research signature\n[]\n")

    def test_passive_handshake_requires_zero_capture_and_events(self):
        evidence = ("probe listening capabilities=0 capture=false\n"
                    "probe handshakes=1 events=0 capabilities=0 capture=false elapsed_ns=123 status=handshake_only\n")
        require_passive(evidence)
        for changed in (evidence.replace("events=0", "events=1"),
                        evidence.replace("capabilities=0", "capabilities=1"),
                        evidence.replace("capture=false", "capture=true"),
                        evidence.replace("handshakes=1", "handshakes=0")):
            with self.assertRaises(ValueError):
                require_passive(changed)

    def test_restoration_requires_byte_verification(self):
        for evidence in ("ok", "UNLOAD_RESTORED query=2", "NOT_UNLOAD_RESTORED_BYTES_VERIFIED", "UNLOAD_RESTORED_BYTES_VERIFIED_FALSE"):
            with self.assertRaisesRegex(ValueError, "64-byte"):
                require_restoration(evidence)
        require_restoration("UNLOAD_RESTORED_BYTES_VERIFIED query=385 lock=3\n")

    def test_qualification_missing_completion_rejects(self):
        with self.assertRaisesRegex(ValueError, "incomplete"):
            validate_qualification("LOCK_ACK\n")

    def test_targeted_qmp_requires_explicit_guest_devices(self):
        class Monitor:
            key = QMP.key
            pair = QMP.pair
            targeted_keyboards = QMP.targeted_keyboards
            merged_hold = QMP.merged_hold

            def __init__(self):
                self.commands = []
                self.devices = [{"name": name, "type": "child<usb-kbd>"}
                                for name in ("keyboard-a", "keyboard-b")]
                self.devices += [{"name": name, "type": "child<secondary-vga>"}
                                 for name in ("display-a", "display-b")]

            def command(self, name, arguments):
                self.commands.append((name, arguments))
                return self.devices if name == "qom-list" else {}

        monitor = Monitor()
        with patch("qmp_qualification.time.sleep"):
            monitor.merged_hold()
        inputs = [arguments for name, arguments in monitor.commands if name == "input-send-event"]
        self.assertEqual(len(inputs), 6)
        self.assertEqual([event["device"] for event in inputs],
                         ["display-a", "display-b", "display-a", "display-b", "display-b", "display-b"])
        for invalid in ("/dev/input/event0", "/machine/peripheral/keyboard-a", "keyboard-c"):
            with self.assertRaises(ValueError):
                monitor.key("a", True, invalid)
        for devices in ([], [{"name": "keyboard-a", "type": "child<virtio-keyboard>"}],
                        monitor.devices + [monitor.devices[0]]):
            monitor.devices = devices
            with self.assertRaises(RuntimeError):
                monitor.targeted_keyboards()

    def test_qmp_steps_only_use_guest_monitor(self):
        class Monitor:
            def __init__(self):
                self.commands = []

            def command(self, command, arguments):
                self.commands.append((command, arguments))

            key = QMP.key
            pair = QMP.pair

        monitor = Monitor()
        with patch("qmp_qualification.time.sleep"):
            step(monitor, "baseline")
            step(monitor, "second-keyboard")
            step(monitor, "remove-second")
        events = [arguments["events"][0]["data"] for command, arguments in monitor.commands
                  if command == "input-send-event"]
        self.assertEqual(len(events), 22)
        self.assertEqual(sum(event["down"] for event in events), 11)
        self.assertTrue(all(event["key"]["type"] == "qcode" for event in events))
        self.assertEqual([command for command, _ in monitor.commands if command != "input-send-event"],
                         ["device_add", "device_del"])
        with self.assertRaises(ValueError):
            step(monitor, "host")

    def test_qmp_errors_are_not_acknowledged(self):
        qmp = QMP.__new__(QMP)
        from io import BytesIO
        qmp.stream = BytesIO()
        with patch.object(qmp, "receive", side_effect=[{"event": "STOP"}, {"error": {"desc": "bad"}}]):
            with self.assertRaises(RuntimeError):
                qmp.command("input-send-event")

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
