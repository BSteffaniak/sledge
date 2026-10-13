"""Collect an exact bounded causal cut through unmodified stock hyprctl."""
import json
import subprocess
import sys


def collect(invoke):
    pages = []
    records = []
    cut = None
    cursor = None
    try:
        command = "sledge:causal-timing"
        for _ in range(172):
            raw = invoke(command)
            if len(raw.encode()) >= 8192:
                raise ValueError("causal page exceeds transport bound")
            page = json.loads(raw)
            if not isinstance(page, dict) or page.get("version") != 1 or page.get("unit") != "ns" or page.get("capacity") != 2048:
                raise ValueError("invalid causal page schema")
            for field in ("snapshot_start", "snapshot_end", "cursor", "next_cursor", "observed_end", "overwritten", "dropped"):
                if type(page.get(field)) is not int or page[field] < 0:
                    raise ValueError(f"invalid {field}")
            for field in ("done", "page_complete"):
                if type(page.get(field)) is not bool:
                    raise ValueError(f"invalid {field}")
            start, end = page["snapshot_start"], page["snapshot_end"]
            if cut is None:
                cut = (start, end)
                cursor = start
            if cut != (start, end) or not start <= cursor <= page["next_cursor"] <= end <= page["observed_end"] or end - start > 2048:
                raise ValueError("causal cut/cursor changed or out of bounds")
            if page["cursor"] != cursor or page["next_cursor"] - cursor > 12:
                raise ValueError("causal page skips cursor")
            if page["done"] != (page["next_cursor"] == end):
                raise ValueError("invalid causal completion")
            pages.append(page)
            if not isinstance(page.get("records"), list) or not isinstance(page.get("missing_ids"), list):
                raise ValueError("causal records/missing IDs absent")
            ids = [r.get("id") for r in page["records"] if isinstance(r, dict)]
            if ids != list(range(cursor + 1, page["next_cursor"] + 1)) or page["missing_ids"] or not page["page_complete"]:
                raise ValueError("causal page incomplete")
            records.extend(page["records"])
            if page["dropped"] or page["overwritten"]:
                raise ValueError("causal history dropped/overwritten; full-run proof unavailable")
            cursor = page["next_cursor"]
            if page["done"]:
                return {"kind": "causal_collection", "complete": True, "snapshot_start": start,
                        "snapshot_end": end, "pages": pages, "records": records}
            if cursor == page["cursor"]:
                raise ValueError("causal cursor stalled")
            command = f"sledge:causal-timing {start} {end} {cursor} 12"
        raise ValueError("causal page count exceeded")
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        return {"kind": "causal_collection", "complete": False, "error": str(error),
                "cut": cut, "pages": pages, "records": records}


if __name__ == "__main__":
    def invoke(command):
        return subprocess.check_output([sys.argv[1], command], text=True, timeout=2)
    result = collect(invoke)
    import base64
    import gzip
    encoded = base64.b64encode(gzip.compress(json.dumps(result, separators=(",", ":")).encode())).decode()
    print("CAUSAL_COLLECTION_GZIP " + encoded)
    sys.exit(0 if result["complete"] else 1)
