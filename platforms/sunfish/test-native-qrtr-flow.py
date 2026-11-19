#!/usr/bin/env python3
"""Check bounded QRTR credit accounting against the pinned Linux function.

This is a host-only test; it neither builds a phone image nor talks to a device.
"""
from pathlib import Path
import hashlib
import os
import re
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ARCHIVE = ROOT / '.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
HASH = 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'


def main():
    digest = hashlib.sha256()
    with ARCHIVE.open('rb') as file:
        for block in iter(lambda: file.read(1024 * 1024), b''):
            digest.update(block)
    if digest.hexdigest() != HASH:
        raise ValueError('pinned Linux archive differs')
    with tarfile.open(ARCHIVE) as archive:
        member = next(m for m in archive.getmembers() if m.name.endswith('/net/qrtr/af_qrtr.c'))
        source = archive.extractfile(member).read().decode()
    for definition in ['#define QRTR_TX_FLOW_HIGH\t10', '#define QRTR_TX_FLOW_LOW\t5']:
        if definition not in source:
            raise ValueError('Linux watermark changed: ' + definition)
    match = re.search(r'static int qrtr_tx_wait\([\s\S]*?\n}\n', source)
    if not match or 'flow->pending == QRTR_TX_FLOW_LOW' not in match.group(0):
        raise ValueError('Linux QRTR transmit function changed')
    if not re.search(r'qrtr_tx_resume\([\s\S]*?flow->pending = 0;', source):
        raise ValueError('Linux QRTR resume no longer resets the counter')
    prelude = r'''
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
typedef uint64_t u64;
#define QRTR_TX_FLOW_HIGH 10
#define QRTR_TX_FLOW_LOW 5
#define QRTR_TYPE_DATA 1
#define GFP_KERNEL 0
#define mutex_lock(x) ((void)(x))
#define mutex_unlock(x) ((void)(x))
#define spin_lock_irq(x) ((void)(x))
#define spin_unlock_irq(x) ((void)(x))
#define init_waitqueue_head(x) ((void)(x))
#define wait_event_interruptible_locked_irq(q, condition) ((void)(q), (condition) ? 0 : -999)
#define kzalloc_obj(x) calloc(1, sizeof(struct qrtr_tx_flow))
#define kfree(x) free(x)
#define xa_err(x) ((x) != NULL)
struct waitq { int lock; };
struct qrtr_tx_flow { struct waitq resume_tx; int pending; int tx_failed; };
struct slot { unsigned long key; struct qrtr_tx_flow *flow; };
struct xarray { struct slot slots[4]; };
struct qrtr_node { int qrtr_tx_lock; struct xarray qrtr_tx_flow; void *ep; };
static void *xa_load(struct xarray *a, unsigned long k) {
  for (int i=0;i<4;i++) if (a->slots[i].flow && a->slots[i].key==k) return a->slots[i].flow;
  return NULL;
}
static void *xa_store(struct xarray *a, unsigned long k, void *p, int flags) {
  (void)flags; for (int i=0;i<4;i++) if (!a->slots[i].flow) { a->slots[i].key=k; a->slots[i].flow=p; return NULL; }
  return (void *)1;
}
'''
    main_c = r'''
int main(void) {
 struct qrtr_node n={.ep=(void *)1};
 for(int i=0;i<10;i++) printf("%d ",qrtr_tx_wait(&n,0,18,QRTR_TYPE_DATA));
 printf("blocked=%d other=%d ",qrtr_tx_wait(&n,0,18,QRTR_TYPE_DATA),qrtr_tx_wait(&n,0,19,QRTR_TYPE_DATA));
 struct qrtr_tx_flow *f=xa_load(&n.qrtr_tx_flow,18); f->pending=0;
 printf("resumed=%d ",qrtr_tx_wait(&n,0,18,QRTR_TYPE_DATA));
 f->tx_failed=1; printf("retry=%d pending=%d\n",qrtr_tx_wait(&n,0,18,QRTR_TYPE_DATA),f->pending);
 return 0;
}
'''
    with tempfile.TemporaryDirectory(prefix='sunfish-qrtr-flow-') as tmp:
        directory = Path(tmp)
        c = directory / 'oracle.c'
        c.write_text(prelude + match.group(0) + main_c)
        subprocess.run(['cc', '-std=c11', '-Wall', '-Wextra', '-Werror', str(c), '-o', str(directory / 'oracle')], check=True)
        output = subprocess.check_output([str(directory / 'oracle')], text=True).strip()
        expected = '0 0 0 0 1 0 0 0 0 0 blocked=-999 other=0 resumed=0 retry=1 pending=1'
        if output != expected:
            raise ValueError(f'Unexpected Linux oracle output: {output}')
        test = directory / 'flow-test'
        subprocess.run(['rustc', '--edition=2024', '-D', 'warnings', '--test', str(ROOT / 'tests/sunfish_qrtr_flow_test.rs'), '-o', str(test)], check=True)
        subprocess.run([str(test)], check=True)
        experiment = 'feature="sunfish-qrtr-rmtfs-flow-experiment"'
        session_test = directory / 'session-test'
        subprocess.run(['rustc', '--edition=2024', '-D', 'warnings', '--cfg', experiment,
                        '--test', str(ROOT / 'tests/sunfish_glink_qmi_test.rs'), '-o', str(session_test)], check=True)
        subprocess.run([str(session_test), '--test-threads=1'], check=True, stdout=subprocess.DEVNULL)
        owner_test = directory / 'owner-test'
        features = ['sunfish-linux-ipa-qmi', 'sunfish-linux-rmtfs', 'sunfish-linux-mpss-glink',
                    'sunfish-native-startup-mpss-runtime', 'sunfish-qrtr-rmtfs-flow-experiment']
        flags = [item for feature in features for item in ['--cfg', f'feature="{feature}"']]
        env = dict(os.environ, RUST_MIN_STACK='16777216',
                   SUNFISH_LINUX_RMTFS_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs/linux-scm.txt'),
                   SUNFISH_RMTFS_WIRE_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs-server/wire.txt'))
        subprocess.run(['rustc', '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2', *flags,
                        '--test', str(ROOT / 'tests/sunfish_linux_rmtfs_server_test.rs'), '-o', str(owner_test)],
                       check=True, env=env)
        owner_result = subprocess.run([str(owner_test), '--test-threads=1'], env=env, text=True, capture_output=True)
        if owner_result.returncode:
            raise ValueError('Owner/storage regressions failed:\n' + owner_result.stdout[-5000:] + owner_result.stderr)
    print('PASS: pinned Linux qrtr_tx_wait oracle and bounded Rust flow tests; no device action.')


if __name__ == '__main__':
    main()
