"""Fail-closed independent-consumer checks, not backend parity qualification."""
from pathlib import Path
import re
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def evidence_files(text):
    parts = re.split(r"^EVIDENCE_FILE (.+)\n", text, flags=re.MULTILINE)
    return {parts[i]: parts[i + 1] for i in range(1, len(parts), 2)}


def phases(text, name):
    return re.findall(rf"PHASE_BEGIN {re.escape(name)}\n(.*?)PHASE_END {re.escape(name)}\n", text, re.DOTALL)


def require_restoration(evidence):
    require(re.search(r"^UNLOAD_RESTORED_BYTES_VERIFIED(?:\s|$)", evidence, re.MULTILINE),
            "missing verified 64-byte hook restoration")


def require_passive(evidence):
    require(re.search(r"^probe listening capabilities=0 capture=false$", evidence, re.MULTILINE),
            "missing passive listener readiness")
    require(re.search(r"^probe handshakes=1 events=0 capabilities=0 capture=false elapsed_ns=\d+ status=handshake_only$",
                      evidence, re.MULTILINE), "missing passive-only handshake success")


def require_ime_lock_exclusion(ime, locker):
    ime_keys = re.findall(r"^IME_KEY code=(\d+) state=(\d+) time=(\d+)$", ime, re.MULTILINE)
    locker_times = re.findall(r"^LOCK_KEY code=\d+ state=\d+ time=(\d+) ack=1$", locker, re.MULTILINE)
    require(len(ime_keys) == 22, "IME received unexpected keys: expected baseline18 + hold2 + revocation2")
    require(bool(locker_times), "missing IME-case locker delivery")
    start, end = min(map(int, locker_times)), max(map(int, locker_times))
    require(all(not start <= int(time) <= end for _, _, time in ime_keys),
            "IME received keys during locker input interval")


def validate_research(text):
    require("SLEDGE_STOCK_RESEARCH_FIXTURE_COMPLETED_NOT_CAPTURE" in text, "research fixture incomplete")
    require("RESEARCH_PLUGIN_LOADED_TWELVE_HOOK_INIT_NOT_DELIVERY_PROOF" in text, "missing twelve-hook init")
    files = evidence_files(text)
    app = files.get("/tmp/stock-consumer-events", "")
    old = phases(app, "research-old-held")
    require(len(old) == 1, "missing old-client held transition")
    require("depressed: 00000001: Shift" in old[0] and "sym: F " in old[0],
            "research lost physical Shift semantics before lock")
    for code in (41, 50):  # wev displays XKB code (evdev + 8).
        require(re.search(rf"key: {code}; state: 0", old[0]), f"old client missing release {code}")
    zero = old[0].rfind("depressed: 00000000")
    leave = old[0].rfind("wl_keyboard] leave:")
    require(0 <= zero < leave, "old-client zero modifiers not before leave")
    unlock = phases(app, "research-unlock-held")
    require(len(unlock) == 1 and "wl_keyboard] key:" not in unlock[0], "locker holds leaked on unlock")
    require("depressed: 00000001" not in unlock[0], "locker Shift leaked on unlock")
    require("sym: j " not in unlock[0] and "sym: J " not in unlock[0] and "sym: Shift_R " not in unlock[0],
            "locker holds leaked through focus enter")
    for name in ("research-lock", "research-ime-lock"):
        locker = files.get(f"/tmp/{name}-events", "")
        require("LOCK_ACK" in locker and "LOCK_UNLOCK_ROUNDTRIP" in locker, f"{name}: missing ACK/unlock")
        require(re.search(r"^LOCK_ENTER held=$", locker, re.MULTILINE), f"{name}: pre-lock keys inherited")
        before_ack = locker.split("LOCK_ACK", 1)[0]
        require("LOCK_MODS depressed=0 latched=0 locked=0 group=0" in before_ack,
                f"{name}: missing neutral enter modifiers")
        first_key = re.search(r"LOCK_KEY code=(\d+) state=(\d+)", locker)
        require(first_key is not None and first_key.groups() == ("30", "1"), f"{name}: prelock up leaked before fresh input")
        require("LOCK_MODS depressed=1 " in locker and "LOCK_MODS depressed=0 " in locker,
                f"{name}: missing fresh locked Shift semantics")
        require(len(re.findall(r"LOCK_KEY .*ack=1", locker)) >= 18, f"{name}: missing fresh locked input")
    ime = files.get("/tmp/research-ime-events", "")
    for code in (33, 42):
        require(re.search(rf"IME_KEY code={code} state=1", ime), f"missing IME-held down {code}")
        require(re.search(rf"IME_KEY code={code} state=0", ime), f"missing IME old-route release {code}")
    require("IME_MODS depressed=0 latched=0 locked=0 group=0" in ime, "missing IME neutral modifiers")
    require_ime_lock_exclusion(ime, files.get("/tmp/research-ime-lock-events", ""))
    for name in ("recovered", "unloaded"):
        samples = phases(app, name)
        require(bool(samples), f"missing {name} stock delivery")
        for sample in samples:
            require(len(re.findall(r"wl_keyboard\] key:", sample)) == 18 and "sym: B " in sample and "sym: C " in sample,
                    f"{name}: missing balanced physical modifier delivery")
    print("PASS_RESEARCH_INDEPENDENT_LOCK_CONSUMERS_NOT_PRODUCTION_CAPTURE")
    print("GAPS: exact cross-client ordering; complete-pair replacement; transport delivery admission; mixed-device keymaps")


