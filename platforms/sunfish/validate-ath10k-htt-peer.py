#!/usr/bin/env python3
"""Check HTT 10.4 peer-map events against the pinned ath10k C definitions."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
LINUX = ROOT / '.cache/sunfish-linux-mainline-ref'
PIN = 'fd78d17954a86ea6ceb0c535e618ea129e09f8dd'


def struct(source, name):
    start = source.index('struct ' + name + ' {')
    opening = source.index('{', start)
    depth = 0
    for at in range(opening, len(source)):
        depth += (source[at] == '{') - (source[at] == '}')
        if depth == 0:
            return source[start:source.index(';', at) + 1]
    raise ValueError(name)


def main():
    assert subprocess.check_output(['git', '-C', str(LINUX), 'rev-parse', 'HEAD'], text=True).strip() == PIN
    base = 'drivers/net/wireless/ath/ath10k/'
    files = [base + name for name in ['htt.h', 'htt_rx.c', 'core.h', 'mac.c']]
    subprocess.run(['git', '-C', str(LINUX), 'diff', '--quiet', 'HEAD', '--', *files], check=True)
    header = (LINUX / files[0]).read_text()
    receive = (LINUX / files[1]).read_text()
    core = (LINUX / files[2]).read_text()
    mac = (LINUX / files[3]).read_text()
    events = header.split('enum htt_10_4_t2h_msg_type {', 1)[1].split('};', 1)[0]
    assert 'HTT_10_4_T2H_MSG_TYPE_PEER_MAP               = 0x3' in events
    assert 'HTT_10_4_T2H_MSG_TYPE_PEER_UNMAP             = 0x4' in events
    assert '#define ATH10K_MAX_NUM_PEER_IDS (1 << 11)' in core
    assert 'ath10k_peer_map_event(htt, &ev)' in receive
    assert 'ath10k_peer_unmap_event(htt, &ev)' in receive
    assert 'ath10k_wait_for_peer_created(ar, vdev_id, addr)' in mac
    prelude = '#include <stdint.h>\n#include <stddef.h>\ntypedef uint8_t u8; typedef uint16_t __le16;\n#define __packed __attribute__((packed))\n'
    checks = '''
_Static_assert(sizeof(struct htt_resp_hdr)==1, "header");
_Static_assert(sizeof(struct htt_rx_peer_map)==11, "map");
_Static_assert(offsetof(struct htt_rx_peer_map,vdev_id)==0, "vdev");
_Static_assert(offsetof(struct htt_rx_peer_map,peer_id)==1, "peer ID");
_Static_assert(offsetof(struct htt_rx_peer_map,addr)==3, "address");
_Static_assert(sizeof(struct htt_rx_peer_unmap)==3, "unmap");
_Static_assert(offsetof(struct htt_rx_peer_unmap,peer_id)==1, "unmap ID");
int main(void) { return 0; }
'''
    with tempfile.TemporaryDirectory(prefix='sunfish-htt-peer-') as directory:
        c = Path(directory) / 'oracle.c'
        c.write_text(prelude + '\n'.join(struct(header, name) for name in
            ['htt_resp_hdr', 'htt_rx_peer_map', 'htt_rx_peer_unmap']) + checks)
        subprocess.run(['cc', '-std=c11', '-Wall', '-Werror', '-fsyntax-only', str(c)], check=True)
    print('PASS: HTT 10.4 peer event IDs, packed offsets, Linux peer-ID bound and create wait')


if __name__ == '__main__':
    main()
