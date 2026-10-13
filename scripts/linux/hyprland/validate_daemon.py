"""Actual daemon research consumer evidence; never a production qualification."""
import json
from pathlib import Path
import re
import sys
from validate_qualification import evidence_files, phases, require
from validate_replacement import native_keys, x_keys


def validate(text):
    text = re.sub(r'\x1b\[[0-9;]*m', '', text)
    require('SLEDGE_ACTUAL_DAEMON_RESEARCH_COMPLETED_QUALIFIED_FALSE' in text, 'daemon fixture incomplete')
    files = evidence_files(text)
    app = files['/tmp/daemon-consumer-events-final']
    for name, keys, count in (
        ('daemon-a', [('56','1'),('56','0')], 2),
        ('invalid-reload', [('56','1'),('56','0')], 1),
        ('valid-reload', [('54','1'),('54','0')], 1),
        ('scope-native_fixture', [('56','1'),('56','0')], 1),
        ('scope-wrong_fixture', [('38','1'),('38','0')], 1),
        ('held-drain', [('54','1'),('54','0')], 1),
        ('detached-unload', [('38','1'),('38','0')], 1),
        ('alt-held', [('64','1'),('54','1'),('54','0'),('64','0')], 1),
    ):
        samples = phases(app, name)
        require(len(samples)==count and all(native_keys(s)==keys for s in samples), f'{name}: unbalanced or unexpected keys')
    alt = phases(app,'alt-held')[0]
    require(re.findall(r'depressed: ([0-9a-f]{8})', alt)==['00000008','00000000','00000008','00000000'], 'Alt isolation/restoration incorrect')
    xapp = files.get('/tmp/daemon-x-events', '')
    for alias, code, symbol in (('x_fixture','56','b'), ('wrong_fixture','38','a')):
        samples = phases(xapp, 'x-'+alias)
        require(len(samples)==1 and x_keys(samples[0])==[('KeyPress','0x0',code,symbol),('KeyRelease','0x0',code,symbol)], 'X11 scoped output incorrect')
    peer = files['/tmp/daemon-events']
    require(re.search(r'Hyprland native reconciliation .*accepted=true', text), 'missing accepted reconciliation')
    require(len(re.findall(r'native delivery outcome .*key=30 state=1 .*result: Replaced,',text))>=6, 'missing actual correlated replaced outcomes')
    require('SIGHUP reload failed' in text, 'invalid reload not evidenced')
    states = [json.loads(line) for line in text.splitlines() if line.startswith('{"version":1,"draining":')]
    require(len(states)==2 and not states[0]['detached'] and states[1]['detached'] and states[1]['quiescent'], 'held drain/detach not evidenced')
    print('PASS_ACTUAL_DAEMON_RESEARCH_RELOAD_SCOPE_ALT_HELD_DRAIN_DETACHED_UNLOAD_NOT_PRODUCTION')


if __name__=='__main__':
    validate(Path(sys.argv[1]).read_text())
