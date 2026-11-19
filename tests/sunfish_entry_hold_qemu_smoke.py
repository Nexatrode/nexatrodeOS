#!/usr/bin/env python3
"""Execute both entry holds under QEMU; this is not native Pixel boot evidence."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time


def packet(connection, command):
    def read(count):
        result = bytearray()
        while len(result) < count:
            chunk = connection.recv(count - len(result))
            if not chunk:
                raise RuntimeError('debugger disconnected')
            result.extend(chunk)
        return bytes(result)

    payload = command.encode()
    connection.sendall(b'$' + payload + b'#' + ('%02x' % (sum(payload) & 255)).encode())
    while read(1) != b'$':
        pass
    response = bytearray()
    while True:
        byte = read(1)
        if byte == b'#':
            break
        response.extend(byte)
    checksum = read(2)
    assert int(checksum, 16) == sum(response) & 255
    connection.sendall(b'+')
    return response.decode()


def run_variant(folder):
    image = folder / 'entry.Image'
    with tempfile.TemporaryDirectory(prefix='sunfish-entry-') as temp:
        endpoint = Path(temp) / 'gdb.sock'
        with open(Path(temp) / 'qemu.log', 'w+') as log:
            process = subprocess.Popen([
                'qemu-system-aarch64', '-M', 'virt', '-cpu', 'cortex-a55',
                '-accel', 'tcg', '-m', '2G', '-display', 'none', '-serial', 'none',
                '-monitor', 'none', '-S', '-gdb', 'unix:%s,server=on,wait=off' % endpoint,
                '-device', 'loader,file=%s,addr=0x80080000,cpu-num=0,force-raw=on' % image,
            ], stdout=log, stderr=log)
            try:
                for _ in range(100):
                    if process.poll() is not None:
                        log.seek(0)
                        raise RuntimeError(log.read())
                    if endpoint.exists():
                        break
                    time.sleep(0.05)
                with socket.socket(socket.AF_UNIX) as connection:
                    connection.settimeout(20)
                    connection.connect(str(endpoint))
                    assert packet(connection, 'Z0,80081018,4') == 'OK'
                    stopped = packet(connection, 'c')
                    assert stopped.startswith(('T05', 'S05')), stopped
                    assert packet(connection, 'z0,80081018,4') == 'OK'
                    registers = bytes.fromhex(packet(connection, 'g'))
                    pc = int.from_bytes(registers[32 * 8:33 * 8], 'little')
                    assert pc == 0x80081018, hex(pc)
                    assert packet(connection, 'm80081018,4') == '00000014'
                    # Check background, foreground, and the bounds on each buffer.
                    for buffer in range(2):
                        base = 0x9c000000 + buffer * 1080 * 2340 * 4
                        for x, y, expected in [(0, 160, 0x00122b52),
                                               (55, 200, 0x0000e5e5),
                                               (0, 159, 0), (0, 320, 0)]:
                            address = base + (y * 1080 + x) * 4
                            value = bytes.fromhex(packet(connection, 'm%x,4' % address))
                            assert int.from_bytes(value, 'little') == expected, (hex(address), value.hex())
                    return {'variant': folder.name, 'pc': hex(pc),
                            'entry_hold_reached': True, 'both_buffers_drawn': True,
                            'surrounding_rows_preserved': True, 'native_pixel_evidence': False}
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('out_dir', type=Path)
    args = parser.parse_args()
    results = [run_variant(args.out_dir.resolve() / name) for name in ['compact', 'large']]
    (args.out_dir / 'qemu-entry-results.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
