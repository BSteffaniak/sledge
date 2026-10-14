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
    @staticmethod
    def tap_sample(name):
        from validate_tap import EXPECTED, SENTINEL
        records = EXPECTED[name] + ([] if name == "tap-hold-down" else SENTINEL)
        result = ""
        for code, state in records:
            result += f"key: {code}; state: {state}\n"
            mask = "00000004" if (code, state) == ("37", "1") else "00000000"
            result += f"depressed: {mask}\nlatched: 00000000\nlocked: 00000000\n"
        return result

    def test_tap_consumer_oracle_rejects_reordering_duplicates_and_stuck_modifiers(self):
        from validate_tap import validate_phase
        sample = self.tap_sample("tap-positive")
        validate_phase(sample, "tap-positive")
        for broken in (
            sample.replace("key: 108; state: 0", "key: 108; state: 1", 1),
            sample + "key: 36; state: 1\n",
            sample.replace("depressed: 00000004", "depressed: 00000008"),
            sample.replace("key: 56; state: 1", "depressed: 00000004\nlatched: 00000000\nlocked: 00000000\nkey: 56; state: 1"),
        ):
            with self.assertRaises(ValueError):
                validate_phase(broken, "tap-positive")
        # Move action ahead of the final physical release, preserving all keys.
        lines = sample.splitlines(keepends=True)
        early = ''.join(lines[:20] + lines[24:40] + lines[20:24] + lines[40:])
        with self.assertRaises(ValueError):
            validate_phase(early, "tap-positive")

    def test_tap_negative_consumer_oracle_and_no_early_hold_action(self):
        from validate_tap import EXPECTED, validate_phase
        for name in EXPECTED:
            validate_phase(self.tap_sample(name), name)
            if name != "tap-positive":
                with self.assertRaises(ValueError):
                    validate_phase(self.tap_sample(name) + "key: 37; state: 1\n", name)

    def test_tap_steps_are_deterministic_and_device_targeted(self):
        class Recorder:
            def __init__(self): self.events = []
            def key(self, code, down, device=None): self.events.append((code, down, device))
            def pair(self, code, device=None):
                self.key(code, True, device)
                self.key(code, False, device)
            def targeted_keyboards(self): return ("display-a", "display-b")
        with patch("tap_cases.time.sleep") as sleep:
            recorder = Recorder()
            step(recorder, "tap-positive")
            self.assertEqual(recorder.events, [("alt_r", True, None), ("alt_r", False, None)] * 3)
            recorder.events.clear()
            step(recorder, "tap-device")
            self.assertEqual([event[2] for event in recorder.events], ["display-a"] * 2 + ["display-b"] * 2 + ["display-a"] * 2)
            step(recorder, "tap-timeout")
            sleep.assert_called_with(0.8)
            with self.assertRaises(ValueError): step(recorder, "tap-unknown")

    def test_tap_report_requires_every_phase_and_explicit_gate(self):
        from validate_tap import EXPECTED, validate
        app = ''.join(f"PHASE_BEGIN {name}\n{self.tap_sample(name)}PHASE_END {name}\n"
                      for name in EXPECTED for _ in range(2 if name == "tap-positive" else 1))
        report = ("TAP_INTERFACE qualification_revision=2 feature_mask=3\n"
                  "TAP_TRANSITION_WITHIN_WINDOW tap-reset elapsed_ns=100000000\n"
                  "TAP_TRANSITION_WITHIN_WINDOW tap-focus elapsed_ns=100000000\n"
                  "SLEDGE_TAP_CONSUMER_MATRIX_COMPLETED_VM_ONLY\n"
                  "SLEDGE_PAYLOAD_REPORT_END status=0 qualified=false\n"
                  "EVIDENCE_FILE /tmp/candidate-consumer-events\n" + app)
        validate(report)
        for broken in (report.replace("feature_mask=3", "feature_mask=1"),
                       report.replace("PHASE_BEGIN tap-reset", "PHASE_BEGIN omitted")):
            with self.assertRaises(ValueError): validate(broken)
        with self.assertRaises(ValueError): validate(report, lifecycle=True)
        gates = "TAP_LOCK_AUTHORITY_ACK\nTAP_IME_AUTHORITY_ACTIVE\n"
        for name, prefix in (("tap-locked", "LOCK_KEY"), ("tap-ime", "IME_KEY")):
            gates += f"EVIDENCE_FILE /tmp/{name}-events\nPHASE_BEGIN {name}\n"
            ack = " ack=1" if prefix == "LOCK_KEY" else ""
            gates += ''.join(f"{prefix} code=100 state={state} time={index}{ack}\n"
                             for index, state in enumerate([1, 0] * 3))
            gates += f"PHASE_END {name}\n"
        validate(report + gates, lifecycle=True)
        with self.assertRaises(ValueError):
            validate(report + gates.replace("code=100", "code=28", 1), lifecycle=True)

    def test_qualified_startup_report_requires_real_replacement_and_passthrough(self):
        from validate_startup import validate
        report = '''SLEDGE_QUALIFIED_STARTUP_REPLACEMENT_DRAIN_COMPLETED_VM_ONLY
SLEDGE_PAYLOAD_REPORT_END status=0 qualified=false
EVIDENCE_FILE /tmp/qualified-startup
{"daemon_pid":123,"operational":true}
EVIDENCE_FILE /tmp/qualified-daemon-events
independently accepted installed Hyprland registration; awaiting native proof feature_mask=1
native reconciliation epoch=5 sequence=2 nonce=7 accepted=true
native delivery outcome generation=1 epoch=5 sequence=3 nonce=7 key=30 state=1 focus_kind=1 report=Acknowledged(Outcome { result: Replaced,
native delivery outcome generation=1 epoch=5 sequence=4 nonce=7 key=30 state=1 focus_kind=1 report=Acknowledged(Outcome { result: Replaced,
EVIDENCE_FILE /tmp/qualified-consumer-events
PHASE_BEGIN candidate-a
key: 56; state: 1
key: 56; state: 0
PHASE_END candidate-a
PHASE_BEGIN candidate-a
key: 56; state: 1
key: 56; state: 0
PHASE_END candidate-a
PHASE_BEGIN detached
key: 38; state: 1
key: 38; state: 0
PHASE_END detached
'''
        validate(report)
        for invalid in (report.replace('"operational":true', '"operational":false'),
                        report.replace('accepted=true', 'accepted=false'),
                        report.replace('key: 56', 'key: 38'),
                        report.replace('key: 38; state: 0', 'key: 56; state: 0')):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                validate(invalid)

    def test_released_reset_requires_one_ordered_consumer_stream(self):
        from validate_candidate import require_released_reset
        valid = ('MAP_RESET8_BEFORE_UP 2\nPHASE_BEGIN map-real-up\n'
                 'MAP_RESET8_BEFORE_B 5\nPHASE_BEGIN map-released-replacement\n')
        require_released_reset(valid)
        for invalid in ('', valid.replace('BEFORE_B 5', 'BEFORE_B 2'),
                        valid.replace('PHASE_BEGIN map-real-up', 'PHASE_END map-real-up'),
                        valid.replace('MAP_RESET8_BEFORE_B 5\n', '') + 'MAP_RESET8_BEFORE_B 5\n'):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                require_released_reset(invalid)
        source = Path(__file__).with_name('candidate-payload-run.sh').read_text()
        self.assertIn('echo "MAP_RESET8_BEFORE_UP $map_reset_before" | tee -a /tmp/candidate-consumer-events', source)
        self.assertIn('echo "MAP_RESET8_BEFORE_B $map_reset_after" | tee -a /tmp/candidate-consumer-events', source)

    def test_packaged_runner_preserves_helpers_and_guest_prerequisites(self):
        root = Path(__file__).resolve().parents[3]
        self.assertIn('SLEDGE_VM_RUNNER=${./scripts/linux/hyprland}/vm-run.sh',
                      (root / 'flake.nix').read_text())
        vm = Path(__file__).with_name('vm.nix').read_text()
        for required in ('pkgs.python3', 'pkgs.wev', '"dialout"', 'qemu-xhci,id=qualification-usb'):
            self.assertIn(required, vm)
        self.assertNotIn('qemu-xhci,id=qualification-usb',
                         (root / 'native/hyprland/stock-hook-investigation.nix').read_text())

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
        self.assertIn('if [[ -f /sledge-payload/hotplug-case ]]; then\n  transition_offset=$(wc -c < /tmp/candidate-daemon-events)\n  step capability-hotplug', source)
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

    def test_map_collector_rejects_drift_and_incomplete_digests(self):
        from collect_maps import collect
        def page(command):
            offset = int(command.split()[-1])
            return {"version": 1, "live": True, "offset": offset, "next_offset": offset + 1,
                    "done": offset == 15, "nonce": 1, "connection_generation": 2,
                    "ownership_epoch": 3, "selected_generation": 4, "generation": 4 if offset == 0 else 0,
                    "present": offset == 0, "selected": offset == 0, "binding_valid": offset == 0,
                    "name": "keyboard" if offset == 0 else "", "name_length_bytes": 8 if offset == 0 else 0,
                    "name_truncated": False, "stored_map": {"status": "complete" if offset == 0 else "absent",
                    "length_bytes": 10 if offset == 0 else 0, "sha256": 'a' * 64 if offset == 0 else None}}
        self.assertTrue(collect(lambda cmd: json.dumps(page(cmd)))['complete'])
        for field, value in [('nonce', 9), ('connection_generation', 9), ('ownership_epoch', 9), ('selected_generation', 9)]:
            def drift(command):
                item = page(command)
                if item['offset'] == 1: item[field] = value
                return json.dumps(item)
            self.assertFalse(collect(drift)['complete'])
        for raw in ('null', '{', 'x' * 8192):
            self.assertFalse(collect(lambda _: raw)['complete'])
        def missing(command):
            item = page(command); item['stored_map']['sha256'] = None
            return json.dumps(item)
        self.assertFalse(collect(missing)['complete'])

    def test_admission_requires_new_scoped_reset_and_native_focus(self):
        from admission_barrier import admitted
        status = {'native_operational': True, 'waiting_for_focus': False,
                  'connection_generation': 2, 'ownership_epoch': 3}
        page = {'version': 1, 'live': True, 'nonce': 7, 'connection_generation': 2,
                'ownership_epoch': 3, 'selected_generation': 4}
        focus = {'class': 'wev', 'xwayland': False}
        controls = 'Hyprland validated lifecycle control nonce=7 sequence=8 epoch=3 control=Reset(8)'
        self.assertTrue(admitted(status, page, controls, focus))
        self.assertFalse(admitted(status, page, '', focus))
        self.assertFalse(admitted(status, page, controls.replace('epoch=3', 'epoch=2'), focus))
        self.assertFalse(admitted(status, page, controls, {'class': 'wev', 'xwayland': True}))
        self.assertFalse(admitted(status, dict(page, connection_generation=9), controls, focus))

    def test_negative_validator_rejects_partial_and_changed_bytes(self):
        from validate_negative import validate
        target = {"kind": "prehook_bytes", "pid": 10, "starttime": "1",
                  "elf_sha256": __import__('prehook_bytes').ELF_SHA,
                  "entries": [{"symbol_value": i * 64, "address": 4096 + i * 64,
                               "bytes": '00' * 32} for i in range(19)]}
        lines = ['SLEDGE_NEGATIVE_ABI_COMPLETED_NOT_QUALIFIED']
        for name in ['wrong-sdk', 'wrong-elf']:
            lines += [f'NEGATIVE_ABI_LOAD name={name}', f'PREHOOK_BYTES_EQUAL name={name}',
                      'isolated candidate scope/ELF/SDK mismatch; hooks not installed',
                      json.dumps(target, separators=(',', ':')), json.dumps(target, separators=(',', ':')),
                      f'PHASE_BEGIN {name}', 'key: 38; state: 1', 'key: 38; state: 0', f'PHASE_END {name}']
        # Empty/partial evidence must never qualify, regardless of completion marker.
        with self.assertRaises(ValueError):
            validate('SLEDGE_NEGATIVE_ABI_COMPLETED_NOT_QUALIFIED')
        changed = '\n'.join(lines).replace('"bytes":"' + '00' * 32, '"bytes":"' + '11' * 32, 1)
        changed += '\n' + '\n'.join('a' * 64 + f'  /sledge-payload/{name}.so' for name in ['wrong-sdk', 'wrong-elf'])
        with self.assertRaisesRegex(ValueError, 'identity/bytes differ'):
            validate(changed, 'a' * 64, 'a' * 64)

    def test_prehook_broker_requires_guest_root(self):
        from prehook_bytes import serve
        with patch('prehook_bytes.os.geteuid', return_value=1000):
            with self.assertRaisesRegex(ValueError, 'guest root'):
                serve()

    def test_prehook_reader_refuses_non_guest_scope(self):
        from prehook_bytes import OFFSETS, snapshot
        self.assertEqual(len(OFFSETS), 10)
        self.assertEqual(len(set(OFFSETS)), 10)
        with patch.dict(os.environ, {"XDG_RUNTIME_DIR": "/not-the-guest"}):
            with self.assertRaisesRegex(ValueError, 'guest scope'):
                snapshot(1)

    def test_send_intervals_allow_receive_before_return_not_before_start(self):
        from validate_causal import PHASES, check_intervals
        times = [10, 20, 40, 60, 70, 80, 90, 100, 120]
        phases = {p: {"at_ns": t, "ok": True} for p, t in zip(PHASES, times)}
        stamps = (30, 45, 65, 110, 130)
        self.assertEqual(check_intervals(phases, stamps), [(0, 10), (0, 15), (0, 10)])
        for bad in ((19, 45, 65, 110, 130), (30, 61, 65, 110, 130),
                    (30, 45, 65, 99, 130), (0, 45, 65, 110, 130)):
            with self.assertRaises(ValueError):
                check_intervals(phases, bad)
        phases['event_send']['ok'] = False
        with self.assertRaises(ValueError):
            check_intervals(phases, stamps)

    def test_causal_collector_pins_cut_and_rejects_partial_pages(self):
        from collect_causal import collect
        def page(cursor, end=13):
            stop = min(cursor + 12, end)
            return {"version": 1, "unit": "ns", "capacity": 2048,
                    "snapshot_start": 0, "snapshot_end": end, "cursor": cursor,
                    "next_cursor": stop, "observed_end": end + 2, "overwritten": 0,
                    "dropped": 0, "done": stop == end, "page_complete": True,
                    "missing_ids": [], "records": [{"id": i} for i in range(cursor + 1, stop + 1)]}
        commands = []
        def invoke(command):
            commands.append(command)
            return json.dumps(page(0 if len(commands) == 1 else 12))
        result = collect(invoke)
        self.assertTrue(result['complete'])
        self.assertEqual(len(result['records']), 13)
        self.assertEqual(commands, ['sledge:causal-timing', 'sledge:causal-timing 0 13 12 12'])
        for field, value in [('missing_ids', [1]), ('dropped', 1), ('overwritten', 1),
                             ('done', True), ('next_cursor', 14), ('cursor', 1), ('version', 2)]:
            bad = page(0); bad[field] = value
            self.assertFalse(collect(lambda _: json.dumps(bad))['complete'])
        for raw in ('{', 'x' * 8192, 'null'):
            self.assertFalse(collect(lambda _: raw)['complete'])

    def test_causal_validation_rejects_missing_and_malformed_status(self):
        from validate_causal import validate_causal
        for text in ('', '{"version":1,', '{"version":1,"causal_timing":{}}'):
            with self.assertRaises(ValueError):
                validate_causal(text)
        ring = {"version": 1, "unit": "ns", "capacity": 2048, "attempted": 0,
                "overwritten": 0, "dropped": 1, "records": []}
        with self.assertRaisesRegex(ValueError, 'ring incomplete'):
            validate_causal(json.dumps({"version": 1, "causal_timing": ring}, separators=(',', ':')))
        ring['dropped'] = 0
        with self.assertRaisesRegex(ValueError, 'Rust causal records missing'):
            validate_causal(json.dumps({"version": 1, "causal_timing": ring}, separators=(',', ':')))

    def test_pipelined_input_has_exact_bounded_receipts(self):
        import io
        qmp = QMP.__new__(QMP)
        qmp.stream = io.BytesIO()
        receipts = iter([{"event": "RESET"}] + [{"id": i, "return": {}} for i in range(200)])
        qmp.receive = lambda: next(receipts)
        qmp.pipelined_pairs()
        requests = [json.loads(line) for line in qmp.stream.getvalue().splitlines()]
        self.assertEqual(len(requests), 200)
        self.assertEqual([r["id"] for r in requests], list(range(200)))
        self.assertEqual([r["arguments"]["events"][0]["data"]["down"] for r in requests], [True, False] * 100)
        with self.assertRaises(ValueError):
            qmp.pipelined_pairs(101)
        for receipt in ({"id": 999, "return": {}}, {"id": 0, "error": {}}, {"id": 0}):
            qmp.receive = lambda receipt=receipt: receipt
            with self.assertRaises(RuntimeError):
                qmp.pipelined_pairs()

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
        for branch in ('deadline', 'mixed-source', 'pipelined', 'different-map'):
            with self.assertRaisesRegex(ValueError, 'required .* missing or incorrect|map observations missing'):
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
