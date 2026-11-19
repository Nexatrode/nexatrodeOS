#!/usr/bin/env python3
"""Check the station-vdev wire layout against the pinned Linux ath10k ABI."""
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
            end = source.index(';', at)
            return source[start:end + 1]
    raise ValueError(name)


def main():
    assert subprocess.check_output(['git', '-C', str(LINUX), 'rev-parse', 'HEAD'], text=True).strip() == PIN
    paths = ['drivers/net/wireless/ath/ath10k/wmi.h', 'drivers/net/wireless/ath/ath10k/wmi.c']
    subprocess.run(['git', '-C', str(LINUX), 'diff', '--quiet', 'HEAD', '--', *paths], check=True)
    h = (LINUX / paths[0]).read_text()
    c = (LINUX / paths[1]).read_text()
    commands = h.split('enum wmi_10_4_cmd_id {', 1)[1].split('};', 1)[0]
    value = None
    found = {}
    for line in commands.splitlines():
        item = line.strip().rstrip(',')
        if item == 'WMI_10_4_START_SCAN_CMDID = WMI_10_4_START_CMDID':
            value = 0x9000
            continue
        if value is not None and item.startswith('WMI_10_4_'):
            value += 1
            found[item] = value
    for name, expected in {'VDEV_CREATE': 0x9013, 'VDEV_DELETE': 0x9014,
                           'VDEV_START_REQUEST': 0x9015, 'VDEV_UP': 0x9017,
                           'VDEV_STOP': 0x9018, 'VDEV_DOWN': 0x9019, 'PEER_CREATE': 0x9022,
                           'PEER_DELETE': 0x9023, 'PEER_ASSOC': 0x9026}.items():
        assert found['WMI_10_4_' + name + '_CMDID'] == expected
    events = h.split('enum wmi_10_4_event_id {', 1)[1].split('};', 1)[0]
    event_tail = events.split('WMI_10_4_SCAN_EVENTID = WMI_10_4_START_EVENTID,', 1)[1]
    event_names = [line.strip().rstrip(',') for line in event_tail.splitlines()
                   if line.strip().startswith('WMI_10_4_')]
    assert event_names[:4] == ['WMI_10_4_ECHO_EVENTID', 'WMI_10_4_UPDATE_STATS_EVENTID',
                               'WMI_10_4_INST_RSSI_STATS_EVENTID', 'WMI_10_4_VDEV_START_RESP_EVENTID']
    ops = c.split('static const struct wmi_ops wmi_10_4_ops = {', 1)[1].split('\n};', 1)[0]
    for marker in ['.gen_vdev_start = ath10k_wmi_op_gen_vdev_start',
                   '.gen_peer_create = ath10k_wmi_op_gen_peer_create',
                   '.gen_vdev_up = ath10k_wmi_op_gen_vdev_up',
                   '.gen_peer_assoc = ath10k_wmi_10_4_op_gen_peer_assoc']:
        assert marker in ops
    start = c.split('ath10k_wmi_op_gen_vdev_start(', 1)[1].split('\nstatic struct sk_buff *', 1)[0]
    for field in ['cmd->vdev_id', 'cmd->beacon_interval', 'cmd->dtim_period',
                  'cmd->flags', 'ath10k_wmi_put_wmi_channel']:
        assert field in start
    assert 'memset(skb->data, 0, round_len)' in c
    peer = c.split('ath10k_wmi_peer_assoc_fill_10_2(', 1)[1].split('\nstatic ', 1)[0]
    assert 'max_mcs = 0xf' in peer and 'max_nss = 0xf' in peer
    assert '#define WMI_PEER_ASSOC_INFO0_MAX_MCS_IDX_LSB 0' in h
    assert '#define WMI_PEER_ASSOC_INFO0_MAX_NSS_LSB 4' in h
    assert '#define MAX_SUPPORTED_RATES 128' in h
    types = ['wmi_mac_addr', 'wmi_ssid', 'wmi_channel', 'wmi_p2p_noa_descriptor',
             'wmi_vdev_create_cmd', 'wmi_vdev_start_request_cmd', 'wmi_vdev_up_cmd',
             'wmi_vdev_stop_cmd', 'wmi_vdev_down_cmd', 'wmi_vdev_delete_cmd',
             'wmi_peer_create_cmd', 'wmi_peer_delete_cmd', 'wmi_vdev_start_response_event',
             'wmi_rate_set', 'wmi_vht_rate_set', 'wmi_common_peer_assoc_complete_cmd',
             'wmi_10_2_peer_assoc_complete_cmd', 'wmi_10_4_peer_assoc_complete_cmd']
    prelude = '#include <stdint.h>\n#include <stddef.h>\ntypedef uint8_t u8; typedef uint32_t u32; typedef uint32_t __le32;\n#define __packed __attribute__((packed))\n#define MAX_SUPPORTED_RATES 128\n'
    assertions = '''
_Static_assert(sizeof(struct wmi_channel)==24, "channel");
_Static_assert(sizeof(struct wmi_vdev_start_request_cmd)==128, "start");
_Static_assert(offsetof(struct wmi_vdev_start_request_cmd,vdev_id)==24, "vdev");
_Static_assert(offsetof(struct wmi_vdev_start_request_cmd,beacon_interval)==32, "beacon");
_Static_assert(offsetof(struct wmi_vdev_start_request_cmd,dtim_period)==36, "dtim");
_Static_assert(offsetof(struct wmi_vdev_start_request_cmd,flags)==40, "flags");
_Static_assert(offsetof(struct wmi_vdev_start_request_cmd,ssid)==44, "ssid");
_Static_assert(sizeof(struct wmi_vdev_create_cmd)==20, "create");
_Static_assert(offsetof(struct wmi_vdev_create_cmd,vdev_type)==4, "create type");
_Static_assert(offsetof(struct wmi_vdev_create_cmd,vdev_macaddr)==12, "create mac");
_Static_assert(sizeof(struct wmi_vdev_up_cmd)==16, "up");
_Static_assert(offsetof(struct wmi_vdev_up_cmd,vdev_assoc_id)==4, "assoc id");
_Static_assert(offsetof(struct wmi_vdev_up_cmd,vdev_bssid)==8, "bssid");
_Static_assert(sizeof(struct wmi_vdev_stop_cmd)==4, "stop");
_Static_assert(sizeof(struct wmi_vdev_down_cmd)==4, "down");
_Static_assert(sizeof(struct wmi_vdev_delete_cmd)==4, "delete");
_Static_assert(sizeof(struct wmi_peer_create_cmd)==16, "peer create");
_Static_assert(offsetof(struct wmi_peer_create_cmd,peer_macaddr)==4, "peer mac");
_Static_assert(offsetof(struct wmi_peer_create_cmd,peer_type)==12, "peer type");
_Static_assert(sizeof(struct wmi_peer_delete_cmd)==12, "peer delete");
_Static_assert(sizeof(struct wmi_vdev_start_response_event)==16, "response");
_Static_assert(sizeof(struct wmi_rate_set)==136, "rate set");
_Static_assert(sizeof(struct wmi_common_peer_assoc_complete_cmd)==348, "common assoc");
_Static_assert(offsetof(struct wmi_common_peer_assoc_complete_cmd,peer_legacy_rates)==48, "legacy rates");
_Static_assert(offsetof(struct wmi_common_peer_assoc_complete_cmd,peer_ht_rates)==184, "ht rates");
_Static_assert(offsetof(struct wmi_common_peer_assoc_complete_cmd,peer_nss)==320, "nss");
_Static_assert(offsetof(struct wmi_common_peer_assoc_complete_cmd,peer_vht_rates)==332, "vht rates");
_Static_assert(sizeof(struct wmi_10_4_peer_assoc_complete_cmd)==356, "10.4 assoc");
_Static_assert(offsetof(struct wmi_10_4_peer_assoc_complete_cmd,cmd.info0)==348, "info0");
_Static_assert(offsetof(struct wmi_10_4_peer_assoc_complete_cmd,peer_bw_rxnss_override)==352, "bandwidth");
int main(void) { return 0; }
'''
    with tempfile.TemporaryDirectory(prefix='sunfish-vdev-abi-') as directory:
        path = Path(directory) / 'oracle.c'
        path.write_text(prelude + '\n'.join(struct(h, name) for name in types) + assertions)
        subprocess.run(['cc', '-std=c11', '-Wall', '-Werror', '-fsyntax-only', str(path)], check=True)
    print('PASS: pinned WMI 10.4 command IDs, ops and C structure layouts')


if __name__ == '__main__':
    main()
