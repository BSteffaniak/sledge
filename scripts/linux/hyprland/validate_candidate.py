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
    files = evidence_files(text)
    peer = files.get("/tmp/candidate-daemon-events", "")
    require("qualification=Candidate feature_mask=1" in peer, "candidate admission absent")
    reconciled = re.findall(r"native reconciliation epoch=(\d+) sequence=(\d+) nonce=(\d+) accepted=true", peer)
    replaced = re.findall(r"native delivery outcome generation=\d+ epoch=(\d+) sequence=(\d+) nonce=(\d+) key=30 state=1 focus_kind=1 report=Acknowledged\(Outcome \{ result: Replaced,", peer)
    require(len(replaced) == 2, "two acknowledged replaced A-down outcomes required")
    for epoch, seq, nonce in replaced:
        require(any(e == epoch and n == nonce and int(s) < int(seq) for e, s, n in reconciled),
                "replacement lacks preceding same-scope authenticated reconciliation")
    samples = phases(files.get("/tmp/candidate-consumer-events", ""), "candidate-a")
    require(len(samples) == 2 and all(native_keys(s) == [("56", "1"), ("56", "0")] for s in samples),
            "consumer requires balanced B pairs, no original A")
    print("PASS_INSTALLED_ISOLATED_CANDIDATE_BASELINE_NOT_HOST_QUALIFIED")


if __name__ == "__main__":
    validate(Path(sys.argv[1]).read_text())
