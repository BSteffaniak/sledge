#!/usr/bin/env python3
"""Source invariants + exact stock multidevice sequence (not native VM evidence)."""
from pathlib import Path
import tempfile
import subprocess

source = Path('/nix/store/8njasxpjmb1n25n5kanrh9ga04a19dsw-source')
with tempfile.TemporaryDirectory(prefix='sledge-observation-source-') as directory:
    root = Path(directory)
    for name in ['src/devices/IKeyboard.hpp', 'src/devices/IKeyboard.cpp', 'src/managers/input/InputManager.cpp']:
        target = root / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((source / name).read_bytes())
    subprocess.run(['git', 'apply', str(Path('native/hyprland/routing.patch').resolve())], cwd=root, check=True)
    old = (source / 'src/managers/input/InputManager.cpp').read_text()
    new = (root / 'src/managers/input/InputManager.cpp').read_text()
    def function(text, start, end):
        return text.split(start, 1)[1].split(end, 1)[0]
    for start, end in [
        ('const std::vector<uint32_t>& CInputManager::getKeysFromAllKBs()', 'uint32_t CInputManager::getModsFromAllKBs'),
        ('bool CInputManager::shareKeyFromAllKBs(', 'uint32_t CInputManager::shareModsFromAllKBs'),
        ('void CInputManager::onKeyboardMod(', 'bool CInputManager::shouldIgnoreVirtualKeyboard'),
    ]:
        assert function(old, start, end) == function(new, start, end)
    assert new.index('pKeyboard->m_delivered.key(event.keycode, false);', new.index('void CInputManager::onKeyboardKey')) < new.index('if (!pKeyboard->m_enabled || !pKeyboard->m_allowed)')
    assert 'if (!k->m_allowed)\n                    k->m_delivered.clear();' in new
    assert 'if (!pKeyboard->m_enabled)\n        pKeyboard->m_delivered.clear();' in new
    assert 'm_delivered.clear(); // observational lifecycle' in (root / 'src/devices/IKeyboard.cpp').read_text()

# Stock release merging model anchored to unchanged shareKeyFromAllKBs and
# onKeyboardKey: consumed B physically holds the key, so A up emits no release.
physical = [False, False]
shadow = [False, False]
seat = set()
def event(device, down, admitted):
    physical[device] = down
    if not down:
        shadow[device] = False
    if not admitted:
        return
    if down:
        shadow[device] = True
    if not down and any(physical):
        return
    if down:
        seat.add(42)
    else:
        seat.discard(42)
event(0, True, True)
event(1, True, False)
event(0, False, True)
assert shadow == [False, False] and seat == {42}
# Focus uses stock seat, NEVER shadow: no new orphan introduced by observation.
assert list(seat) == [42]
event(1, False, True)
assert not seat
print('PASS source invariants: stock focus/modifier/merge unchanged; observational clears; stock A/B consumed sequence preserved')
