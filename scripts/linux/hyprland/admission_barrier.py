"""Post-transition live admission evidence; never input-driven readiness."""
import json
from pathlib import Path
import re
import subprocess
import sys
import time


def admitted(status, page, controls, focus):
    if not (focus.get('class') == 'wev' and focus.get('xwayland') is False and
            status.get('native_operational') is True and status.get('waiting_for_focus') is False):
        return False
    if not (page.get('version') == 1 and page.get('live') is True and page.get('selected_generation', 0) > 0):
        return False
    if (page.get('connection_generation'), page.get('ownership_epoch')) != (status.get('connection_generation'), status.get('ownership_epoch')):
        return False
    controls = re.sub(r'\x1b\[[0-9;]*m', '', controls)
    return any(int(nonce) == page.get('nonce') and int(epoch) == page.get('ownership_epoch')
               for nonce, epoch in re.findall(r'Hyprland validated lifecycle control nonce=(\d+) sequence=\d+ epoch=(\d+) control=Reset\(8\)', controls))


def wait(hyprctl, offset):
    deadline = time.monotonic() + 5
    last = {}
    while time.monotonic() < deadline:
        invoke = lambda command: json.loads(subprocess.check_output([hyprctl, command], text=True, timeout=2))
        try:
            status = invoke('sledge:status')
            page = invoke('sledge:maps 0')
            focus = json.loads(subprocess.check_output([hyprctl, '-j', 'activewindow'], text=True, timeout=2))
            controls = Path('/tmp/candidate-daemon-events').read_text()[offset:]
            last = {'kind': 'admission_barrier', 'status': status, 'map_scope': page, 'focus': focus,
                    'new_controls': controls, 'admitted': admitted(status, page, controls, focus), 'atomic': False}
            if last['admitted']:
                # Selected slot may not be slot zero: prove its actual binding.
                from collect_maps import collect
                maps = collect(lambda command: subprocess.check_output([hyprctl, command], text=True, timeout=2))
                if maps['complete'] and all((p['nonce'], p['connection_generation'], p['ownership_epoch']) ==
                                           (page['nonce'], page['connection_generation'], page['ownership_epoch']) for p in maps['pages']):
                    last['maps'] = maps
                    print(json.dumps(last, separators=(',', ':')))
                    return
        except (ValueError, OSError, subprocess.SubprocessError) as error:
            last = {'kind': 'admission_barrier', 'admitted': False, 'error': str(error)}
        time.sleep(0.05)
    print(json.dumps(last, separators=(',', ':')))
    raise ValueError('post-transition admission proof missing; no replacement input sent')


if __name__ == '__main__':
    wait(sys.argv[1], int(sys.argv[2]))
