#!/usr/bin/env python3
"""Prepare the header trace runner using the pinned temporary Linux reference."""
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[4]
private = root / '.cache/sunfish-native-wifi/private'
prior = private / 'elf-locator-reference-prepared-v1'
out = private / 'qrtr-header-reference-prepared-v1'
module = Path(sys.argv[1])
manifest = json.loads((prior / 'manifest.json').read_text())


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


for name, expected in manifest['input_sha256'].items():
    if sha(root / name) != expected:
        raise ValueError(f'Changed reference input {name}')
out.mkdir(mode=0o700)
shutil.copyfile(prior / "known-hosts", out / "known-hosts")
shutil.copyfile(module, out / 'qrtr-smd.ko')

setup = r'''
eventdir="$glink_trace_instance/events/sunfish_qrtr/header"
test -w "$eventdir/enable"
for field in tx version type src_node src_port dst_node dst_port confirm length control key_node key_port qmi_kind qmi_txn qmi_msg; do
    grep -Eq "field:.*[[:space:]]${field};" "$eventdir/format"
done
cp "$eventdir/format" "$work/logs/qrtr-header-format.txt"
printf '1\n' > "$eventdir/enable"
test "$(cat "$eventdir/enable")" = 1
'''
for name, expected in manifest['script_sha256'].items():
    src = prior / name
    assert sha(src) == expected, src
    script = src.read_text()
    anchor = 'printf \'1\\n\' > "$glink_trace_instance/tracing_on"'
    assert script.count(anchor) == 1
    script = script.replace(anchor, setup + anchor)
    dest = out / name
    dest.write_text(script)
    subprocess.run(['sh', '-n', str(dest)], check=True)

runner = (prior / 'run-capture.py').read_text()
runner = runner.replace("manifest = json.loads((base / 'manifest.json').read_text())", """manifest = json.loads((base / 'manifest.json').read_text())
for name, expected in manifest['script_sha256'].items():
    if hashlib.sha256((base / name).read_bytes()).hexdigest() != expected:
        raise RuntimeError(f'Capture artifact changed: {name}')
""")
runner = runner.replace("    (tracer, tracer.name),",
                        "    (tracer, tracer.name),\n    (base / 'qrtr-smd.ko', 'qrtr-smd.ko'),")
runner = runner.replace("print('RAM-backed storage and firmware verified', flush=True)", """print('RAM-backed storage and firmware verified', flush=True)
record('load-header-module', ssh(
    f'set -eu; cd {work}; '
    'if test -d /sys/module/qrtr_smd; then rmmod qrtr_smd; fi; '
    'mountpoint -q /sys/kernel/tracing || mount -t tracefs tracefs /sys/kernel/tracing; '
    'insmod ./qrtr-smd.ko; '
    'test -d /sys/kernel/tracing/events/sunfish_qrtr/header',
    check=False,
))""")
runner = runner.replace("'logs/tftp-metadata.txt',", "'logs/qrtr-header-format.txt', 'logs/tftp-metadata.txt',")
runner = runner.replace("print(f'Capture scripts finished", r"""if not status:
    stats = (out / 'logs/glink-event-stats.txt').read_text()
    counters = re.findall(r'(?m)^\s*(?:overrun|commit overrun|dropped events):\s*(\d+)', stats)
    if len(counters) != 24 or any(int(count) for count in counters):
        raise RuntimeError('Capture has missing or nonzero trace loss counters')
    trace = (out / 'logs/glink-events.txt').read_text()
    for direction, length in (('RX', 44), ('TX', 48), ('TX', 72)):
        if not re.search(rf': header: {direction} v=.* len={length} ', trace):
            raise RuntimeError(f'Capture is missing {direction} length {length}; see saved logs')
    rows = trace.splitlines()
    wrq = next((i for i, row in enumerate(rows)
                if re.search(r': header: RX v=.* type=1 .*dst=1:0x4000 .* len=108 ', row)), None)
    if wrq is None:
        raise RuntimeError('First WRQ-sized QRTR receive is missing; see saved logs')
    stamp = re.compile(r' (\d+\.\d+): ')
    origin = float(stamp.search(rows[wrq])[1])
    window = []
    for row in rows:
        match = stamp.search(row)
        if match and origin - 0.002 <= float(match[1]) <= origin + 0.025:
            window.append(row)
    (out / 'first-wrq-window.txt').write_text('\n'.join(window) + '\n')
    print('QRTR header events and zero-loss trace verified', flush=True)
print(f'Capture scripts finished""")
(out / 'run-capture.py').write_text(runner)
compile(runner, 'run-capture.py', 'exec')
manifest['script_sha256'] = {name: sha(out / name) for name in
                             (*manifest['script_sha256'], 'qrtr-smd.ko', 'run-capture.py')}
manifest['attempt_consumed'] = False
manifest['scope'] = ('Temporary Linux with RAM-backed storage and private scalar QRTR, GLINK and '
                     'TFTP tracing. QMI identifiers are header candidates for 44/48-byte DATA.')
(out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print(out)
