#!/usr/bin/env python3
"""Prepare a one-shot Linux QRTR socket-metadata comparison; no device I/O."""
from pathlib import Path
import hashlib
import json
import re
import shutil
import subprocess

root = Path(__file__).resolve().parents[3]
base = root / 'platforms/sunfish/mobian-comparison/start-coldwifi.sh'
boot = root / '.cache/sunfish-mobian-comparison/artifacts/sunfish-linux-comparison-stage0-v2.img'
bundle = root / '.cache/sunfish-mobian-comparison/wifi-preparation/private/cold-wifi-inputs.tar.gz'
prepared = root / '.cache/sunfish-native-wifi/private/tftp-metadata-trace-prepared-v4'
out = root / '.cache/sunfish-native-wifi/private/tftp-reference-metadata-capture-v2'
trace = prepared / 'trace.so'
trace_manifest = json.loads((prepared / 'manifest.json').read_text())
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
assert digest(base) == '1d97e6f4c5c693271581928703a46ece40ff97f2a00990ed514e199ebb178af6'
assert digest(boot) == '6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d'
assert digest(bundle) == '7292f81a6c76d580d774ebfaacad4b8e591ff2125365dffd922f06e1e1b3f2c5'
assert digest(trace) == trace_manifest['tracer_sha256']
assert digest(root / 'platforms/sunfish/mobian-comparison/tftp-metadata-trace.c') == trace_manifest['source_sha256']
assert digest(root / 'platforms/sunfish/mobian-comparison/test-tftp-metadata-trace.py') == trace_manifest['test_sha256']
assert trace_manifest['reference_tqftpserv_sha256'] == '01e063925e3e86b0c33af5f87227633cf0e9305cdd35d1b1552262d489bddf56'
dynamic = (prepared / 'elf-dynamic.txt').read_text()
needed = re.findall(r'\(NEEDED\).*\[(.*?)\]', dynamic)
assert needed == ['libc.so.6'] and '(RPATH)' not in dynamic and '(RUNPATH)' not in dynamic
versions = set(re.findall(r'Name: (GLIBC_\d+\.\d+)', (prepared / 'elf-versions.txt').read_text()))
assert versions <= {'GLIBC_2.17', 'GLIBC_2.34'}, versions

setup = r'''
# Metadata observer applies to the temporary tqftpserv unit only. No packet
# payload or path is intentionally recorded by the observer. Logs can alter
# timing; absence of a line is not proof a peer did not send a packet.
test "$(sha256sum /usr/bin/tqftpserv | awk '{print $1}')" = @REFERENCE_HASH@
test "$(sha256sum "$work/tftp-metadata-trace.so" | awk '{print $1}')" = @TRACE_HASH@
LD_PRELOAD="$work/tftp-metadata-trace.so" /usr/bin/true 2> "$work/logs/tftp-loader-check.txt"
test ! -s "$work/logs/tftp-loader-check.txt"
mkdir -p /run/systemd/system/sunfish-wifi-tftp.service.d
cat > /run/systemd/system/sunfish-wifi-tftp.service.d/10-metadata.conf <<'UNIT'
[Service]
Environment=LD_PRELOAD=/run/sunfish-coldwifi/tftp-metadata-trace.so
UNIT
systemd-analyze verify /run/systemd/system/sunfish-wifi-tftp.service
systemctl daemon-reload
systemctl show sunfish-wifi-tftp.service -p Environment --value | grep -Fqx 'LD_PRELOAD=/run/sunfish-coldwifi/tftp-metadata-trace.so'
capture_tftp_metadata() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    journalctl -b -u sunfish-wifi-tftp.service --no-pager -o short-monotonic | awk 'index($0, "TFTP_META event=")' > "$work/logs/tftp-metadata.txt"
    exit "$status"
}
trap capture_tftp_metadata EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
'''.replace('@REFERENCE_HASH@', trace_manifest['reference_tqftpserv_sha256']).replace('@TRACE_HASH@', digest(trace))
text = base.read_text()
anchor = 'systemctl start sunfish-wifi-rmtfs.service sunfish-wifi-tftp.service\n'
assert text.count(anchor) == 1
text = text.replace(anchor, setup + anchor, 1)
original_journal = 'journalctl -b --no-pager -o short-monotonic > "$work/logs/journal-after.txt"\n'
assert text.count(original_journal) == 1
text = text.replace(original_journal, '# The EXIT trap exports only TFTP_META lines from this unit.\n', 1)
assert not out.exists(), f'preserve existing output: {out}'
out.mkdir(parents=True, mode=0o700)
script = out / 'start-coldwifi-tftp-metadata.sh'
script.write_text(text)
shutil.copy2(trace, out / 'tftp-metadata-trace.so')
subprocess.run(['sh', '-n', str(script)], check=True)
manifest = {
    'reference_boot_sha256': digest(boot), 'input_bundle_sha256': digest(bundle),
    'reference_script_sha256': digest(base), 'capture_script_sha256': digest(script),
    'tracer_sha256': digest(out / 'tftp-metadata-trace.so'),
    'reference_tqftpserv_sha256': trace_manifest['reference_tqftpserv_sha256'],
    'tracer_build_manifest_sha256': digest(prepared / 'manifest.json'),
    'scope': 'One temporary Linux reference boot with existing RAM-backed EFS and firmware. A unit-local LD_PRELOAD records QRTR socket endpoint, opcode, byte-count, result, errno and monotonic-time metadata during modem and WLAN firmware startup. It does not log packet contents or paths, associate to a network, flash partitions or persist modem-storage writes.',
    'limitations': 'An ARM64 Linux synthetic loopback/stdbuf test passed; QRTR interposition itself and this startup script have not been exercised on the device. Logs can alter timing; missing metadata is not proof of no traffic. The original bringup and dmesg logs may still contain firmware or service paths. Linux app send completion is not modem application receipt.',
    'device_actions': [], 'attempt_consumed': False,
}
(out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print('Prepared:', script)
print('Capture script SHA-256:', manifest['capture_script_sha256'])
