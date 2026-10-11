#!/usr/bin/env python3
"""Generate narrow lifecycle patch from exact read-only pinned Nix source."""
from pathlib import Path
import difflib

root = Path('/nix/store/8njasxpjmb1n25n5kanrh9ga04a19dsw-source')
changes = {}
def edit(file, old, new):
    text = changes.get(file, (root / file).read_text())
    if text.count(old) != 1:
        raise RuntimeError(f'{file}: expected unique source anchor')
    changes[file] = text.replace(old, new)

edit('src/devices/IKeyboard.hpp', '#include "IHID.hpp"', '#include "IHID.hpp"\n#include "DeliveredKeyboardState.hpp"')
edit('src/devices/IKeyboard.hpp', '    xkb_keymap* m_xkbKeymap = nullptr;', '    DeliveredKeyboardState m_delivered;\n\n    xkb_keymap* m_xkbKeymap = nullptr;')
for old, new in [
('        m_xkbSymState    = xkb_state_new(keymap);', '        m_xkbSymState    = xkb_state_new(keymap);\n        if (!m_delivered.setKeymap(keymap))\n            Debug::log(ERR, "Failed to rebuild delivered keyboard state");'),
('            m_xkbSymState    = xkb_state_new(KEYMAP);', '            m_xkbSymState    = xkb_state_new(KEYMAP);\n            if (!m_delivered.setKeymap(KEYMAP))\n                Debug::log(ERR, "Failed to rebuild delivered keyboard state");'),
('    m_xkbSymState    = xkb_state_new(NEWKEYMAP);', '    m_xkbSymState    = xkb_state_new(NEWKEYMAP);\n    if (!m_delivered.setKeymap(NEWKEYMAP))\n        Debug::log(ERR, "Failed to rebuild delivered keyboard state");')]:
    edit('src/devices/IKeyboard.cpp', old, new)
edit('src/devices/IKeyboard.cpp', 'IKeyboard::~IKeyboard() {\n    m_events.destroy.emit();', 'IKeyboard::~IKeyboard() {\n    m_delivered.clear(); // observational lifecycle, not output reconciliation\n    m_events.destroy.emit();')
edit('src/managers/input/InputManager.cpp', '    pKeyboard->m_enabled           = ENABLED;', '    pKeyboard->m_enabled           = ENABLED;\n    if (!pKeyboard->m_enabled)\n        pKeyboard->m_delivered.clear();')
edit('src/managers/input/InputManager.cpp', '                k->m_allowed = r->result() == PERMISSION_RULE_ALLOW_MODE_ALLOW;', '                k->m_allowed = r->result() == PERMISSION_RULE_ALLOW_MODE_ALLOW;\n                if (!k->m_allowed)\n                    k->m_delivered.clear();')
edit('src/managers/input/InputManager.cpp', '    } else\n        pKeyboard->m_allowed = PERM == PERMISSION_RULE_ALLOW_MODE_ALLOW;', '    } else\n        pKeyboard->m_allowed = PERM == PERMISSION_RULE_ALLOW_MODE_ALLOW;\n    if (!pKeyboard->m_allowed)\n        pKeyboard->m_delivered.clear();')
edit('src/managers/input/InputManager.cpp', '    if (!pKeyboard->m_enabled || !pKeyboard->m_allowed)\n        return;', '    // Shadow is observational only: never controls client/stock routing.\n    if (event.state == WL_KEYBOARD_KEY_STATE_RELEASED)\n        pKeyboard->m_delivered.key(event.keycode, false);\n    if (!pKeyboard->m_enabled || !pKeyboard->m_allowed) {\n        pKeyboard->m_delivered.clear();\n        return;\n    }')
edit('src/managers/input/InputManager.cpp', '    if (passEvent) {\n        auto state', '    if (passEvent) {\n        // Observed stock admission, NOT confirmed consumer ownership.\n        if (event.state == WL_KEYBOARD_KEY_STATE_PRESSED)\n            pKeyboard->m_delivered.key(event.keycode, true);\n        auto state')
newfile = 'src/devices/DeliveredKeyboardState.hpp'
changes[newfile] = Path('native/hyprland/DeliveredKeyboardState.hpp').read_text()
patch = ''
for file, new in changes.items():
    old = (root / file).read_text() if (root / file).exists() else ''
    patch += ''.join(difflib.unified_diff(old.splitlines(True), new.splitlines(True), fromfile='a/'+file if old else '/dev/null', tofile='b/'+file))
Path('native/hyprland/routing.patch').write_text(patch)
print('Generated observational lifecycle patch; stock client/focus routing unchanged, no capture')
