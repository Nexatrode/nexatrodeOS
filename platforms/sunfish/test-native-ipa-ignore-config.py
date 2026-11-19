#!/usr/bin/env python3
"""Test the opt-in 0x27 discard against the pinned Linux handler table."""
from pathlib import Path
import argparse
import hashlib
import os
import re
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--unhandled', action='store_true')
parser.add_argument('--init-response', action='store_true')
args = parser.parse_args()
if args.init_response and not args.unhandled:
    parser.error('--init-response requires --unhandled')
archive = ROOT / '.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
digest = hashlib.sha256()
with archive.open('rb') as f:
    for block in iter(lambda: f.read(1024 * 1024), b''):
        digest.update(block)
if digest.hexdigest() != 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2':
    raise ValueError('pinned Linux archive differs')
with tarfile.open(archive) as tar:
    def source(suffix):
        member = next(m for m in tar.getmembers() if m.name.endswith(suffix))
        return tar.extractfile(member).read().decode()
    ipa = source('/drivers/net/ipa/ipa_qmi.c')
    qmi = source('/drivers/soc/qcom/qmi_interface.c')
handlers = re.search(r'ipa_server_msg_handlers\[\]\s*=\s*\{([\s\S]*?)\n\};', ipa)
if not handlers or re.findall(r'\.msg_id\s*=\s*(\w+)', handlers.group(1)) != [
    'IPA_QMI_INDICATION_REGISTER', 'IPA_QMI_DRIVER_INIT_COMPLETE']:
    raise ValueError('pinned IPA server handler table changed')
invoke = re.search(r'static void qmi_invoke_handler\([\s\S]*?\n}', qmi)
if not invoke or not re.search(r'if\s*\(!handler->fn\)\s*return\s*;', invoke.group(0)):
    raise ValueError('unmatched Linux QMI handler behavior changed')
if args.init_response:
    sender = re.search(r'ssize_t qmi_send_indication\([\s\S]*?\n}', qmi)
    receiver = re.search(r'static void qmi_handle_message\([\s\S]*?\n}', qmi)
    if not sender or 'qmi_txn_cancel(&txn)' not in sender.group(0):
        raise ValueError('Linux indication transaction behavior changed')
    if not receiver or not re.search(r'if\s*\(!txn\)\s*\{[\s\S]*?return\s*;', receiver.group(0)):
        raise ValueError('Linux unmatched response behavior changed')
    if not re.search(r'qmi_send_indication\(qmi,\s*sq,\s*IPA_QMI_INIT_COMPLETE', ipa):
        raise ValueError('Linux IPA init indication behavior changed')
env = dict(os.environ, RUST_MIN_STACK='16777216',
    SUNFISH_LINUX_QMI_LINK_C=str(ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi-link/linux-wire.txt'),
    SUNFISH_LINUX_RMTFS_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs/linux-scm.txt'),
    SUNFISH_RMTFS_WIRE_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs-server/wire.txt'))
common = ['sunfish-linux-ipa-qmi', 'sunfish-qrtr-rmtfs-flow-experiment',
          'sunfish-qrtr-ipa-flow-experiment', 'sunfish-qrtr-servreg-flow-experiment',
          'sunfish-ipa-ignore-config-reference']
if args.unhandled:
    common.append('sunfish-ipa-ignore-unhandled-reference')
if args.init_response:
    common.append('sunfish-ipa-ignore-init-response-reference')
runtime = ['sunfish-linux-rmtfs', 'sunfish-linux-mpss-glink',
           'sunfish-native-startup-mpss-runtime', 'sunfish-qrtr-deferred-requests']
with tempfile.TemporaryDirectory(prefix='sunfish-ipa-ignore-config-') as temp:
    for name, features, source_file in [('owner', common, 'sunfish_ipa_linux_qmi_owner_test.rs'),
                                        ('runtime', common + runtime, 'sunfish_linux_rmtfs_server_test.rs')]:
        flags = [flag for feature in features for flag in ('--cfg', f'feature="{feature}"')]
        binary = Path(temp) / name
        subprocess.run(['rustc', '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2',
                        *flags, '--test', str(ROOT / 'tests' / source_file), '-o', str(binary)],
                       cwd=ROOT, env=env, check=True)
        result = subprocess.run([str(binary), '--test-threads=1'], cwd=ROOT, env=env,
                                capture_output=True, text=True, check=True)
        required_tests = ['reference_ipa_config_is_consumed_without_fabricating_a_qmi_response',
                          'reference_ipa_config_rejects_bad_headers_and_duplicate_tlvs']
        if args.unhandled:
            required_tests.extend(['reference_unhandled_filter_request_is_consumed_without_success_reply',
                                   'reference_unhandled_request_still_rejects_malformed_envelopes'])
        if args.init_response:
            required_tests.extend(['reference_ipa_init_response_is_ignored_without_completing_indication',
                                   'reference_ipa_init_response_rejects_unpublished_wrong_peer_or_transaction'])
        for required in required_tests:
            if f'test {required} ... ok' not in result.stdout:
                raise ValueError(f'{name}: missing {required}')
        print(f'PASS: {name}: {result.stdout.split("test result: ok.")[-1].strip().splitlines()[0]}')
print('PASS: pinned reference ignores unmatched QMI handlers; no device action or config-success receipt.')
