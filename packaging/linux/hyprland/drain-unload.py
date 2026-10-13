#!/usr/bin/env python3
"""Unload only after the native adapter commits irreversible passthrough.

This controller never loads plugins or treats a quiescent snapshot as a lease.
Direct administrative unload/forced eject remains outside this supported path.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time


def checked_command(hyprctl, instance, arguments, timeout):
    result = subprocess.run(
        [str(hyprctl), "--instance", instance, *arguments],
        check=True,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    return result.stdout


def detached_status(text):
    status = json.loads(text)
    if not isinstance(status, dict) or type(status.get("version")) is not int:
        raise ValueError("invalid native status schema")
    if status["version"] != 1:
        raise ValueError("unsupported native status version")
    for field in ("draining", "detached", "quiescent"):
        if type(status.get(field)) is not bool:
            raise ValueError(f"missing or invalid {field}")
    for field in ("connection_generation", "ownership_epoch"):
        if type(status.get(field)) is not int or status[field] < 0:
            raise ValueError(f"missing or invalid {field}")
    return status["draining"] and status["detached"]


def drain_unload(hyprctl, instance, plugin, timeout, command=checked_command):
    deadline = time.monotonic() + timeout

    def invoke(arguments):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("native adapter did not detach; plugin left loaded")
        return command(hyprctl, instance, arguments, min(remaining, 2.0))

    invoke(["dispatch", "sledge:drain"])
    while not detached_status(invoke(["sledge:status"])):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("native adapter did not detach; plugin left loaded")
        time.sleep(min(remaining, 0.05))
    # Detached is irreversible: subsequent physical input follows stock paths.
    # A plain quiescent=true snapshot is deliberately insufficient.
    invoke(["plugin", "unload", str(plugin)])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hyprctl", type=Path, required=True)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--plugin", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=10.0)
    args = parser.parse_args()
    if not args.hyprctl.is_absolute() or not args.plugin.is_absolute():
        parser.error("hyprctl and plugin paths must be absolute")
    if not args.instance or not 0 < args.timeout <= 60:
        parser.error("instance must be nonempty and timeout must be in (0, 60]")
    drain_unload(args.hyprctl, args.instance, args.plugin, args.timeout)


if __name__ == "__main__":
    main()
