#!/usr/bin/env python3
"""Create an offline Linux install image; never activate services or alter devices."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "packaging/linux/systemd"


def entries(binaries):
    return [
        (binaries / "sledge", "usr/bin/sledge", 0o755),
        (binaries / "sledge-linux-broker", "usr/libexec/sledge-linux-broker", 0o755),
        (
            ASSETS / "sledge-broker.service",
            "usr/lib/systemd/system/sledge-broker.service",
            0o644,
        ),
        (
            ASSETS / "sledge-broker.socket",
            "usr/lib/systemd/system/sledge-broker.socket",
            0o644,
        ),
        (ASSETS / "sledge.service", "usr/lib/systemd/user/sledge.service", 0o644),
        (
            ROOT / "docs/linux-installation.md",
            "usr/share/doc/sledge/linux-installation.md",
            0o644,
        ),
        (
            ROOT / "docs/linux-compatibility.md",
            "usr/share/doc/sledge/linux-compatibility.md",
            0o644,
        ),
    ]


def check_source(source):
    if source.is_symlink() or not source.is_file():
        raise ValueError(f"source must be a regular non-symlink file: {source}")
    return hashlib.sha256(source.read_bytes()).hexdigest()


def stage_image(destination, manifest):
    # Refuse overwrite, including an existing empty directory. The caller selects
    # a new child of a trusted staging parent, never a live filesystem root.
    if (
        not destination.is_absolute()
        or destination.parent.resolve() != destination.parent
    ):
        raise ValueError("stage must be absolute and have a canonical existing parent")
    if not destination.parent.is_dir():
        raise ValueError("stage parent must exist")
    if destination == Path("/") or os.path.lexists(destination):
        raise ValueError("stage must be a new offline directory; refusing overwrite")
    destination.mkdir(mode=0o755)
    for entry in manifest:
        target = destination / entry["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(entry["source"], target)
        target.chmod(entry["mode"])
        if hashlib.sha256(target.read_bytes()).hexdigest() != entry["sha256"]:
            raise ValueError(f"source changed during staging: {entry['source']}")
    (destination / "sledge-package-manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )


def archive_image(path, destination, manifest):
    # Exclusive creation: do not replace an earlier release archive.
    with path.open("xb") as output, tarfile.open(fileobj=output, mode="w") as archive:
        for entry in manifest:
            target = destination / entry["path"]
            info = archive.gettarinfo(str(target), arcname=entry["path"])
            info.uid = info.gid = 0
            info.uname = info.gname = "root"
            info.mtime = 0
            with target.open("rb") as source:
                archive.addfile(info, source)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binaries",
        required=True,
        type=Path,
        help="directory containing already-built Linux release binaries",
    )
    parser.add_argument(
        "--stage", type=Path, help="new absolute offline image directory"
    )
    parser.add_argument(
        "--archive", type=Path, help="new uncompressed root-owned .tar archive"
    )
    parser.add_argument(
        "--apply", action="store_true", help="consent to offline image writes only"
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="validate and print plan without writes (default)",
    )
    args = parser.parse_args(argv)
    try:
        if args.apply and args.dry_run:
            raise ValueError("--apply and --dry-run are mutually exclusive")
        if args.apply and not args.stage:
            raise ValueError("--apply requires --stage")
        if args.archive and not args.stage:
            raise ValueError("--archive requires --stage")
        if args.apply and os.geteuid() == 0:
            raise ValueError("run offline packaging as an unprivileged user, not root")
        manifest = [
            {
                "source": str(source.resolve()),
                "path": path,
                "mode": mode,
                "sha256": check_source(source),
            }
            for source, path, mode in entries(args.binaries)
        ]
        print(
            json.dumps(
                {
                    "apply": args.apply,
                    "stage": str(args.stage) if args.stage else None,
                    "files": manifest,
                },
                indent=2,
            )
        )
        if args.apply:
            if args.archive and os.path.lexists(args.archive):
                raise ValueError("archive must not already exist")
            stage_image(args.stage, manifest)
            if args.archive:
                archive_image(args.archive, args.stage, manifest)
        return 0
    except (ValueError, OSError) as error:
        print(f"package: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
