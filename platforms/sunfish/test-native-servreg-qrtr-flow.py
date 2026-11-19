#!/usr/bin/env python3
"""Test opt-in SERVREG destination credit with real simulated GLINK receipts."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
env = dict(os.environ, RUST_MIN_STACK='16777216',
    SUNFISH_LINUX_QMI_LINK_C=str(ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi-link/linux-wire.txt'),
    SUNFISH_LINUX_RMTFS_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs/linux-scm.txt'),
    SUNFISH_RMTFS_WIRE_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs-server/wire.txt'))
common = ['sunfish-linux-ipa-qmi', 'sunfish-qrtr-rmtfs-flow-experiment',
          'sunfish-qrtr-ipa-flow-experiment', 'sunfish-qrtr-servreg-flow-experiment']
runtime = ['sunfish-linux-rmtfs', 'sunfish-linux-mpss-glink',
           'sunfish-native-startup-mpss-runtime', 'sunfish-qrtr-deferred-requests']
with tempfile.TemporaryDirectory(prefix='sunfish-servreg-flow-') as temp:
    for name, features, source in [('owner', common, 'sunfish_ipa_linux_qmi_owner_test.rs'),
                                   ('runtime', common + runtime, 'sunfish_linux_rmtfs_server_test.rs')]:
        flags = [flag for feature in features for flag in ('--cfg', f'feature="{feature}"')]
        binary = Path(temp) / name
        subprocess.run(['rustc', '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2',
                        *flags, '--test', str(ROOT / 'tests' / source),
                        '-o', str(binary)], cwd=ROOT, env=env, check=True)
        result = subprocess.run([str(binary), '--test-threads=1'],
                                cwd=ROOT, env=env, capture_output=True, text=True, check=True)
        for required in ('servreg_reference_releases_reply_after_glink_without_qrtr_resume',
                         'servreg_reference_applies_watermarks_and_resumes_destination'):
            if f'test {required} ... ok' not in result.stdout:
                raise ValueError(f'{name}: missing {required}')
        print(f'PASS: {name}: {result.stdout.split("test result: ok.")[-1].strip().splitlines()[0]}')
print('No device action; real GLINK completion and destination flow credit remain required.')
