"""Admit only the Nix-generated runner's exact immutable executable path."""
from pathlib import Path
import hashlib
import os
import re
import sys


def admit(candidate, expected):
    if not re.fullmatch(r"/nix/store/[a-z0-9]{32}-[^/]+/bin/run-sledge-qualification-vm", expected):
        raise ValueError("expected runner is not a pinned Nix store executable")
    path = Path(candidate)
    if candidate != expected:
        raise ValueError("runner executable identity differs from pinned artifact")
    resolved = path.resolve()
    if not re.fullmatch(r"/nix/store/[a-z0-9]{32}-[^/]+", str(resolved)) and str(resolved) != expected:
        raise ValueError("runner target is not an immutable Nix store artifact")
    if not path.is_file() or not os.access(path, os.X_OK):
        raise ValueError("pinned runner executable missing")
    return hashlib.sha256(path.read_bytes()).hexdigest()


if __name__ == "__main__":
    print("VM_RUNNER_IDENTITY", sys.argv[1], admit(sys.argv[1], sys.argv[2]))
