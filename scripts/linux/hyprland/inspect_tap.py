"""Summarize failing qualification without ever upgrading it to acceptance."""
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
import json
import re
import sys

from validate_qualification import evidence_files, phases
from validate_tap import EXPECTED, keys, masks, validate, validate_phase


def inspect(text):
    files = evidence_files(text)
    app = files.get('/tmp/candidate-consumer-events', '')
    result = {'qualification': 'unaccepted', 'phases': [], 'lifecycle': {}}
    for name in EXPECTED:
        samples = phases(app, name)
        for index, sample in enumerate(samples):
            error = None
            try:
                validate_phase(sample, name)
            except ValueError as failure:
                error = str(failure)
            record = dict(name=name, occurrence=index + 1, passed=error is None,
                          error=error, keys=keys(sample))
            if name == 'tap-positive':
                # Exact consumer excerpt preserves physical release -> projected
                # modifier -> Enter pair -> zero sentinel, including serials.
                match = list(re.finditer(r'key: 108; state: 0', sample))
                if match:
                    record['release_action_excerpt'] = sample[match[-1].start():]
            result['phases'].append(record)
    for name, prefix in [('tap-ime', 'IME_KEY'), ('tap-locked', 'LOCK_KEY')]:
        evidence = files.get('/tmp/' + name + '-events', '')
        samples = phases(evidence, name)
        delivered = re.findall(rf'^{prefix} code=(\d+) state=([01])', samples[0], re.MULTILINE) if len(samples) == 1 else []
        result['lifecycle'][name] = dict(phase_count=len(samples), delivered=delivered,
            ordinary_only=delivered == [('100', '1'), ('100', '0')] * 3,
            application_keys=[keys(p) for p in phases(app, name)])
    try:
        with redirect_stdout(StringIO()):
            validate(text, lifecycle=True)
        result['validator_error'] = None
    except ValueError as error:
        result['validator_error'] = str(error)
    return result


if __name__ == '__main__':
    print(json.dumps(inspect(Path(sys.argv[1]).read_text()), indent=2))
