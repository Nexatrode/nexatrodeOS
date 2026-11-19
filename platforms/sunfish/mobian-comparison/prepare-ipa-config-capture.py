#!/usr/bin/env python3
"""Prepare a RAM-only reference startup script with active-only IPA samples."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys

root = Path(__file__).resolve().parents[3]
base = root / 'platforms/sunfish/mobian-comparison/start-coldwifi.sh'
module = root / '.cache/sunfish-native-wifi/ipa-config-readback/sunfish_ipa_config_readback.ko'
image = root / '.cache/sunfish-mobian-comparison/artifacts/sunfish-linux-comparison-stage0-v2.img'
output = root / '.cache/sunfish-native-wifi/ipa-config-readback/prepared'
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
assert digest(base) == '1d97e6f4c5c693271581928703a46ece40ff97f2a00990ed514e199ebb178af6'
assert digest(module) == '8978901e9718fa64be64ccd915efd332b6db9689869822cdacc3087d0be61972'
assert digest(image) == '6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d'
text = base.read_text()
function = '''
# Active-only module refuses to wake a suspended IPA; unavailable is not zero.
snapshot_ipa_config() {
    phase=$1
    sample="$work/logs/ipa-config-$phase.txt"
    if insmod "$work/sunfish_ipa_config_readback.ko" >"$sample" 2>&1; then
        rmmod sunfish_ipa_config_readback || return 1
        dmesg | grep 'sunfish-ipa-config:' | tail -n 1 >> "$sample"
    else
        printf 'ACTIVE SNAPSHOT UNAVAILABLE\\n' >> "$sample"
    fi
}
'''
anchor = 'test "$(cat "$rp/state")" = offline\n'
assert text.count(anchor) == 1
text = text.replace(anchor, anchor + 'snapshot_ipa_config before-modem\n', 1)
anchor = 'result=$?\nset -e\n'
assert text.count(anchor) == 1
text = text.replace(anchor, anchor + 'snapshot_ipa_config after-bringup\n', 1)
anchor = 'work=/run/sunfish-coldwifi\n'
assert text.count(anchor) == 1
text = text.replace(anchor, anchor + function, 1)
output.mkdir(parents=True, exist_ok=False)
script = output / 'start-coldwifi-ipa-config.sh'
script.write_text(text)
subprocess.run(['sh', '-n', str(script)], check=True)
manifest = {'reference_boot_sha256': digest(image), 'module_sha256': digest(module),
            'reference_script_sha256': digest(base), 'capture_script_sha256': digest(script),
            'scope': 'Temporary Linux reference, RAM-backed EFS and firmware, one modem/WLAN startup. Active-only two-register snapshots before modem and after bringup; skips each snapshot if IPA is suspended. No IPA register writes, no persistent EFS writes or flash.',
            'limitations': 'A suspended IPA makes a sample unavailable. Two snapshots do not identify who wrote a register or exactly when it changed.',
            'device_actions': []}
(output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print('Prepared', script)
print('Script SHA-256', digest(script))