def validate_rollback(text):
    require("SLEDGE_STOCK_ROLLBACK_FIXTURE_COMPLETED_NOT_CAPTURE" in text, "rollback fixture incomplete")
    files = evidence_files(text)
    evidence = files.get("/tmp/stock-hook-evidence", "")
    require(re.search(r"^ROLLBACK_ALL_NINE_BYTES_VERIFIED(?:\s|$)", evidence, re.MULTILINE),
            "missing all-nine rollback byte verification")
    require_restoration(evidence)
    app = files.get("/tmp/stock-consumer-events", "")
    for name in ("baseline", "rollback", "unloaded"):
        samples = phases(app, name)
        require(len(samples) == 1 and len(re.findall(r"wl_keyboard\] key:", samples[0])) == 18,
                f"{name}: missing stock key delivery")
        require("sym: B " in samples[0] and "sym: C " in samples[0], f"{name}: missing physical Shift semantics")
    print("PASS_ALL_NINE_ROLLBACK_BYTES_AND_STOCK_DELIVERY_NOT_CAPTURE")


def validate(text, passive=False):
    require("SLEDGE_STOCK_EXPANDED_FIXTURE_COMPLETED_NOT_PARITY" in text, "fixture incomplete")
    files = evidence_files(text)
    app = files.get("/tmp/stock-consumer-events", "")
    for name in ("baseline", "loaded", "unloaded", "recovered"):
        samples = phases(app, name)
        require(bool(samples), f"missing {name}")
        for sample in samples:
            require(len(re.findall(r"wl_keyboard\] key:", sample)) == 18, f"{name}: unbalanced/missing key events")
            for symbol in ("Shift_L", "Shift_R", "B", "C", "Control_L", "Alt_R"):
                require(re.search(rf"sym: {symbol}\s", sample), f"{name}: missing {symbol}")
            for mask in ("00000001: Shift", "00000004: Control", "00000008: Mod1"):
                require(mask in sample, f"{name}: missing modifier {mask}")
    virtual = phases(app, "virtual-baseline")
    require(len(virtual) == 1 and "sym: a " in virtual[0] and "sym: b " in virtual[0], "missing virtual baseline")
    suppressed = phases(app, "virtual-loaded")
    require(len(suppressed) == 1 and "sym: a " not in suppressed[0] and "sym: b " in suppressed[0], "fixed virtual fixture suppression missing")
    for name in ("ime", "locked", "crashed"):
        samples = phases(app, name)
        require(bool(samples), f"missing {name}")
        require(all("wl_keyboard] key:" not in sample for sample in samples), f"app received keys during {name}")
    for name in ("stock-lock", "stock-relock"):
        locker = files.get(f"/tmp/{name}-events", "")
        require("LOCK_REQUEST outputs=1" in locker and "committed=1" in locker, f"{name}: no surface")
        require("LOCK_ACK" in locker and "LOCK_UNLOCK_ROUNDTRIP" in locker, f"{name}: no ACK/unlock")
        require(len(re.findall(r"LOCK_KEY .*ack=1", locker)) >= 18, f"{name}: missing locker input")
    require("LOCK_ABRUPT_DISCONNECT" in files.get("/tmp/stock-crash-events", ""), "missing crash")
    ime = files.get("/tmp/stock-ime-events", "")
    counts = re.search(r"IME_RELEASE keys=(\d+) modifiers=(\d+)", ime)
    require("IME_GRAB_READY" in ime and counts is not None, "missing real IME delivery")
    require(int(counts[1]) == 18 and int(counts[2]) >= 9, "missing IME keys/modifier transitions")
    trace = files.get("/tmp/stock-hook-events", "")
    for marker in ("virtual=0 name=qemu-virtio-keyboard", "virtual=0 name=qemu-qemu-usb-keyboard", "ime_key", "ime_mod", "lock_before", "lock_after"):
        require(marker in trace, f"missing native path evidence: {marker}")
    require_restoration(files.get("/tmp/stock-hook-evidence", ""))
    if passive:
        require_passive(files.get("/tmp/stock-passive-peer-events", ""))
        samples = phases(app, "passive")
        require(len(samples) == 1 and len(re.findall(r"wl_keyboard\] key:", samples[0])) == 18,
                "passive plugin changed independent app key delivery")
        require("PASSIVE_UNLOAD_COMPOSITOR_ALIVE_NOT_BYTE_PROOF" in text, "missing passive unload survival")
        print("PASS_PASSIVE_HANDSHAKE_ONLY_NO_CAPTURE")
    print("PASS_INDEPENDENT_CONSUMER_CHECKS_NOT_PARITY")
    held = re.search(r"LOCK_ENTER held=([^\n]+)", files.get("/tmp/stock-lock-events", ""))
    if held:
        print(f"OBSERVED_STOCK_PRELOCK_HELD_ENTER: {held[1]} (quarantine NOT qualified)")
    print("GAPS: atomic revocation; replacement/transport; simultaneous merged-device holds; "
          "IME text/source switching; multiple outputs/layouts; hardware/repeat/conflict rollback")


if __name__ == "__main__":
    text = Path(sys.argv[1]).read_text()
    if "--research" in sys.argv[2:]:
        validate_research(text)
    elif "--rollback" in sys.argv[2:]:
        validate_rollback(text)
    else:
        validate(text, passive="--passive" in sys.argv[2:])
