import contextlib
import io
import unittest
from validate_tap import ALT, ACTION, SENTINEL
from validate_usability import validate


def sample(action=True):
    text = ''
    for code, state in ALT * 3 + (ACTION if action else []) + SENTINEL:
        if (code, state) == ('36', '1'):
            text += 'depressed: 00000004\nlatched: 00000000\nlocked: 00000000\n'
        text += f'key: {code}; state: {state}\n'
        if (code, state) != ('36', '1'):
            text += 'depressed: 00000000\nlatched: 00000000\nlocked: 00000000\n'
    return text


class UsabilityTests(unittest.TestCase):
    def report(self):
        text = ('ACTUAL_DESKTOP_EXEC /nix/store/test-login/bin/sledge-hyprland-session\n'
                'NORMAL_WRAPPER_SETTLED exit=0\n'
                'NORMAL_DAEMON_EXIT_CONFIRMED pid=123\n'
                'SLEDGE_NORMAL_DESKTOP_EXEC_COMPLETED_VM_ONLY_NOT_GDM_AUTHENTICATION\n'
                'SLEDGE_PAYLOAD_REPORT_END status=0 qualified=false\n'
                'EVIDENCE_FILE /tmp/usability-first-app-events\n'
                'INHERITED_SESSION {"version":1,"backend":"hyprland","compositor_pid":123,"instance":"fixture","ipc_socket":"/tmp/usability-runtime/sledge.fixture/health.sock"}\n'
                'EFFECTIVE_TEST_CONFIG /tmp/usability-config/sledge/config.toml\n'
                'CLI_BEGIN status exit=0\nCLI_END status\n' * 1)
        text += ('CLI_BEGIN status exit=0\nCLI_END status\n'
                 'CLI_BEGIN reload exit=1\nCLI_END reload\n'
                 'CLI_BEGIN reload exit=0\nCLI_END reload\n' * 1)
        text += ('CLI_BEGIN reload exit=0\nCLI_END reload\n'
                 'SLEDGE_NORMAL_FIRST_APP_MATRIX_COMPLETED\n'
                 'EVIDENCE_FILE /tmp/usability-consumer-events\n')
        for name, count, action in [('normal-positive', 2, True), ('invalid-reload-kept-tap', 1, True),
                                    ('valid-reload-no-third-action', 1, False), ('restored-positive', 1, True)]:
            for _ in range(count):
                text += f'PHASE_BEGIN {name}\n{sample(action)}PHASE_END {name}\n'
        text += ('EVIDENCE_FILE /tmp/usability-session-events\n'
                 '{"event":"drain_acknowledged","instance":"fixture"}\n'
                 '{"event":"adapter_detached","instance":"fixture"}\n'
                 '{"event":"daemon_stop_acknowledged","daemon_pid":123}\n'
                 '{"event":"plugin_unload_acknowledged","instance":"fixture","plugin":"/nix/store/hh81r9q72ap3l5j34hffaz3ldp4v5mfm-sledge-hyprland-exact-qualified-1/lib/qualified.so"}\n'
                 'EVIDENCE_FILE /tmp/usability-runtime/sledge.fixture/startup.log\n'
                 '{"event":"daemon_started","daemon_pid":123,"compositor_pid":123,"instance":"fixture","daemon_path":"/nix/store/test-daemon/bin/sledge"}\n'
                 '{"daemon_pid":123,"operational":true}\n')
        return text

    def test_complete_report_and_mutation_rejection(self):
        report = self.report()
        with contextlib.redirect_stdout(io.StringIO()):
            validate(report)
        for broken in [report.replace('NORMAL_WRAPPER_SETTLED exit=0', 'NORMAL_WRAPPER_SETTLED exit=1'),
                       report.replace('status=0 qualified=false', 'status=1 qualified=false'),
                       report.replace('CLI_BEGIN reload exit=1', 'CLI_BEGIN reload exit=0'),
                       report.replace('00000004', '0000000c'),
                       report.replace('"event":"daemon_stop_acknowledged","daemon_pid":123', '"event":"daemon_stop_acknowledged","daemon_pid":124'),
                       report.replace('daemon_stop_acknowledged', 'missing_daemon_stop'),
                       report.replace('adapter_detached', 'missing_detach'),
                       report.replace('"daemon_pid":123,"operational":true', '"daemon_pid":124,"operational":true'),
                       report.replace('NORMAL_DAEMON_EXIT_CONFIRMED pid=123', 'NORMAL_DAEMON_EXIT_CONFIRMED pid=124'),
                       report.replace('PHASE_BEGIN restored-positive', 'PHASE_BEGIN omitted')]:
            with self.assertRaises(ValueError):
                validate(broken)


if __name__ == '__main__':
    unittest.main()
