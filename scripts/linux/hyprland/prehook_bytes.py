"""Guest-only read-only snapshot of ten pinned stock hook entry points."""
import hashlib
import json
import os
from pathlib import Path
import struct
import sys
import socket


def request_snapshot(pid):
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(3)
        client.connect('/run/sledge-prehook-reader.sock')
        client.sendall(f'{pid}\n'.encode())
        chunks = bytearray()
        while chunk := client.recv(4096):
            chunks.extend(chunk)
            if len(chunks) > 16384:
                raise ValueError('root snapshot response oversized')
    result = json.loads(chunks)
    if 'error' in result:
        raise ValueError(result['error'])
    return result


def serve():
    if os.geteuid() != 0 or not Path('/etc/sledge-qualification-candidate').is_file():
        raise ValueError('isolated guest root required')
    os.environ['XDG_RUNTIME_DIR'] = '/tmp/hyprland-prerequisite'
    with socket.socket(socket.AF_UNIX) as server:
        server.bind('/run/sledge-prehook-reader.sock')
        os.chmod('/run/sledge-prehook-reader.sock', 0o666)
        server.listen(1)
        server.settimeout(100)
        for _ in range(4):
            connection, _ = server.accept()
            with connection:
                connection.settimeout(3)
                try:
                    raw = connection.recv(32)
                    if not raw.endswith(b'\n') or not raw[:-1].isdigit():
                        raise ValueError('invalid PID request')
                    pid = int(raw)
                    uid = struct.unpack('3i', connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))[1]
                    import pwd
                    if uid != pwd.getpwnam('probe').pw_uid or Path(f'/proc/{pid}').stat().st_uid != uid:
                        raise ValueError('only probe-owned stock PID allowed')
                    result = snapshot(pid)
                except (ValueError, OSError) as error:
                    result = {'error': str(error)}
                connection.sendall(json.dumps(result, separators=(',', ':')).encode())

ELF_SHA = "8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca"
# Exact dynamic-symbol st_values verified with nm -D on ELF_SHA; native hook order.
OFFSETS = (0x9cf7c0, 0x9ca700, 0x68b0f0, 0x9c9d10, 0x72e9c0,
           0x72a6a0, 0x72da70, 0x9c9530, 0x6cdd50, 0x3f7b20)


def snapshot(pid):
    if os.environ.get("XDG_RUNTIME_DIR") != "/tmp/hyprland-prerequisite":
        raise ValueError("only isolated qualification guest scope")
    proc = Path(f"/proc/{pid}")
    before = (proc / "stat").read_text()
    executable = (proc / "exe").resolve()
    data = executable.read_bytes()
    if hashlib.sha256(data).hexdigest() != ELF_SHA or data[:6] != b'\x7fELF\x02\x01':
        raise ValueError("wrong stock ELF identity")
    targets = Path('/sledge-payload/hook-targets.json')
    offsets = OFFSETS
    if not targets.is_file():
        raise ValueError('nineteen-target hook manifest missing')
    if targets.is_file():
        manifest = json.loads(targets.read_text())
        if manifest.get('elf_sha256') != ELF_SHA or manifest.get('elf') != str(executable):
            raise ValueError('hook target ELF mismatch')
        entries = manifest.get('targets', []) + manifest.get('passive_targets', [])
        offsets = tuple(int(entry['elf_value'], 16) for entry in entries)
        if len(offsets) != 19 or len(set(offsets)) != 19:
            raise ValueError('all nineteen hook targets required')
    phoff = struct.unpack_from('<Q', data, 32)[0]
    entsize, count = struct.unpack_from('<HH', data, 54)
    segments = [struct.unpack_from('<IIQQQQQQ', data, phoff + i * entsize) for i in range(count)]
    maps = [line.split(maxsplit=5) for line in (proc / "maps").read_text().splitlines()]
    entries = []
    with (proc / "mem").open('rb', buffering=0) as memory:
        for offset in offsets:
            segment = next(s for s in segments if s[0] == 1 and s[1] & 1 and s[3] <= offset < s[3] + s[5])
            candidates = [m for m in maps if len(m) == 6 and m[5] == str(executable) and
                          'x' in m[1] and int(m[2], 16) == segment[2] // 4096 * 4096]
            if len(candidates) != 1:
                raise ValueError("ambiguous executable segment mapping")
            address = int(candidates[0][0].split('-')[0], 16) - segment[3] // 4096 * 4096 + offset
            memory.seek(address)
            value = memory.read(32)
            if len(value) != 32:
                raise ValueError("short prehook memory read")
            entries.append({"symbol_value": offset, "address": address, "bytes": value.hex()})
    after = (proc / "stat").read_text()
    # starttime is field 22; command may contain spaces/parentheses.
    identity = lambda stat: stat.rsplit(')', 1)[1].split()[19]
    if identity(before) != identity(after):
        raise ValueError("process identity changed")
    return {"kind": "prehook_bytes", "pid": pid, "starttime": identity(before),
            "elf_sha256": ELF_SHA, "entries": entries}


if __name__ == '__main__':
    if sys.argv[1] == '--serve':
        serve()
    else:
        print(json.dumps(request_snapshot(int(sys.argv[1])), separators=(',', ':')))
