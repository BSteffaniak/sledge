#!/usr/bin/env python3
"""Offline packaging tests: no service execution, device access, or root needed."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "package", Path(__file__).with_name("package.py")
)
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="sledge-package-test-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.binaries = self.base / "binaries"
        self.binaries.mkdir()
        for name in ("sledge", "sledge-linux-broker"):
            (self.binaries / name).write_bytes(b"test artifact; never executed\n")
        self.image = self.base / "image"

    def invoke(self, *args, uid=1000):
        with contextlib.redirect_stdout(
            io.StringIO()
        ) as output, contextlib.redirect_stderr(io.StringIO()), mock.patch.object(
            PACKAGE.os, "geteuid", return_value=uid
        ):
            result = PACKAGE.main(["--binaries", str(self.binaries), *args])
        return result, output.getvalue()

    def test_default_dry_run_creates_nothing(self):
        code, output = self.invoke("--stage", str(self.image))
        self.assertEqual(code, 0)
        self.assertFalse(self.image.exists())
        self.assertFalse(json.loads(output)["apply"])

    def test_apply_and_archive_have_only_explicit_paths_and_safe_modes(self):
        archive = self.base / "sledge.tar"
        code, _ = self.invoke(
            "--stage", str(self.image), "--archive", str(archive), "--apply"
        )
        self.assertEqual(code, 0)
        with tarfile.open(archive) as result:
            files = result.getmembers()
            self.assertEqual(len(files), 7)
            for entry in files:
                self.assertEqual((entry.uid, entry.gid), (0, 0))
                self.assertTrue(entry.isfile())
                self.assertFalse(entry.name.startswith("/"))
                self.assertNotIn("..", Path(entry.name).parts)
                self.assertIn(entry.mode, (0o644, 0o755))
            self.assertIn("usr/libexec/sledge-linux-broker", result.getnames())
        manifest = json.loads((self.image / "sledge-package-manifest.json").read_text())
        for entry in manifest:
            self.assertEqual(
                PACKAGE.check_source(self.image / entry["path"]), entry["sha256"]
            )

    def test_no_overwrite_or_symlink_destination(self):
        self.image.mkdir()
        self.assertEqual(self.invoke("--stage", str(self.image), "--apply")[0], 1)
        link = self.base / "link"
        link.symlink_to(self.image)
        self.assertEqual(self.invoke("--stage", str(link), "--apply")[0], 1)
        self.assertEqual(list(self.image.iterdir()), [])

    def test_no_source_symlinks_and_missing_binary(self):
        broker = self.binaries / "sledge-linux-broker"
        broker.unlink()
        self.assertEqual(self.invoke()[0], 1)
        broker.symlink_to(self.binaries / "sledge")
        self.assertEqual(self.invoke()[0], 1)

    def test_root_apply_and_conflicting_flags_refused(self):
        self.assertEqual(
            self.invoke("--stage", str(self.image), "--apply", uid=0)[0], 1
        )
        self.assertEqual(self.invoke("--apply", "--dry-run")[0], 1)
        self.assertFalse(self.image.exists())

    def test_existing_archive_not_overwritten_and_no_staging(self):
        archive = self.base / "sledge.tar"
        archive.write_bytes(b"preserve")
        self.assertEqual(
            self.invoke(
                "--stage", str(self.image), "--archive", str(archive), "--apply"
            )[0],
            1,
        )
        self.assertEqual(archive.read_bytes(), b"preserve")
        self.assertFalse(self.image.exists())

    def test_missing_parent_and_relative_stage_refused(self):
        for path in ("relative-image", str(self.base / "missing" / "image"), "/"):
            self.assertEqual(self.invoke("--stage", path, "--apply")[0], 1)

    def test_units_do_not_grant_input_access_or_logout_persistence(self):
        service = (PACKAGE.ASSETS / "sledge-broker.service").read_text()
        socket = (PACKAGE.ASSETS / "sledge-broker.socket").read_text()
        user = (PACKAGE.ASSETS / "sledge.service").read_text()
        self.assertIn("DevicePolicy=closed", service)
        self.assertIn("PrivateDevices=yes", service)
        self.assertNotIn("DeviceAllow=", service)
        self.assertIn("SocketMode=0600", socket)
        self.assertIn("DirectoryMode=0700", socket)
        self.assertIn("BindsTo=graphical-session.target", user)
        self.assertIn("Requisite=graphical-session.target", user)
        self.assertNotIn("default.target", user)
        self.assertNotIn("SupplementaryGroups=", service + user)


if __name__ == "__main__":
    unittest.main()
