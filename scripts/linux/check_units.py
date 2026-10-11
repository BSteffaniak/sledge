#!/usr/bin/env python3
"""Verify unit syntax in an isolated disposable root; never execute services.

Temporary executable fixtures satisfy systemd's path existence checks only.
They do not validate the real binaries or host target integration.
"""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    analyzer = shutil.which("systemd-analyze")
    if analyzer is None:
        print("SKIP: systemd-analyze unavailable")
        return 77
    with tempfile.TemporaryDirectory(prefix="sledge-unit-check-") as directory:
        image = Path(directory)
        units = image / "usr/lib/systemd/system"
        units.mkdir(parents=True)
        for source in (ROOT / "packaging/linux/systemd").glob("*"):
            shutil.copyfile(source, units / source.name)
        # Minimal inert dependency fixtures, not shipped installation assets.
        for name in (
            "sysinit.target",
            "basic.target",
            "shutdown.target",
            "sockets.target",
            "graphical-session.target",
            "systemd-logind.service",
            "dbus.service",
        ):
            content = "[Unit]\nDescription=Offline verification fixture\nDefaultDependencies=no\n"
            if name.endswith(".service"):
                content += "[Service]\nType=oneshot\nExecStart=/usr/bin/sledge\n"
            (units / name).write_text(content)
        for name in ("usr/bin/sledge", "usr/libexec/sledge-linux-broker"):
            binary = image / name
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_text("#!/bin/sh\nexit 1\n")
            binary.chmod(0o755)
        command = [
            analyzer,
            "--root=" + directory,
            "--man=no",
            "verify",
            str(units / "sledge-broker.socket"),
            str(units / "sledge-broker.service"),
            str(units / "sledge.service"),
        ]
        print(
            "Checking unit syntax with temporary executable/target fixtures; no services run.",
            flush=True,
        )
        return subprocess.run(
            command,
            check=False,
            env={
                **os.environ,
                "SYSTEMD_COLORS": "0",
                "SYSTEMD_UNIT_PATH": "/usr/lib/systemd/system",
            },
        ).returncode


if __name__ == "__main__":
    sys.exit(main())
