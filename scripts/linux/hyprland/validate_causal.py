"""Strict bounded native/Rust timing joins; missing records are not completion."""
import json
import re

from validate_qualification import require

PHASES = ("event_enqueue", "event_send_start", "event_send", "decision_receive", "mailbox_publish",
          "route_complete", "outcome_enqueue", "outcome_send_start", "outcome_send")
KEYS = ("nonce", "epoch", "sequence", "generation", "deadline_ns")


def check_intervals(phases, stamps):
    received, decision_start, decision_end, outcome, terminal = stamps
    native = [phases[p]["at_ns"] for p in PHASES]
    require(all(type(value) is int and value > 0 for value in native + list(stamps)),
            "causal clock/timestamp missing")
    require(all(phases[p]["ok"] is True for p in PHASES), "native causal phase failure")
    require(native == sorted(native) and received <= decision_start <= decision_end <= outcome <= terminal and
            phases["event_send_start"]["at_ns"] <= received and
            decision_start <= phases["decision_receive"]["at_ns"] and
            phases["outcome_send_start"]["at_ns"] <= outcome,
            "causal clock/order invalid")
    # Post-syscall timestamps can follow receipt on another CPU. Transit is an
    # interval, never receiver minus send-return asserted as an exact duration.
    return [(max(0, receiver - end), receiver - start) for start, end, receiver in (
        (phases["event_send_start"]["at_ns"], phases["event_send"]["at_ns"], received),
        (decision_start, decision_end, phases["decision_receive"]["at_ns"]),
        (phases["outcome_send_start"]["at_ns"], phases["outcome_send"]["at_ns"], outcome))]


def validate_causal(text):
    import base64
    import zlib
    expanded = []
    for line in text.splitlines():
        if line.startswith("CAUSAL_COLLECTION_GZIP "):
            encoded = line.split(" ", 1)[1]
            require(len(encoded) <= 2000000, "encoded causal evidence oversized")
            decoder = zlib.decompressobj(31)
            raw = decoder.decompress(base64.b64decode(encoded, validate=True), 4000000)
            require(decoder.eof and not decoder.unconsumed_tail and not decoder.unused_data,
                    "compressed causal evidence incomplete/oversized")
            expanded.append(raw.decode())
        else:
            expanded.append(line)
    text = "\n".join(expanded)
    snapshots = []
    for line in text.splitlines():
        if line.startswith('{"version"'):
            try:
                snapshots.append(json.loads(line))
            except ValueError as error:
                raise ValueError("native status truncated/malformed; timing incomplete") from error
    require(bool(snapshots), "native timing status missing")
    ring = snapshots[-1].get("causal_timing", {})
    collections = []
    for line in text.splitlines():
        if line.startswith('{"kind":"causal_collection"'):
            collection = json.loads(line)
            require(collection.get("complete") is True, "paged causal collection partial")
            from collect_causal import collect
            pages = iter(collection.get("pages", []))
            try:
                verified = collect(lambda _: json.dumps(next(pages)))
            except StopIteration as error:
                raise ValueError("paged causal pages missing") from error
            require(verified.get("complete") is True and verified["records"] == collection.get("records"),
                    "paged causal cut/records unverified")
            collections.append(verified)
    if collections:
        collection = collections[-1]
        ring = {"version": 1, "unit": "ns", "capacity": 2048, "dropped": 0, "overwritten": 0,
                "attempted": collection["snapshot_end"] - collection["snapshot_start"],
                "records": collection["records"]}
    require(ring.get("version") == 1 and ring.get("capacity") == 2048 and ring.get("unit") == "ns",
            "causal ring schema missing")
    require(ring.get("dropped") == 0 and ring.get("overwritten") == 0, "causal ring incomplete")
    records = ring.get("records", [])
    require(len(records) == ring.get("attempted"), "native causal records missing")
    groups = {}
    for record in records:
        key = tuple(record[k] for k in KEYS)
        phases = groups.setdefault(key, {})
        require(record["phase"] not in phases, "duplicate native causal phase")
        phases[record["phase"]] = record
    joined = 0
    seen = set()
    margins = []
    for line in text.splitlines():
        if "Hyprland causal transaction" not in line or "terminal=Acknowledged" not in line:
            continue
        identity = [re.search(rf"\b{k}=(\d+)", line) for k in KEYS]
        require(all(identity), "Rust causal identity missing")
        key = tuple(int(value[1]) for value in identity)
        if key in seen:
            continue
        seen.add(key)
        phases = groups.get(key, {})
        require(all(p in phases for p in PHASES), "native/Rust join incomplete")
        stamps = [re.search(rf"\b{k}=Some\((\d+)\)", line) for k in
                  ("received_ns", "decision_send_start_ns", "decision_sent_ns", "outcome_received_ns", "terminal_ns")]
        require(all(stamps), "Rust clock/timestamp missing")
        values = tuple(int(s[1]) for s in stamps)
        check_intervals(phases, values)
        outcome = values[3]
        require(outcome <= key[-1], "acknowledged outcome exceeded deadline")
        margins.append(key[-1] - outcome)
        joined += 1
    require(joined > 0, "acknowledged Rust causal records missing")
    return {"joined": joined, "minimum_ack_margin_ns": min(margins)}
