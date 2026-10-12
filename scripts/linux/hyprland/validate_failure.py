"""Failure consumer observations: not route-outcome or production qualification."""
from pathlib import Path
import re
import sys

from peer_evidence import require_failure_outcome
from validate_qualification import evidence_files, phases, require
from validate_replacement import native_keys, x_keys


def validate(text, case):
    require(case in {"preflight", "mods", "down"}, "unknown failure case")
    require(f"FAILURE_OBSERVATION_COMPLETED {case}" in text, "failure fixture incomplete")
    files = evidence_files(text)
    peer = files.get("/tmp/replacement-peer-events", "")
    result = "RejectedOriginalAdmitted" if case == "preflight" else "PartialUncertainFatal"
    require_failure_outcome(peer, result)
    app = files.get("/tmp/stock-consumer-events", "")
    samples = phases(app, "replacement-extra")
    require(len(samples) == 1, "missing native failure input")
    expected = {
        "preflight": [("50", "1"), ("38", "1"), ("38", "0"), ("50", "0")],
        "mods": [("50", "1"), ("50", "0")],
        "down": [("50", "1"), ("56", "1"), ("56", "0"), ("50", "0")],
    }
    require(native_keys(samples[0]) == expected[case], f"{case}: missing output/ownership or original replay")
    masks = re.findall(r"depressed: ([0-9a-f]{8})", samples[0])
    required_masks = ["00000001", "00000000"] if case == "preflight" else ["00000001", "00000000", "00000001", "00000000"]
    require(masks == required_masks, "failure modifier restoration incorrect")
    recovery = phases(app, "failure-recovery")
    require(len(recovery) == 1 and native_keys(recovery[0]) == [
        ("54", "1"), ("54", "0"), ("50", "1"), ("40", "1"), ("40", "0"), ("50", "0")],
        "native recovery missing keys or stale original replay")
    require("sym: D " in recovery[0] and "sym: c " in recovery[0], "native recovery modifier semantics incorrect")
    xrecovery = phases(files.get("/tmp/replacement-xev-events", ""), "x-failure-recovery")
    require(len(xrecovery) == 1 and x_keys(xrecovery[0]) == [
        ("KeyPress", "0x0", "54", "c"), ("KeyRelease", "0x0", "54", "c"),
        ("KeyPress", "0x0", "50", "Shift_L"), ("KeyPress", "0x1", "40", "D"),
        ("KeyRelease", "0x1", "40", "D"), ("KeyRelease", "0x1", "50", "Shift_L")],
        "XWayland recovery missing keys/modifiers or stale original replay")
    print(f"PASS_{case.upper()}_CONSUMER_OBSERVATIONS_ONLY")
    print("UNQUALIFIED: fresh trigger reenable, timeout/reconnect/lock races")
    if case == "mods":
        print("REQUIRED: zero target key events after mods-only failure; no bare target up")


if __name__ == "__main__":
    validate(Path(sys.argv[1]).read_text(), sys.argv[2])
