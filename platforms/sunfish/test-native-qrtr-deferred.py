#!/usr/bin/env python3
"""Host-only checks for the explicitly selected bounded deferral experiment."""
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[2]
features = ['sunfish-qrtr-rmtfs-flow-experiment', 'sunfish-qrtr-deferred-requests']
owner_features = features + ['sunfish-linux-ipa-qmi', 'sunfish-linux-rmtfs',
                            'sunfish-linux-mpss-glink', 'sunfish-native-startup-mpss-runtime']
env = dict(os.environ, RUST_MIN_STACK='16777216',
           SUNFISH_LINUX_RMTFS_C=str(root / '.cache/sunfish-native-wifi/private/linux-rmtfs/linux-scm.txt'),
           SUNFISH_RMTFS_WIRE_C=str(root / '.cache/sunfish-native-wifi/private/linux-rmtfs-server/wire.txt'))
with tempfile.TemporaryDirectory(prefix='sunfish-deferred-') as directory:
    for name, enabled in [('sunfish_glink_fifo_test', features),
                          ('sunfish_glink_qmi_test', features),
                          ('sunfish_linux_rmtfs_server_test', owner_features)]:
        binary = str(Path(directory) / name)
        flags = [item for feature in enabled for item in ['--cfg', f'feature="{feature}"']]
        # The session-only harness does not include the adapter that uses its retry helpers.
        allowance = ['-A', 'dead_code'] if name == 'sunfish_glink_qmi_test' else []
        subprocess.run(['rustc', '--edition=2024', '-D', 'warnings', *allowance, '-C', 'opt-level=2',
                        *flags, '--test', str(root / 'tests' / (name + '.rs')), '-o', binary],
                       check=True, env=env)
        subprocess.run([binary, '--test-threads=1', '--quiet'], check=True, env=env)
print('PASS: bounded deferred-request FIFO, session and storage tests; no device action.')
