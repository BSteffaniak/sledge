"""Controller tests never contact the active compositor."""
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "drain_unload", Path(__file__).with_name("drain-unload.py")
)
controller = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controller)


def status(detached=False):
    return json.dumps(dict(version=1, draining=True, detached=detached,
                           quiescent=True, connection_generation=1,
                           ownership_epoch=2))


class DrainUnloadTests(unittest.TestCase):
    def test_quiescent_snapshot_does_not_authorize_unload(self):
        self.assertFalse(controller.detached_status(status()))
        self.assertTrue(controller.detached_status(status(True)))

    def test_old_or_forged_status_rejected(self):
        for text in ('{}', '[]', '{"version":true}',
                     status(True).replace('"detached": true', '"detached": 1')):
            with self.assertRaises(ValueError):
                controller.detached_status(text)

    def test_unload_occurs_only_after_detach(self):
        calls = []
        answers = iter([status(), status(True)])

        def command(hyprctl, instance, arguments, timeout):
            calls.append(arguments)
            return next(answers) if arguments == ["sledge:status"] else "ok"

        controller.drain_unload(Path("/hyprctl"), "fixture", Path("/plugin.so"),
                                1, command)
        self.assertEqual(calls, [["dispatch", "sledge:drain"],
                                 ["sledge:status"], ["sledge:status"],
                                 ["plugin", "unload", "/plugin.so"]])

    def test_timeout_leaves_plugin_loaded(self):
        calls = []

        def command(hyprctl, instance, arguments, timeout):
            calls.append(arguments)
            return status() if arguments == ["sledge:status"] else "ok"

        with self.assertRaises(TimeoutError):
            controller.drain_unload(Path("/hyprctl"), "fixture",
                                    Path("/plugin.so"), 0.01, command)
        self.assertFalse(any(args[:2] == ["plugin", "unload"] for args in calls))


if __name__ == "__main__":
    unittest.main()
