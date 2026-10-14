"""Guest-only tap inputs. No protocol or backend diagnostic is an oracle."""
import time


def tap_step(qmp, name):
    """Return False for existing non-tap steps; retain their dispatch unchanged."""
    if not name.startswith("tap-"):
        return False
    simple = {"tap-baseline", "tap-positive", "tap-locked", "tap-ime"}
    if name in simple:
        for _ in range(3):
            qmp.pair("alt_r")
    elif name == "tap-prefix":
        for _ in range(2):
            qmp.pair("alt_r")
    elif name == "tap-suffix":
        qmp.pair("alt_r")
    elif name == "tap-timeout":
        qmp.pair("alt_r")
        qmp.pair("alt_r")
        time.sleep(0.8)
        qmp.pair("alt_r")
    elif name == "tap-hold-down":
        qmp.key("alt_r", True)
        time.sleep(0.8)
    elif name == "tap-hold-up":
        qmp.key("alt_r", False)
    elif name == "tap-chord":
        qmp.pair("alt_r")
        qmp.key("alt_r", True)
        qmp.pair("e")
        qmp.key("alt_r", False)
        qmp.pair("alt_r")
    elif name == "tap-unknown":
        qmp.pair("alt_r")
        qmp.key("alt_r", True)
        qmp.pair("menu")
        qmp.key("alt_r", False)
        qmp.pair("alt_r")
    elif name == "tap-modifier":
        qmp.key("shift", True)
        for _ in range(3):
            qmp.pair("alt_r")
        qmp.key("shift", False)
    elif name == "tap-device":
        first, second = qmp.targeted_keyboards()
        qmp.pair("alt_r", first)
        qmp.pair("alt_r", second)
        qmp.pair("alt_r", first)
    elif name == "tap-sentinel":
        qmp.pair("b")
    else:
        raise ValueError(f"unknown tap step: {name}")
    return True
