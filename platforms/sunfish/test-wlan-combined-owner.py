#!/usr/bin/env python3
"""Run the actual owner fixture with the opt-in WLAN TFTP composition."""
import json
import os
from pathlib import Path
import subprocess
import tomllib

root = Path(__file__).resolve().parents[2]
private = root / '.cache/sunfish-native-wifi/private'
out = private / 'native-tftp-wlan-mux-candidate-v1'
out.mkdir(exist_ok=True)
source = (private / 'native-ipa-tftp-dispatch-v1/runtime-transport.rs').read_text()
cases = (root / 'tests/sunfish_modem_owner_test.rs').read_text()
for feature, name in [
    ('sunfish-tftp-wlan-close-handoff', 'quiescent_firmware_close_retires_only_its_session_and_continues'),
    ('sunfish-tftp-wlan-close-handoff', 'firmware_close_validation_fault_retains_session_and_stops_owner'),
    ('sunfish-tftp-wlan-close-handoff', 'firmware_close_discards_only_its_exact_unpublished_retained_reply'),
    ('sunfish-tftp-wlan-mux', 'combined_owner_routes_firmware_rrq_on_shared_server_port'),
    ('sunfish-tftp-wlan-mux', 'combined_owner_completes_multiblock_firmware_read'),
    ('sunfish-tftp-wlan-mux', 'combined_owner_firmware_dispatch_fault_does_not_create_a_transfer'),
    ('sunfish-tftp-wlan-mux', 'firmware_close_before_dispatch_stops_without_publishing_a_reply'),
]:
    marker = f'#[cfg(feature = "{feature}")]\n#[test]\nfn {name}()'
    start = cases.index(marker)
    end = cases.index('\n}\n', start) + 3
    source += '\n' + cases[start:end]
test_source = out / 'wlan-combined-owner.rs'
test_source.write_text(source)
binary = out / 'wlan-combined-owner'
args = json.loads((private / 'native-ipa-reg-rx-first-v1/host-final/runtime-transport-command.json').read_text())
base = args[:-3]
args = []
i = 0
while i < len(base):
    if base[i] == '--cfg' and i + 1 < len(base) and base[i + 1].startswith('feature='):
        i += 2
    else:
        args.append(base[i])
        i += 1
features = set((out / 'features.txt').read_text().strip().split(','))
graph = tomllib.loads((root / 'rust/Cargo.toml').read_text())['features']
todo = list(features)
while todo:
    for feature in graph.get(todo.pop(), []):
        if feature in graph and feature not in features:
            features.add(feature)
            todo.append(feature)
assert 'sunfish-tftp-close-handoff-prototype' not in features
args += [item for feature in sorted(features) for item in ('--cfg', f'feature="{feature}"')]
args += [str(test_source), '-o', str(binary)]
args[args.index('opt-level=2')] = 'opt-level=0'
env = dict(os.environ, RUST_MIN_STACK='16777216',
           SUNFISH_LINUX_RMTFS_C=str(private / 'linux-rmtfs/linux-scm.txt'),
           SUNFISH_RMTFS_WIRE_C=str(private / 'linux-rmtfs-server/wire.txt'),
           SUNFISH_GLK_MEMORY_C=str(private / 'mpss-glink-memory-linux'),
           SUNFISH_GLK_TRANSPORT_C=str(private / 'mpss-glink-transport-linux'))
with (out / 'wlan-combined-owner-build.log').open('w') as log:
    subprocess.run(args, cwd=root, env=env, stdout=log, stderr=log, check=True)
with (out / 'wlan-combined-owner-tests.log').open('w') as log:
    subprocess.run([str(binary), '--test-threads=1'], cwd=root, env=env, stdout=log, stderr=log, check=True)
print((out / 'wlan-combined-owner-tests.log').read_text().splitlines()[-2])
