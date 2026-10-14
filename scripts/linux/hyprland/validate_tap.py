"""Independent wev oracle for tap-v1-r1; never accepts native ACKs as delivery."""
from pathlib import Path
import re
import sys

from validate_qualification import evidence_files, phases, require

ALT = [("108", "1"), ("108", "0")]
ACTION = [("37", "1"), ("36", "1"), ("36", "0"), ("37", "0")]
SENTINEL = [("56", "1"), ("56", "0")]
EXPECTED = {
    "tap-baseline": ALT * 3,
    "tap-positive": ALT * 3 + ACTION,
    "tap-chord": ALT + [("108", "1"), ("26", "1"), ("26", "0"), ("108", "0")] + ALT,
    "tap-hold-down": [("108", "1")],
    "tap-hold-up": [("108", "0")],
    "tap-timeout": ALT * 3,
    "tap-modifier": [("50", "1")] + ALT * 3 + [("50", "0")],
    "tap-device": ALT * 3,
    "tap-focus": ALT * 3,
    "tap-reset": ALT * 3,
}


def keys(sample):
    return re.findall(r"key: (\d+); state: ([01])", sample)


def masks(sample):
    return re.findall(r"depressed: ([0-9a-fA-F]{8}).*?latched: ([0-9a-fA-F]{8}).*?locked: ([0-9a-fA-F]{8})", sample, re.DOTALL)


def validate_phase(sample, name):
    expected = EXPECTED[name] if name == "tap-hold-down" else EXPECTED[name] + SENTINEL
    require(keys(sample) == expected,
            f"{name}: physical releases/action/sentinel order or balance incorrect")
    # Look at modifier snapshots surrounding the action, not an unrelated zero
    # elsewhere in the trace. Under the explicit US fixture Ctrl is bit 2.
    if name == "tap-positive":
        release = list(re.finditer(r"key: 108; state: 0", sample))[-1]
        ctrl_down = sample.index("key: 37; state: 1")
        enter_down = sample.index("key: 36; state: 1")
        require(ctrl_down > release.end(), "action preceded final physical release")
        before_enter = masks(sample[ctrl_down:enter_down])
        require(before_enter and before_enter[-1] == ("00000004", "00000000", "00000000"),
                "Ctrl+Enter lacks isolated Ctrl-only post-release projection")
    if name == "tap-hold-down":
        return  # The held physical Alt is intentional; no action or sentinel.
    sentinel = sample.index("key: 56; state: 1")
    before = masks(sample[:sentinel])
    require(before and before[-1] == ("00000000", "00000000", "00000000"),
            f"{name}: stuck/latched/locked modifier before sentinel")


def validate(text, lifecycle=False):
    require("TAP_INTERFACE qualification_revision=2 feature_mask=3" in text,
            "missing explicit tap-v1-r1 artifact interface label")
    require(re.search(r"^SLEDGE_PAYLOAD_REPORT_END status=0 qualified=(?:true|false)$", text, re.MULTILINE),
            "missing successful outer payload settlement")
    require("SLEDGE_TAP_CONSUMER_MATRIX_COMPLETED_VM_ONLY" in text, "tap matrix incomplete")
    files = evidence_files(text)
    streams = [files[path] for path in ("/tmp/candidate-consumer-events", "/tmp/qualified-consumer-events") if path in files]
    require(len(streams) == 1, "need exactly one independent native consumer stream")
    app = streams[0]
    required = list(EXPECTED)
    for name in required:
        samples = phases(app, name)
        require(len(samples) == (2 if name == "tap-positive" else 1), f"missing/duplicate {name}")
        for sample in samples:
            validate_phase(sample, name)
    for transition in ("tap-reset", "tap-focus"):
        times = re.findall(rf"^TAP_TRANSITION_WITHIN_WINDOW {transition} elapsed_ns=(\d+)$", text, re.MULTILINE)
        require(len(times) == 1 and 0 < int(times[0]) < 520000000,
                f"{transition}: no bounded sub-window transition proof (timeout is not reset proof)")
    if lifecycle:
        for name, prefix in (("tap-locked", "LOCK_KEY"), ("tap-ime", "IME_KEY")):
            samples = phases(files.get(f"/tmp/{name}-events", ""), name)
            require(len(samples) == 1, f"missing independent {name} recipient trace")
            suffix = r" time=\d+ ack=1$" if prefix == "LOCK_KEY" else r" time=\d+$"
            delivered = re.findall(rf"^{prefix} code=(\d+) state=([01]){suffix}", samples[0], re.MULTILINE)
            require(delivered == [("100", "1"), ("100", "0")] * 3,
                    f"{name}: expected ordinary RightAlt only, no synthetic action")
            require(not phases(app, name) or all(not keys(s) for s in phases(app, name)),
                    f"{name}: application received excluded input")
        require("TAP_LOCK_AUTHORITY_ACK" in text and "TAP_IME_AUTHORITY_ACTIVE" in text,
                "missing authoritative lock/IME fixture activation")
    print("PASS_TAP_INDEPENDENT_CONSUMER_VM_ONLY" if lifecycle else
          "PASS_TAP_KEY_MATRIX_ONLY_NOT_LIFECYCLE_QUALIFICATION")


if __name__ == "__main__":
    validate(Path(sys.argv[1]).read_text(), lifecycle="--lifecycle" in sys.argv[2:])
