"""Validate compositor prerequisites, never parity."""
import json
import sys


def validate(version, monitors, executable, expected):
    if executable != expected or version.get("version") != "0.52.2":
        raise ValueError("wrong runtime artifact/version")
    outputs = [m for m in monitors if m.get("name") == "Virtual-1" and not m.get("disabled", False) and m.get("width") == 640 and m.get("height") == 480 and m.get("scale") == 1 and m.get("dpmsStatus") is True]
    if len(outputs) != 1:
        raise ValueError("expected enabled 640x480 virtual output absent")


if __name__ == "__main__":
    validate(json.load(open(sys.argv[1])), json.load(open(sys.argv[2])), sys.argv[3], sys.argv[4])
    print("SLEDGE_GUEST_COMPOSITOR_PREREQUISITE_PASS_NOT_PARITY")
