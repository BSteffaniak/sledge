import json
import os
import re
import subprocess
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from candidate_payload import candidate_scope, MACHINE_ID, BOOT_MARKER
from payload_disk import verify_payload
from runner_identity import admit
from qmp_ready import probe
from validate_guest import validate
from qmp_qualification import QMP, step
from peer_evidence import require_failure_outcome
from validate_failure import validate as validate_failure
from validate_qualification import require_ime_lock_exclusion, require_passive, require_restoration, validate_research, validate as validate_qualification


class HarnessTests(unittest.TestCase):
    def test_candidate_payload_requires_exact_scope_and_store_path(self):
        with tempfile.TemporaryDirectory(dir="/hdd") as directory:
            root = Path(directory)
            self.assertFalse(candidate_scope(root))
            (root / "scope").mkdir()
            (root / "scope/machine-id").write_bytes(MACHINE_ID)
            (root / "scope/boot-marker").write_bytes(BOOT_MARKER)
            (root / "store").mkdir()
            (root / ("store/" + "a" * 32 + "-candidate")).mkdir()
            self.assertTrue(candidate_scope(root))
            malformed = (
                b"736c656467652d63616e64696461746531\n",  # Regression: old 34-hex pin.
                b"host-machine-id\n", MACHINE_ID.rstrip(b"\n"),
                MACHINE_ID.upper(), b"0" * 32 + b"\n",
            )
            for value in malformed:
                with self.subTest(machine_id=value):
                    (root / "scope/machine-id").write_bytes(value)
                    with self.assertRaisesRegex(ValueError, "requires 32"):
                        candidate_scope(root)
            (root / "scope/machine-id").write_bytes(b"1" * 32 + b"\n")
            with self.assertRaisesRegex(ValueError, "scope bytes"):
                candidate_scope(root)
            (root / "scope/machine-id").write_bytes(MACHINE_ID)
            (root / "store/not-a-store-path").mkdir()
            with self.assertRaisesRegex(ValueError, "store path"):
                candidate_scope(root)

    def test_candidate_startup_uses_natural_enter_and_separate_late_load(self):
        source = (Path(__file__).parent / "candidate-payload-run.sh").read_text()
        self.assertNotIn("dispatch focuswindow", source)
        self.assertNotIn("step initial-reconcile", source)
        self.assertNotIn("step capability-hotplug", source.split('if [[ $mode == matrix ]]; then')[0])
        self.assertIn('if [[ -f /sledge-payload/hotplug-case ]]; then\n  step capability-hotplug', source)
        self.assertLess(source.index('if [[ $mode == late-load ]]; then start_consumer; fi'),
                        source.index('hyprctl plugin load "$plugin"'))
        self.assertLess(source.index('hyprctl plugin load "$plugin"'),
                        source.index('if [[ $mode == baseline ]]; then start_consumer; fi'))
        self.assertIn('s["waiting_for_focus"] is True and s["native_operational"] is False', source)
        self.assertIn("SLEDGE_INSTALLED_LATE_LOAD_WAITING_DRAIN_COMPLETED_NOT_BASELINE", source)

    def test_candidate_baseline_rejects_rejected_original_outcomes(self):
        from validate_candidate import validate
        report = '''SLEDGE_CANDIDATE_SCOPE_PRESERVED
SLEDGE_CANDIDATE_STORE_TRANSFER_VERIFIED
SLEDGE_INSTALLED_CANDIDATE_COMPLETED_QUALIFIED_FALSE
SLEDGE_PAYLOAD_REPORT_END status=0 qualified=false
{"hyprland_candidate_configured":true,"hyprland_capture_qualified":false,"hyprland_research_configured":false}
EVIDENCE_FILE /tmp/candidate-daemon-events
qualification=Candidate feature_mask=1
native reconciliation epoch=7 sequence=6 nonce=123 accepted=true
native delivery outcome generation=2 epoch=7 sequence=7 nonce=123 key=30 state=1 focus_kind=1 report=Acknowledged(Outcome { result: RejectedOriginalAdmitted,
'''
        with self.assertRaisesRegex(ValueError, "replaced A-down"):
            validate(report)

    def test_required_acceptance_branches_reject_missing_phases(self):
        from validate_candidate import validate
        report = '''SLEDGE_CANDIDATE_SCOPE_PRESERVED
SLEDGE_CANDIDATE_STORE_TRANSFER_VERIFIED
SLEDGE_INSTALLED_CANDIDATE_COMPLETED_QUALIFIED_FALSE
SLEDGE_PAYLOAD_REPORT_END status=0 qualified=false
{"hyprland_candidate_configured":true,"hyprland_capture_qualified":false,"hyprland_research_configured":false}
EVIDENCE_FILE /tmp/candidate-daemon-events
qualification=Candidate feature_mask=1
native reconciliation epoch=7 sequence=6 nonce=123 accepted=true
'''
        for seq in (7, 9):
            report += f'native delivery outcome generation=2 epoch=7 sequence={seq} nonce=123 key=30 state=1 focus_kind=1 report=Acknowledged(Outcome {{ result: Replaced,\n'
        report += 'EVIDENCE_FILE /tmp/candidate-consumer-events\n'
        for _ in range(2):
            report += 'PHASE_BEGIN candidate-a\nkey: 56; state: 1\nkey: 56; state: 0\nPHASE_END candidate-a\n'
        for branch in ('deadline', 'mixed-source'):
            with self.assertRaisesRegex(ValueError, 'required .* missing or incorrect'):
                validate(report + f'REQUIRED_CASE {branch}\n')

    def test_payload_manifest_rejects_tampering_extra_files_and_symlinks(self):
        import hashlib
        with tempfile.TemporaryDirectory(dir="/hdd") as directory:
            root = Path(directory)
            names = ("run.sh", "boot.sh", "sledge", "plugin.so")
            for name in names:
                (root / name).write_bytes(b"fixture")
            manifest = "".join(f"{hashlib.sha256(b'fixture').hexdigest()}  {name}\n" for name in names)
            (root / "manifest.sha256").write_text(manifest)
            self.assertEqual(verify_payload(root), root)
            (root / "run.sh").write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "checksum"):
                verify_payload(root)
            (root / "run.sh").write_bytes(b"fixture")
            (root / "extra").symlink_to("sledge")
            with self.assertRaisesRegex(ValueError, "symlinks"):
                verify_payload(root)
            (root / "extra").unlink()
            (root / "extra").write_bytes(b"undeclared")
            with self.assertRaisesRegex(ValueError, "checksum"):
                verify_payload(root)
            (root / "manifest.sha256").write_text(manifest + "0" * 64 + "  ../escape\n")
            with self.assertRaisesRegex(ValueError, "unsafe"):
                verify_payload(root)

    def test_reporter_preserves_native_boundary_telemetry(self):
        source = (Path(__file__).resolve().parents[3] / "native/hyprland/stock-hook-investigation.nix").read_text()
        match = re.search(r"grep -E '([^']+)' \"\$log\"", source)
        self.assertIsNotNone(match)
        telemetry = ("[LOG] SLEDGE_RESEARCH_REGISTER device=1\n"
                     "[LOG] SLEDGE_RESEARCH_EXIT modeled=26\n"
                     "[LOG] SLEDGE_RESEARCH_BOUNDARY predicate=consumer_map count=26\n"
                     "[LOG] SLEDGE_RESEARCH_BOUNDARY queued=0 queue_failed=0\n"
                     "[LOG] SLEDGE_RESEARCH_CLOSE reason=layout count=1\n"
                     "[LOG] SLEDGE_RESEARCH_FUTURE_TELEMETRY count=1\n"
                     "[LOG] SLEDGE_WORKER hello_send=1 hello_ack=0\n"
                     "SLEDGE_WORKER_PACKET direction=notify generation=1 hex=53485031\n")
        result = subprocess.run(["grep", "-E", match[1]], input=telemetry, text=True,
                                capture_output=True, check=True)
        self.assertEqual(result.stdout, telemetry)

    def test_failure_evidence_requires_same_nonce_reason8_and_replace(self):
        evidence = ("reconciliation connection=1 nonce=9 scope={} accepted=true\n"
                    "control connection=1 scope=Scope { nonce: 9 } body=Reset(8)\n"
                    "outcome connection=1 nonce=9 generation=4 epoch=2 sequence=5 deadline=100 key=30 state=1 focus_epoch=3 focus_kind=1 decision=Replace { key: 48, modifiers: 0 } report=PartialUncertainFatal\n")
        require_failure_outcome(evidence, "PartialUncertainFatal")
        for bad in (evidence.replace("accepted=true", "accepted=false"),
                    evidence.replace("nonce: 9", "nonce: 8"),
                    evidence.replace("decision=Replace", "decision=Pass"),
                    evidence.replace("generation=4", "generation=0"),
                    evidence + evidence.splitlines()[-1] + "\n"):
            with self.assertRaises(ValueError):
                require_failure_outcome(bad, "PartialUncertainFatal")

    def test_mods_failure_rejects_bare_target_up(self):
        text = ("FAILURE_OBSERVATION_COMPLETED mods\n"
                "EVIDENCE_FILE /tmp/replacement-peer-events\n"
                "reconciliation connection=0 nonce=7 scope={} accepted=true\n"
                "control connection=0 scope=Scope { nonce: 7 } body=Reset(8)\n"
                "outcome connection=0 nonce=7 generation=1 epoch=2 sequence=3 deadline=100 key=30 state=1 focus_epoch=1 focus_kind=1 decision=Replace { key: 48, modifiers: 0 } report=PartialUncertainFatal\n"
                "EVIDENCE_FILE /tmp/stock-consumer-events\nPHASE_BEGIN replacement-extra\n"
                "key: 50; state: 1\nkey: 56; state: 0\nkey: 50; state: 0\n"
                "PHASE_END replacement-extra\n")
        with self.assertRaisesRegex(ValueError, "output/ownership"):
            validate_failure(text, "mods")
        for case in ("preflight", "mods", "down"):
            with self.assertRaisesRegex(ValueError, "incomplete"):
                validate_failure("", case)

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
