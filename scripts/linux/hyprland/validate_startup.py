"""Validate qualified-mode startup/replacement/drain in a disposable VM only."""
import json
from pathlib import Path
import re
import sys
from validate_qualification import evidence_files, phases, require
from validate_replacement import native_keys


def validate(text):
    text = re.sub(r"\x1b\[[0-9;]*m", "", text)
    require('SLEDGE_QUALIFIED_STARTUP_REPLACEMENT_DRAIN_COMPLETED_VM_ONLY' in text,
            'qualified startup/drain fixture incomplete')
    require('SLEDGE_PAYLOAD_REPORT_END status=0 qualified=false' in text,
            'guest report unsuccessful')
    files = evidence_files(text)
    startup = json.loads(files.get('/tmp/qualified-startup', ''))
    require(startup.get('operational') is True and type(startup.get('daemon_pid')) is int,
            'startup barrier never became operational')
    peer = files.get('/tmp/qualified-daemon-events', '')
    require('independently accepted installed Hyprland registration; awaiting native proof feature_mask=1' in peer,
            'qualified admission absent')
    reconciled = re.findall(r'native reconciliation epoch=(\d+) sequence=(\d+) nonce=(\d+) accepted=true', peer)
    replaced = re.findall(r'native delivery outcome generation=\d+ epoch=(\d+) sequence=(\d+) nonce=(\d+) key=30 state=1 focus_kind=1 report=Acknowledged\(Outcome \{ result: Replaced,', peer)
    require(len(replaced) == 2, 'two authenticated replacements required')
    for epoch, seq, nonce in replaced:
        require(any(e == epoch and n == nonce and int(s) < int(seq) for e, s, n in reconciled),
                'replacement lacks same-scope reconciliation')
    app = files.get('/tmp/qualified-consumer-events', '')
    samples = phases(app, 'candidate-a')
    require(len(samples) == 2 and all(native_keys(s) == [('56', '1'), ('56', '0')] for s in samples),
            'consumer must receive balanced B pairs without original A')
    samples = phases(app, 'detached')
    require(len(samples) == 1 and native_keys(samples[0]) == [('38', '1'), ('38', '0')],
            'post-drain stock passthrough missing')


if __name__ == '__main__':
    validate(Path(sys.argv[1]).read_text())
    print('PASS_QUALIFIED_STARTUP_REPLACEMENT_DRAIN_VM_ONLY_NOT_RELEASE_APPROVAL')
