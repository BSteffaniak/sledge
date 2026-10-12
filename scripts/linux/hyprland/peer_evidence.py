"""Fail-closed parsing of the published research peer's correlated evidence."""
import re

from validate_qualification import require


FIELDS = ("connection", "nonce", "generation", "epoch", "sequence", "deadline", "key", "state", "focus_epoch", "focus_kind")


def outcome_records(evidence):
    records = []
    for line in evidence.splitlines():
        if not line.startswith("outcome "):
            continue
        values = {}
        for field in FIELDS:
            matches = re.findall(rf"(?:^|\s){field}=(\d+)(?=\s|$)", line)
            require(len(matches) == 1, f"malformed outcome {field}")
            values[field] = int(matches[0])
        require(all(values[field] > 0 for field in ("nonce", "generation", "sequence", "deadline")),
                "invalid outcome transaction scope")
        require(" decision=" in line and " report=" in line, "missing outcome decision/report")
        values["decision"] = line.split(" decision=", 1)[1].split(" report=", 1)[0]
        values["report"] = line.split(" report=", 1)[1]
        values["line"] = line
        records.append(values)
    identities = [(r["connection"], r["nonce"], r["generation"], r["epoch"], r["sequence"]) for r in records]
    require(len(identities) == len(set(identities)), "duplicate correlated outcome")
    return records


def require_reconciliation(evidence, connection, nonce):
    accepted = re.search(rf"^reconciliation connection={connection} nonce={nonce} .*accepted=true$",
                         evidence, re.MULTILINE)
    require(accepted is not None, "missing authenticated accepted reconciliation")
    controls = [line for line in evidence.splitlines()
                if line.startswith(f"control connection={connection} ") and "body=Reset(8)" in line
                and f"nonce: {nonce}" in line]
    require(len(controls) == 1, "missing unique same-connection reason8")
    return accepted.start()


def require_failure_outcome(evidence, result):
    records = outcome_records(evidence)
    selected = [r for r in records if r["key"] == 30 and r["state"] == 1
                and r["decision"].startswith("Replace") and result in r["report"]]
    require(len(selected) == 1, f"missing unique selected Replace/{result} for A down")
    record = selected[0]
    position = require_reconciliation(evidence, record["connection"], record["nonce"])
    require(position < evidence.index(record["line"]), "reason8 did not precede failure transaction")
    return record
