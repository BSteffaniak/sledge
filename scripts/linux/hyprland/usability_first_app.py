"""Observable first application for the actual packaged login, guest only."""
import json
import os
from pathlib import Path
import subprocess
import time


def cli(command, success=True):
    executable = os.environ['SLEDGE_TEST_CLI']
    result = subprocess.run([executable, command], text=True, capture_output=True, timeout=3)
    print(f'CLI_BEGIN {command} exit={result.returncode}\n{result.stdout}{result.stderr}CLI_END {command}', flush=True)
    if (result.returncode == 0) != success:
        raise RuntimeError(f'unexpected CLI {command} result')
    return result.stdout


def atomic_config(path, text):
    temporary = path.with_name('qualification-config.new')
    temporary.write_text(text)
    temporary.replace(path)


def main():
    events = Path('/tmp/usability-consumer-events')
    with events.open('a') as output:
        consumer = subprocess.Popen(['stdbuf', '-oL', '-eL', 'wev'], stdout=output, stderr=output)
    try:
        deadline = time.monotonic() + 65
        marker = Path(os.environ['SLEDGE_SESSION_FILE'])
        while not marker.exists() and time.monotonic() < deadline:
            if consumer.poll() is not None:
                raise RuntimeError('independent consumer exited')
            time.sleep(0.05)
        selection = json.loads(marker.read_text())
        print('INHERITED_SESSION ' + json.dumps(selection, sort_keys=True), flush=True)
        text = cli('status')
        if 'ready' not in text or '"hyprland_capture_qualified":true' not in text.replace(' ', ''):
            raise RuntimeError('plain CLI did not report accepted operational backend')
        config = Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home()/'.config'))) / 'sledge/config.toml'
        original = config.read_text()
        print('EFFECTIVE_TEST_CONFIG ' + str(config), flush=True)
        with open('/dev/ttyS1', 'r+b', buffering=0) as control:
            def step(name):
                control.write(f'STEP {name}\n'.encode())
                if control.readline() != b'OK\n':
                    raise RuntimeError('QMP guest receipt missing')
            def phase(name):
                with events.open('a') as log:
                    log.write(f'PHASE_BEGIN {name}\n')
                step('tap-positive')
                step('tap-sentinel')
                time.sleep(0.2)
                with events.open('a') as log:
                    log.write(f'PHASE_END {name}\n')
            phase('normal-positive')
            time.sleep(0.8)
            phase('normal-positive')
            atomic_config(config, 'invalid = [\n')
            cli('reload', success=False)
            time.sleep(0.8)
            phase('invalid-reload-kept-tap')
            disabled = original.replace('count = 3', 'count = 4')
            if disabled == original:
                raise RuntimeError('fixture must use explicit count = 3')
            atomic_config(config, disabled)
            cli('reload')
            time.sleep(0.8)
            phase('valid-reload-no-third-action')
            atomic_config(config, original)
            cli('reload')
            time.sleep(0.8)
            phase('restored-positive')
            cli('status')
        print('SLEDGE_NORMAL_FIRST_APP_MATRIX_COMPLETED', flush=True)
        Path('/tmp/usability-controller-done').write_text('ok\n')
        # Remain a real focused consumer until normal wrapper logout is requested.
        consumer.wait(timeout=30)
    finally:
        if consumer.poll() is None:
            consumer.terminate()
            consumer.wait(timeout=3)


if __name__ == '__main__':
    main()
