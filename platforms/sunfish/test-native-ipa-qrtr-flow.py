#!/usr/bin/env python3
"""Exercise opt-in IPA destination credit through link, owner and runtime."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def main():
    env = dict(os.environ, RUST_MIN_STACK='16777216',
        SUNFISH_LINUX_QMI_LINK_C=str(ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi-link/linux-wire.txt'),
        SUNFISH_LINUX_RMTFS_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs/linux-scm.txt'),
        SUNFISH_RMTFS_WIRE_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs-server/wire.txt'))
    common = ['sunfish-linux-ipa-qmi', 'sunfish-qrtr-rmtfs-flow-experiment', 'sunfish-qrtr-ipa-flow-experiment']
    runtime = ['sunfish-linux-rmtfs', 'sunfish-linux-mpss-glink',
               'sunfish-native-startup-mpss-runtime', 'sunfish-qrtr-deferred-requests']
    with tempfile.TemporaryDirectory(prefix='sunfish-ipa-qrtr-flow-') as temp:
        for label, source, features, required in [
            ('link', 'sunfish_ipa_linux_qmi_link_test.rs', common,
             'linuxlink_reference_flow_accepts_response_after_real_glink_completion'),
            ('owner', 'sunfish_ipa_linux_qmi_owner_test.rs', common,
             'linuxowner_holds_early_response_and_reply_requests_while_initial_record_is_retained'),
            ('runtime', 'sunfish_linux_rmtfs_server_test.rs', common + runtime,
             'linuxowner_all_six_handshake_orders_are_serialized_without_manual_protocol_commits'),
        ]:
            binary = Path(temp) / label
            flags = [flag for feature in features for flag in ('--cfg', f'feature="{feature}"')]
            subprocess.run(['rustc', '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2',
                            *flags, '--test', str(ROOT / 'tests' / source), '-o', str(binary)],
                           cwd=ROOT, env=env, check=True)
            result = subprocess.run([str(binary), '--test-threads=1'], cwd=ROOT, env=env,
                                    text=True, capture_output=True, check=True)
            if f'test {required} ... ok' not in result.stdout:
                raise ValueError(f'{label}: required case missing')
            print(f'PASS: {label}: {result.stdout.split("test result: ok.")[-1].strip().splitlines()[0]}')
    print('No device action; QRTR credit does not replace GLINK completion or QMI response validation.')

if __name__ == '__main__':
    main()
