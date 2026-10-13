"""Bounded live map observations, rejecting identity drift (not atomic proof)."""
import json
import re
import subprocess
import sys


def collect(invoke):
    pages = []
    identity = None
    try:
        for offset in range(16):
            raw = invoke(f"sledge:maps {offset}")
            if len(raw.encode()) >= 8192:
                raise ValueError("map page oversized")
            page = json.loads(raw)
            if not isinstance(page, dict) or page.get("version") != 1 or page.get("live") is not True:
                raise ValueError("invalid map observation schema")
            for key in ("nonce", "connection_generation", "ownership_epoch", "selected_generation", "generation", "offset", "next_offset", "name_length_bytes"):
                if type(page.get(key)) is not int or page[key] < 0:
                    raise ValueError(f"invalid map {key}")
            for key in ("done", "present", "selected", "binding_valid", "name_truncated"):
                if type(page.get(key)) is not bool:
                    raise ValueError(f"invalid map {key}")
            scope = tuple(page[k] for k in ("nonce", "connection_generation", "ownership_epoch", "selected_generation"))
            if identity is None:
                identity = scope
            if scope != identity or page['offset'] != offset or page['next_offset'] != offset + 1 or page['done'] != (offset == 15):
                raise ValueError("map scope/cursor changed")
            pages.append(page)
            if not isinstance(page.get('name'), str) or page['name_truncated']:
                raise ValueError("map name absent/truncated")
            stored = page.get('stored_map')
            if not isinstance(stored, dict):
                raise ValueError("stored map missing")
            if page['present']:
                if not page['binding_valid'] or page['generation'] == 0 or stored.get('status') != 'complete' or not re.fullmatch('[0-9a-f]{64}', stored.get('sha256') or ''):
                    raise ValueError("map binding/digest incomplete")
                if type(stored.get('length_bytes')) is not int or not 0 < stored['length_bytes'] <= 262144:
                    raise ValueError("map length invalid")
                if page['selected'] != (page['generation'] == page['selected_generation']):
                    raise ValueError("selected map binding inconsistent")
        selected = [p for p in pages if p['selected']]
        if len(selected) != 1:
            raise ValueError("selected consumer missing/ambiguous")
        return {'kind': 'map_collection', 'complete': True, 'atomic': False, 'pages': pages}
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        return {'kind': 'map_collection', 'complete': False, 'atomic': False, 'error': str(error), 'pages': pages}


if __name__ == '__main__':
    result = collect(lambda command: subprocess.check_output([sys.argv[1], command], text=True, timeout=2))
    print(json.dumps(result, separators=(',', ':')))
    sys.exit(0 if result['complete'] else 1)
