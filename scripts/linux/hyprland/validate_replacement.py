"""Independent pair/mask checks; route acknowledgments alone never qualify input."""
from pathlib import Path
import re
import sys

from validate_qualification import evidence_files, phases, require


def native_keys(sample):
    return re.findall(r"key: (\d+); state: ([01])", sample)


def x_keys(sample):
    return re.findall(r"(KeyPress|KeyRelease) event.*?state (0x[0-9a-f]+), keycode (\d+) \(keysym [^,]+, ([^)]+)\)",
                      sample, re.DOTALL)


def validate(text):
    require("SLEDGE_REPLACEMENT_OBSERVATION_COMPLETED_NOT_QUALIFICATION" in text, "fixture incomplete")
    files = evidence_files(text)
    app = files.get("/tmp/stock-consumer-events", "")
    xapp = files.get("/tmp/replacement-xev-events", "")
    baseline = phases(app, "baseline")
    require(len(baseline) == 1 and len(native_keys(baseline[0])) == 18 and
            "sym: B " in baseline[0] and "sym: C " in baseline[0], "native baseline missing")
    xbaseline = phases(xapp, "x-baseline")
    require(len(xbaseline) == 1 and len(x_keys(xbaseline[0])) == 18 and
            "keysym 0x42, B" in xbaseline[0] and "keysym 0x43, C" in xbaseline[0], "XWayland baseline missing")
    native = phases(app, "replacement-a")
    require(len(native) == 2, "need two fresh native replacement attempts")
    for sample in native:
        require(native_keys(sample) == [("56", "1"), ("56", "0")], "native fresh A->B pair not isolated")
        require("depressed: 00000000" in sample and "sym: b " in sample, "native pair masks missing")
    xsamples = phases(xapp, "x-replacement-a")
    require(len(xsamples) == 2, "need two fresh XWayland replacement attempts")
    for sample in xsamples:
        require(x_keys(sample) == [("KeyPress", "0x0", "56", "b"), ("KeyRelease", "0x0", "56", "b")],
                "XWayland fresh A->B pair not isolated")
    extra = phases(app, "replacement-extra")
    require(len(extra) == 1 and native_keys(extra[0]) == [("50", "1"), ("56", "1"), ("56", "0"), ("50", "0")],
            "native held-extra replacement keys incorrect")
    # Modifier blocks must show held -> isolated zero -> restored held -> physical zero.
    masks = re.findall(r"depressed: ([0-9a-f]{8})", extra[0])
    require(masks == ["00000001", "00000000", "00000001", "00000000"], "native modifier isolation/restoration missing")
    xextra = phases(xapp, "x-replacement-extra")
    require(len(xextra) == 1 and x_keys(xextra[0]) == [
        ("KeyPress", "0x0", "50", "Shift_L"), ("KeyPress", "0x0", "56", "b"),
        ("KeyRelease", "0x0", "56", "b"), ("KeyRelease", "0x1", "50", "Shift_L")],
        "XWayland held-extra masks incorrect")
    target = phases(app, "replacement-target-held")
    require(len(target) == 1 and native_keys(target[0]) == [("56", "1"), ("38", "1"), ("38", "0"), ("56", "0")],
            "native target-held rejection did not preserve originals")
    xtarget = phases(xapp, "x-replacement-target-held")
    require(len(xtarget) == 1 and x_keys(xtarget[0]) == [
        ("KeyPress", "0x0", "56", "b"), ("KeyPress", "0x0", "38", "a"),
        ("KeyRelease", "0x0", "38", "a"), ("KeyRelease", "0x0", "56", "b")],
        "XWayland target-held rejection incorrect")
    peer = files.get("/tmp/replacement-peer-events", "")
    require(re.search(r"research proposals=12 acknowledgments=24 uncertain=0 qualified=false", peer),
            "missing actual peer lifecycle acknowledgments")
    print("PASS_TWO_FRESH_NATIVE_AND_XWAYLAND_PAIRS_MASKS_REJECTION_NOT_CAPTURE")
    print("PEER: 12 proposals, 24 acknowledgments, zero uncertain; disconnect on unload is not recovery qualification")
    print("GAPS: failure/timeout/disconnect/lock proposal; repeat/unknown keys; layouts/merged devices; production capture")


if __name__ == "__main__":
    validate(Path(sys.argv[1]).read_text())
