"""Installed isolated candidate baseline checks; never host qualification."""
from pathlib import Path
import re
import sys
from validate_qualification import evidence_files, phases, require
from validate_replacement import native_keys


def validate(text):
    text = re.sub(r"\x1b\[[0-9;]*m", "", text)
    require("SLEDGE_CANDIDATE_SCOPE_PRESERVED" in text, "scope not preserved")
    require("SLEDGE_CANDIDATE_STORE_TRANSFER_VERIFIED" in text, "store transfer not verified")
    require("SLEDGE_INSTALLED_CANDIDATE_COMPLETED_QUALIFIED_FALSE" in text,
            "candidate fixture incomplete")
    require("SLEDGE_PAYLOAD_REPORT_END status=0 qualified=false" in text,
            "guest report unsuccessful")
    require('"hyprland_candidate_configured":true' in text and
            '"hyprland_capture_qualified":false' in text and
            '"hyprland_research_configured":false' in text, "incorrect candidate health labels")
    if "REQUIRED_CASE identity" in text:
        for probe in ("wrong-pid", "plugin_sha256", "build_id", "sdk_api_hash", "plugin_path"):
            require(f"IDENTITY_REJECT probe={probe} exit=1" in text,
                    f"identity rejection missing: {probe}")
    if "REQUIRED_CASE causal" in text:
        from validate_causal import validate_causal
        validate_causal(text)
    if "REQUIRED_CASE different-map" in text:
        from collect_maps import collect
        collections = [json.loads(line) for line in text.splitlines() if line.startswith('{"kind":"map_collection"')]
        require(len(collections) == 3, "different/restored/entered map observations missing")
        reset_before = re.search(r'MAP_RESET8_BEFORE_UP (\d+)', text)
        reset_after = re.search(r'MAP_RESET8_BEFORE_B (\d+)', text)
        require(reset_before and reset_after and int(reset_after[1]) > int(reset_before[1]) and
                text.index(reset_before[0]) < text.index('PHASE_BEGIN map-real-up') < text.index(reset_after[0]) < text.index('PHASE_BEGIN map-released-replacement'),
                'released Reset8 proof before replacement missing')
        hashes = []
        for collection in collections:
            pages = iter(collection.get('pages', []))
            try:
                verified = collect(lambda _: json.dumps(next(pages)))
            except StopIteration as error:
                raise ValueError('map observation pages missing') from error
            require(collection.get('complete') is True and verified['complete'], 'map observations partial')
            devices = {p['name']: p for p in verified['pages'] if p['present']}
            require(all(name in devices for name in ('qemu-qemu-usb-keyboard', 'qemu-virtio-keyboard-1')),
                    'targeted device map identities missing')
            hashes.append(tuple(devices[name]['stored_map']['sha256'] for name in
                                ('qemu-qemu-usb-keyboard', 'qemu-virtio-keyboard-1')))
        require(hashes[0][0] != hashes[0][1] and hashes[1][0] == hashes[1][1] and hashes[2][0] == hashes[2][1],
                'serialized maps must differ then restore compatible bytes')
    files = evidence_files(text)
    peer = files.get("/tmp/candidate-daemon-events", "")
    require("qualification=Candidate feature_mask=1" in peer, "candidate admission absent")
    reconciled = re.findall(r"native reconciliation epoch=(\d+) sequence=(\d+) nonce=(\d+) accepted=true", peer)
    replaced = re.findall(r"native delivery outcome generation=\d+ epoch=(\d+) sequence=(\d+) nonce=(\d+) key=30 state=1 focus_kind=1 report=Acknowledged\(Outcome \{ result: Replaced,", peer)
    require(len(replaced) >= 2, "two acknowledged replaced A-down outcomes required")
    for epoch, seq, nonce in replaced:
        require(any(e == epoch and n == nonce and int(s) < int(seq) for e, s, n in reconciled),
                "replacement lacks preceding same-scope authenticated reconciliation")
    samples = phases(files.get("/tmp/candidate-consumer-events", ""), "candidate-a")
    require(len(samples) == 2 and all(native_keys(s) == [("56", "1"), ("56", "0")] for s in samples),
            "consumer requires balanced B pairs, no original A")
    for label, expected in (
        ("pipelined-100", [("56", "1"), ("56", "0")] * 100),
        ("stress-100", [("56", "1"), ("56", "0")] * 100),
        ("deadline-expired", [("38", "1"), ("38", "0")]),
        ("deadline-recovered", [("56", "1"), ("56", "0")]),
        ("pending-stop", [("38", "1"), ("38", "0")]),
        ("map-held-alt", [("64", "1")]),
        ("map-held-original", [("38", "1"), ("38", "0")]),
        ("map-held-layout-probe", [("29", "1"), ("29", "0")]),
        ("map-real-up", [("64", "0")]),
        ("map-released-replacement", [("56", "1"), ("56", "0")]),
        ("mixed-alt-down", [("64", "1")]),
        ("mixed-replacement", [("56", "1"), ("56", "0")]),
        ("mixed-alt-up", [("64", "0")]),
    ):
        sample = phases(files.get("/tmp/candidate-consumer-events", ""), label)
        required = ("REQUIRED_CASE deadline" in text and label.startswith(("stress", "deadline", "pending"))) or (
            "REQUIRED_CASE mixed-source" in text and label.startswith("mixed")) or (
            "REQUIRED_CASE pipelined" in text and label == "pipelined-100") or (
            "REQUIRED_CASE different-map" in text and label.startswith("map-"))
        if sample or required:
            require(len(sample) == 1 and native_keys(sample[0]) == expected,
                    f"required {label} balanced output missing or incorrect")
        if label == "map-held-layout-probe" and sample:
            require("sym: z" in sample[0], "German map behavioral probe missing")
        if label == "mixed-replacement" and sample:
            require(re.findall(r"depressed: ([0-9a-f]{8})", sample[0]) == ["00000000", "00000008"],
                    "mixed replacement must clear then restore physical Alt")
    if phases(files.get("/tmp/candidate-consumer-events", ""), "held-drain"):
        app = files["/tmp/candidate-consumer-events"]
        for name, keys in (
            ("alt-held", [("64", "1"), ("56", "1"), ("56", "0"), ("64", "0")]),
            ("scope-native_fixture", [("56", "1"), ("56", "0")]),
            ("scope-wrong_fixture", [("38", "1"), ("38", "0")]),
            ("valid-reload", [("54", "1"), ("54", "0")]),
            ("invalid-reload", [("54", "1"), ("54", "0")]),
            ("held-drain", [("56", "1"), ("56", "0")] if phases(app, "daemon-restart") else [("54", "1"), ("54", "0")]),
        ):
            sample = phases(app, name)
            require(len(sample) == 1 and native_keys(sample[0]) == keys, f"matrix {name} output incorrect")
        require("SIGHUP reload failed" in peer, "invalid reload failure missing")
        if phases(app, "after-unlock"):
            locker = files.get("/tmp/candidate-lock-events", "")
            require("LOCK_ACK" in locker and "LOCK_UNLOCK_ROUNDTRIP" in locker and
                    re.findall(r"LOCK_KEY code=(\d+) state=(\d+).*ack=(\d+)", locker) ==
                    [("30", "1", "1"), ("30", "0", "1")],
                    "real acknowledged locker must receive balanced original A only")
        for name in ("after-unlock", "two-keyboards", "keyboard-removed", "held-repeat", "layout-de", "layout-us"):
            sample = phases(app, name)
            if sample:
                require(len(sample) == 1 and native_keys(sample[0]) == [("56", "1"), ("56", "0")],
                        f"{name} requires balanced replacement B, not original A")
        restarted = phases(app, "daemon-restart")
        if restarted:
            require(len(restarted) == 1 and native_keys(restarted[0]) == [("56", "1"), ("56", "0")],
                    "new daemon natural-enter replacement incorrect")
            require(len({nonce for _, _, nonce in reconciled}) >= 2,
                    "restart lacks authenticated proof from distinct daemon connection")
        detached = phases(app, "detached-unload")
        require(len(detached) == 1 and native_keys(detached[0]) == [("38", "1"), ("38", "0")],
                "detached held unload must preserve balanced stock A")
        xevents = files.get("/tmp/candidate-x-events", "")
        for name, code in (("x-x_fixture", "56"), ("x-wrong_fixture", "38")):
            sample = phases(xevents, name)
            require(len(sample) == 1 and re.findall(r"keycode (\d+)", sample[0]) == [code, code] and
                    re.findall(r"(KeyPress|KeyRelease) event", sample[0]) == ["KeyPress", "KeyRelease"],
                    f"typed X11 {name} balanced output incorrect")
        require('"draining":true' in text and '"detached":false' in text, "held drain gate missing")
        if phases(app, "caps-on"):
            for name, keys, masks in (
                ("caps-on", [("66", "1"), ("66", "0")], ["00000002", "00000002"]),
                ("caps-replacement", [("56", "1"), ("56", "0")], ["00000000", "00000002"]),
                ("caps-off", [("66", "1"), ("66", "0")], ["00000002", "00000000"]),
            ):
                sample = phases(app, name)
                require(len(sample) == 1 and native_keys(sample[0]) == keys and
                        re.findall(r"locked: ([0-9a-f]{8})", sample[0]) == masks,
                        f"{name} genuine Caps state not restored")
    print("PASS_INSTALLED_ISOLATED_CANDIDATE_BASELINE_NOT_HOST_QUALIFIED")


if __name__ == "__main__":
    validate(Path(sys.argv[1]).read_text())
