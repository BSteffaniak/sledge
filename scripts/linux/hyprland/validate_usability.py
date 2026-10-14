"""Independent normal packaged-launch evidence, never GDM authentication proof."""
from pathlib import Path
import json
import re
import sys
from validate_qualification import evidence_files, phases, require
from validate_tap import validate_phase


def validate(text):
    require('DIAGNOSTIC_RETRY_NOT_ACCEPTANCE' not in text, 'manual retry is not normal launch acceptance')
    require('SLEDGE_NORMAL_DESKTOP_EXEC_COMPLETED_VM_ONLY_NOT_GDM_AUTHENTICATION' in text,
            'normal packaged launcher incomplete')
    require(re.search(r'^SLEDGE_PAYLOAD_REPORT_END status=0 qualified=(?:true|false)$', text, re.MULTILINE),
            'guest did not settle successfully')
    require(re.search(r'^ACTUAL_DESKTOP_EXEC /nix/store/[^\s]+/bin/sledge-hyprland-session$', text, re.MULTILINE),
            'actual desktop Exec identity missing')
    require('NORMAL_WRAPPER_SETTLED exit=0' in text, 'normal logout did not settle')
    require(re.search(r'^NORMAL_DAEMON_EXIT_CONFIRMED pid=[1-9]\d*$', text, re.MULTILINE),
            'daemon exit after compositor shutdown not confirmed')
    files = evidence_files(text)
    app = files.get('/tmp/usability-consumer-events', '')
    for name, count, expected in [('normal-positive', 2, 'tap-positive'),
                                  ('invalid-reload-kept-tap', 1, 'tap-positive'),
                                  ('valid-reload-no-third-action', 1, 'tap-baseline'),
                                  ('restored-positive', 1, 'tap-positive')]:
        samples = phases(app, name)
        require(len(samples) == count, f'missing/duplicate {name}')
        for sample in samples:
            validate_phase(sample, expected)
    controller = files.get('/tmp/usability-first-app-events', '')
    markers = re.findall(r'^INHERITED_SESSION (.+)$', controller, re.MULTILINE)
    require(len(markers) == 1, 'missing unique inherited marker')
    marker = json.loads(markers[0])
    require(marker.get('version') == 1 and marker.get('backend') == 'hyprland'
            and type(marker.get('compositor_pid')) is int and marker['compositor_pid'] > 0
            and isinstance(marker.get('instance'), str) and bool(marker['instance'])
            and re.fullmatch(r'/tmp/usability-runtime/sledge\.[^/]+/health\.sock', marker.get('ipc_socket', '')),
            'invalid inherited session selection')
    require('EFFECTIVE_TEST_CONFIG /tmp/usability-config/sledge/config.toml' in controller,
            'inherited marker/same effective config missing')
    require(len(re.findall(r'^CLI_BEGIN status exit=0$', controller, re.MULTILINE)) == 2,
            'plain status did not succeed twice')
    require(len(re.findall(r'^CLI_BEGIN reload exit=0$', controller, re.MULTILINE)) == 2,
            'valid plain reload evidence missing')
    require(re.search(r'^CLI_BEGIN reload exit=[1-9]\d*$', controller, re.MULTILINE),
            'invalid same-file reload was not rejected')
    session_log = files.get('/tmp/usability-session-events', '')
    lifecycle = [json.loads(line) for line in session_log.splitlines() if line.startswith('{')]
    expected_events = ['drain_acknowledged', 'adapter_detached', 'daemon_stop_acknowledged', 'plugin_unload_acknowledged']
    require([r.get('event') for r in lifecycle] == expected_events,
            'missing/out-of-order wrapper detach/unload acknowledgments')
    require(all(r.get('instance') == marker['instance'] for r in lifecycle if r.get('event') != 'daemon_stop_acknowledged'),
            'logout acknowledgment instance mismatch')
    require(lifecycle[-1].get('plugin') == '/nix/store/hh81r9q72ap3l5j34hffaz3ldp4v5mfm-sledge-hyprland-exact-qualified-1/lib/qualified.so',
            'logout did not acknowledge exact accepted plugin')
    startup_logs = [value for path, value in files.items()
                    if re.fullmatch(r'/tmp/usability-runtime/sledge\.[^/]+/startup\.log', path)]
    require(len(startup_logs) == 1, 'missing unique private startup log')
    records = [json.loads(line) for line in startup_logs[0].splitlines() if line.startswith('{')]
    started = [r for r in records if r.get('event') == 'daemon_started']
    ready = [r for r in records if r.get('operational') is True]
    require(len(started) == len(ready) == 1, 'missing unique startup identity/readiness')
    require(started[0].get('compositor_pid') == marker['compositor_pid']
            and started[0].get('instance') == marker['instance']
            and started[0].get('daemon_pid') == ready[0].get('daemon_pid'),
            'startup identity does not match inherited session')
    require(re.fullmatch(r'/nix/store/[^/]+/bin/sledge', started[0].get('daemon_path', '')),
            'startup daemon is not immutable packaged executable')
    require(lifecycle[2].get('daemon_pid') == started[0]['daemon_pid'],
            'guardian stop acknowledgment daemon identity mismatch')
    exits = re.findall(r'^NORMAL_DAEMON_EXIT_CONFIRMED pid=(\d+)$', text, re.MULTILINE)
    require(exits == [str(started[0]['daemon_pid'])], 'daemon exit identity mismatch')
    require('SLEDGE_NORMAL_FIRST_APP_MATRIX_COMPLETED' in controller, 'first app matrix incomplete')
    print('PASS_NORMAL_DESKTOP_EXEC_VM_ONLY_NOT_GDM_AUTHENTICATION')


if __name__ == '__main__':
    validate(Path(sys.argv[1]).read_text())
