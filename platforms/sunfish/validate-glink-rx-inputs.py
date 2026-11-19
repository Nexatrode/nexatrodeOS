#!/usr/bin/env python3
"""Pin the complete native intent inventory and one fixed local receive promise."""
import hashlib,json,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-intents-v2-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-intents-v2-probe/native-observation.json').read_text())
    if prior['artifact_sha256']!='2291f313842bebe6011141050ead963a63f32eedc472742c3f25b4d45c0ab91d' or prior['native_result']!='INTENT INVENTORY PASS' or prior['error']!='0x0' or not prior['queue_complete'] or not prior['tx_consumed']:raise ValueError('missing complete native inventory')
    if [prior[k]for k in ['control_records','intent_records','intent_entries','intent_requests','request_size_bytes','duplicate_ids','zero_sizes']]!=[31,30,30,1,52,0,0]:raise ValueError('changed native inventory/request')
    if [(g['size_bytes'],g['count'])for g in prior['size_groups']]!=[(128,16),(512,8),(1152,5),(8320,1)]:raise ValueError('changed native size groups')
    if prior['min_intent_id']!='0x0' or prior['max_intent_id']!='0x1d':raise ValueError('changed peer intent ID range')
    source=ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_native.c';text=source.read_text()
    for token in ['cmd.id = cpu_to_le16(RPM_CMD_INTENT);','cmd.lcid = cpu_to_le16(channel->lcid);','cmd.count = cpu_to_le32(1);','cmd.size = cpu_to_le32(intent->size);','cmd.liid = cpu_to_le32(intent->id);','idr_alloc_cyclic(&channel->liids, intent, 1, -1, GFP_ATOMIC)','msg.cmd = cpu_to_le16(RPM_CMD_RX_INTENT_REQ_ACK);','msg.param1 = cpu_to_le16(channel->lcid);','msg.param2 = cpu_to_le32(granted);','__le32 chunk_size;','__le32 left_size;','liid = le32_to_cpu(hdr.msg.param2);']:
        if token not in text:raise ValueError('changed receive-intent/data layout')
    sources={}
    for rel in ['drivers/rpmsg/qcom_glink_native.c','net/qrtr/qrtr.c','include/uapi/linux/qrtr.h']:
        sources[rel]=hashlib.sha256((ROOT/'.cache/sunfish-lineage-kernel'/rel).read_bytes()).hexdigest()
    proof.update(previous_native_complete_inventory=prior,receive_sources_sha256=sources,
        owned_actual_complete_channel_checkpoint_required=True,copied_report_cannot_mint_receive_checkpoint=True,
        fixed_receive_packets=[5,0,1,0,1,0,0,0,64,0,0,0,1,0,0,0,8,0,1,0,1,0,0,0],
        receive_tx_offset=40,receive_tx_head_after=64,receive_ap_rx_tail_unchanged=8,
        receive_local_channel_id=1,receive_local_intent_id=1,receive_buffer_bytes=64,receive_guard_bytes=32,
        receive_buffer_address_transmitted=False,receive_buffer_lease_retained_until_restart=True,
        receive_expected_request_bytes=52,receive_expected_data_header=[9,2,1],receive_expected_chunk_bytes=52,
        receive_expected_left_bytes=0,receive_expected_wire_bytes=72,receive_max_wire_read_bytes=80,
        preserved_unread_prefix_bytes=576,receive_data_rx_offset=584,receive_notifications=1,total_glink_notifications=4,
        post_auth_smem_write_bytes=84,rx_done_sent=False,receive_tail_written=False,peer_intent_selected=False,
        outgoing_qrtr_payload=False,qrtr_header_decoding_is_diagnostic_only=True,
        scope='One fixed64-byte local receive intent1 on local channel1 and positive request ACK after an owned actual complete native-shaped intent checkpoint. Recheck current request52, IDs/groups, full576 retained RX bytes, exact descriptor40/40/8/584, all TX, readiness and private empty guarded buffer. Append24 constant scalar control bytes at TX40, verify all TX twice, revalidate then publish head64 and notify bit12 once outside lock. Never transmit the buffer address or select a peer intent. Wait for TXtail64 and one9/remote2/local-intent1 packet with chunk52/left0,72 aligned bytes, bounded80-byte double copies. Preserve all prior RX bytes and tail8; copy only52 bytes to the unique private static64-byte buffer with32 guard bytes and zero remainder, verify twice and revalidate ready/state before success. Decode eight QRTR words/body word only as diagnostics; no outgoing QRTR, RX_DONE, RX consumption, reuse, other-channel acceptance, extra receive intents, WLAN/DMA, retry or cleanup. Keep the buffer lease reserved through restart even on ambiguity.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
