"""Strict independent bad-pin prehook evidence; never positive qualification."""
import json
from pathlib import Path
import sys

from prehook_bytes import ELF_SHA
from validate_candidate import native_keys
from validate_qualification import phases, require


def validate(text, sdk_sha=None, elf_sha=None):
    for name, digest in [('wrong-sdk', sdk_sha), ('wrong-elf', elf_sha)]:
        require(digest is not None and len(digest) == 64 and
                f'{digest}  /sledge-payload/{name}.so' in text, 'fresh negative artifact hash missing')
    require('SLEDGE_NEGATIVE_ABI_COMPLETED_NOT_QUALIFIED' in text, 'negative fixture incomplete')
    snapshots = [json.loads(line) for line in text.splitlines() if line.startswith('{"kind":"prehook_bytes"')]
    require(len(snapshots) == 4, 'four pre/post snapshots required')
    require(len({(s['pid'], s['starttime'], s['elf_sha256']) for s in snapshots}) == 1,
            'negative loaders must share exact stock process identity')
    # Payload cleanup repeats the same named consumer evidence file. Verify
    # repeated framing byte-for-byte rather than treating it as a second input.
    consumer_marker = 'EVIDENCE_FILE /tmp/candidate-consumer-events\n'
    if text.count(consumer_marker) > 1:
        from validate_qualification import evidence_files
        blocks = text.split(consumer_marker)[1:]
        framed = [block.split('EVIDENCE_FILE ', 1)[0].split('SLEDGE_NEGATIVE_ABI_COMPLETED_NOT_QUALIFIED', 1)[0].strip() for block in blocks]
        # Only compare the actual PHASE spans; footer/status framing differs.
        spans = ['\n'.join(__import__('re').findall(r'PHASE_BEGIN .*?PHASE_END [^\n]+', block, __import__('re').S)) for block in framed]
        require(all(span == spans[0] and span for span in spans), 'repeated consumer evidence differs')
        text = text.split(consumer_marker)[0] + consumer_marker + framed[0] + '\nSLEDGE_NEGATIVE_ABI_COMPLETED_NOT_QUALIFIED'
    for index, name in enumerate(('wrong-sdk', 'wrong-elf')):
        before, after = snapshots[index * 2:index * 2 + 2]
        require(before == after and before['elf_sha256'] == ELF_SHA and int(before['starttime']) > 0,
                'same-process prehook identity/bytes differ')
        entries = before['entries']
        require(len(entries) == 19 and len({e['symbol_value'] for e in entries}) == 19,
                'nineteen distinct hook targets required')
        biases = {entry['address'] - entry['symbol_value'] for entry in entries}
        require(len(biases) == 1 and next(iter(biases)) > 0 and all(len(bytes.fromhex(e['bytes'])) == 32 for e in entries),
                'exact PIE bias/32-byte snapshots required')
        require(f'PREHOOK_BYTES_EQUAL name={name}' in text and
                f'NEGATIVE_ABI_LOAD name={name}' in text, 'negative load/comparison missing')
        sample = phases(text, name)
        require(len(sample) == 1 and native_keys(sample[0]) == [('38', '1'), ('38', '0')],
                'negative stock A pair missing')
    require(text.count('isolated candidate scope/ELF/SDK mismatch; hooks not installed') >= 2,
            'independent explicit guard rejection missing')


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log')
    parser.add_argument('--sdk-sha', required=True)
    parser.add_argument('--elf-sha', required=True)
    args = parser.parse_args()
    validate(Path(args.log).read_text(), args.sdk_sha, args.elf_sha)
    print('PASS_NEGATIVE_ABI_19_PREHOOK_TARGETS_NOT_QUALIFIED')
