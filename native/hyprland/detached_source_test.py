#!/usr/bin/env python3
"""Structural regression: detached guard precedes every stateful detour."""
from pathlib import Path
source = Path(__file__).with_name("stock_adapter.cpp").read_text()
for name in ("inputKey", "seatKey", "seatMods", "focus", "imeKey", "imeMods", "lock",
             "capabilities", "enter", "resourceMods", "keybind", "newKeyboard",
             "destroyKeyboard", "setKeyboardLayout", "keyboardResourceDestroyed", "selectKeyboard"):
    start = source.index(("bool " if name == "keybind" else "void ") + name + "(")
    brace = source.index("{", start)
    first = source[brace + 1:].lstrip().splitlines()[0]
    assert first.startswith("if (adapter->drain.detached())"), (name, first)
    assert "original(" in first, name
    if name == "enter":
        assert "self,surface,original" in first  # original array, not filtered keys
for name in ("inputMods", "locked"):
    start = source.index(("bool " if name == "locked" else "void ") + name + "(")
    brace = source.index("{", start)
    body = source[brace + 1:source.index("}", brace)]
    assert "original(" in body and "research" not in body
print("PASS all 18 detours: detached early originals; enter preserves original keys")
