"""Strict VM-only candidate scope and exact store-path transfer validation."""
from pathlib import Path
import re

MACHINE_ID = b"736c656467652d63616e646964617465\n"
BOOT_MARKER = b"sledge-hyprland-isolated-candidate-v1\n"


def candidate_scope(root):
    root = Path(root)
    scope = root / "scope"
    if not scope.exists():
        if (root / "store").exists():
            raise ValueError("store transfer requires candidate scope")
        return False
    machine_id = (scope / "machine-id").read_bytes()
    if not re.fullmatch(rb"[0-9a-f]{32}\n", machine_id) or machine_id == b"0" * 32 + b"\n":
        raise ValueError("candidate machine-id requires 32 nonzero lowercase hex characters plus newline")
    if machine_id != MACHINE_ID or (scope / "boot-marker").read_bytes() != BOOT_MARKER:
        raise ValueError("candidate scope bytes do not match approved VM pin")
    if {p.name for p in scope.iterdir()} != {"machine-id", "boot-marker"}:
        raise ValueError("undeclared candidate scope input")
    store = root / "store"
    if not store.is_dir():
        raise ValueError("candidate requires explicit store files")
    for entry in store.iterdir():
        if not entry.is_dir() or not re.fullmatch(r"[a-z0-9]{32}-[A-Za-z0-9+._-]+", entry.name):
            raise ValueError("invalid exact candidate store path")
    return True
